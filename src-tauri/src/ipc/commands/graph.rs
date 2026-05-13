//! Tauri commands for the insight Lance graph (`graph_nodes` / `graph_edges`).

use std::sync::Arc;

use serde::Serialize;
use tauri::State;
use uuid::Uuid;

use crate::memory::graph::clusters::{attach_louvain_metadata, cluster_0_display_name, louvain_partition};
use crate::memory::graph::schema::{GraphNode, GraphSubgraph};
use crate::memory::graph::traversal::{find_path, god_nodes};
use crate::storage::graph_store::GraphStore;
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
    Ok(find_path(&sub, from, to).map(|(nodes, avg_confidence)| GraphPathDto {
        nodes,
        avg_confidence,
    }))
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

/// Drain `pending_graph_updates` into Lance when resource gating allows.
pub async fn commit_graph_updates(state: Arc<AppState>) -> Result<(), String> {
    if !crate::system_resources::allows_graph_idle_commit(state.as_ref()) {
        return Ok(());
    }
    let pending: Vec<crate::PendingGraphUpdate> = {
        let mut q = state.pending_graph_updates.lock();
        std::mem::take(&mut *q)
    };
    if pending.is_empty() {
        return Ok(());
    }
    let t0 = std::time::Instant::now();
    let gs = GraphStore::new(state.store.clone());
    let mut merged_nodes = 0usize;
    let mut merged_edges = 0usize;
    let mut conflicts = 0usize;
    for batch in pending {
        for n in &batch.nodes {
            gs.upsert_node(n).await.map_err(|e| e.to_string())?;
            merged_nodes += 1;
        }
        for e in &batch.edges {
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
        conflicts,
        stale_marked = stale,
        elapsed_ms,
        "commit_graph_updates completed"
    );
    Ok(())
}
