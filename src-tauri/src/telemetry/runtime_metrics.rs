//! In-process latency and counter metrics (no PII). Used by Pipeline Inspector and tuning.

use crate::AppState;
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_RECENT: usize = 256;
const EWMA_ALPHA: f64 = 0.125;

#[derive(Default, Clone)]
struct Agg {
    n: u64,
    sum_ms: u64,
    max_ms: u64,
    ewma_ms: f64,
}

impl Agg {
    fn record(&mut self, ms: u64) {
        self.n += 1;
        self.sum_ms = self.sum_ms.saturating_add(ms);
        self.max_ms = self.max_ms.max(ms);
        self.ewma_ms = if self.n == 1 {
            ms as f64
        } else {
            EWMA_ALPHA * ms as f64 + (1.0 - EWMA_ALPHA) * self.ewma_ms
        };
    }

    fn to_snapshot(&self) -> AggregateSnapshot {
        let avg_ms = if self.n > 0 {
            self.sum_ms as f64 / self.n as f64
        } else {
            0.0
        };
        AggregateSnapshot {
            n: self.n,
            sum_ms: self.sum_ms,
            max_ms: self.max_ms,
            avg_ms,
            ewma_ms: self.ewma_ms,
        }
    }
}

struct Recent {
    ts_ms: i64,
    op: &'static str,
    ms: u64,
    meta: Option<&'static str>,
}

struct Inner {
    aggregates: HashMap<&'static str, Agg>,
    counters: HashMap<&'static str, u64>,
    recent: VecDeque<Recent>,
}

pub struct RuntimeMetrics {
    inner: Mutex<Inner>,
}

impl RuntimeMetrics {
    fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                aggregates: HashMap::new(),
                counters: HashMap::new(),
                recent: VecDeque::with_capacity(MAX_RECENT),
            }),
        }
    }

    /// Record a duration sample for `op` (static string — no allocation in hot path).
    pub fn record_ms(&self, op: &'static str, ms: u64) {
        self.record_ms_with_meta(op, ms, None);
    }

    pub fn record_ms_with_meta(&self, op: &'static str, ms: u64, meta: Option<&'static str>) {
        let ts_ms = now_ms();
        let mut g = self.inner.lock();
        g.aggregates.entry(op).or_default().record(ms);
        g.recent.push_back(Recent {
            ts_ms,
            op,
            ms,
            meta,
        });
        while g.recent.len() > MAX_RECENT {
            g.recent.pop_front();
        }
    }

    pub fn bump(&self, counter: &'static str) {
        let mut g = self.inner.lock();
        *g.counters.entry(counter).or_insert(0) += 1;
    }

    fn snapshot_inner(
        &self,
    ) -> (
        BTreeMap<String, AggregateSnapshot>,
        BTreeMap<String, u64>,
        Vec<RecentSnapshot>,
    ) {
        let g = self.inner.lock();
        let aggregates = g
            .aggregates
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_snapshot()))
            .collect();
        let counters = g
            .counters
            .iter()
            .map(|(k, v)| (k.to_string(), *v))
            .collect();
        let recent = g
            .recent
            .iter()
            .rev()
            .take(64)
            .map(|r| RecentSnapshot {
                ts_ms: r.ts_ms,
                op: r.op.to_string(),
                ms: r.ms,
                meta: r.meta.map(|m| m.to_string()),
            })
            .collect();
        (aggregates, counters, recent)
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

static GLOBAL: OnceLock<RuntimeMetrics> = OnceLock::new();

pub fn global() -> &'static RuntimeMetrics {
    GLOBAL.get_or_init(RuntimeMetrics::new)
}

pub fn record_ms(op: &'static str, ms: u64) {
    global().record_ms(op, ms);
}

pub fn record_ms_with_meta(op: &'static str, ms: u64, meta: Option<&'static str>) {
    global().record_ms_with_meta(op, ms, meta);
}

pub fn bump(counter: &'static str) {
    global().bump(counter);
}

// --- RSS cache (macOS) -----------------------------------------------------

static RSS_CACHED_AT_MS: AtomicI64 = AtomicI64::new(0);
static RSS_CACHED_BYTES: AtomicU64 = AtomicU64::new(0);
const RSS_CACHE_TTL_MS: i64 = 8_000;

#[cfg(target_os = "macos")]
fn process_rss_bytes_sample() -> Option<u64> {
    use libc::{
        kern_return_t, mach_task_basic_info, mach_task_self, task_info, KERN_SUCCESS,
        MACH_TASK_BASIC_INFO, MACH_TASK_BASIC_INFO_COUNT,
    };
    use std::mem::MaybeUninit;

    let mut info = MaybeUninit::<mach_task_basic_info>::uninit();
    let mut count = MACH_TASK_BASIC_INFO_COUNT;
    let kr: kern_return_t = unsafe {
        task_info(
            mach_task_self(),
            MACH_TASK_BASIC_INFO,
            info.as_mut_ptr() as *mut _,
            &mut count,
        )
    };
    if kr != KERN_SUCCESS {
        return None;
    }
    let info = unsafe { info.assume_init() };
    Some(info.resident_size as u64)
}

#[cfg(not(target_os = "macos"))]
fn process_rss_bytes_sample() -> Option<u64> {
    None
}

fn process_rss_bytes_cached() -> Option<u64> {
    let now = now_ms();
    let cached_at = RSS_CACHED_AT_MS.load(Ordering::Relaxed);
    if now - cached_at < RSS_CACHE_TTL_MS {
        let b = RSS_CACHED_BYTES.load(Ordering::Relaxed);
        return if b > 0 { Some(b) } else { None };
    }
    if let Some(bytes) = process_rss_bytes_sample() {
        RSS_CACHED_BYTES.store(bytes, Ordering::Relaxed);
        RSS_CACHED_AT_MS.store(now, Ordering::Relaxed);
        Some(bytes)
    } else {
        None
    }
}

// --- Snapshot DTOs ---------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct AggregateSnapshot {
    pub n: u64,
    pub sum_ms: u64,
    pub max_ms: u64,
    pub avg_ms: f64,
    pub ewma_ms: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecentSnapshot {
    pub ts_ms: i64,
    pub op: String,
    pub ms: u64,
    pub meta: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaptureMetricsSnapshot {
    pub frames_captured: u64,
    pub frames_dropped: u64,
    pub last_capture_time_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct EmbeddingMetricsSnapshot {
    pub backend: String,
    pub degraded: bool,
    pub detail: String,
    pub model_name: String,
    pub dimension: usize,
    pub clip_session_loaded: bool,
    pub last_clip_infer_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct InferenceMetricsSnapshot {
    pub ai_model_available: bool,
    pub ai_model_loaded: bool,
    pub loaded_model_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeMetricsSnapshot {
    pub generated_at_ms: i64,
    pub process_rss_bytes: Option<u64>,
    pub capture: CaptureMetricsSnapshot,
    pub embedding: EmbeddingMetricsSnapshot,
    pub inference: InferenceMetricsSnapshot,
    pub aggregates: BTreeMap<String, AggregateSnapshot>,
    pub counters: BTreeMap<String, u64>,
    pub recent: Vec<RecentSnapshot>,
}

pub fn build_snapshot(
    state: &AppState,
    embedding: EmbeddingMetricsSnapshot,
    inference: InferenceMetricsSnapshot,
) -> RuntimeMetricsSnapshot {
    let (aggregates, counters, recent) = global().snapshot_inner();
    RuntimeMetricsSnapshot {
        generated_at_ms: now_ms(),
        process_rss_bytes: process_rss_bytes_cached(),
        capture: CaptureMetricsSnapshot {
            frames_captured: state.frames_captured.load(Ordering::Relaxed),
            frames_dropped: state.frames_dropped.load(Ordering::Relaxed),
            last_capture_time_ms: state.last_capture_time.load(Ordering::Relaxed),
        },
        embedding,
        inference,
        aggregates,
        counters,
        recent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn aggregate_tracks_max_and_count() {
        let m = RuntimeMetrics::new();
        m.record_ms("test.op", 10);
        m.record_ms("test.op", 30);
        m.record_ms("test.op", 20);
        let (agg, _, _) = m.snapshot_inner();
        let a = agg.get("test.op").expect("agg");
        assert_eq!(a.n, 3);
        assert_eq!(a.sum_ms, 60);
        assert_eq!(a.max_ms, 30);
        assert!(a.ewma_ms > 0.0);
    }

    #[test]
    fn concurrent_records_no_panic() {
        let m = Arc::new(RuntimeMetrics::new());
        let mut handles = vec![];
        for _ in 0..32 {
            let mm = m.clone();
            handles.push(thread::spawn(move || {
                for i in 0..100 {
                    mm.record_ms("concurrent", i % 50);
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        let (agg, _, _) = m.snapshot_inner();
        let a = agg.get("concurrent").unwrap();
        assert_eq!(a.n, 3200);
    }

    #[test]
    fn bump_increments_counter() {
        let m = RuntimeMetrics::new();
        m.bump("c1");
        m.bump("c1");
        let (_, c, _) = m.snapshot_inner();
        assert_eq!(c.get("c1"), Some(&2));
    }
}
