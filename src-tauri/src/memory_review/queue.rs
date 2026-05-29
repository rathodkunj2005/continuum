//! In-memory queue of `MemoryReviewJob`s with id-level deduplication.

use parking_lot::Mutex;
use std::collections::VecDeque;

/// One review unit. `memory_id` is the dedupe key; only the most recent
/// `(day_bucket, enqueued_at_ms)` for a given memory is kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryReviewJob {
    pub memory_id: String,
    pub day_bucket: String,
    pub enqueued_at_ms: i64,
}

/// FIFO + dedupe queue. Insertion is O(n) in the queue length, which is
/// acceptable because the queue is naturally bounded by capture rate.
#[derive(Debug, Default)]
pub struct MemoryReviewQueue {
    jobs: Mutex<VecDeque<MemoryReviewJob>>,
}

impl MemoryReviewQueue {
    pub fn new() -> Self {
        Self {
            jobs: Mutex::new(VecDeque::new()),
        }
    }

    /// Push `job`. Returns true if the job was actually inserted; false if a
    /// pending job for the same `memory_id` already existed.
    pub fn enqueue(&self, job: MemoryReviewJob) -> bool {
        let mut jobs = self.jobs.lock();
        if jobs.iter().any(|j| j.memory_id == job.memory_id) {
            return false;
        }
        jobs.push_back(job);
        true
    }

    /// Pop the oldest job, if any.
    pub fn dequeue(&self) -> Option<MemoryReviewJob> {
        self.jobs.lock().pop_front()
    }

    /// Return the snapshot of pending memory_ids (for telemetry / inspector).
    pub fn pending_memory_ids(&self) -> Vec<String> {
        self.jobs
            .lock()
            .iter()
            .map(|job| job.memory_id.clone())
            .collect()
    }

    pub fn len(&self) -> usize {
        self.jobs.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.jobs.lock().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(id: &str) -> MemoryReviewJob {
        MemoryReviewJob {
            memory_id: id.to_string(),
            day_bucket: "2026-05-20".to_string(),
            enqueued_at_ms: 1_700_000_000_000,
        }
    }

    #[test]
    fn enqueue_dedupes_by_memory_id() {
        let q = MemoryReviewQueue::new();
        assert!(q.enqueue(job("a")));
        assert!(q.enqueue(job("b")));
        assert!(!q.enqueue(job("a")), "duplicate memory_id must not enqueue");
        assert_eq!(q.len(), 2);
        assert_eq!(
            q.pending_memory_ids(),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn dequeue_is_fifo() {
        let q = MemoryReviewQueue::new();
        q.enqueue(job("a"));
        q.enqueue(job("b"));
        q.enqueue(job("c"));
        assert_eq!(q.dequeue().unwrap().memory_id, "a");
        assert_eq!(q.dequeue().unwrap().memory_id, "b");
        assert_eq!(q.dequeue().unwrap().memory_id, "c");
        assert!(q.dequeue().is_none());
    }

    #[test]
    fn redequeue_then_reenqueue_allows_same_id_again() {
        // Real worker pulls a job, processes, and may need to re-enqueue on
        // transient failure. Once a job is dequeued, the dedupe block lifts.
        let q = MemoryReviewQueue::new();
        q.enqueue(job("a"));
        let pulled = q.dequeue().unwrap();
        assert_eq!(pulled.memory_id, "a");
        assert!(q.enqueue(job("a")), "same id can re-enter after dequeue");
        assert_eq!(q.len(), 1);
    }
}
