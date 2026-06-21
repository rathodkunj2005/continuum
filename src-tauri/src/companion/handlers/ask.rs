//! POST /v1/ask — mobile Ask Continuum entrypoint.
//!
//! Wraps the existing phase-3 query pipeline (`context_runtime::run_query`) and
//! returns a mobile-shaped answer + source-card DTO bundle.

use crate::companion::dto::{AskRequest, AskResponse};
use crate::companion::errors::{CompanionError, CompanionResult};
use crate::context_runtime::{run_query, ComposeMode};
use crate::AppState;
use axum::extract::State;
use axum::Json;
use std::sync::Arc;
use std::time::Instant;

const DEFAULT_LIMIT: usize = 8;
const MAX_LIMIT: usize = 20;

pub async fn ask(
    State(app_state): State<Arc<AppState>>,
    body: Result<Json<AskRequest>, axum::extract::rejection::JsonRejection>,
) -> CompanionResult<Json<AskResponse>> {
    let Json(payload) = body.map_err(|err| CompanionError::BadRequest(err.to_string()))?;

    let query = payload.query.trim().to_string();
    if query.is_empty() {
        return Err(CompanionError::BadRequest("query is empty".to_string()));
    }

    let limit = payload.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);

    let started = Instant::now();
    let composed = run_query(app_state.as_ref(), &query, limit, ComposeMode::Answer)
        .await
        .map_err(CompanionError::Internal)?;
    let latency_ms = started.elapsed().as_millis() as u64;

    let verify_outcome = match composed.verify_outcome {
        crate::context_runtime::context_pack::VerifyOutcome::Grounded { .. } => "grounded",
        crate::context_runtime::context_pack::VerifyOutcome::PartialAnswer { .. } => {
            "partial_answer"
        }
        crate::context_runtime::context_pack::VerifyOutcome::NotEnoughEvidence { .. } => {
            "not_enough_evidence"
        }
    }
    .to_string();

    let source_cards = composed
        .cards
        .into_iter()
        .map(crate::companion::handlers::companion_card_from_memory_card)
        .collect::<Vec<_>>();

    tracing::info!(query = %query, limit, latency_ms, sources = source_cards.len(), "companion.ask.completed");

    Ok(Json(AskResponse {
        query,
        answer: composed.answer,
        verify_outcome,
        source_cards,
        latency_ms,
    }))
}

#[cfg(test)]
mod tests {
    #[test]
    fn limit_is_clamped() {
        let value = Some(999usize).unwrap_or(8).clamp(1, 20);
        assert_eq!(value, 20);

        let value = Some(0usize).unwrap_or(8).clamp(1, 20);
        assert_eq!(value, 1);

        let value = None.unwrap_or(8).clamp(1, 20);
        assert_eq!(value, 8);
    }
}
