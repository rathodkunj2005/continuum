use continuum_lib::config::DEFAULT_IMAGE_EMBEDDING_DIM;
use continuum_lib::embedding::EMBEDDING_DIM;
use continuum_lib::graph::GraphStore;
use continuum_lib::storage::{
    EdgeType, GraphEdge, GraphNode, MemoryRecord, NodeType, Store, Task, TaskType,
};
use std::collections::HashSet;
use std::sync::Arc;

fn embedding(value: f32) -> Vec<f32> {
    let mut vector = vec![0.0; EMBEDDING_DIM];
    vector[0] = value;
    vector
}

fn record(id: &str, snippet: &str, embedding_value: f32) -> MemoryRecord {
    MemoryRecord {
        id: id.to_string(),
        timestamp: chrono::Utc::now().timestamp_millis(),
        day_bucket: "2026-04-21".to_string(),
        app_name: "Codex".to_string(),
        bundle_id: None,
        window_title: "Regression".to_string(),
        session_id: "session-1".to_string(),
        text: snippet.to_string(),
        clean_text: snippet.to_string(),
        ocr_confidence: 0.99,
        ocr_block_count: 3,
        snippet: snippet.to_string(),
        summary_source: "fallback".to_string(),
        noise_score: 0.0,
        session_key: "codex:regression".to_string(),
        lexical_shadow: snippet.to_string(),
        embedding: embedding(embedding_value),
        image_embedding: vec![0.0; DEFAULT_IMAGE_EMBEDDING_DIM],
        screenshot_path: None,
        url: None,
        snippet_embedding: embedding(embedding_value),
        support_embedding: embedding(embedding_value),
        decay_score: 1.0,
        last_accessed_at: 0,
        // `record_insert_dedup_key` buckets by URL + title + 5m clock; without a unique
        // hash, distinct regression memories in the same bucket collapse to one insert.
        content_hash: format!("store-graph-regression-{id}"),
        ..Default::default()
    }
}

#[test]
fn has_memories_and_targeted_graph_upserts_replace_in_place() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::new(dir.path()).expect("store");
    let rt = tokio::runtime::Runtime::new().expect("runtime");

    rt.block_on(async {
        assert!(!store.has_memories().await.expect("initial has_memories"));

        store
            .add_batch(&[record("mem-1", "Investigated invoice OCR mismatch", 1.0)])
            .await
            .expect("add memory");
        assert!(store
            .has_memories()
            .await
            .expect("has_memories after insert"));

        store
            .upsert_nodes(&[GraphNode {
                id: "task:seed".to_string(),
                node_type: NodeType::Task,
                label: "First".to_string(),
                created_at: 1,
                metadata: serde_json::json!({"version": 1}),
            }])
            .await
            .expect("insert node");
        store
            .upsert_nodes(&[GraphNode {
                id: "task:seed".to_string(),
                node_type: NodeType::Task,
                label: "Second".to_string(),
                created_at: 2,
                metadata: serde_json::json!({"version": 2}),
            }])
            .await
            .expect("replace node");

        let nodes = store.get_all_nodes().await.expect("nodes");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].label, "Second");
        assert_eq!(nodes[0].metadata["version"], 2);

        store
            .upsert_edges(&[GraphEdge {
                id: "edge-1".to_string(),
                source: "task:seed".to_string(),
                target: "memory:mem-1".to_string(),
                edge_type: EdgeType::ReferenceForTask,
                timestamp: 1,
                metadata: serde_json::json!({"version": 1}),
            }])
            .await
            .expect("insert edge");
        store
            .upsert_edges(&[GraphEdge {
                id: "edge-2".to_string(),
                source: "task:seed".to_string(),
                target: "memory:mem-1".to_string(),
                edge_type: EdgeType::ReferenceForTask,
                timestamp: 2,
                metadata: serde_json::json!({"version": 2}),
            }])
            .await
            .expect("replace edge");

        let edges = store.get_all_edges().await.expect("edges");
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].id, "edge-2");
        assert_eq!(edges[0].metadata["version"], 2);
    });
}

#[test]
fn graph_ingest_and_task_link_are_idempotent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(Store::new(dir.path()).expect("store"));
    let graph = GraphStore::new(store.clone());
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let memory = record("mem-1", "Prepared quarterly planning notes", 1.0);

    rt.block_on(async {
        store
            .add_batch(&[memory.clone()])
            .await
            .expect("add memory");
        graph.ingest_memory(&memory).await.expect("ingest memory");
        graph
            .ingest_memory(&memory)
            .await
            .expect("re-ingest memory");

        let task = Task {
            id: "task-1".to_string(),
            title: "Quarterly planning".to_string(),
            description: String::new(),
            source_app: "Codex".to_string(),
            source_memory_id: Some(memory.id.clone()),
            created_at: memory.timestamp,
            due_date: None,
            is_completed: false,
            is_dismissed: false,
            task_type: TaskType::Todo,
            linked_urls: Vec::new(),
            linked_memory_ids: vec![memory.id.clone()],
        };
        graph.link_task(&task).await.expect("link task");
        graph.link_task(&task).await.expect("re-link task");

        let nodes = store.get_all_nodes().await.expect("nodes");
        assert_eq!(
            nodes
                .iter()
                .filter(|node| node.id == "memory:mem-1")
                .count(),
            1
        );
        assert_eq!(
            nodes
                .iter()
                .filter(|node| node.id == "session:session-1")
                .count(),
            1
        );
        assert_eq!(
            nodes.iter().filter(|node| node.id == "task:task-1").count(),
            1
        );

        let edges = store.get_all_edges().await.expect("edges");
        assert_eq!(
            edges
                .iter()
                .filter(|edge| {
                    edge.edge_type == EdgeType::PartOfSession
                        && edge.source == "memory:mem-1"
                        && edge.target == "session:session-1"
                })
                .count(),
            1
        );
        assert_eq!(
            edges
                .iter()
                .filter(|edge| {
                    edge.edge_type == EdgeType::ReferenceForTask
                        && edge.source == "task:task-1"
                        && edge.target == "memory:mem-1"
                })
                .count(),
            1
        );
    });
}

#[test]
fn auto_link_to_task_seeds_cluster_without_duplicates() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(Store::new(dir.path()).expect("store"));
    let graph = GraphStore::new(store.clone());
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let existing = vec![
        record("mem-1", "Reviewed quarterly planning notes", 1.0),
        record("mem-2", "Updated quarterly planning agenda", 1.0),
    ];
    let incoming = record("mem-3", "Finalized quarterly planning checklist", 1.0);

    rt.block_on(async {
        store.add_batch(&existing).await.expect("add cluster peers");
        for memory in &existing {
            graph.ingest_memory(memory).await.expect("ingest peer");
        }

        graph
            .auto_link_to_task(&incoming)
            .await
            .expect("auto link cluster");
        graph
            .auto_link_to_task(&incoming)
            .await
            .expect("auto link cluster again");

        let nodes = store.get_all_nodes().await.expect("nodes");
        assert_eq!(
            nodes
                .iter()
                .filter(|node| node.node_type == NodeType::Task)
                .count(),
            1
        );

        let edges = store.get_all_edges().await.expect("edges");
        let reference_edges = edges
            .iter()
            .filter(|edge| edge.edge_type == EdgeType::ReferenceForTask)
            .collect::<Vec<_>>();
        assert_eq!(reference_edges.len(), 3);

        let targets = reference_edges
            .iter()
            .map(|edge| edge.target.clone())
            .collect::<HashSet<_>>();
        assert_eq!(
            targets,
            HashSet::from([
                "memory:mem-1".to_string(),
                "memory:mem-2".to_string(),
                "memory:mem-3".to_string(),
            ])
        );
    });
}

#[test]
fn auto_link_to_task_joins_existing_task_without_duplicates() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(Store::new(dir.path()).expect("store"));
    let graph = GraphStore::new(store.clone());
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let existing = record("mem-1", "Debugged quarterly planning timeline", 1.0);
    let incoming = record("mem-2", "Refined quarterly planning timeline", 1.0);

    rt.block_on(async {
        store
            .add_batch(&[existing.clone()])
            .await
            .expect("add memory");
        graph.ingest_memory(&existing).await.expect("ingest memory");
        graph
            .link_task(&Task {
                id: "task-join".to_string(),
                title: "Quarterly planning".to_string(),
                description: String::new(),
                source_app: "Codex".to_string(),
                source_memory_id: Some(existing.id.clone()),
                created_at: existing.timestamp,
                due_date: None,
                is_completed: false,
                is_dismissed: false,
                task_type: TaskType::Todo,
                linked_urls: Vec::new(),
                linked_memory_ids: Vec::new(),
            })
            .await
            .expect("link task");

        graph
            .auto_link_to_task(&incoming)
            .await
            .expect("auto link join");
        graph
            .auto_link_to_task(&incoming)
            .await
            .expect("auto link join again");

        let edges = store.get_all_edges().await.expect("edges");
        let reference_edges = edges
            .iter()
            .filter(|edge| edge.edge_type == EdgeType::ReferenceForTask)
            .collect::<Vec<_>>();
        assert_eq!(reference_edges.len(), 2);
        assert!(reference_edges
            .iter()
            .any(|edge| edge.target == "memory:mem-1"));
        assert!(reference_edges
            .iter()
            .any(|edge| edge.target == "memory:mem-2"));
    });
}
