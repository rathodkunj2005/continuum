//! Subagent 9 — post-capture **Memory Review Worker**.
//!
//! Capture writes a fresh `MemoryRecord` to LanceDB with
//! `enrichment_status = "pending"` and a (de-duplicated) `MemoryReviewJob`
//! enqueued on `AppState`. A background worker — pressure-gated and
//! serialized through `AppState::model_pipeline_lock` — drains one job at a
//! time, asks the local inference engine to **review** the record against its
//! bounded evidence (clean_text capped, window_title, url, current
//! memory_context, current display_summary, synthesis_branch, same-day
//! candidates), validates the output against the OCR evidence and the
//! narration filter, regenerates `display_summary`, re-derives the insight
//! columns, recomputes the embedding_text, and writes the upgraded record
//! back.
//!
//! On success the record carries:
//! - `enrichment_status = "reviewed_local"`
//! - `reviewed_at_ms = now`
//! - `reviewer_generation += 1`
//! - `synthesis_branch = "reviewed_local"`
//!
//! On failure the original record content is preserved and the row is marked
//! `enrichment_status = "review_failed"` with the reason logged.
//!
//! The module is structured so an `InferenceEngine`-backed provider plugs in
//! for production while tests can substitute a deterministic stub via the
//! `ReviewProvider` trait — no real LLM, no cloud, no network.

mod backfill;
mod daily;
mod inference_provider;
mod pipeline;
mod queue;
mod worker;

pub use backfill::{backfill_memory_review_in_range, BackfillReviewSummary};
pub use daily::{
    parse_day_range_local, run_daily_memory_review, spawn_daily_scheduler, DailyReviewOutcome,
    DailyReviewSummary, DAILY_REVIEW_TICK_INTERVAL,
};
pub use inference_provider::InferenceReviewProvider;
pub use pipeline::{
    review_one_memory, review_one_memory_with_mode, MemoryReviewOutcome, ReviewError, ReviewInput,
    ReviewProvider, ReviewWriteMode, ReviewedMemory, SameDayCandidate,
};
pub use queue::{MemoryReviewJob, MemoryReviewQueue};
pub use worker::{
    spawn as spawn_worker, status as worker_status, tick_once, DeferReason,
    MemoryReviewWorkerStatus, TickOutcome,
};

use crate::AppState;
use std::sync::atomic::Ordering;

/// Lifecycle status strings. Persisted in `MemoryRecord::enrichment_status`.
pub const STATUS_PENDING: &str = "pending";
pub const STATUS_REVIEWED_LOCAL: &str = "reviewed_local";
pub const STATUS_REVIEWED_DAILY: &str = "reviewed_daily";
pub const STATUS_REVIEW_FAILED: &str = "review_failed";

/// `synthesis_branch` written by a successful local review pass.
pub const SYNTHESIS_BRANCH_REVIEWED_LOCAL: &str = "reviewed_local";
/// `synthesis_branch` written by a successful daily-batch review.
pub const SYNTHESIS_BRANCH_REVIEWED_DAILY: &str = "reviewed_daily";

/// Maximum same-day candidate titles surfaced to the reviewer. Keeps the
/// prompt bounded and the candidate set scannable for the LLM.
pub const MAX_SAME_DAY_CANDIDATES: usize = 12;

/// Cap on `related_memory_ids` returned by a single review pass. Anything
/// beyond this is dropped before write-back.
pub const MAX_RELATED_MEMORY_IDS: usize = 3;

/// Pressure gate for the memory_review worker. Composes the existing battery /
/// CPU / pause gates with an inference-engine availability check so the
/// worker is a no-op until the local model is loaded. The worker also defers
/// when the model pipeline lock is already held by capture or by an explicit
/// IPC call.
pub fn allows_memory_review_worker(state: &AppState) -> bool {
    if state.is_paused.load(Ordering::Relaxed) {
        return false;
    }
    if state.inference.read().is_none() {
        return false;
    }
    crate::system_resources::allows_graph_idle_commit(state)
}
