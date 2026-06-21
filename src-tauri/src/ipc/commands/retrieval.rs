//! Phase 4 Tauri commands for the agentic-graph-rag pipeline.
//!
//! Each command is a thin wrapper around `context_runtime::run_query` or an
//! existing handler that the new `continuum.*` namespace re-exposes.

use crate::context_runtime::context_pack::ComposedAnswer;
use crate::context_runtime::{run_query, ComposeMode};
use crate::search::MemoryCard;
use crate::AppState;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::Arc;
use tauri::State;

const DEFAULT_LIMIT: usize = 12;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ContinuumSearchResponse {
    pub query: String,
    pub cards: Vec<MemoryCard>,
}

#[tauri::command]
pub async fn continuum_search(
    state: State<'_, Arc<AppState>>,
    query: String,
    limit: Option<usize>,
) -> Result<ContinuumSearchResponse, String> {
    let answer = run_query(
        state.inner(),
        &query,
        limit.unwrap_or(DEFAULT_LIMIT),
        ComposeMode::Cards,
    )
    .await?;
    crate::telemetry::runtime_metrics::bump("continuum.mcp.search.calls");
    Ok(ContinuumSearchResponse {
        query: answer.query,
        cards: answer.cards,
    })
}

#[tauri::command]
pub async fn continuum_answer(
    state: State<'_, Arc<AppState>>,
    query: String,
    limit: Option<usize>,
) -> Result<ComposedAnswer, String> {
    let result = run_query(
        state.inner(),
        &query,
        limit.unwrap_or(DEFAULT_LIMIT),
        ComposeMode::Answer,
    )
    .await;
    crate::telemetry::runtime_metrics::bump("continuum.mcp.answer.calls");
    result
}

#[tauri::command]
pub async fn continuum_build_context_pack(
    state: State<'_, Arc<AppState>>,
    query: String,
    session_id: Option<String>,
    project: Option<String>,
    budget_tokens: Option<u32>,
) -> Result<crate::storage::ContextPack, String> {
    let request = crate::context_runtime::ContextRequest {
        query,
        session_id,
        project,
        agent_type: String::new(),
        active_files: Vec::new(),
        budget_tokens: budget_tokens.unwrap_or(0),
    };
    crate::telemetry::runtime_metrics::bump("continuum.mcp.build_context_pack.calls");
    crate::context_runtime::build_context_pack(state.inner(), request).await
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ContinuumSubgraphResponse {
    pub seed_ids: Vec<String>,
    pub node_count: usize,
    pub edge_count: usize,
}

#[tauri::command]
pub async fn continuum_get_memory_subgraph(
    _state: State<'_, Arc<AppState>>,
    seed_ids: Vec<String>,
    _max_hops: Option<u8>,
) -> Result<ContinuumSubgraphResponse, String> {
    // The typed insight-graph table is not yet persisted (see context_runtime
    // run_query notes). Until it lands, this command returns the bounded
    // descriptor so the UI can show "no graph yet" without erroring.
    crate::telemetry::runtime_metrics::bump("continuum.mcp.get_memory_subgraph.calls");
    Ok(ContinuumSubgraphResponse {
        seed_ids,
        node_count: 0,
        edge_count: 0,
    })
}

#[tauri::command]
pub async fn continuum_get_related_memories(
    state: State<'_, Arc<AppState>>,
    memory_id: String,
    limit: Option<usize>,
) -> Result<Vec<MemoryCard>, String> {
    let Some(record) = state
        .store
        .get_memory_by_id(&memory_id)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(Vec::new());
    };
    crate::telemetry::runtime_metrics::bump("continuum.mcp.get_related_memories.calls");
    let answer = run_query(
        state.inner(),
        &record.text,
        limit.unwrap_or(8),
        ComposeMode::Cards,
    )
    .await?;
    Ok(answer
        .cards
        .into_iter()
        .filter(|c| c.id != memory_id)
        .collect())
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Default)]
pub struct ContinuumQualityStatus {
    pub stored_count: u64,
    pub dropped_count: u64,
    pub flagged_count: u64,
}

#[tauri::command]
pub async fn continuum_quality_status(
    state: State<'_, Arc<AppState>>,
) -> Result<ContinuumQualityStatus, String> {
    crate::telemetry::runtime_metrics::bump("continuum.mcp.quality_status.calls");
    Ok(ContinuumQualityStatus {
        stored_count: state.capture_stats.total_stored(),
        dropped_count: state
            .frames_dropped
            .load(std::sync::atomic::Ordering::Relaxed),
        flagged_count: 0,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ContinuumTimelineEntry {
    pub memory_id: String,
    pub timestamp: i64,
    pub snippet: String,
}

#[tauri::command]
pub async fn continuum_timeline(
    state: State<'_, Arc<AppState>>,
    limit: Option<usize>,
    project: Option<String>,
) -> Result<Vec<ContinuumTimelineEntry>, String> {
    crate::telemetry::runtime_metrics::bump("continuum.mcp.timeline.calls");
    let events = state
        .store
        .list_activity_events(limit.unwrap_or(20), project.as_deref())
        .await
        .map_err(|e| e.to_string())?;
    Ok(events
        .into_iter()
        .map(|e| ContinuumTimelineEntry {
            memory_id: e.memory_id,
            timestamp: e.end_time,
            snippet: e.title,
        })
        .collect())
}
