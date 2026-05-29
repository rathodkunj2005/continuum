//! Tauri commands for the insight Lance graph (`graph_nodes` / `graph_edges`).

use std::sync::Arc;

use serde::Serialize;
use tauri::State;
use uuid::Uuid;

use crate::graph::community::{attach_louvain_metadata, cluster_0_display_name, louvain_partition};
use crate::graph::graph_store::GraphStore;
use crate::graph::pathfinding::find_path;
use crate::graph::schema::{GraphNode, GraphSubgraph};
use crate::graph::traversal::god_nodes;
use crate::memory_embedding_document::{
    annotate_graph_node_embedding, compose_graph_node_embedding_text, EmbeddingStatus,
};
use crate::telemetry::runtime_metrics;
use crate::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct GraphPathDto {
    pub nodes: Vec<Uuid>,
    pub avg_confidence: f32,
}

#[tauri::command]
pub async fn get_graph_for_project(
    state: State<'_, Arc<AppState>>,
    project_label: String,
) -> Result<GraphSubgraph, String> {
    let gs = GraphStore::new(state.store.clone());
    let mut sub = gs
        .get_project_subgraph(&project_label)
        .await
        .map_err(|e| e.to_string())?;
    attach_louvain_metadata(&mut sub);
    Ok(sub)
}

#[tauri::command]
pub async fn get_full_graph(state: State<'_, Arc<AppState>>) -> Result<GraphSubgraph, String> {
    let gs = GraphStore::new(state.store.clone());
    let nodes = gs.all_nodes().await.map_err(|e| e.to_string())?;
    let edges = gs.all_edges().await.map_err(|e| e.to_string())?;
    let mut sub = GraphSubgraph {
        nodes,
        edges,
        ..Default::default()
    };
    attach_louvain_metadata(&mut sub);
    Ok(sub)
}

#[tauri::command]
pub async fn search_graph(
    state: State<'_, Arc<AppState>>,
    query_embedding: Vec<f32>,
    k: usize,
) -> Result<Vec<GraphNode>, String> {
    let gs = GraphStore::new(state.store.clone());
    gs.search_nodes(&query_embedding, k)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_node_detail(
    state: State<'_, Arc<AppState>>,
    id: Uuid,
) -> Result<Option<GraphNode>, String> {
    let gs = GraphStore::new(state.store.clone());
    let nodes = gs.all_nodes().await.map_err(|e| e.to_string())?;
    Ok(nodes.into_iter().find(|n| n.id == id))
}

#[tauri::command]
pub async fn find_graph_path(
    state: State<'_, Arc<AppState>>,
    from: Uuid,
    to: Uuid,
) -> Result<Option<GraphPathDto>, String> {
    let gs = GraphStore::new(state.store.clone());
    let nodes = gs.all_nodes().await.map_err(|e| e.to_string())?;
    let edges = gs.all_edges().await.map_err(|e| e.to_string())?;
    let sub = GraphSubgraph {
        nodes,
        edges,
        ..Default::default()
    };
    Ok(
        find_path(&sub, from, to).map(|(nodes, avg_confidence)| GraphPathDto {
            nodes,
            avg_confidence,
        }),
    )
}

#[derive(Debug, Clone, Serialize)]
pub struct GodNodesResponse {
    pub nodes: Vec<(Uuid, f32)>,
    pub louvain: std::collections::HashMap<Uuid, usize>,
    pub cluster_0_name: String,
}

#[tauri::command]
pub async fn get_god_nodes(
    state: State<'_, Arc<AppState>>,
    k: usize,
) -> Result<GodNodesResponse, String> {
    let gs = GraphStore::new(state.store.clone());
    let nodes = gs.all_nodes().await.map_err(|e| e.to_string())?;
    let edges = gs.all_edges().await.map_err(|e| e.to_string())?;
    let sub = GraphSubgraph {
        nodes,
        edges,
        ..Default::default()
    };
    let ranked = god_nodes(&sub, k.max(1));
    let part = louvain_partition(&sub);
    let c0 = cluster_0_display_name(&sub, &part);
    Ok(GodNodesResponse {
        nodes: ranked,
        louvain: part,
        cluster_0_name: c0,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphBackfillReport {
    pub scanned: usize,
    pub queued: usize,
    pub low_confidence_queued: usize,
}

async fn commit_graph_updates_internal(
    state: Arc<AppState>,
    bypass_resource_gate: bool,
) -> Result<(), String> {
    if !bypass_resource_gate && !crate::system_resources::allows_graph_idle_commit(state.as_ref()) {
        return Ok(());
    }
    let pending: Vec<crate::PendingGraphUpdate> = {
        let mut q = state.pending_graph_updates.lock();
        std::mem::take(&mut *q)
    };
    let low_confidence: Vec<crate::PendingGraphUpdate> = {
        let mut q = state.low_confidence_graph_candidates.lock();
        std::mem::take(&mut *q)
    };
    if pending.is_empty() && low_confidence.is_empty() {
        return Ok(());
    }
    let t0 = std::time::Instant::now();
    let gs = GraphStore::new(state.store.clone());
    let graph_embedder = super::common::shared_embedder().ok();
    let mut merged_nodes = 0usize;
    let mut merged_edges = 0usize;
    let mut conflicts = 0usize;
    let mut low_conf_nodes = 0usize;
    let mut low_conf_edges = 0usize;

    for batch in pending.iter().chain(low_confidence.iter()) {
        let is_low_confidence = batch.overall_confidence < 0.5;
        let source_memory = state
            .store
            .get_memory_by_id(&batch.memory_id)
            .await
            .ok()
            .flatten();
        let node_texts = batch
            .nodes
            .iter()
            .map(|node| compose_graph_node_embedding_text(node, source_memory.as_ref()))
            .collect::<Vec<_>>();
        let node_vectors = graph_embedder.and_then(|embedder| {
            let contexts = node_texts
                .iter()
                .map(|text| {
                    (
                        "Insight graph".to_string(),
                        batch.memory_id.clone(),
                        text.clone(),
                    )
                })
                .collect::<Vec<_>>();
            embedder.embed_batch_with_context(&contexts).ok()
        });
        for (index, n) in batch.nodes.iter().enumerate() {
            let mut node = n.clone();
            let source_text = node_texts.get(index).cloned().unwrap_or_default();
            if let Some(vector) = node_vectors.as_ref().and_then(|vectors| vectors.get(index)) {
                node.embedding = Some(vector.clone());
                annotate_graph_node_embedding(
                    &mut node,
                    EmbeddingStatus::Ready,
                    &source_text,
                    None,
                );
            } else {
                annotate_graph_node_embedding(
                    &mut node,
                    EmbeddingStatus::Unavailable,
                    &source_text,
                    Some("text embedder unavailable during graph idle commit".to_string()),
                );
            }
            if is_low_confidence {
                low_conf_nodes += 1;
                if let Some(obj) = node.metadata.as_object_mut() {
                    obj.insert("low_confidence".to_string(), serde_json::json!(true));
                } else {
                    node.metadata = serde_json::json!({ "low_confidence": true });
                }
            }
            gs.upsert_node(&node).await.map_err(|e| e.to_string())?;
            merged_nodes += 1;
        }
        for e in &batch.edges {
            if is_low_confidence {
                low_conf_edges += 1;
            }
            if e.conflict_flag {
                conflicts += 1;
            }
            gs.upsert_edge(e).await.map_err(|e| e.to_string())?;
            merged_edges += 1;
        }
    }
    let stale = gs.mark_stale(30).await.map_err(|e| e.to_string())?;
    let elapsed_ms = t0.elapsed().as_millis() as u64;
    runtime_metrics::record_ms("graph.commit_ms", elapsed_ms);
    tracing::info!(
        target: "fndr::graph_commit",
        merged_nodes,
        merged_edges,
        low_conf_nodes,
        low_conf_edges,
        conflicts,
        stale_marked = stale,
        elapsed_ms,
        "commit_graph_updates completed"
    );
    Ok(())
}

/// Drain queued graph updates when resource gating allows.
pub async fn commit_graph_updates(state: Arc<AppState>) -> Result<(), String> {
    commit_graph_updates_internal(state, false).await
}

/// Immediate graph commit path for post-capture freshness.
pub async fn commit_graph_updates_now(state: Arc<AppState>) -> Result<(), String> {
    commit_graph_updates_internal(state, true).await
}

#[tauri::command]
pub async fn backfill_graph_from_existing_memories(
    state: State<'_, Arc<AppState>>,
    limit: Option<usize>,
) -> Result<GraphBackfillReport, String> {
    let all = state
        .inner()
        .store
        .list_all_memories()
        .await
        .map_err(|e| e.to_string())?;
    let take_n = limit.unwrap_or(2500).max(1);
    let scanned = all.len().min(take_n);
    for record in all.iter().take(take_n) {
        state.inner().enqueue_graph_from_flushed_memory(record);
    }
    let queued = state.inner().pending_graph_updates.lock().len();
    let low_confidence_queued = state.inner().low_confidence_graph_candidates.lock().len();
    commit_graph_updates_internal(state.inner().clone(), true).await?;
    Ok(GraphBackfillReport {
        scanned,
        queued,
        low_confidence_queued,
    })
}
