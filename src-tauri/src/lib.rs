//! FNDR Library
//!
//! Core functionality for the FNDR memory search application.
#![recursion_limit = "512"]

pub mod accessibility;
pub mod capture;
pub mod config;
pub mod context_runtime;
pub mod downloads;
pub mod embedding;
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
    pub inference: RwLock<Option<Arc<InferenceEngine>>>,
    /// Vision Language Model for intelligent screen analysis (optional)
    pub vlm: RwLock<Option<Arc<VlmEngine>>>,
    inference_init: AsyncMutex<()>,
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
            inference: RwLock::new(inference),
            vlm: RwLock::new(vlm),
            inference_init: AsyncMutex::new(()),
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

    tracing::info!(
        "Skipping eager VLM warm-up; VLM loads on demand (Settings tier: {}).",
        config.vlm_model_size
    );
    let vlm = None;

    LoadedAiEngines { inference, vlm }
}
