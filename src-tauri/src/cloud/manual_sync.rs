//! Manual + scheduled "sync now" — push recent local memories to the team
//! graph on demand (a button) or once a day (a scheduler), independent of the
//! continuous capture-driven worker in [`crate::cloud::sync`].
//!
//! Clicking the button (or the daily run) is an explicit share action, so it
//! **bypasses the cluster/manager policy gate**. The per-observation safety
//! floor still applies: `BLOCKED` / `LOCAL_ONLY` content never leaves the
//! device. Each push reuses the same descriptor → classify → scrub → embed →
//! `agent-sync` path as the live worker, so embeddings stay consistent.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::cloud::descriptor::Descriptor;
use crate::cloud::embed::{embed_descriptor_bge, to_cloud_embedding};
use crate::cloud::share_policy::{classify, scrub, ClassifyCtx, ShareDecision};
use crate::cloud::sync::{push_node, CloudSyncJob};
use crate::cloud::{self, CloudConfig};
use crate::embedding::Embedder;
use crate::inference::model_config::{BGE_V5_MODEL_ID, EMBEDDING_MODEL_ID};
use crate::AppState;

/// Default lookback for the manual button: the local retention window (7 days).
pub const MANUAL_SYNC_WINDOW_HOURS: u32 = 24 * 7;
/// Daily scheduler pushes the previous day's memories.
pub const DAILY_SYNC_WINDOW_HOURS: u32 = 24;

/// Outcome of a sync-now run, surfaced to the UI.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ManualSyncReport {
    /// Records that yielded a shareable descriptor and were considered.
    pub considered: usize,
    pub pushed: usize,
    pub skipped_blocked: usize,
    pub skipped_local_only: usize,
    /// Records with no shareable content (metadata-only / empty).
    pub skipped_empty: usize,
    /// Already pushed within the dedup window this session.
    pub skipped_duplicate: usize,
    pub failed: usize,
}

/// Push memories captured in the last `hours` to the team graph.
pub async fn sync_now(state: &AppState, hours: u32) -> Result<ManualSyncReport, String> {
    let cfg = CloudConfig::from_env().ok_or_else(|| "Cloud sync is not configured.".to_string())?;
    let session = cloud::ensure_fresh_session(&cfg).await?;
    let identity = cloud::auth::resolve_identity(&cfg, &session).await?;
    let cluster_id = identity
        .cluster_id
        .ok_or_else(|| "You haven't joined a cluster yet.".to_string())?;

    let records = state
        .store
        .get_recent_memories(hours)
        .await
        .map_err(|e| format!("Couldn't read local memories: {e}"))?;

    let client = reqwest::Client::builder()
        .user_agent("Continuum/1.0")
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("HTTP client init failed: {e}"))?;

    // Use the richer BGE-large embedder when its model is present; otherwise the
    // record's local vector projected to the cluster width.
    let bge = Embedder::new_bge_v5_for_query().ok();
    let blocklist = state.config.read().blocklist.clone();
    let private_mode = state.is_incognito.load(Ordering::SeqCst);
    let now_ms = chrono::Utc::now().timestamp_millis();

    let mut report = ManualSyncReport::default();
    for record in records {
        let Some(descriptor) = Descriptor::from_memory_record(&record) else {
            report.skipped_empty += 1;
            continue;
        };
        report.considered += 1;

        let ctx = ClassifyCtx {
            bundle_id: record.bundle_id.as_deref(),
            app_name: Some(record.app_name.as_str()),
            url: record.url.as_deref(),
            window_title: Some(record.window_title.as_str()),
            private_mode,
            user_blocklist: &blocklist,
        };
        // Safety floor still applies; the cluster policy gate is intentionally
        // skipped (the manual/daily action is the explicit consent).
        match classify(&descriptor, &ctx) {
            ShareDecision::Blocked => {
                report.skipped_blocked += 1;
                continue;
            }
            ShareDecision::LocalOnly => {
                report.skipped_local_only += 1;
                continue;
            }
            ShareDecision::SharedAnon => {}
        }

        let scrubbed = scrub(&descriptor);
        let content_hash = scrubbed.content_hash();
        if state.cloud_sync.dedup.is_duplicate_at(&content_hash, now_ms) {
            report.skipped_duplicate += 1;
            continue;
        }

        let (embedding, embed_model) = embed_for(state, bge.as_ref(), &scrubbed, &record.embedding).await;
        let job = CloudSyncJob {
            descriptor: scrubbed,
            content_hash,
            enqueued_at_ms: now_ms,
            embedding,
            embed_model,
        };
        match push_node(&client, &cfg, &session.access_token, &cluster_id, &job).await {
            Ok(()) => report.pushed += 1,
            Err(e) => {
                report.failed += 1;
                tracing::debug!(target: "continuum::cloud_sync", "manual push failed: {e}");
            }
        }
    }

    tracing::info!(
        target: "continuum::cloud_sync",
        considered = report.considered,
        pushed = report.pushed,
        failed = report.failed,
        "manual/daily sync complete"
    );
    Ok(report)
}

/// Pick the embedding for a descriptor: BGE-large (under the model lock) when
/// available, else the record's local vector projected to the cluster width.
async fn embed_for(
    state: &AppState,
    bge: Option<&Embedder>,
    descriptor: &Descriptor,
    fallback: &[f32],
) -> (Vec<f32>, String) {
    if let Some(embedder) = bge {
        let _guard = state.model_pipeline_lock.lock().await;
        if let Some(v) = embed_descriptor_bge(embedder, descriptor) {
            return (v, BGE_V5_MODEL_ID.to_string());
        }
    }
    (to_cloud_embedding(fallback), EMBEDDING_MODEL_ID.to_string())
}

/// Spawn the daily auto-sync. Wakes hourly and runs [`sync_now`] once per UTC
/// day when signed in and joined to a cluster. Robust to restarts (the per-day
/// guard is recomputed from the wall clock, not a timer).
pub fn spawn_daily_scheduler(state: Arc<AppState>) {
    // Use Tauri's runtime handle, not a bare `tokio::spawn`: this is invoked
    // from the app setup on the main thread, where `tokio::spawn` has no
    // reactor in scope and would panic (aborting launch).
    tauri::async_runtime::spawn(async move {
        let mut last_run_day: Option<String> = None;
        loop {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
            if last_run_day.as_deref() == Some(today.as_str()) {
                continue;
            }
            if !state.cloud_sync.runtime.read().ready() {
                continue;
            }
            match sync_now(&state, DAILY_SYNC_WINDOW_HOURS).await {
                Ok(report) => {
                    last_run_day = Some(today);
                    tracing::info!(
                        target: "continuum::cloud_sync",
                        pushed = report.pushed,
                        "daily auto-sync ran"
                    );
                }
                Err(e) => tracing::debug!(target: "continuum::cloud_sync", "daily auto-sync skipped: {e}"),
            }
        }
    });
}
