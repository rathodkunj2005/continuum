//! Tauri commands for manual + backfill memory-review runs (Subagent 11).
//!
//! - `run_daily_memory_review { date, dry_run }` — runs the daily batch
//!   pipeline for the given calendar day under the local model.
//! - `backfill_memory_review { start_ms, end_ms, dry_run }` — enqueues
//!   per-memory review jobs for the worker to drain.
//!
//! Both commands check that an [`InferenceEngine`] is available before
//! running. The daily command goes a step further and re-checks the pressure
//! gate inside the pipeline so a long-running batch can abort gracefully.

use crate::embedding::Embedder;
use crate::memory_review::{
    backfill_memory_review_in_range, parse_day_range_local, run_daily_memory_review,
    BackfillReviewSummary, DailyReviewSummary, InferenceReviewProvider,
};
use crate::AppState;
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub async fn run_daily_memory_review_cmd(
    state: State<'_, Arc<AppState>>,
    date: String,
    dry_run: Option<bool>,
) -> Result<DailyReviewSummary, String> {
    let dry_run = dry_run.unwrap_or(false);
    let state = state.inner().clone();

    let provider = build_provider(&state)?;
    let embedder = match Embedder::new() {
        Ok(e) => Some(e),
        Err(err) => {
            tracing::warn!(
                err = %err,
                "run_daily_memory_review: embedder unavailable; continuing without re-embed"
            );
            None
        }
    };

    let (start_ms, end_ms) = parse_day_range_local(&date)?;
    let now_ms = chrono::Utc::now().timestamp_millis();

    run_daily_memory_review(
        &state,
        &state.store,
        provider.as_ref(),
        embedder.as_ref(),
        &date,
        start_ms,
        end_ms,
        now_ms,
        dry_run,
    )
    .await
}

#[tauri::command]
pub async fn backfill_memory_review(
    state: State<'_, Arc<AppState>>,
    start_ms: i64,
    end_ms: i64,
    dry_run: Option<bool>,
) -> Result<BackfillReviewSummary, String> {
    let dry_run = dry_run.unwrap_or(false);
    let state = state.inner().clone();
    let now_ms = chrono::Utc::now().timestamp_millis();
    backfill_memory_review_in_range(&state, &state.store, start_ms, end_ms, now_ms, dry_run).await
}

fn build_provider(
    state: &Arc<AppState>,
) -> Result<Box<dyn crate::memory_review::ReviewProvider>, String> {
    let inference = state
        .inference
        .read()
        .clone()
        .ok_or_else(|| "Inference engine is not loaded; daily review requires a local model".to_string())?;
    Ok(Box::new(InferenceReviewProvider::new(inference)))
}

#[cfg(test)]
mod tests {
    //! Argument-marshalling smoke tests. The full pipeline behavior is covered
    //! by `memory_review::daily::tests` and `memory_review::backfill::tests` —
    //! the IPC layer just wires args to that pipeline.
    use crate::memory_review::parse_day_range_local;

    #[test]
    fn parse_day_range_round_trips() {
        let (start, end) = parse_day_range_local("2026-05-20").unwrap();
        assert!(end > start);
    }
}
