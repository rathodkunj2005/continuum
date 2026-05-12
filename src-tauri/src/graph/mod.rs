use crate::embed::Embedder;
use crate::search::HybridSearcher;
use crate::store::{EdgeType, GraphEdge, GraphNode, MeetingSegment, MemoryRecord, NodeType, Store};
use crate::tasks::Task;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryCard {
    pub id: String,
    pub timestamp: i64,
    pub app_name: String,
    pub window_title: String,
    pub snippet: String,
    pub url: Option<String>,
    pub screenshot_path: Option<String>,
    pub score: f32,
    pub related_tasks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryReconstruction {
    pub answer: String,
    pub cards: Vec<MemoryCard>,
    pub structural_context: Vec<String>,
}

/// Persisted graph store.
pub struct GraphStore {
    store: Arc<Store>,
}

impl GraphStore {
    pub fn new(store: Arc<Store>) -> Self {
        Self { store }
    }

    pub async fn ingest_memory(
        &self,
        record: &MemoryRecord,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let now = chrono::Utc::now().timestamp_millis();

        let memory_node_id = memory_node_id(&record.id);
        let narrative = if !record.memory_context.trim().is_empty() {
            record.memory_context.clone()
        } else {
            record.snippet.clone()
        };
        let node = GraphNode {
            id: memory_node_id.clone(),
            node_type: NodeType::MemoryChunk,
            label: compress_node_label(record),
            created_at: record.timestamp,
            metadata: json!({
                "app_name": record.app_name,
                "bundle_id": record.bundle_id,
                "window_title": record.window_title,
                "day_bucket": record.day_bucket,
                "session_id": record.session_id,
                "session_key": record.session_key,
                "summary_source": record.summary_source,
                "memory_context": narrative,
                "memory_type": classify_memory_type(
                    &record.app_name,
                    record.url.as_deref(),
                    &record.summary_source,
                ),
                "url": record.url,
            }),
        };

        let session_node_id = session_node_id(&record.session_id);
        let s_node = GraphNode {
            id: session_node_id.clone(),
            node_type: NodeType::Entity,
            label: format!("Session {}", record.day_bucket),
            created_at: record.timestamp,
            metadata: json!({
                "entity_type": "session",
                "session_id": record.session_id,
                "day_bucket": record.day_bucket,
            }),
        };

        // We use a set-based approach for nodes to mimic upsert behavior
        // In a real DB we'd check if they exist, but here we just send them to the store's upsert
        self.store.upsert_nodes(&[node, s_node]).await?;

        let edge = GraphEdge {
            id: uuid::Uuid::new_v4().to_string(),
            source: memory_node_id.clone(),
            target: session_node_id.clone(),
            edge_type: EdgeType::PartOfSession,
            timestamp: now,
            metadata: json!({}),
        };
        self.store.upsert_edges(&[edge]).await?;

        if let Some(url) = record.url.as_ref() {
            let url_node_id = url_node_id(url);
            let u_node = GraphNode {
                id: url_node_id.clone(),
                node_type: NodeType::Url,
                label: url.clone(),
                created_at: record.timestamp,
                metadata: json!({ "host": host_from_url(url) }),
            };
            self.store.upsert_nodes(&[u_node]).await?;

            let u_edge = GraphEdge {
                id: uuid::Uuid::new_v4().to_string(),
                source: memory_node_id,
                target: url_node_id,
                edge_type: EdgeType::OccurredAt,
                timestamp: now,
                metadata: json!({}),
            };
            self.store.upsert_edges(&[u_edge]).await?;
        }

        Ok(())
    }

    pub async fn link_task(&self, task: &Task) -> Result<(), Box<dyn std::error::Error>> {
        let now = chrono::Utc::now().timestamp_millis();
        let task_node_id = task_node_id(&task.id);

        let t_node = GraphNode {
            id: task_node_id.clone(),
            node_type: NodeType::Task,
            label: task.title.clone(),
            created_at: task.created_at,
            metadata: json!({
                "task_type": format!("{:?}", task.task_type),
                "source_app": task.source_app,
                "is_completed": task.is_completed,
            }),
        };
        self.store.upsert_nodes(&[t_node]).await?;

        let mut edges = Vec::new();
        if let Some(source_memory_id) = task.source_memory_id.as_ref() {
            edges.push(GraphEdge {
                id: uuid::Uuid::new_v4().to_string(),
                source: task_node_id.clone(),
                target: memory_node_id(source_memory_id),
                edge_type: EdgeType::ReferenceForTask,
                timestamp: now,
                metadata: json!({"reason": "source_memory"}),
            });
        }

        for memory_id in &task.linked_memory_ids {
            edges.push(GraphEdge {
                id: uuid::Uuid::new_v4().to_string(),
                source: task_node_id.clone(),
                target: memory_node_id(memory_id),
                edge_type: EdgeType::ReferenceForTask,
                timestamp: now,
                metadata: json!({"reason": "linked_memory"}),
            });
        }

        for url in &task.linked_urls {
            let url_id = url_node_id(url);
            let u_node = GraphNode {
                id: url_id.clone(),
                node_type: NodeType::Url,
                label: url.clone(),
                created_at: task.created_at,
                metadata: json!({ "host": host_from_url(url) }),
            };
            self.store.upsert_nodes(&[u_node]).await?;

            edges.push(GraphEdge {
                id: uuid::Uuid::new_v4().to_string(),
                source: task_node_id.clone(),
                target: url_id,
                edge_type: EdgeType::ReferenceForTask,
                timestamp: now,
                metadata: json!({"reason": "linked_url"}),
            });
        }

        if !edges.is_empty() {
            self.store.upsert_edges(&edges).await?;
        }
        Ok(())
    }

    /// For each segment in `segments`, query memories that overlap the segment's
    /// time window and create `OccurredDuringAudio` edges + AudioSegment nodes.
    pub async fn link_audio_to_memories(
        &self,
        segments: &[MeetingSegment],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let now = chrono::Utc::now().timestamp_millis();
        let mut nodes = Vec::new();
        let mut edges = Vec::new();

        for segment in segments {
            let seg_node_id = format!("audio_segment:{}", segment.id);

            nodes.push(GraphNode {
                id: seg_node_id.clone(),
                node_type: NodeType::AudioSegment,
                label: segment.text.chars().take(120).collect(),
                created_at: segment.start_timestamp,
                metadata: serde_json::json!({
                    "meeting_id": segment.meeting_id,
                    "index": segment.index,
                    "start_ts": segment.start_timestamp,
                    "end_ts": segment.end_timestamp,
                    "model": segment.model,
                }),
            });

            let memories = self
                .store
                .get_memories_in_range(segment.start_timestamp, segment.end_timestamp)
                .await
                .unwrap_or_default();

            for memory in &memories {
                edges.push(GraphEdge {
                    id: uuid::Uuid::new_v4().to_string(),
                    source: memory_node_id(&memory.id),
                    target: seg_node_id.clone(),
                    edge_type: EdgeType::OccurredDuringAudio,
                    timestamp: now,
                    metadata: serde_json::json!({}),
                });
            }
        }

        if !nodes.is_empty() {
            self.store.upsert_nodes(&nodes).await?;
        }
        if !edges.is_empty() {
            self.store.upsert_edges(&edges).await?;
        }
        Ok(())
    }

    /// Auto-link a new memory to an existing task cluster, or seed one if
    /// ≥ 2 other semantically similar memories exist with no task yet.
    ///
    /// Called fire-and-forget from the capture loop — never blocks capture.
    pub async fn auto_link_to_task(
        &self,
        record: &MemoryRecord,
    ) -> Result<(), Box<dyn std::error::Error>> {
        const SIMILARITY_THRESHOLD: f32 = 0.82;
        const MIN_CLUSTER_PEERS: usize = 2; // + current record = 3 total
        const SEARCH_K: usize = 10;

        if record.embedding.is_empty() || record.embedding.iter().all(|value| *value == 0.0) {
            return Ok(());
        }

        let nearby = self
            .store
            .vector_search(&record.embedding, SEARCH_K, Some("7d"), None)
            .await?;

        let similar: Vec<_> = nearby
            .iter()
            .filter(|r| r.id != record.id && r.score >= SIMILARITY_THRESHOLD)
            .collect();

        if similar.is_empty() {
            return Ok(());
        }

        let similar_memory_node_ids = similar
            .iter()
            .map(|result| memory_node_id(&result.id))
            .collect::<Vec<_>>();
        let edges = self
            .store
            .get_task_reference_edges_for_targets(&similar_memory_node_ids)
            .await?;
        let task_node_ids = edges
            .iter()
            .map(|edge| edge.source.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let task_nodes = self.store.get_nodes_by_ids(&task_node_ids).await?;
        let task_node_ids = task_nodes
            .iter()
            .filter(|node| node.node_type == NodeType::Task)
            .map(|node| node.id.clone())
            .collect::<HashSet<_>>();

        // memory_node_id → task_node_id for all ReferenceForTask edges
        let mut memory_to_task: HashMap<String, String> = HashMap::new();
        for edge in edges {
            if task_node_ids.contains(&edge.source) {
                memory_to_task.insert(edge.target.clone(), edge.source.clone());
            }
        }

        let current_mem_node = memory_node_id(&record.id);
        let now = chrono::Utc::now().timestamp_millis();

        // If any similar memory already belongs to a task, join that task.
        for sr in &similar {
            if let Some(task_id) = memory_to_task.get(&memory_node_id(&sr.id)) {
                self.store
                    .upsert_edges(&[GraphEdge {
                        id: uuid::Uuid::new_v4().to_string(),
                        source: task_id.clone(),
                        target: current_mem_node,
                        edge_type: EdgeType::ReferenceForTask,
                        timestamp: now,
                        metadata: serde_json::json!({"reason": "auto_cluster"}),
                    }])
                    .await?;
                return Ok(());
            }
        }

        // No existing task — create one if cluster is big enough.
        if similar.len() < MIN_CLUSTER_PEERS {
            return Ok(());
        }

        let title = infer_task_title(record);
        let task_node_id = format!("task:auto:{}", task_slug(&title));

        // Idempotent: upsert_nodes handles existing node gracefully.
        self.store
            .upsert_nodes(&[GraphNode {
                id: task_node_id.clone(),
                node_type: NodeType::Task,
                label: title.clone(),
                created_at: now,
                metadata: serde_json::json!({
                    "auto_generated": true,
                    "cluster_size": similar.len() + 1,
                }),
            }])
            .await?;

        // Link current record + all similar peers to the new task.
        let mut edges_to_add = vec![GraphEdge {
            id: uuid::Uuid::new_v4().to_string(),
            source: task_node_id.clone(),
            target: current_mem_node,
            edge_type: EdgeType::ReferenceForTask,
            timestamp: now,
            metadata: serde_json::json!({"reason": "auto_cluster_seed"}),
        }];
        for sr in &similar {
            edges_to_add.push(GraphEdge {
                id: uuid::Uuid::new_v4().to_string(),
                source: task_node_id.clone(),
                target: memory_node_id(&sr.id),
                edge_type: EdgeType::ReferenceForTask,
                timestamp: now,
                metadata: serde_json::json!({"reason": "auto_cluster_seed"}),
            });
        }
        self.store.upsert_edges(&edges_to_add).await?;

        tracing::info!(
            "Auto-created task cluster '{}' ({} memories)",
            title,
            edges_to_add.len()
        );
        Ok(())
    }

    pub async fn related_urls_for_task(&self, task_id: &str) -> Vec<String> {
        let nodes = match self.store.get_all_nodes().await {
            Ok(nodes) => nodes,
            Err(_) => return Vec::new(),
        };
        let edges = match self.store.get_all_edges().await {
            Ok(edges) => edges,
            Err(_) => return Vec::new(),
        };
        related_urls_for_task_from_snapshot(&nodes, &edges, task_id)
    }

    pub async fn reconstruct(
        &self,
        store: &Store,
        embedder: &Embedder,
        query: &str,
        limit: usize,
    ) -> Result<MemoryReconstruction, Box<dyn std::error::Error>> {
        let results = HybridSearcher::search(store, embedder, query, limit, None, None).await?;
        let nodes = self.store.get_all_nodes().await?;
        let edges = self.store.get_all_edges().await?;

        let cards = self.map_cards(results, &nodes, &edges);
        let structural_context = self.structural_context_for_query(query, &nodes, &edges);

        Ok(MemoryReconstruction {
            answer: String::new(),
            cards,
            structural_context,
        })
    }

    fn structural_context_for_query(
        &self,
        _query: &str,
        nodes: &[GraphNode],
        edges: &[GraphEdge],
    ) -> Vec<String> {
        // Always surface task context — the hybrid search pipeline already
        // ensures that only semantically relevant results reach this point,
        // so hard-coded keyword gates ("task", "todo", etc.) are unnecessary.
        let mut task_nodes: Vec<&GraphNode> = nodes
            .iter()
            .filter(|node| node.node_type == NodeType::Task)
            .collect();
        task_nodes.sort_by_key(|node| std::cmp::Reverse(node.created_at));

        let mut notes = Vec::new();
        for task in task_nodes.into_iter().take(5) {
            let id = task.id.trim_start_matches("task:");
            let urls = related_urls_for_task_from_snapshot(nodes, edges, id);
            if urls.is_empty() {
                notes.push(format!("Task '{}': no linked URL context", task.label));
            } else {
                notes.push(format!(
                    "Task '{}': linked URLs {}",
                    task.label,
                    urls.join(", ")
                ));
            }
        }
        notes
    }

    fn map_cards(
        &self,
        results: Vec<crate::store::SearchResult>,
        nodes: &[GraphNode],
        edges: &[GraphEdge],
    ) -> Vec<MemoryCard> {
        let memory_to_tasks = task_edges_by_memory(nodes, edges);

        results
            .into_iter()
            .map(|result| {
                let task_titles = memory_to_tasks
                    .get(&memory_node_id(&result.id))
                    .cloned()
                    .unwrap_or_default();
                MemoryCard {
                    id: result.id,
                    timestamp: result.timestamp,
                    app_name: result.app_name,
                    window_title: result.window_title,
                    snippet: result.snippet,
                    url: result.url,
                    screenshot_path: result.screenshot_path,
                    score: result.score,
                    related_tasks: task_titles,
                }
            })
            .collect()
    }

    /// Export all nodes and edges for frontend visualization.
    pub async fn export_for_visualization(&self) -> (Vec<GraphNode>, Vec<GraphEdge>) {
        let nodes = match self.store.get_all_nodes().await {
            Ok(nodes) => nodes,
            Err(_) => return (Vec::new(), Vec::new()),
        };
        let edges = match self.store.get_all_edges().await {
            Ok(edges) => edges,
            Err(_) => return (Vec::new(), Vec::new()),
        };
        (nodes, edges)
    }

    /// Clear all graph data is managed via store reset/deletion in this version.
    pub async fn clear_all(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Implementation depends on Store's ability to truncate specific tables.
        // For now, we omit individual clear in favor of consolidated store management.
        Ok(())
    }
}

fn trim_label(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut out = value
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    out.push_str("...");
    out
}

/// Produce a compact, human-readable graph-node label from a `MemoryRecord`.
/// Prefers `topic` when meaningful, falls back to the first salient span of
/// `memory_context`, then to a trimmed snippet. The full memory_context is
/// still carried in node metadata for retrieval/reopen.
pub fn compress_node_label(record: &MemoryRecord) -> String {
    let topic = record.topic.trim();
    if !topic.is_empty()
        && !topic.eq_ignore_ascii_case("unknown")
        && topic.chars().count() <= 80
    {
        return topic.to_string();
    }
    let context = record.memory_context.trim();
    if !context.is_empty() {
        let head = context
            .split("\n\n")
            .next()
            .unwrap_or(context)
            .lines()
            .next()
            .unwrap_or(context)
            .trim();
        let words: Vec<&str> = head.split_whitespace().take(12).collect();
        if !words.is_empty() {
            let mut joined = words.join(" ");
            if joined.chars().count() > 90 {
                joined = joined.chars().take(87).collect::<String>();
                joined.push_str("...");
            }
            return joined;
        }
    }
    trim_label(&record.snippet, 90)
}

fn memory_node_id(memory_id: &str) -> String {
    format!("memory:{memory_id}")
}

fn session_node_id(session_id: &str) -> String {
    format!("session:{session_id}")
}

fn task_node_id(task_id: &str) -> String {
    format!("task:{task_id}")
}

fn url_node_id(url: &str) -> String {
    format!("url:{}", url.to_lowercase())
}

fn host_from_url(url: &str) -> String {
    let trimmed = url
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    trimmed.split('/').next().unwrap_or(trimmed).to_string()
}

fn classify_memory_type(app_name: &str, url: Option<&str>, summary_source: &str) -> &'static str {
    let app = app_name.to_lowercase();
    if url.is_some() || is_browser_app(&app) {
        return "web";
    }

    if summary_source.eq_ignore_ascii_case("vlm") {
        return "visual";
    }

    "general"
}

fn is_browser_app(app_name: &str) -> bool {
    app_name.contains("safari")
        || app_name.contains("chrome")
        || app_name.contains("arc")
        || app_name.contains("brave")
        || app_name.contains("edge")
        || app_name.contains("firefox")
}

fn task_edges_by_memory(nodes: &[GraphNode], edges: &[GraphEdge]) -> HashMap<String, Vec<String>> {
    let node_map: HashMap<&str, &GraphNode> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut mapping: HashMap<String, Vec<String>> = HashMap::new();

    for edge in edges {
        if edge.edge_type != EdgeType::ReferenceForTask {
            continue;
        }
        let Some(source) = node_map.get(edge.source.as_str()) else {
            continue;
        };
        let Some(target) = node_map.get(edge.target.as_str()) else {
            continue;
        };
        if source.node_type != NodeType::Task || target.node_type != NodeType::MemoryChunk {
            continue;
        }

        mapping
            .entry(target.id.clone())
            .or_default()
            .push(source.label.clone());
    }

    for titles in mapping.values_mut() {
        *titles = unique_keep_order(std::mem::take(titles));
    }

    mapping
}

fn related_urls_for_task_from_snapshot(
    nodes: &[GraphNode],
    edges: &[GraphEdge],
    task_id: &str,
) -> Vec<String> {
    let task_node = task_node_id(task_id);
    let node_map: HashMap<&str, &GraphNode> =
        nodes.iter().map(|node| (node.id.as_str(), node)).collect();

    let mut memory_targets = Vec::new();
    let mut urls = Vec::new();

    for edge in edges {
        if edge.source != task_node || edge.edge_type != EdgeType::ReferenceForTask {
            continue;
        }
        if let Some(target) = node_map.get(edge.target.as_str()) {
            match target.node_type {
                NodeType::Url => urls.push(target.label.clone()),
                NodeType::MemoryChunk => memory_targets.push(target.id.clone()),
                _ => {}
            }
        }
    }

    for memory_id in memory_targets {
        for edge in edges {
            if edge.source == memory_id && edge.edge_type == EdgeType::OccurredAt {
                if let Some(target) = node_map.get(edge.target.as_str()) {
                    if target.node_type == NodeType::Url {
                        urls.push(target.label.clone());
                    }
                }
            }
        }
    }

    unique_keep_order(urls)
}

/// Derive a human-readable task title from a memory record's URL + window title.
fn infer_task_title(record: &MemoryRecord) -> String {
    if let Some(url) = &record.url {
        let host = host_from_url(url);
        let hint: String = record
            .window_title
            .splitn(2, &['-', '|', '·', '—'][..])
            .next()
            .unwrap_or(&record.window_title)
            .trim()
            .chars()
            .take(48)
            .collect();
        if hint.is_empty() || hint == host {
            return host;
        }
        return format!("{host}: {hint}");
    }
    format!(
        "{}: {}",
        record.app_name,
        record.window_title.chars().take(60).collect::<String>()
    )
}

/// Deterministic slug for use as a node ID — keeps task nodes idempotent.
fn task_slug(s: &str) -> String {
    let slug: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    slug.chars().take(64).collect()
}

fn unique_keep_order(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for value in values {
        if seen.insert(value.clone()) {
            output.push(value);
        }
    }
    output
}

#[cfg(test)]
mod compress_node_label_tests {
    use super::compress_node_label;
    use crate::store::MemoryRecord;

    #[test]
    fn prefers_non_unknown_topic_within_length_cap() {
        let mut r = MemoryRecord::default();
        r.topic = "Search quality tuning".to_string();
        r.memory_context = "Ignored long context.".to_string();
        r.snippet = "Fallback snippet text.".to_string();
        assert_eq!(compress_node_label(&r), "Search quality tuning");
    }

    #[test]
    fn long_topic_falls_back_to_memory_context_head() {
        let mut r = MemoryRecord::default();
        r.topic = "x".repeat(81);
        r.memory_context =
            "First line introduces the durable memory context.\n\nSecond paragraph.".to_string();
        r.snippet = "Snippet only if context empty.".to_string();
        assert_eq!(
            compress_node_label(&r),
            "First line introduces the durable memory context."
        );
    }

    #[test]
    fn memory_context_head_truncates_very_long_first_line() {
        let mut r = MemoryRecord::default();
        r.topic = "unknown".to_string();
        // First 12 words must exceed 90 chars so `compress_node_label` adds "...".
        let chunk = "abcdefghij"; // 10 chars × 12 + 11 spaces > 90
        let words: String = std::iter::repeat(chunk).take(12).collect::<Vec<_>>().join(" ");
        r.memory_context = format!("{words}\nmore");
        let label = compress_node_label(&r);
        assert!(
            label.ends_with("..."),
            "expected ellipsis truncation, got {label:?}"
        );
        assert!(label.chars().count() <= 90, "label too long: {}", label.len());
    }

    #[test]
    fn falls_back_to_snippet_when_topic_and_context_missing() {
        let mut r = MemoryRecord::default();
        r.topic = "unknown".to_string();
        r.memory_context = String::new();
        r.snippet = "short".to_string();
        assert_eq!(compress_node_label(&r), "short");
    }
}
