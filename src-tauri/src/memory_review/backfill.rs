//! Backfill driver for the post-capture memory-review worker.
//!
//! `backfill_memory_review_in_range` enqueues a [`MemoryReviewJob`] for every
//! memory in `[start_ms, end_ms]` whose `enrichment_status` indicates it has
//! not been fully reviewed yet (empty / `pending` / `review_failed`). The
//! worker then drains the queue as the pressure gate allows — the backfill
//! call itself never blocks capture, never holds the model-pipeline lock, and
//! never calls the LLM directly.
//!
//! `dry_run = true` returns the count that *would* have been queued without
//! mutating the queue. Used by the IPC dry-run path so the UI can show a
//! before/after counter.

use crate::storage::Store;
use crate::AppState;
use serde::{Deserialize, Serialize};

use super::queue::MemoryReviewJob;
use super::{STATUS_REVIEWED_DAILY, STATUS_REVIEWED_LOCAL};

/// Aggregate stats returned by [`backfill_memory_review_in_range`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BackfillReviewSummary {
    pub start_ms: i64,
    pub end_ms: i64,
    pub dry_run: bool,
    /// Memories scanned in the range.
    pub scanned: usize,
    /// Memories actually enqueued (always 0 when `dry_run`).
    pub queued: usize,
    /// Memories that would have been enqueued in a non-dry-run.
    pub would_queue: usize,
    /// Memories already reviewed (`reviewed_local` or `reviewed_daily`) —
    /// no review is queued for them.
    pub already_reviewed: usize,
    /// Memories that were already in the queue when this backfill ran.
    pub already_queued: usize,
}

/// Walk the memory range and enqueue a review job for each unreviewed row.
///
/// Returns the per-bucket counts so the IPC layer can report a useful summary
/// even in dry-run mode.
pub async fn backfill_memory_review_in_range(
    state: &AppState,
    store: &Store,
    start_ms: i64,
    end_ms: i64,
    now_ms: i64,
    dry_run: bool,
) -> Result<BackfillReviewSummary, String> {
    if end_ms < start_ms {
        return Err(format!(
            "backfill_memory_review: end_ms ({end_ms}) is before start_ms ({start_ms})"
        ));
    }

    let records = store
        .get_memories_in_range(start_ms, end_ms)
        .await
        .map_err(|err| err.to_string())?;

    let scanned = records.len();
    let mut summary = BackfillReviewSummary {
        start_ms,
        end_ms,
        dry_run,
        scanned,
        queued: 0,
        would_queue: 0,
        already_reviewed: 0,
        already_queued: 0,
    };

    if scanned == 0 {
        return Ok(summary);
    }

    let already_pending: std::collections::HashSet<String> = state
        .pending_memory_reviews
        .pending_memory_ids()
        .into_iter()
        .collect();

    for record in records {
        if record.enrichment_status == STATUS_REVIEWED_LOCAL
            || record.enrichment_status == STATUS_REVIEWED_DAILY
        {
            summary.already_reviewed += 1;
            continue;
        }
        if already_pending.contains(&record.id) {
            summary.already_queued += 1;
            continue;
        }
        if dry_run {
            summary.would_queue += 1;
            continue;
        }
        let inserted = state.pending_memory_reviews.enqueue(MemoryReviewJob {
            memory_id: record.id.clone(),
            day_bucket: record.day_bucket.clone(),
            enqueued_at_ms: now_ms,
        });
        if inserted {
            summary.queued += 1;
        } else {
            summary.already_queued += 1;
        }
    }

    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::graph::GraphStore;
    use crate::storage::{MemoryRecord, StateStore};
    use crate::AppState;
    use std::sync::Arc;

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

    fn make(id: &str, ts: i64, status: &str) -> MemoryRecord {
        let mut r = MemoryRecord::default();
        r.id = id.to_string();
        r.timestamp = ts;
        r.enrichment_status = status.to_string();
        r.clean_text = format!("captured {id}");
        r.app_name = "Chrome".to_string();
        r.embedding = vec![0.0; 384];
        r.image_embedding = vec![0.0; 768];
        r.snippet_embedding = vec![0.0; 384];
        r.support_embedding = vec![0.0; 384];
        r
    }

    #[tokio::test]
    async fn dry_run_does_not_enqueue() {
        let (state, store) = build_state_with_store().await;
        let base = 1_700_000_000_000;
        store
            .add_batch_preserving_ids(&[
                make("a", base + 1_000, super::super::STATUS_PENDING),
                make("b", base + 2_000, ""),
                make("c", base + 3_000, STATUS_REVIEWED_LOCAL),
            ])
            .await
            .unwrap();

        let summary = backfill_memory_review_in_range(
            &state,
            &store,
            base,
            base + 10_000,
            chrono::Utc::now().timestamp_millis(),
            true,
        )
        .await
        .unwrap();

        assert!(summary.dry_run);
        assert_eq!(summary.scanned, 3);
        assert_eq!(summary.would_queue, 2);
        assert_eq!(summary.queued, 0);
        assert_eq!(summary.already_reviewed, 1);
        assert_eq!(state.pending_memory_reviews.len(), 0);
    }

    #[tokio::test]
    async fn non_dry_run_queues_unreviewed_rows() {
        let (state, store) = build_state_with_store().await;
        let base = 1_700_000_000_000;
        store
            .add_batch_preserving_ids(&[
                make("a", base + 1_000, super::super::STATUS_PENDING),
                make("b", base + 2_000, ""),
                make("c", base + 3_000, STATUS_REVIEWED_LOCAL),
                make("d", base + 4_000, super::super::STATUS_REVIEW_FAILED),
            ])
            .await
            .unwrap();

        let summary = backfill_memory_review_in_range(
            &state,
            &store,
            base,
            base + 10_000,
            chrono::Utc::now().timestamp_millis(),
            false,
        )
        .await
        .unwrap();

        assert!(!summary.dry_run);
        assert_eq!(summary.scanned, 4);
        // a, b, d — three unreviewed memories.
        assert_eq!(summary.queued, 3);
        assert_eq!(summary.already_reviewed, 1);
        assert_eq!(state.pending_memory_reviews.len(), 3);
    }

    #[tokio::test]
    async fn rejects_inverted_range() {
        let (state, store) = build_state_with_store().await;
        let err =
            backfill_memory_review_in_range(&state, &store, 1_000, 500, 1_000, false)
                .await
                .unwrap_err();
        assert!(err.contains("before start_ms"));
    }

    #[tokio::test]
    async fn rows_already_queued_are_counted_separately() {
        let (state, store) = build_state_with_store().await;
        let base = 1_700_000_000_000;
        store
            .add_batch_preserving_ids(&[make(
                "preloaded",
                base + 1_000,
                super::super::STATUS_PENDING,
            )])
            .await
            .unwrap();
        state.pending_memory_reviews.enqueue(MemoryReviewJob {
            memory_id: "preloaded".to_string(),
            day_bucket: String::new(),
            enqueued_at_ms: base + 999,
        });

        let summary = backfill_memory_review_in_range(
            &state,
            &store,
            base,
            base + 10_000,
            chrono::Utc::now().timestamp_millis(),
            false,
        )
        .await
        .unwrap();
        assert_eq!(summary.queued, 0);
        assert_eq!(summary.already_queued, 1);
        // The pre-existing queued job is preserved.
        assert_eq!(state.pending_memory_reviews.len(), 1);
    }
}
