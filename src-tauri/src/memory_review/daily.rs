//! Daily memory-review batch driver (Subagent 11).
//!
//! Iterates a calendar day's `MemoryRecord`s and runs the same per-memory
//! review pipeline as the post-capture worker — but writes the success status
//! as `reviewed_daily` and supports a dry-run that computes patches without
//! mutating LanceDB. The driver always uses the local model: the
//! [`ReviewProvider`] is supplied by the caller, and the scheduler defers
//! whenever inference isn't loaded or the system-pressure gate is closed.
//!
//! ## Scheduling
//! [`spawn_daily_scheduler`] runs in the background and checks once per hour
//! whether the previous calendar day still has unreviewed memories. It will
//! only run when `allows_memory_review_worker` returns true, so it inherits
//! the pause / inference / pressure gating used by the per-capture worker.
//! Each successful run records the date so the next tick is a no-op until
//! tomorrow.
//!
//! ## Dry run
//! When `dry_run = true`, the driver runs each memory through validation +
//! the narration filter and counts how many records *would have changed*
//! (`would_change`), but never calls `replace_memory_preserving_chunks`.

use crate::embedding::Embedder;
use crate::memory_review::pipeline::{
    review_one_memory_with_mode, MemoryReviewOutcome, ReviewError, ReviewWriteMode,
};
use crate::storage::Store;
use crate::AppState;
use chrono::{Local, NaiveDate, TimeZone};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use super::queue::MemoryReviewJob;
use super::{allows_memory_review_worker, STATUS_REVIEWED_DAILY};

/// Pressure gate for the daily-review *pipeline*. This is a strict subset of
/// [`allows_memory_review_worker`] — it deliberately omits the
/// "inference engine loaded" check because the pipeline takes a
/// `ReviewProvider` by reference; the caller has already chosen which provider
/// to use, and unit tests legitimately drive the pipeline with a deterministic
/// stub.
///
/// Under `cfg(test)` the system-pressure check (`pmset -g batt` / CPU load) is
/// skipped so the test outcome doesn't depend on the host's instantaneous
/// battery / load state. Production builds always go through the full
/// `allows_graph_idle_commit` heuristic.
fn daily_pipeline_gate_open(state: &AppState) -> bool {
    if state.is_paused.load(Ordering::Relaxed) {
        return false;
    }
    #[cfg(test)]
    {
        let _ = state;
        return true;
    }
    #[cfg(not(test))]
    crate::system_resources::allows_graph_idle_commit(state)
}

/// Scheduler tick cadence. We only do real work at most once per calendar day,
/// but we poll hourly to catch the first idle window after midnight.
pub const DAILY_REVIEW_TICK_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// Per-memory result for a daily-review batch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DailyReviewOutcome {
    /// The patch was applied (or in dry-run, would have applied).
    Changed { memory_id: String },
    /// The reviewer ran but its output was rejected by validation — the
    /// original record was preserved.
    InvalidPatch {
        memory_id: String,
        reason: String,
    },
    /// The reviewer call itself failed (e.g. LLM unavailable mid-batch).
    ProviderFailure {
        memory_id: String,
        reason: String,
    },
    /// The record had already been reviewed at or after the batch's window —
    /// skipped to avoid wasting an LLM call.
    AlreadyReviewed { memory_id: String },
}

/// Aggregate stats returned by [`run_daily_memory_review`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DailyReviewSummary {
    /// "YYYY-MM-DD" the run targeted.
    pub day: String,
    /// `[start_ms, end_ms]` the run scanned (inclusive of start, inclusive of end).
    pub start_ms: i64,
    pub end_ms: i64,
    /// Whether the run was a dry run. `true` means no rows were mutated.
    pub dry_run: bool,
    /// Number of records scanned in the day window.
    pub scanned: usize,
    /// Records the run actually patched (always 0 when `dry_run`).
    pub changed: usize,
    /// Records that would have been patched in a non-dry-run.
    pub would_change: usize,
    /// Records whose review surfaced an invalid patch (grounding / narration).
    pub failed: usize,
    /// Records skipped because the pressure gate closed mid-batch.
    pub skipped_pressure: usize,
    /// Records that were already `reviewed_daily` for this generation.
    pub skipped_already_reviewed: usize,
    /// Per-record outcomes, in scan order. Bounded so the IPC payload stays small.
    pub outcomes: Vec<DailyReviewOutcome>,
}

/// Cap on the number of per-record outcomes kept in the summary. The actual
/// pipeline still processes every memory in the window; this only bounds the
/// shape of the IPC payload.
const MAX_OUTCOMES_IN_SUMMARY: usize = 256;

/// Run the daily review batch for `[start_ms, end_ms]`.
///
/// * `state` — used by the pressure gate. When [`allows_memory_review_worker`]
///   returns `false` mid-batch the remaining memories are counted as
///   `skipped_pressure` and the batch returns early.
/// * `mode` — `ReviewedDaily` for production, `DryRun` to skip persistence.
pub async fn run_daily_memory_review(
    state: &AppState,
    store: &Store,
    provider: &dyn super::pipeline::ReviewProvider,
    embedder: Option<&Embedder>,
    day: &str,
    start_ms: i64,
    end_ms: i64,
    now_ms: i64,
    dry_run: bool,
) -> Result<DailyReviewSummary, String> {
    let records = store
        .get_memories_in_range(start_ms, end_ms)
        .await
        .map_err(|err| err.to_string())?;

    let scanned = records.len();
    let mut summary = DailyReviewSummary {
        day: day.to_string(),
        start_ms,
        end_ms,
        dry_run,
        scanned,
        changed: 0,
        would_change: 0,
        failed: 0,
        skipped_pressure: 0,
        skipped_already_reviewed: 0,
        outcomes: Vec::new(),
    };

    if scanned == 0 {
        return Ok(summary);
    }

    let mode = if dry_run {
        ReviewWriteMode::DryRun
    } else {
        ReviewWriteMode::ReviewedDaily
    };

    // We hold the model-pipeline lock for the duration of a single batch so a
    // concurrent capture flush can't fire its own LLM call in parallel. The
    // per-tick worker uses the same lock for the same reason.
    let _guard = state.model_pipeline_lock.lock().await;

    for record in records {
        // Pressure / pause gate — re-checked on every iteration so a long
        // batch doesn't hog the system if conditions change. We use the
        // *pipeline* gate (no inference-availability check) because the
        // caller has already supplied a provider; the scheduler is the one
        // that enforces "inference engine must be loaded".
        if !daily_pipeline_gate_open(state) {
            tracing::info!(
                target: "fndr::memory_review::daily",
                processed = summary.changed + summary.would_change + summary.failed,
                remaining = scanned
                    .saturating_sub(summary.changed + summary.would_change + summary.failed),
                "daily review deferred: pressure gate closed mid-batch"
            );
            summary.skipped_pressure = scanned
                .saturating_sub(summary.changed + summary.would_change + summary.failed);
            return Ok(summary);
        }

        // Skip records already reviewed in this generation. The worker fast
        // path keeps the daily run lightweight on subsequent invocations.
        if record.enrichment_status == STATUS_REVIEWED_DAILY && record.reviewed_at_ms >= start_ms {
            summary.skipped_already_reviewed += 1;
            push_outcome(
                &mut summary.outcomes,
                DailyReviewOutcome::AlreadyReviewed {
                    memory_id: record.id.clone(),
                },
            );
            continue;
        }

        let job = MemoryReviewJob {
            memory_id: record.id.clone(),
            day_bucket: record.day_bucket.clone(),
            enqueued_at_ms: now_ms,
        };

        match review_one_memory_with_mode(store, provider, embedder, &job, now_ms, mode).await {
            Ok(MemoryReviewOutcome::Reviewed { memory_id, .. }) => {
                if dry_run {
                    summary.would_change += 1;
                } else {
                    summary.changed += 1;
                }
                push_outcome(
                    &mut summary.outcomes,
                    DailyReviewOutcome::Changed { memory_id },
                );
            }
            Ok(MemoryReviewOutcome::Failed { memory_id, reason }) => {
                summary.failed += 1;
                push_outcome(
                    &mut summary.outcomes,
                    daily_outcome_from_failure(memory_id, reason),
                );
            }
            Ok(MemoryReviewOutcome::Skipped { memory_id, reason }) => {
                tracing::debug!(
                    memory_id = %memory_id,
                    reason = %reason,
                    "daily review: skipped"
                );
            }
            Err(err) => {
                summary.failed += 1;
                push_outcome(
                    &mut summary.outcomes,
                    DailyReviewOutcome::ProviderFailure {
                        memory_id: record.id.clone(),
                        reason: err,
                    },
                );
            }
        }
    }

    Ok(summary)
}

fn push_outcome(out: &mut Vec<DailyReviewOutcome>, outcome: DailyReviewOutcome) {
    if out.len() < MAX_OUTCOMES_IN_SUMMARY {
        out.push(outcome);
    }
}

fn daily_outcome_from_failure(memory_id: String, reason: ReviewError) -> DailyReviewOutcome {
    match reason {
        ReviewError::ProviderError(reason) => DailyReviewOutcome::ProviderFailure {
            memory_id,
            reason,
        },
        other => DailyReviewOutcome::InvalidPatch {
            memory_id,
            reason: other.label().to_string(),
        },
    }
}

/// Convert a "YYYY-MM-DD" string into `[start_ms, end_ms]` for the local
/// calendar day. Errors when the input doesn't parse; the IPC layer surfaces
/// the error verbatim. `end_ms` is the last millisecond of the day so the
/// range filter is inclusive on both ends.
pub fn parse_day_range_local(day: &str) -> Result<(i64, i64), String> {
    let parsed = NaiveDate::parse_from_str(day.trim(), "%Y-%m-%d")
        .map_err(|err| format!("invalid date '{day}': {err}"))?;
    let start = Local
        .from_local_datetime(&parsed.and_hms_opt(0, 0, 0).expect("00:00:00 is valid"))
        .earliest()
        .ok_or_else(|| format!("could not localize {day} (DST gap?)"))?
        .timestamp_millis();
    let end_naive = parsed
        .and_hms_milli_opt(23, 59, 59, 999)
        .expect("23:59:59.999 is valid");
    let end = Local
        .from_local_datetime(&end_naive)
        .earliest()
        .ok_or_else(|| format!("could not localize {day} end (DST gap?)"))?
        .timestamp_millis();
    Ok((start, end))
}

/// Spawn the daily-review scheduler. The task wakes hourly, checks whether
/// today's run has already been done, and — when the pressure gate allows —
/// runs the previous day's review batch under the local model.
///
/// `provider_factory` returns a fresh [`ReviewProvider`] per tick so the
/// scheduler doesn't hold a reference to the inference engine while the gate
/// is closed.
pub fn spawn_daily_scheduler(state: Arc<AppState>) {
    static LAST_RUN_DAY: Mutex<Option<String>> = Mutex::new(None);

    tauri::async_runtime::spawn(async move {
        // Same warmup as the per-tick worker so we don't pile on at startup.
        tokio::time::sleep(Duration::from_secs(60 * 2)).await;
        let mut ticker = tokio::time::interval(DAILY_REVIEW_TICK_INTERVAL);
        loop {
            ticker.tick().await;

            if !allows_memory_review_worker(&state) {
                continue;
            }

            let today = Local::now().format("%Y-%m-%d").to_string();
            let yesterday = Local::now()
                .date_naive()
                .pred_opt()
                .map(|d| d.format("%Y-%m-%d").to_string());

            let Some(yesterday) = yesterday else {
                continue;
            };

            {
                let last = LAST_RUN_DAY.lock();
                if last.as_deref() == Some(today.as_str()) {
                    continue;
                }
            }

            let provider = match build_inference_provider(&state) {
                Some(p) => p,
                None => continue,
            };
            let embedder = match Embedder::new() {
                Ok(e) => Some(e),
                Err(err) => {
                    tracing::warn!(
                        err = %err,
                        "daily review: embedder unavailable; will retry next tick"
                    );
                    None
                }
            };

            let (start_ms, end_ms) = match parse_day_range_local(&yesterday) {
                Ok(range) => range,
                Err(err) => {
                    tracing::warn!(err, "daily review: could not parse yesterday's range");
                    continue;
                }
            };
            let now_ms = chrono::Utc::now().timestamp_millis();
            let outcome = run_daily_memory_review(
                &state,
                &state.store,
                provider.as_ref(),
                embedder.as_ref(),
                &yesterday,
                start_ms,
                end_ms,
                now_ms,
                false,
            )
            .await;

            match outcome {
                Ok(summary) => {
                    tracing::info!(
                        target: "fndr::memory_review::daily",
                        day = %summary.day,
                        scanned = summary.scanned,
                        changed = summary.changed,
                        failed = summary.failed,
                        skipped_pressure = summary.skipped_pressure,
                        skipped_already_reviewed = summary.skipped_already_reviewed,
                        "daily review pass complete"
                    );
                    if summary.skipped_pressure == 0 {
                        *LAST_RUN_DAY.lock() = Some(today);
                    }
                }
                Err(err) => {
                    tracing::warn!(err = %err, "daily review pass failed");
                }
            }
        }
    });
}

fn build_inference_provider(
    state: &AppState,
) -> Option<Box<dyn super::pipeline::ReviewProvider>> {
    let inference = state.inference.read().clone()?;
    Some(Box::new(
        super::inference_provider::InferenceReviewProvider::new(inference),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::graph::GraphStore;
    use crate::memory_review::pipeline::{ReviewInput, ReviewedMemory};
    use crate::storage::{MemoryRecord, StateStore};
    use crate::AppState;
    use futures::future::{BoxFuture, FutureExt};
    use std::sync::Arc;

    struct StubProvider {
        result: Result<ReviewedMemory, String>,
    }

    impl super::super::pipeline::ReviewProvider for StubProvider {
        fn review<'a>(
            &'a self,
            _input: &'a ReviewInput,
        ) -> BoxFuture<'a, Result<ReviewedMemory, String>> {
            let r = self.result.clone();
            async move { r }.boxed()
        }
    }

    async fn build_state_with_store() -> (Arc<AppState>, Arc<Store>) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().to_path_buf();
        std::mem::forget(dir);
        let store_path = path.clone();
        let store =
            tokio::task::spawn_blocking(move || Arc::new(Store::new(&store_path).expect("store")))
                .await
                .expect("spawn store");
        let state_store_path = path.clone();
        let state_store = tokio::task::spawn_blocking(move || {
            Arc::new(StateStore::new(&state_store_path).expect("state store"))
        })
        .await
        .expect("spawn state store");
        let graph = GraphStore::new(store.clone());
        let state = Arc::new(AppState::new(
            path,
            Config::default(),
            store.clone(),
            state_store,
            graph,
            None,
            None,
        ));
        (state, store)
    }

    fn record(id: &str, ts_ms: i64) -> MemoryRecord {
        let mut r = MemoryRecord::default();
        r.id = id.to_string();
        r.timestamp = ts_ms;
        r.day_bucket = chrono::Local
            .timestamp_millis_opt(ts_ms)
            .single()
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_default();
        r.app_name = "Chrome".to_string();
        r.window_title = format!("Window for {id}");
        r.clean_text = format!("Captured page for {id} — design discussion.");
        r.snippet = format!("Captured {id}");
        r.display_summary = format!("Original summary for {id}");
        r.memory_context = format!("Original memory context for {id}");
        r.enrichment_status = super::super::STATUS_PENDING.to_string();
        r.embedding = vec![0.0; 384];
        r.image_embedding = vec![0.0; 768];
        r.snippet_embedding = vec![0.0; 384];
        r.support_embedding = vec![0.0; 384];
        r
    }

    #[tokio::test]
    async fn dry_run_does_not_mutate_rows() {
        let (state, store) = build_state_with_store().await;

        let day_start = parse_day_range_local("2026-05-20").unwrap().0;
        let r1 = record("mem-dry-1", day_start + 60_000);
        let r2 = record("mem-dry-2", day_start + 120_000);
        store
            .add_batch_preserving_ids(&[r1.clone(), r2.clone()])
            .await
            .unwrap();

        let provider = StubProvider {
            result: Ok(ReviewedMemory {
                memory_context: "Reviewed and synthesized the design notes.".to_string(),
                display_summary: "Synthesized design notes".to_string(),
                ..ReviewedMemory::default()
            }),
        };
        let (start_ms, end_ms) = parse_day_range_local("2026-05-20").unwrap();
        let summary = run_daily_memory_review(
            &state,
            &store,
            &provider,
            None,
            "2026-05-20",
            start_ms,
            end_ms,
            chrono::Utc::now().timestamp_millis(),
            true, // dry_run
        )
        .await
        .unwrap();

        assert!(summary.dry_run);
        assert_eq!(summary.scanned, 2);
        assert_eq!(summary.changed, 0);
        assert_eq!(summary.would_change, 2);
        assert_eq!(summary.failed, 0);

        // No row mutated by a dry run.
        let written = store.get_memory_by_id("mem-dry-1").await.unwrap().unwrap();
        assert_eq!(written.memory_context, "Original memory context for mem-dry-1");
        assert_eq!(written.display_summary, "Original summary for mem-dry-1");
        assert_eq!(written.enrichment_status, super::super::STATUS_PENDING);
        assert_eq!(written.reviewer_generation, 0);
    }

    #[tokio::test]
    async fn non_dry_run_applies_reviewed_daily_status() {
        let (state, store) = build_state_with_store().await;

        let day_start = parse_day_range_local("2026-05-20").unwrap().0;
        let r1 = record("mem-real-1", day_start + 60_000);
        store
            .add_batch_preserving_ids(&[r1.clone()])
            .await
            .unwrap();

        let provider = StubProvider {
            result: Ok(ReviewedMemory {
                memory_context: "Reviewed and synthesized the design notes.".to_string(),
                display_summary: "Synthesized design notes".to_string(),
                ..ReviewedMemory::default()
            }),
        };
        let (start_ms, end_ms) = parse_day_range_local("2026-05-20").unwrap();
        let now_ms = chrono::Utc::now().timestamp_millis();
        let summary = run_daily_memory_review(
            &state,
            &store,
            &provider,
            None,
            "2026-05-20",
            start_ms,
            end_ms,
            now_ms,
            false,
        )
        .await
        .unwrap();

        assert!(!summary.dry_run);
        assert_eq!(summary.scanned, 1);
        assert_eq!(summary.changed, 1);
        assert_eq!(summary.would_change, 0);

        let written = store
            .get_memory_by_id("mem-real-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(written.enrichment_status, STATUS_REVIEWED_DAILY);
        assert_eq!(written.reviewer_generation, 1);
        assert_eq!(
            written.synthesis_branch,
            super::super::SYNTHESIS_BRANCH_REVIEWED_DAILY
        );
        assert!(written.memory_context.contains("design"));
        assert!(written.reviewed_at_ms >= now_ms);
    }

    #[tokio::test]
    async fn pressure_gate_defers_remaining_batch() {
        let (state, store) = build_state_with_store().await;
        // Pause first so the gate is closed; populate a day so we exercise
        // the "skipped_pressure == scanned" early-return path.
        state.pause();

        let day_start = parse_day_range_local("2026-05-20").unwrap().0;
        let r = record("mem-pressure", day_start + 60_000);
        store.add_batch_preserving_ids(&[r.clone()]).await.unwrap();

        let provider = StubProvider {
            result: Ok(ReviewedMemory {
                memory_context: "Would have been reviewed.".to_string(),
                display_summary: "Pending".to_string(),
                ..ReviewedMemory::default()
            }),
        };
        let (start_ms, end_ms) = parse_day_range_local("2026-05-20").unwrap();
        let summary = run_daily_memory_review(
            &state,
            &store,
            &provider,
            None,
            "2026-05-20",
            start_ms,
            end_ms,
            chrono::Utc::now().timestamp_millis(),
            false,
        )
        .await
        .unwrap();

        assert_eq!(summary.skipped_pressure, 1);
        assert_eq!(summary.changed, 0);

        // Original row untouched.
        let written = store
            .get_memory_by_id("mem-pressure")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(written.enrichment_status, super::super::STATUS_PENDING);
        assert_eq!(written.reviewer_generation, 0);
    }

    #[tokio::test]
    async fn invalid_patch_preserves_original() {
        let (state, store) = build_state_with_store().await;

        let day_start = parse_day_range_local("2026-05-20").unwrap().0;
        let r1 = record("mem-bad-1", day_start + 60_000);
        store.add_batch_preserving_ids(&[r1.clone()]).await.unwrap();

        // Patch that injects an ungrounded URL — must trip
        // `GroundingViolation` and preserve the original row.
        let provider = StubProvider {
            result: Ok(ReviewedMemory {
                memory_context: "Reviewed https://hallucinated.example/path".to_string(),
                display_summary: "Hallucinated content".to_string(),
                ..ReviewedMemory::default()
            }),
        };
        let (start_ms, end_ms) = parse_day_range_local("2026-05-20").unwrap();
        let summary = run_daily_memory_review(
            &state,
            &store,
            &provider,
            None,
            "2026-05-20",
            start_ms,
            end_ms,
            chrono::Utc::now().timestamp_millis(),
            false,
        )
        .await
        .unwrap();

        assert_eq!(summary.failed, 1);
        assert_eq!(summary.changed, 0);

        let written = store
            .get_memory_by_id("mem-bad-1")
            .await
            .unwrap()
            .unwrap();
        // mark_failed sets review_failed; original content stays intact.
        assert_eq!(
            written.enrichment_status,
            super::super::STATUS_REVIEW_FAILED
        );
        assert_eq!(written.memory_context, "Original memory context for mem-bad-1");
        assert_eq!(written.display_summary, "Original summary for mem-bad-1");
        assert_eq!(written.reviewer_generation, 0);

        // The outcome surface labels the failure as an invalid patch so the
        // UI can distinguish grounding violations from transient LLM failures.
        match summary.outcomes.first() {
            Some(DailyReviewOutcome::InvalidPatch { memory_id, reason }) => {
                assert_eq!(memory_id, "mem-bad-1");
                assert_eq!(reason, "grounding_violation");
            }
            other => panic!("expected InvalidPatch outcome, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn already_reviewed_records_are_skipped_in_same_generation() {
        let (state, store) = build_state_with_store().await;

        let day_start = parse_day_range_local("2026-05-20").unwrap().0;
        let mut r1 = record("mem-already", day_start + 60_000);
        r1.enrichment_status = STATUS_REVIEWED_DAILY.to_string();
        r1.reviewed_at_ms = day_start + 100_000;
        r1.reviewer_generation = 1;
        store.add_batch_preserving_ids(&[r1.clone()]).await.unwrap();

        let provider = StubProvider {
            result: Ok(ReviewedMemory {
                memory_context: "Should never run.".to_string(),
                display_summary: "Should never run".to_string(),
                ..ReviewedMemory::default()
            }),
        };
        let (start_ms, end_ms) = parse_day_range_local("2026-05-20").unwrap();
        let summary = run_daily_memory_review(
            &state,
            &store,
            &provider,
            None,
            "2026-05-20",
            start_ms,
            end_ms,
            chrono::Utc::now().timestamp_millis(),
            false,
        )
        .await
        .unwrap();

        assert_eq!(summary.skipped_already_reviewed, 1);
        assert_eq!(summary.changed, 0);
        assert_eq!(summary.failed, 0);
    }

    #[test]
    fn parse_day_range_local_round_trips_well_formed_dates() {
        let (start, end) = parse_day_range_local("2026-05-20").unwrap();
        assert!(end > start);
        assert!(end - start >= 23 * 3600 * 1000);
    }

    #[test]
    fn parse_day_range_local_rejects_malformed_input() {
        assert!(parse_day_range_local("not-a-date").is_err());
        assert!(parse_day_range_local("2026/05/20").is_err());
    }
}
