//! L1 dedup — the cheapest layer, run before any network/embedding cost.
//!
//! A user staring at the same screen produces a stream of identical
//! descriptors. We drop repeats within a sliding window using only the
//! `content_hash`, so duplicates never reach the sync queue (and therefore
//! never reach the paid embedding + insert in `agent-sync`). Ports the
//! reference desktop's in-memory dedup; the DB unique constraint and the Edge
//! Function's pre-embedding check remain the L2/L3 backstops.
//!
//! A hit does **not** refresh the timestamp, so a long-lived context naturally
//! re-syncs once per window — a heartbeat that keeps the graph fresh without
//! flooding it.

use std::collections::HashMap;

use parking_lot::Mutex;

/// Default window: 5 minutes, matching the reference desktop.
pub const DEFAULT_WINDOW_MS: i64 = 5 * 60 * 1000;

/// Tracks recently-seen content hashes within a time window.
#[derive(Debug)]
pub struct RecentDedup {
    window_ms: i64,
    seen: Mutex<HashMap<String, i64>>,
}

impl RecentDedup {
    pub fn new(window_ms: i64) -> Self {
        Self {
            window_ms: window_ms.max(0),
            seen: Mutex::new(HashMap::new()),
        }
    }

    /// True if `content_hash` was seen within the window. Evicts expired
    /// entries and records first sightings. Uses the wall clock.
    pub fn is_duplicate(&self, content_hash: &str) -> bool {
        self.is_duplicate_at(content_hash, now_ms())
    }

    /// Clock-injected variant for deterministic tests.
    pub fn is_duplicate_at(&self, content_hash: &str, now_ms: i64) -> bool {
        let mut seen = self.seen.lock();
        seen.retain(|_, ts| now_ms - *ts <= self.window_ms);
        if seen.contains_key(content_hash) {
            return true;
        }
        seen.insert(content_hash.to_string(), now_ms);
        false
    }

    /// Number of live entries (post-eviction is not forced here).
    pub fn len(&self) -> usize {
        self.seen.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.seen.lock().is_empty()
    }
}

impl Default for RecentDedup {
    fn default() -> Self {
        Self::new(DEFAULT_WINDOW_MS)
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_sighting_is_not_a_duplicate() {
        let d = RecentDedup::new(DEFAULT_WINDOW_MS);
        assert!(!d.is_duplicate_at("hash-a", 0));
    }

    #[test]
    fn repeat_within_window_is_a_duplicate() {
        let d = RecentDedup::new(1000);
        assert!(!d.is_duplicate_at("hash-a", 0));
        assert!(d.is_duplicate_at("hash-a", 500));
    }

    #[test]
    fn repeat_after_window_is_not_a_duplicate() {
        let d = RecentDedup::new(1000);
        assert!(!d.is_duplicate_at("hash-a", 0));
        // 1001ms later the first sighting has expired.
        assert!(!d.is_duplicate_at("hash-a", 1001));
    }

    #[test]
    fn distinct_hashes_are_independent() {
        let d = RecentDedup::new(1000);
        assert!(!d.is_duplicate_at("hash-a", 0));
        assert!(!d.is_duplicate_at("hash-b", 100));
        assert!(d.is_duplicate_at("hash-a", 200));
        assert!(d.is_duplicate_at("hash-b", 300));
    }

    #[test]
    fn hit_does_not_refresh_so_context_re_syncs_each_window() {
        let d = RecentDedup::new(1000);
        assert!(!d.is_duplicate_at("hash-a", 0)); // sync
        assert!(d.is_duplicate_at("hash-a", 800)); // dup, no refresh
        // Eviction is measured from the original t=0, not t=800.
        assert!(!d.is_duplicate_at("hash-a", 1100)); // re-sync (heartbeat)
    }
}
