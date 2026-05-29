//! POST /v1/memories/search — mobile memory search endpoint.
//!
//! Uses the same hybrid retrieval boundary as desktop search to keep ranking
//! and filtering semantics aligned with "Mac as the brain".

use crate::companion::dto::{MemorySearchRequest, MemorySearchResponse};
use crate::companion::errors::{CompanionError, CompanionResult};
use crate::embedding::Embedder;
use crate::search::{HybridSearcher, MemoryCardSynthesizer};
use crate::AppState;
use axum::extract::State;
use axum::Json;
use std::sync::Arc;
use std::time::Instant;

const DEFAULT_LIMIT: usize = 12;
const MAX_LIMIT: usize = 40;

pub async fn search_memories(
    State(app_state): State<Arc<AppState>>,
    body: Result<Json<MemorySearchRequest>, axum::extract::rejection::JsonRejection>,
) -> CompanionResult<Json<MemorySearchResponse>> {
    let Json(payload) = body.map_err(|err| CompanionError::BadRequest(err.to_string()))?;

    let query = payload.query.trim().to_string();
    if query.is_empty() {
        return Err(CompanionError::BadRequest("query is empty".to_string()));
    }

    let limit = payload.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let app_filter = normalized_filter(payload.app_filter.as_deref());
    let project_filter = normalized_filter(payload.project_filter.as_deref());
    let time_filter = normalized_filter(payload.time_filter.as_deref());

    let started = Instant::now();

    let embedder = Embedder::new()
        .map_err(|e| CompanionError::Internal(format!("embedder init failed: {e}")))?;
    let search_config = app_state.config.read().search.clone().normalized();

    let mut results = HybridSearcher::search_hybrid_memories(
        &app_state.store,
        &embedder,
        &query,
        limit,
        time_filter.as_deref(),
        app_filter.as_deref(),
        &search_config,
    )
    .await
    .map_err(|e| CompanionError::Internal(format!("search_hybrid_memories failed: {e}")))?;

    if let Some(project) = project_filter.as_deref() {
        results.retain(|row| row.project.trim().eq_ignore_ascii_case(project));
    }

    let cards = MemoryCardSynthesizer::deterministic_from_results(&query, &results, limit)
        .into_iter()
        .map(crate::companion::handlers::companion_card_from_memory_card)
        .collect::<Vec<_>>();

    let latency_ms = started.elapsed().as_millis() as u64;

    tracing::info!(
        query = %query,
        limit,
        cards = cards.len(),
        latency_ms,
        "companion.search.completed"
    );

    Ok(Json(MemorySearchResponse {
        query,
        total: cards.len(),
        cards,
        latency_ms,
    }))
}

fn normalized_filter(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_filter_strips_whitespace_and_empty_values() {
        assert_eq!(normalized_filter(None), None);
        assert_eq!(normalized_filter(Some("   ")), None);
        assert_eq!(
            normalized_filter(Some("  project-fndr  ")),
            Some("project-fndr".to_string())
        );
    }
}
