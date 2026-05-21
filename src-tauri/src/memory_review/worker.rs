//! Background worker that drains [`MemoryReviewQueue`] and runs one
//! [`review_one_memory`] pass per tick. The worker is intentionally cheap:
//! per tick it checks the pressure gate, pulls one job, acquires the global
//! `model_pipeline_lock` so the LLM call never races capture, and writes the
//! result back. If the pressure gate is closed the job is **re-enqueued** so
//! the next tick (or the next conducive system state) picks it up.
//!
//! For tests we expose [`tick_once`] so the worker behavior can be exercised
//! synchronously without spawning background tasks.

use crate::embedding::Embedder;
use crate::storage::Store;
use crate::AppState;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use super::allows_memory_review_worker;
use super::pipeline::{review_one_memory, MemoryReviewOutcome, ReviewProvider};
#[cfg(test)]
use super::queue::MemoryReviewJob;

/// Outcome of a single worker tick. Distinguishes "intentionally skipped"
/// from "no work" so tests and the inspector can tell them apart.
#[derive(Debug, Clone, PartialEq)]
pub enum TickOutcome {
    /// Pressure gate denied the tick. The next job (if any) stays in the
    /// queue.
    Deferred(DeferReason),
    /// The queue was empty when we polled.
    Idle,
    /// The tick processed a job. The inner outcome describes what happened.
    Processed(MemoryReviewOutcome),
    /// The job was dequeued but the provider returned an error before
    /// `review_one_memory` could persist anything; the job is re-enqueued.
    Requeued { memory_id: String, error: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum DeferReason {
    PressureGate,
    InferenceUnavailable,
    Paused,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryReviewWorkerStatus {
    pub queue_depth: usize,
    pub last_review_at_ms: i64,
    pub last_error_kind: Option<String>,
    pub worker_enabled: bool,
    pub pressure_blocked: bool,
}

static LAST_REVIEW_AT_MS: AtomicI64 = AtomicI64::new(0);
static LAST_ERROR_KIND: Mutex<Option<String>> = Mutex::new(None);

pub fn status(state: &AppState) -> MemoryReviewWorkerStatus {
    let pressure_blocked = !state.is_paused.load(Ordering::Relaxed)
        && state.inference.read().is_some()
        && !crate::system_resources::allows_graph_idle_commit(state);
    MemoryReviewWorkerStatus {
        queue_depth: state.pending_memory_reviews.len(),
        last_review_at_ms: LAST_REVIEW_AT_MS.load(Ordering::Relaxed),
        last_error_kind: LAST_ERROR_KIND.lock().clone(),
        worker_enabled: !state.is_paused.load(Ordering::Relaxed)
            && state.inference.read().is_some(),
        pressure_blocked,
    }
}

/// Run one worker tick. Returns the outcome so callers (the spawned loop and
/// unit tests) can observe behavior without coupling to time.
pub async fn tick_once(
    state: &AppState,
    store: &Store,
    provider: &dyn ReviewProvider,
    embedder: Option<&Embedder>,
    now_ms: i64,
) -> TickOutcome {
    if !allows_memory_review_worker(state) {
        if state.is_paused.load(std::sync::atomic::Ordering::Relaxed) {
            set_last_error("paused");
            return TickOutcome::Deferred(DeferReason::Paused);
        }
        if state.inference.read().is_none() {
            set_last_error("inference_unavailable");
            return TickOutcome::Deferred(DeferReason::InferenceUnavailable);
        }
        set_last_error("pressure_gate");
        return TickOutcome::Deferred(DeferReason::PressureGate);
    }

    let Some(job) = state.pending_memory_reviews.dequeue() else {
        return TickOutcome::Idle;
    };
    tracing::info!(
        target: "fndr::memory_review",
        memory_id = %job.memory_id,
        queue_depth = state.pending_memory_reviews.len(),
        "memory_review job started"
    );

    // Acquire the global model pipeline lock so capture and the worker never
    // talk to the LLM at the same time. The lock is released when this scope
    // ends — `_guard` makes that explicit.
    let _guard = state.model_pipeline_lock.lock().await;

    match review_one_memory(store, provider, embedder, &job, now_ms).await {
        Ok(outcome) => {
            match &outcome {
                MemoryReviewOutcome::Reviewed { .. } => {
                    LAST_REVIEW_AT_MS.store(now_ms, Ordering::Relaxed);
                    clear_last_error();
                    tracing::info!(
                        target: "fndr::memory_review",
                        memory_review_outcome = ?outcome,
                        "memory_review write-back complete"
                    );
                }
                MemoryReviewOutcome::Failed { reason, .. } => {
                    set_last_error(reason.label());
                    tracing::warn!(
                        target: "fndr::memory_review",
                        memory_review_outcome = ?outcome,
                        "memory_review job failed"
                    );
                }
                MemoryReviewOutcome::Skipped { .. } => {}
            }
            TickOutcome::Processed(outcome)
        }
        Err(err) => {
            set_last_error("worker_error");
            tracing::warn!(
                memory_id = %job.memory_id,
                err = %err,
                "memory_review: worker tick failed; re-enqueueing"
            );
            // Re-enqueue. Dedupe still applies, so if the job races back in
            // from capture it'll be a no-op.
            state.pending_memory_reviews.enqueue(job.clone());
            TickOutcome::Requeued {
                memory_id: job.memory_id,
                error: err,
            }
        }
    }
}

fn set_last_error(kind: &str) {
    *LAST_ERROR_KIND.lock() = Some(kind.to_string());
}

fn clear_last_error() {
    *LAST_ERROR_KIND.lock() = None;
}

/// Spawn the background worker. Called once at app startup with a clone of
/// `AppState`. Each tick waits `interval` between attempts and skips when
/// the pressure gate denies the tick. The worker is robust to inference
/// being unavailable at startup — it just keeps deferring.
pub fn spawn(state: Arc<AppState>, interval: std::time::Duration) {
    tauri::async_runtime::spawn(async move {
        // Same warmup delay as the graph commit worker so we don't pile on
        // when the app is still loading models.
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        let mut ticker = tokio::time::interval(interval);
        loop {
            ticker.tick().await;

            let provider = match build_inference_review_provider(&state) {
                Some(provider) => provider,
                None => continue,
            };
            let embedder = match Embedder::new() {
                Ok(embedder) => Some(embedder),
                Err(err) => {
                    tracing::warn!(
                        err = %err,
                        "memory_review: embedder unavailable this tick; will retry"
                    );
                    None
                }
            };

            let now_ms = chrono::Utc::now().timestamp_millis();
            let outcome = tick_once(
                &state,
                &state.store,
                provider.as_ref(),
                embedder.as_ref(),
                now_ms,
            )
            .await;
            match &outcome {
                TickOutcome::Processed(processed) => {
                    tracing::info!(
                        memory_review_outcome = ?processed,
                        "memory_review: tick processed"
                    );
                }
                TickOutcome::Requeued { memory_id, error } => {
                    tracing::warn!(
                        memory_id = %memory_id,
                        error = %error,
                        "memory_review: tick requeued job"
                    );
                }
                _ => {}
            }
        }
    });
}

fn build_inference_review_provider(state: &AppState) -> Option<Box<dyn ReviewProvider>> {
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
    use crate::storage::StateStore;
    use futures::future::FutureExt;
    use std::sync::Arc;

    struct AlwaysOkProvider;

    impl ReviewProvider for AlwaysOkProvider {
        fn review<'a>(
            &'a self,
            _input: &'a ReviewInput,
        ) -> futures::future::BoxFuture<'a, Result<ReviewedMemory, String>> {
            async move {
                Ok(ReviewedMemory {
                    memory_context: "Reviewed by stub.".to_string(),
                    display_summary: "Stub reviewed".to_string(),
                    ..ReviewedMemory::default()
                })
            }
            .boxed()
        }
    }

    async fn build_state() -> Arc<AppState> {
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
        Arc::new(AppState::new(
            path,
            Config::default(),
            store,
            state_store,
            graph,
            None,
            None,
        ))
    }

    #[tokio::test]
    async fn tick_returns_inference_unavailable_when_engine_missing() {
        let state = build_state().await;
        // Inference is None by construction → gate denies the tick.
        let outcome = tick_once(
            &state,
            &state.store,
            &AlwaysOkProvider,
            None,
            1_700_000_000_000,
        )
        .await;
        assert_eq!(
            outcome,
            TickOutcome::Deferred(DeferReason::InferenceUnavailable)
        );
        let status = status(&state);
        assert_eq!(
            status.last_error_kind.as_deref(),
            Some("inference_unavailable")
        );
        assert!(!status.worker_enabled);
    }

    #[tokio::test]
    async fn tick_returns_paused_when_capture_paused_even_with_jobs() {
        let state = build_state().await;
        // Force the paused gate to fire first by enqueueing a job and pausing.
        state.pending_memory_reviews.enqueue(MemoryReviewJob {
            memory_id: "mem-paused".to_string(),
            day_bucket: "2026-05-20".to_string(),
            enqueued_at_ms: 1_700_000_000_000,
        });
        state.pause();
        let outcome = tick_once(
            &state,
            &state.store,
            &AlwaysOkProvider,
            None,
            1_700_000_000_000,
        )
        .await;
        assert_eq!(outcome, TickOutcome::Deferred(DeferReason::Paused));
        let status = status(&state);
        assert_eq!(status.queue_depth, 1);
        assert_eq!(status.last_error_kind.as_deref(), Some("paused"));
        // Job MUST remain in the queue for the next tick.
        assert_eq!(state.pending_memory_reviews.len(), 1);
    }

    #[tokio::test]
    async fn tick_re_enqueues_job_when_pressure_gate_denies_after_dequeue() {
        // Two-stage check: queue has a job, gate is closed → outcome is
        // Deferred and the job stays in the queue. We exercise the
        // `Paused` branch because it's the only gate we can flip
        // deterministically from a unit test (battery/CPU heuristics are
        // platform-dependent).
        let state = build_state().await;
        state.pending_memory_reviews.enqueue(MemoryReviewJob {
            memory_id: "mem-keep".to_string(),
            day_bucket: "2026-05-20".to_string(),
            enqueued_at_ms: 1_700_000_000_000,
        });
        state.pause();
        let outcome = tick_once(
            &state,
            &state.store,
            &AlwaysOkProvider,
            None,
            1_700_000_000_000,
        )
        .await;
        assert!(matches!(outcome, TickOutcome::Deferred(_)));
        assert_eq!(
            state.pending_memory_reviews.pending_memory_ids(),
            vec!["mem-keep".to_string()],
            "deferred tick must preserve the queued job"
        );
    }
}
