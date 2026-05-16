//! FNDR Library
//!
//! Core functionality for the FNDR memory search application.
#![recursion_limit = "512"]

pub mod accessibility;
pub mod agent;
pub mod capture;
pub mod config;
pub mod context_runtime;
pub mod downloads;
pub mod embed;
pub mod embedding;
pub mod evals;
pub mod http_util;
pub mod inference;
pub mod ipc;
pub mod mcp;
pub mod meeting;
pub mod memory;
pub mod memory_compaction;
pub mod memory_insight;
pub mod memory_quality;
pub mod models;
pub mod ocr;
pub mod privacy;
pub mod search;
pub mod speech;
pub mod storage;
pub mod summariser;
pub mod system_resources;
pub mod tasks;
pub mod telemetry;
pub mod timeline;
pub mod wiki;

use config::Config;
use inference::{InferenceEngine, VlmEngine};
use memory::graph::GraphStore;
use parking_lot::{Mutex, RwLock};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use storage::{StateStore, Stats, Store};
use tokio::sync::Mutex as AsyncMutex;

/// Queued insight-graph upsert work (see `capture` flush + idle `commit_graph_updates`).
#[derive(Debug, Clone)]
pub struct PendingGraphUpdate {
    pub memory_id: String,
    pub nodes: Vec<memory::graph::schema::GraphNode>,
    pub edges: Vec<memory::graph::schema::GraphEdge>,
    pub overall_confidence: f32,
}

pub struct LoadedAiEngines {
    pub inference: Option<Arc<InferenceEngine>>,
    pub vlm: Option<Arc<VlmEngine>>,
}

/// A proactive suggestion surfaced when the current screen matches a past memory.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProactiveSuggestion {
    pub memory_id: String,
    pub snippet: String,
    pub similarity: f32,
    pub task_title: Option<String>,
}

/// A privacy alert surfaced when the capture pipeline detects a highly sensitive context.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PrivacyAlert {
    pub id: String,
    pub domain_or_title: String,
    pub detected_at: i64,
}

/// Per-reason capture pipeline counters.
///
/// Every terminal branch in the capture loop bumps exactly one of these
/// (or one of the `stored_*` counters). The legacy `frames_captured` /
/// `frames_dropped` atomics on `AppState` only counted successful stores
/// and dedup drops — every other "skip" (surface policy, low signal, noise,
/// grounding, visual admission, OCR failure, blocklist, …) was invisible
/// to the UI, which is why the Capture Status card reported `Frames: 0 /
/// Dropped: 1` while the log clearly showed many frames being evaluated.
///
/// See [`SkipReason`] for the enumeration the helper accepts.
pub struct CapturePipelineStats {
    /// Times we entered the loop tick *and* attempted to act on a frame
    /// (i.e. capture was unpaused and FPS > 0). Useful denominator for
    /// the UI's storage-rate metric.
    pub evaluated: AtomicU64,
    /// User blocklist (app name / URL / title patterns from settings).
    pub skipped_blocklist: AtomicU64,
    /// FNDR is frontmost — we never capture our own UI (privacy + recursion).
    pub skipped_self_app: AtomicU64,
    /// Frames blocked by `classify_capture_surface_policy::SkipFrame`
    /// or by the browser semantic shape gate (nav-heavy, low signal).
    pub skipped_surface_policy: AtomicU64,
    /// Perceptual-hash dedup hit (current frame ≈ last frame).
    pub skipped_perceptual_dup: AtomicU64,
    /// (app, window, clean_text) hash already seen inside the semantic window.
    pub skipped_semantic_dup: AtomicU64,
    /// Apple Vision OCR or browser semantic extraction failed.
    pub skipped_ocr_failed: AtomicU64,
    /// Text-quality gate: avg line score / keep ratio failed.
    pub skipped_low_signal_text: AtomicU64,
    /// `text_cleanup::estimate_noise_score` above the configured threshold.
    pub skipped_noise: AtomicU64,
    /// LLM-grounding gate: extraction_grounding_confidence ≤ 0.10.
    pub skipped_grounding: AtomicU64,
    /// LLM-extraction "stacked critical issues" gate.
    pub skipped_stacked_extraction: AtomicU64,
    /// Visual admission: image below `visual_admission_min_image_dim`.
    pub skipped_visual_small: AtomicU64,
    /// Visual admission: novelty below adaptive threshold.
    pub skipped_visual_novelty: AtomicU64,
    /// Visual admission: VLM/compose pipeline error.
    pub skipped_visual_compose_failed: AtomicU64,
    /// Screen-capture syscall itself failed.
    pub skipped_screen_capture_failed: AtomicU64,
    /// Memory stored via the OCR-narrative path.
    pub stored_ocr_path: AtomicU64,
    /// Memory stored via the visual-narrative path.
    pub stored_visual_path: AtomicU64,
    /// Memory stored via the URL-only browser surface path.
    pub stored_url_only: AtomicU64,
    /// Most recent skip reason + app, one-line "Last: skipped_grounding (Cursor)"
    /// surface for the UI. We keep this small (single owned tuple) so the
    /// IPC poll doesn't have to read the JSONL signals file on each tick.
    pub last_skip: parking_lot::RwLock<Option<LastSkipEntry>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LastSkipEntry {
    pub reason: String,
    pub app_name: String,
    pub timestamp_ms: i64,
}

/// Terminal classification for a single capture tick.
///
/// Used by [`CapturePipelineStats::record_skip`] and `record_store` so every
/// `continue` path in the capture loop bumps the right counter once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// FNDR itself is the active app; capture is intentionally disabled.
    SelfApp,
    Blocklist,
    SurfacePolicy,
    PerceptualDup,
    SemanticDup,
    OcrFailed,
    LowSignalText,
    Noise,
    Grounding,
    StackedExtraction,
    VisualSmall,
    VisualNovelty,
    VisualComposeFailed,
    ScreenCaptureFailed,
}

impl SkipReason {
    pub fn as_str(self) -> &'static str {
        match self {
            SkipReason::SelfApp => "self_app",
            SkipReason::Blocklist => "blocklist",
            SkipReason::SurfacePolicy => "surface_policy",
            SkipReason::PerceptualDup => "perceptual_dup",
            SkipReason::SemanticDup => "semantic_dup",
            SkipReason::OcrFailed => "ocr_failed",
            SkipReason::LowSignalText => "low_signal_text",
            SkipReason::Noise => "noise",
            SkipReason::Grounding => "grounding",
            SkipReason::StackedExtraction => "stacked_extraction",
            SkipReason::VisualSmall => "visual_small",
            SkipReason::VisualNovelty => "visual_novelty",
            SkipReason::VisualComposeFailed => "visual_compose_failed",
            SkipReason::ScreenCaptureFailed => "screen_capture_failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreOutcome {
    OcrPath,
    VisualPath,
    UrlOnly,
}

impl Default for CapturePipelineStats {
    fn default() -> Self {
        Self {
            evaluated: AtomicU64::new(0),
            skipped_blocklist: AtomicU64::new(0),
            skipped_self_app: AtomicU64::new(0),
            skipped_surface_policy: AtomicU64::new(0),
            skipped_perceptual_dup: AtomicU64::new(0),
            skipped_semantic_dup: AtomicU64::new(0),
            skipped_ocr_failed: AtomicU64::new(0),
            skipped_low_signal_text: AtomicU64::new(0),
            skipped_noise: AtomicU64::new(0),
            skipped_grounding: AtomicU64::new(0),
            skipped_stacked_extraction: AtomicU64::new(0),
            skipped_visual_small: AtomicU64::new(0),
            skipped_visual_novelty: AtomicU64::new(0),
            skipped_visual_compose_failed: AtomicU64::new(0),
            skipped_screen_capture_failed: AtomicU64::new(0),
            stored_ocr_path: AtomicU64::new(0),
            stored_visual_path: AtomicU64::new(0),
            stored_url_only: AtomicU64::new(0),
            last_skip: parking_lot::RwLock::new(None),
        }
    }
}

impl CapturePipelineStats {
    pub fn record_skip(&self, reason: SkipReason, app_name: &str) {
        let counter = match reason {
            SkipReason::SelfApp => &self.skipped_self_app,
            SkipReason::Blocklist => &self.skipped_blocklist,
            SkipReason::SurfacePolicy => &self.skipped_surface_policy,
            SkipReason::PerceptualDup => &self.skipped_perceptual_dup,
            SkipReason::SemanticDup => &self.skipped_semantic_dup,
            SkipReason::OcrFailed => &self.skipped_ocr_failed,
            SkipReason::LowSignalText => &self.skipped_low_signal_text,
            SkipReason::Noise => &self.skipped_noise,
            SkipReason::Grounding => &self.skipped_grounding,
            SkipReason::StackedExtraction => &self.skipped_stacked_extraction,
            SkipReason::VisualSmall => &self.skipped_visual_small,
            SkipReason::VisualNovelty => &self.skipped_visual_novelty,
            SkipReason::VisualComposeFailed => &self.skipped_visual_compose_failed,
            SkipReason::ScreenCaptureFailed => &self.skipped_screen_capture_failed,
        };
        counter.fetch_add(1, Ordering::Relaxed);
        *self.last_skip.write() = Some(LastSkipEntry {
            reason: reason.as_str().to_string(),
            app_name: app_name.to_string(),
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
        });
    }

    pub fn record_store(&self, outcome: StoreOutcome) {
        let counter = match outcome {
            StoreOutcome::OcrPath => &self.stored_ocr_path,
            StoreOutcome::VisualPath => &self.stored_visual_path,
            StoreOutcome::UrlOnly => &self.stored_url_only,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_evaluated(&self) {
        self.evaluated.fetch_add(1, Ordering::Relaxed);
    }

    /// Sum of every `skipped_*` counter. Cheap (atomic reads only).
    pub fn total_skipped(&self) -> u64 {
        self.skipped_blocklist.load(Ordering::Relaxed)
            + self.skipped_self_app.load(Ordering::Relaxed)
            + self.skipped_surface_policy.load(Ordering::Relaxed)
            + self.skipped_perceptual_dup.load(Ordering::Relaxed)
            + self.skipped_semantic_dup.load(Ordering::Relaxed)
            + self.skipped_ocr_failed.load(Ordering::Relaxed)
            + self.skipped_low_signal_text.load(Ordering::Relaxed)
            + self.skipped_noise.load(Ordering::Relaxed)
            + self.skipped_grounding.load(Ordering::Relaxed)
            + self.skipped_stacked_extraction.load(Ordering::Relaxed)
            + self.skipped_visual_small.load(Ordering::Relaxed)
            + self.skipped_visual_novelty.load(Ordering::Relaxed)
            + self.skipped_visual_compose_failed.load(Ordering::Relaxed)
            + self.skipped_screen_capture_failed.load(Ordering::Relaxed)
    }

    /// Sum of every `stored_*` counter.
    pub fn total_stored(&self) -> u64 {
        self.stored_ocr_path.load(Ordering::Relaxed)
            + self.stored_visual_path.load(Ordering::Relaxed)
            + self.stored_url_only.load(Ordering::Relaxed)
    }
}

/// Application state shared across threads
pub struct AppState {
    pub app_data_dir: PathBuf,
    pub config: RwLock<Config>,
    pub store: Arc<Store>,
    pub state_store: Arc<StateStore>,
    pub graph: GraphStore,
    pub is_paused: AtomicBool,
    pub is_incognito: AtomicBool,
    pub frames_captured: AtomicU64,
    pub frames_dropped: AtomicU64,
    pub last_capture_time: AtomicU64,
    /// Per-reason capture pipeline accounting. See [`CapturePipelineStats`].
    /// Legacy `frames_captured` / `frames_dropped` remain for MCP/IPC
    /// compatibility but only reflect a subset of paths.
    pub capture_stats: CapturePipelineStats,
    pub inference: RwLock<Option<Arc<InferenceEngine>>>,
    /// Vision Language Model for intelligent screen analysis (optional)
    pub vlm: RwLock<Option<Arc<VlmEngine>>>,
    inference_init: AsyncMutex<()>,
    /// One-frame-at-a-time gate. Any code path that drives the LLM, VLM,
    /// MTMD, BGE batch embedding, or CLIP batch embedding must hold this
    /// lock for the duration of the call. This serializes heavy model
    /// execution across the capture loop, IPC imports (Meta Glasses, file
    /// picker), and any other consumer so the Metal backend never sees more
    /// than one tenant at a time.
    ///
    /// We deliberately favor a single coarse-grained async mutex over a
    /// semaphore: the user's stated invariant is "pause the capture, run
    /// one extraction, restart capture, stay light and even throughout",
    /// and a binary lock encodes exactly that pause/run/release shape.
    pub model_pipeline_lock: AsyncMutex<()>,
    /// Cached stats: (result, computed_at_ms). Invalidated by stats_dirty.
    pub stats_cache: RwLock<Option<(Stats, i64)>>,
    pub stats_dirty: AtomicBool,
    /// Cached app-name list: (result, computed_at_ms).
    pub app_names_cache: RwLock<Option<(Vec<String>, i64)>>,
    /// Most recent text embedding from the capture loop — used by proactive surface.
    pub last_embedding: RwLock<Vec<f32>>,
    pub proactive_tx: tokio::sync::watch::Sender<Option<ProactiveSuggestion>>,
    pub proactive_rx: tokio::sync::watch::Receiver<Option<ProactiveSuggestion>>,

    // ── Focus Mode (Tier-2 drift detection) ──────────────────────────────────
    /// Description of the task the user is trying to stay focused on.
    pub focus_task: RwLock<Option<String>>,
    /// Embedding of `focus_task` used to compute cosine similarity each capture.
    pub focus_task_embedding: RwLock<Option<Vec<f32>>>,
    /// Counter of consecutive off-task captures. Resets on an on-task capture.
    pub focus_drift_count: std::sync::atomic::AtomicU32,

    // Privacy state memory
    pub pending_privacy_alerts: RwLock<Vec<PrivacyAlert>>,
    /// Key: domain_or_title, Value: snooze expiration timestamp (sec)
    pub snoozed_privacy_alerts: RwLock<std::collections::HashMap<String, i64>>,
    /// Active context runtime subscriptions (session_ids).
    pub runtime_subscriptions: RwLock<std::collections::HashSet<String>>,
    pub app_handle: RwLock<Option<tauri::AppHandle>>,
    /// High-confidence graph extractions waiting for idle Lance commit.
    pub pending_graph_updates: Mutex<Vec<PendingGraphUpdate>>,
    /// Extractions below the auto-commit confidence threshold (never auto-written).
    pub low_confidence_graph_candidates: Mutex<Vec<PendingGraphUpdate>>,
    /// When true, idle graph commit treats the machine as battery-saver tier.
    pub graph_governor_battery_saver: AtomicBool,
}

impl AppState {
    pub fn new(
        app_data_dir: PathBuf,
        config: Config,
        store: Arc<Store>,
        state_store: Arc<StateStore>,
        graph: GraphStore,
        inference: Option<Arc<InferenceEngine>>,
        vlm: Option<Arc<VlmEngine>>,
    ) -> Self {
        let (proactive_tx, proactive_rx) = tokio::sync::watch::channel(None);
        Self {
            app_data_dir,
            config: RwLock::new(config),
            store,
            state_store,
            graph,
            is_paused: AtomicBool::new(false),
            is_incognito: AtomicBool::new(false),
            frames_captured: AtomicU64::new(0),
            frames_dropped: AtomicU64::new(0),
            last_capture_time: AtomicU64::new(0),
            capture_stats: CapturePipelineStats::default(),
            inference: RwLock::new(inference),
            vlm: RwLock::new(vlm),
            inference_init: AsyncMutex::new(()),
            model_pipeline_lock: AsyncMutex::new(()),
            stats_cache: RwLock::new(None),
            stats_dirty: AtomicBool::new(false),
            app_names_cache: RwLock::new(None),
            last_embedding: RwLock::new(Vec::new()),
            proactive_tx,
            proactive_rx,
            focus_task: RwLock::new(None),
            focus_task_embedding: RwLock::new(None),
            focus_drift_count: std::sync::atomic::AtomicU32::new(0),
            pending_privacy_alerts: RwLock::new(Vec::new()),
            snoozed_privacy_alerts: RwLock::new(std::collections::HashMap::new()),
            runtime_subscriptions: RwLock::new(std::collections::HashSet::new()),
            app_handle: RwLock::new(None),
            pending_graph_updates: Mutex::new(Vec::new()),
            low_confidence_graph_candidates: Mutex::new(Vec::new()),
            graph_governor_battery_saver: AtomicBool::new(false),
        }
    }

    /// Queue graph extraction from a flushed memory row (normalized like Lance indexing).
    pub fn enqueue_graph_from_flushed_memory(&self, record: &storage::MemoryRecord) {
        let normalized = storage::normalize_record_for_index(record);
        let memory_id = normalized.id.clone();
        let ex = capture::entity_extractor::extract(&normalized);
        let node_count = ex.nodes.len();
        let edge_count = ex.edges.len();
        let overall = ex.overall_confidence;
        let update = PendingGraphUpdate {
            memory_id: memory_id.clone(),
            nodes: ex.nodes,
            edges: ex.edges,
            overall_confidence: overall,
        };
        if overall >= 0.5 {
            tracing::info!(
                target: "fndr::graph_queue",
                memory_id = %memory_id,
                node_count,
                edge_count,
                overall,
                queue = "pending_graph_updates",
                "graph_extraction queued"
            );
            self.pending_graph_updates.lock().push(update);
        } else {
            tracing::info!(
                target: "fndr::graph_queue",
                memory_id = %memory_id,
                node_count,
                edge_count,
                overall,
                queue = "low_confidence_graph_candidates",
                "graph_extraction queued"
            );
            self.low_confidence_graph_candidates.lock().push(update);
        }
    }

    pub fn set_app_handle(&self, handle: tauri::AppHandle) {
        *self.app_handle.write() = Some(handle);
    }

    pub fn pause(&self) {
        self.is_paused.store(true, Ordering::SeqCst);
        tracing::info!("Capture paused");
    }

    pub fn resume(&self) {
        self.is_paused.store(false, Ordering::SeqCst);
        tracing::info!("Capture resumed");
    }

    pub fn is_capturing(&self) -> bool {
        !self.is_paused.load(Ordering::SeqCst) && !self.is_incognito.load(Ordering::SeqCst)
    }

    pub fn inference_engine(&self) -> Option<Arc<InferenceEngine>> {
        self.inference.read().clone()
    }

    pub fn vlm_engine(&self) -> Option<Arc<VlmEngine>> {
        self.vlm.read().clone()
    }

    pub fn ai_model_loaded(&self) -> bool {
        self.inference.read().is_some()
    }

    pub fn ai_model_available(&self) -> bool {
        let preferred_model_id = self.inference_preferred_model_id();
        models::resolve_model(
            preferred_model_id.as_deref(),
            Some(self.app_data_dir.as_path()),
        )
        .is_some()
    }

    /// Raw onboarding selection (may disagree with `config.vlm_model_size`).
    pub fn preferred_model_id(&self) -> Option<String> {
        models::preferred_model_id_from_onboarding(self.app_data_dir.as_path())
    }

    /// GGUF id used to load [`InferenceEngine`], aligned with `config.vlm_model_size`.
    pub fn inference_preferred_model_id(&self) -> Option<String> {
        let config = self.config.read();
        models::inference_preferred_model_id(self.app_data_dir.as_path(), &config)
    }

    pub fn loaded_model_id(&self) -> Option<String> {
        self.inference
            .read()
            .as_ref()
            .map(|engine| engine.model_id().to_string())
    }

    pub fn replace_ai_engines(
        &self,
        inference: Option<Arc<InferenceEngine>>,
        vlm: Option<Arc<VlmEngine>>,
    ) {
        *self.inference.write() = inference;
        *self.vlm.write() = vlm;
    }

    pub fn invalidate_memory_derived_caches(&self) {
        self.stats_dirty.store(true, Ordering::SeqCst);
        *self.stats_cache.write() = None;
        *self.app_names_cache.write() = None;
    }

    pub async fn ensure_inference_engine(&self) -> Result<Option<Arc<InferenceEngine>>, String> {
        if let Some(engine) = self.inference_engine() {
            return Ok(Some(engine));
        }

        let preferred_model_id = self.inference_preferred_model_id();
        if models::resolve_model(
            preferred_model_id.as_deref(),
            Some(self.app_data_dir.as_path()),
        )
        .is_none()
        {
            return Ok(None);
        }

        let _guard = self.inference_init.lock().await;
        if let Some(engine) = self.inference_engine() {
            return Ok(Some(engine));
        }

        let engine = InferenceEngine::new(Some(self.app_data_dir.clone()), preferred_model_id)
            .await
            .map_err(|err| err.to_string())?;
        let engine = Arc::new(engine);
        *self.inference.write() = Some(engine.clone());
        Ok(Some(engine))
    }
}

pub async fn load_ai_engines(app_data_dir: &Path, config: &Config) -> LoadedAiEngines {
    let preferred_model_id = models::inference_preferred_model_id(app_data_dir, config);
    let inference =
        match InferenceEngine::new(Some(app_data_dir.to_path_buf()), preferred_model_id).await {
            Ok(engine) => {
                tracing::info!(
                    "AI inference engine initialized successfully with {}",
                    engine.model_id()
                );
                Some(Arc::new(engine))
            }
            Err(err) => {
                tracing::warn!("AI inference initialization failed: {}", err);
                None
            }
        };

    tracing::info!("Skipping eager VLM warm-up; VLM loads on demand.");
    let vlm = None;

    LoadedAiEngines { inference, vlm }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_stats_record_skip_increments_correct_counter_and_last_entry() {
        let stats = CapturePipelineStats::default();
        stats.record_skip(SkipReason::Grounding, "Cursor");
        stats.record_skip(SkipReason::Grounding, "Cursor");
        stats.record_skip(SkipReason::Noise, "Google Chrome");
        stats.record_skip(SkipReason::LowSignalText, "Instagram");

        assert_eq!(stats.skipped_grounding.load(Ordering::Relaxed), 2);
        assert_eq!(stats.skipped_noise.load(Ordering::Relaxed), 1);
        assert_eq!(stats.skipped_low_signal_text.load(Ordering::Relaxed), 1);
        assert_eq!(stats.total_skipped(), 4);

        let last = stats.last_skip.read().clone().expect("last skip set");
        assert_eq!(last.reason, "low_signal_text");
        assert_eq!(last.app_name, "Instagram");
    }

    #[test]
    fn capture_stats_record_store_does_not_touch_skip_counters() {
        let stats = CapturePipelineStats::default();
        stats.record_store(StoreOutcome::OcrPath);
        stats.record_store(StoreOutcome::OcrPath);
        stats.record_store(StoreOutcome::VisualPath);
        stats.record_store(StoreOutcome::UrlOnly);

        assert_eq!(stats.stored_ocr_path.load(Ordering::Relaxed), 2);
        assert_eq!(stats.stored_visual_path.load(Ordering::Relaxed), 1);
        assert_eq!(stats.stored_url_only.load(Ordering::Relaxed), 1);
        assert_eq!(stats.total_stored(), 4);
        assert_eq!(stats.total_skipped(), 0);
        assert!(stats.last_skip.read().is_none());
    }

    #[test]
    fn capture_stats_evaluated_is_independent_of_outcome() {
        let stats = CapturePipelineStats::default();
        for _ in 0..5 {
            stats.record_evaluated();
        }
        stats.record_skip(SkipReason::Blocklist, "Slack");
        stats.record_store(StoreOutcome::OcrPath);

        assert_eq!(stats.evaluated.load(Ordering::Relaxed), 5);
        assert_eq!(stats.total_skipped(), 1);
        assert_eq!(stats.total_stored(), 1);
    }

    #[test]
    fn skip_reason_as_str_matches_known_ipc_labels() {
        // These strings are what the UI reads via `last_skip_reason`;
        // keep them stable so renaming a variant doesn't silently break
        // the inspector chips.
        assert_eq!(SkipReason::Grounding.as_str(), "grounding");
        assert_eq!(SkipReason::SemanticDup.as_str(), "semantic_dup");
        assert_eq!(SkipReason::VisualNovelty.as_str(), "visual_novelty");
        assert_eq!(
            SkipReason::ScreenCaptureFailed.as_str(),
            "screen_capture_failed"
        );
        assert_eq!(SkipReason::SelfApp.as_str(), "self_app");
    }

    #[test]
    fn capture_stats_self_app_counts_separately_from_blocklist() {
        let stats = CapturePipelineStats::default();
        stats.record_skip(SkipReason::SelfApp, "FNDR");
        stats.record_skip(SkipReason::SelfApp, "FNDR");
        stats.record_skip(SkipReason::Blocklist, "1Password");
        assert_eq!(stats.skipped_self_app.load(Ordering::Relaxed), 2);
        assert_eq!(stats.skipped_blocklist.load(Ordering::Relaxed), 1);
        assert_eq!(stats.total_skipped(), 3);
    }
}
