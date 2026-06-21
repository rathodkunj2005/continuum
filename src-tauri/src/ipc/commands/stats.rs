//! Capture status, MCP/context, voice/capture toggles, stats, daily summary, time/focus.

use super::common::{shared_embedder, strip_internal_continuum_results, truncate_chars};
use super::search::{
    cache_is_fresh, card_domain, card_summary, is_low_signal_summary, is_low_signal_title,
    title_from_summary,
};
use crate::context_runtime;
use crate::embedding::{embedding_runtime_status, Embedder, EmbeddingBackend};
use crate::mcp::{self, McpServerStatus};
use crate::privacy::Blocklist;
use crate::speech;
use crate::storage::{SearchResult, Stats};
use crate::AppState;
use chrono::TimeZone;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

/// Per-reason capture pipeline breakdown surfaced to the UI.
///
/// Mirrors [`crate::CapturePipelineStats`] one-for-one so the inspector can
/// show "stored vs skipped (with reasons)" without parsing the JSONL signals
/// file on every poll.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapturePipelineBreakdown {
    pub evaluated: u64,
    pub stored_ocr_path: u64,
    pub stored_visual_path: u64,
    pub stored_url_only: u64,
    pub stored_total: u64,
    pub skipped_blocklist: u64,
    pub skipped_self_app: u64,
    pub skipped_surface_policy: u64,
    pub skipped_perceptual_dup: u64,
    pub skipped_semantic_dup: u64,
    pub skipped_ocr_failed: u64,
    pub skipped_low_signal_text: u64,
    pub skipped_noise: u64,
    pub skipped_grounding: u64,
    pub skipped_stacked_extraction: u64,
    pub skipped_visual_small: u64,
    pub skipped_visual_novelty: u64,
    pub skipped_visual_compose_failed: u64,
    pub skipped_screen_capture_failed: u64,
    pub skipped_total: u64,
    pub last_skip_reason: Option<String>,
    pub last_skip_app: Option<String>,
    pub last_skip_timestamp_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureStatus {
    pub is_capturing: bool,
    pub is_paused: bool,
    pub is_incognito: bool,
    pub frames_captured: u64,
    pub frames_dropped: u64,
    pub last_capture_time: u64,
    pub ai_model_available: bool,
    pub ai_model_loaded: bool,
    pub loaded_model_id: Option<String>,
    pub embedding_backend: String,
    pub embedding_degraded: bool,
    pub embedding_detail: String,
    pub embedding_model_name: String,
    pub embedding_dimension: usize,
    pub pipeline: CapturePipelineBreakdown,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceTranscriptionResult {
    pub text: String,
    pub backend: String,
}
#[tauri::command]
pub async fn get_status(state: State<'_, Arc<AppState>>) -> Result<CaptureStatus, String> {
    Ok(build_capture_status(state.inner()))
}

pub fn build_capture_status(state: &AppState) -> CaptureStatus {
    let embed_status = embedding_runtime_status();
    CaptureStatus {
        is_capturing: state.is_capturing(),
        is_paused: state.is_paused.load(Ordering::SeqCst),
        is_incognito: state.is_incognito.load(Ordering::SeqCst),
        frames_captured: state.frames_captured.load(Ordering::Relaxed),
        frames_dropped: state.frames_dropped.load(Ordering::Relaxed),
        last_capture_time: state.last_capture_time.load(Ordering::Relaxed),
        ai_model_available: state.ai_model_available(),
        ai_model_loaded: state.ai_model_loaded(),
        loaded_model_id: state.loaded_model_id(),
        embedding_backend: embed_status.backend,
        embedding_degraded: embed_status.degraded,
        embedding_detail: embed_status.detail,
        embedding_model_name: embed_status.model_name,
        embedding_dimension: embed_status.dimension,
        pipeline: capture_pipeline_breakdown(state),
    }
}

/// Push the current capture status to the UI over `capture://status`.
///
/// No-op before the app handle is registered during setup.
pub fn emit_capture_status(state: &AppState) {
    let handle = state.app_handle.read().clone();
    if let Some(handle) = handle {
        let _ = handle.emit("capture://status", build_capture_status(state));
    }
}

#[tauri::command]
pub async fn get_memory_review_status(
    state: State<'_, Arc<AppState>>,
) -> Result<crate::memory_review::MemoryReviewWorkerStatus, String> {
    Ok(crate::memory_review::worker_status(state.inner()))
}

/// Snapshot the per-reason capture counters into a flat IPC payload.
///
/// All reads are `Relaxed` atomic loads, so this is cheap and safe to call on
/// every UI poll. The `last_skip_*` fields are taken under a short read lock
/// on the stats' `parking_lot::RwLock`.
pub fn capture_pipeline_breakdown(state: &AppState) -> CapturePipelineBreakdown {
    let s = &state.capture_stats;
    let last_skip = s.last_skip.read().clone();
    CapturePipelineBreakdown {
        evaluated: s.evaluated.load(Ordering::Relaxed),
        stored_ocr_path: s.stored_ocr_path.load(Ordering::Relaxed),
        stored_visual_path: s.stored_visual_path.load(Ordering::Relaxed),
        stored_url_only: s.stored_url_only.load(Ordering::Relaxed),
        stored_total: s.total_stored(),
        skipped_blocklist: s.skipped_blocklist.load(Ordering::Relaxed),
        skipped_self_app: s.skipped_self_app.load(Ordering::Relaxed),
        skipped_surface_policy: s.skipped_surface_policy.load(Ordering::Relaxed),
        skipped_perceptual_dup: s.skipped_perceptual_dup.load(Ordering::Relaxed),
        skipped_semantic_dup: s.skipped_semantic_dup.load(Ordering::Relaxed),
        skipped_ocr_failed: s.skipped_ocr_failed.load(Ordering::Relaxed),
        skipped_low_signal_text: s.skipped_low_signal_text.load(Ordering::Relaxed),
        skipped_noise: s.skipped_noise.load(Ordering::Relaxed),
        skipped_grounding: s.skipped_grounding.load(Ordering::Relaxed),
        skipped_stacked_extraction: s.skipped_stacked_extraction.load(Ordering::Relaxed),
        skipped_visual_small: s.skipped_visual_small.load(Ordering::Relaxed),
        skipped_visual_novelty: s.skipped_visual_novelty.load(Ordering::Relaxed),
        skipped_visual_compose_failed: s.skipped_visual_compose_failed.load(Ordering::Relaxed),
        skipped_screen_capture_failed: s.skipped_screen_capture_failed.load(Ordering::Relaxed),
        skipped_total: s.total_skipped(),
        last_skip_reason: last_skip.as_ref().map(|e| e.reason.clone()),
        last_skip_app: last_skip.as_ref().map(|e| e.app_name.clone()),
        last_skip_timestamp_ms: last_skip.as_ref().map(|e| e.timestamp_ms),
    }
}

/// Get MCP server status
#[tauri::command]
pub async fn get_mcp_server_status() -> Result<McpServerStatus, String> {
    Ok(mcp::status())
}

#[tauri::command]
pub async fn get_context_runtime_status(
    state: State<'_, Arc<AppState>>,
) -> Result<crate::storage::ContextRuntimeStatus, String> {
    context_runtime::get_context_runtime_status(state.inner()).await
}

#[tauri::command]
pub async fn list_recent_context_packs(
    state: State<'_, Arc<AppState>>,
    limit: Option<usize>,
) -> Result<Vec<crate::storage::ContextPack>, String> {
    context_runtime::list_recent_context_packs(state.inner(), limit.unwrap_or(8)).await
}

#[tauri::command]
pub async fn continuum_subscribe(
    state: State<'_, Arc<AppState>>,
    session_id: String,
) -> Result<bool, String> {
    state
        .runtime_subscriptions
        .write()
        .insert(session_id.clone());
    tracing::info!(session_id, "Context runtime subscription active");
    Ok(true)
}

#[tauri::command]
pub async fn continuum_unsubscribe(
    state: State<'_, Arc<AppState>>,
    session_id: String,
) -> Result<bool, String> {
    let removed = state.runtime_subscriptions.write().remove(&session_id);
    tracing::info!(session_id, removed, "Context runtime subscription removed");
    Ok(removed)
}

/// Start MCP server (optional custom port)
#[tauri::command]
pub async fn start_mcp_server(
    state: State<'_, Arc<AppState>>,
    port: Option<u16>,
) -> Result<McpServerStatus, String> {
    mcp::start(state.inner().clone(), None, port).await
}

/// Stop MCP server
#[tauri::command]
pub async fn stop_mcp_server() -> Result<McpServerStatus, String> {
    Ok(mcp::stop().await)
}

/// Transcribe a short voice input clip for voice search and voice control
#[tauri::command]
pub async fn transcribe_voice_input(
    app: AppHandle,
    audio_bytes: Vec<u8>,
    mime_type: Option<String>,
) -> Result<VoiceTranscriptionResult, String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let text =
        speech::transcribe_audio_bytes(&app_data_dir, &audio_bytes, mime_type.as_deref()).await?;

    Ok(VoiceTranscriptionResult {
        text,
        backend: "whisper-small-ggml (enhanced mic mode)".to_string(),
    })
}

/// Pause capture
#[tauri::command]
pub async fn pause_capture(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.inner().pause();
    emit_capture_status(state.inner());
    Ok(())
}

/// Resume capture
#[tauri::command]
pub async fn resume_capture(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.inner().resume();
    emit_capture_status(state.inner());
    Ok(())
}

/// Get statistics
#[tauri::command]
pub async fn get_stats(state: State<'_, Arc<AppState>>) -> Result<Stats, String> {
    let app_state = state.inner();
    if !app_state.stats_dirty.load(Ordering::Relaxed) {
        if let Some((stats, computed_at_ms)) = app_state.stats_cache.read().clone() {
            if cache_is_fresh(computed_at_ms) {
                return Ok(stats);
            }
        }
    }

    let stats = app_state
        .store
        .get_stats()
        .await
        .map_err(|e: Box<dyn std::error::Error>| e.to_string())?;
    *app_state.stats_cache.write() = Some((stats.clone(), chrono::Utc::now().timestamp_millis()));
    app_state.stats_dirty.store(false, Ordering::Relaxed);
    Ok(stats)
}

/// Get retention days (0 = keep forever)
#[tauri::command]
pub async fn get_retention_days(state: State<'_, Arc<AppState>>) -> Result<u32, String> {
    Ok(state.inner().config.read().retention_days)
}

/// Set retention days (0 = keep forever)
#[tauri::command]
pub async fn set_retention_days(state: State<'_, Arc<AppState>>, days: u32) -> Result<(), String> {
    let mut config = state.inner().config.write();
    config.retention_days = days;
    config
        .save()
        .map_err(|e: Box<dyn std::error::Error>| e.to_string())?;
    Ok(())
}

/// Get unique app names for filter dropdown
#[tauri::command]
pub async fn get_app_names(state: State<'_, Arc<AppState>>) -> Result<Vec<String>, String> {
    let app_state = state.inner();
    if let Some((apps, computed_at_ms)) = app_state.app_names_cache.read().clone() {
        if cache_is_fresh(computed_at_ms) {
            return Ok(apps);
        }
    }

    let mut apps = app_state
        .store
        .get_app_names()
        .await
        .map_err(|e: Box<dyn std::error::Error>| e.to_string())?;
    apps.retain(|name| !Blocklist::is_internal_app(name, None));
    *app_state.app_names_cache.write() =
        Some((apps.clone(), chrono::Utc::now().timestamp_millis()));
    Ok(apps)
}

/// Delete records older than the given number of days; returns count deleted
#[tauri::command]
pub async fn delete_older_than(
    state: State<'_, Arc<AppState>>,
    days: u32,
) -> Result<usize, String> {
    let deleted = state
        .inner()
        .store
        .delete_older_than(days)
        .await
        .map_err(|e: Box<dyn std::error::Error>| e.to_string())?;
    if deleted > 0 {
        state.invalidate_memory_derived_caches();
    }
    Ok(deleted)
}
// ── Daily Summary Commands ───────────────────────────────────────────

#[derive(Debug, Clone)]
struct DailyActivityCluster {
    app_name: String,
    label: String,
    first_ts: i64,
    last_ts: i64,
    count: usize,
    samples: Vec<String>,
}

impl DailyActivityCluster {
    fn add(&mut self, result: &SearchResult) {
        self.first_ts = self.first_ts.min(result.timestamp);
        self.last_ts = self.last_ts.max(result.timestamp);
        self.count += 1;

        let sample = daily_activity_sample(result);
        if sample.is_empty() {
            return;
        }

        let sample_key = sample.to_lowercase();
        let already_seen = self
            .samples
            .iter()
            .any(|existing| existing.to_lowercase() == sample_key);
        if !already_seen && self.samples.len() < 3 {
            self.samples.push(sample);
        }
    }
}

pub(crate) fn build_daily_activity_summary(records: &[SearchResult], day_label: &str) -> String {
    if records.is_empty() {
        return format!("No memories recorded for {day_label}.");
    }

    let mut sorted = records.to_vec();
    sorted.sort_by_key(|record| record.timestamp);

    let first_ts = sorted.first().map(|record| record.timestamp).unwrap_or(0);
    let last_ts = sorted
        .last()
        .map(|record| record.timestamp)
        .unwrap_or(first_ts);
    let span_ms = (last_ts - first_ts).max(0);
    let span_minutes = ((span_ms + 59_999) / 60_000).max(1);

    let mut clusters: HashMap<String, DailyActivityCluster> = HashMap::new();
    for result in &sorted {
        let key = daily_activity_key(result);
        let label = daily_activity_label(result);
        clusters
            .entry(key)
            .and_modify(|cluster| cluster.add(result))
            .or_insert_with(|| {
                let mut cluster = DailyActivityCluster {
                    app_name: result.app_name.clone(),
                    label,
                    first_ts: result.timestamp,
                    last_ts: result.timestamp,
                    count: 0,
                    samples: Vec::new(),
                };
                cluster.add(result);
                cluster
            });
    }

    let mut clusters = clusters.into_values().collect::<Vec<_>>();
    clusters.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| b.last_ts.cmp(&a.last_ts))
            .then_with(|| a.app_name.cmp(&b.app_name))
    });

    let mut lines = Vec::new();
    let memory_word = if sorted.len() == 1 {
        "memory"
    } else {
        "memories"
    };
    if sorted.len() == 1 {
        lines.push(format!(
            "- Continuum captured 1 memory {day_label} at {}.",
            format_local_time(first_ts)
        ));
    } else {
        lines.push(format!(
            "- Continuum captured {} {memory_word} across {} {day_label}, from {} to {}.",
            sorted.len(),
            human_duration_minutes(span_minutes),
            format_local_time(first_ts),
            format_local_time(last_ts)
        ));
    }

    let top_limit = if span_minutes <= 45 {
        3
    } else if span_minutes <= 240 {
        5
    } else {
        7
    };

    for cluster in clusters.iter().take(top_limit) {
        lines.push(daily_cluster_bullet(cluster));
    }

    if clusters.len() > top_limit {
        let mut app_counts: HashMap<String, usize> = HashMap::new();
        for cluster in clusters.iter().skip(top_limit) {
            *app_counts.entry(cluster.app_name.clone()).or_insert(0) += cluster.count;
        }
        let mut app_counts = app_counts.into_iter().collect::<Vec<_>>();
        app_counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let labels = app_counts
            .iter()
            .take(4)
            .map(|(app, count)| format!("{app} ({})", capture_count(*count)))
            .collect::<Vec<_>>();
        if !labels.is_empty() {
            lines.push(format!(
                "- Lighter activity also appeared in {}.",
                labels.join(", ")
            ));
        }
    }

    lines.join("\n")
}

fn daily_activity_key(result: &SearchResult) -> String {
    let label = daily_activity_label(result).to_lowercase();
    format!("{}|{}", result.app_name.to_lowercase(), label)
}

fn daily_activity_label(result: &SearchResult) -> String {
    if let Some(domain) = result.url.as_deref().and_then(card_domain) {
        return truncate_chars(&domain, 72);
    }

    let title = normalize_daily_fragment(&result.window_title);
    if !is_low_signal_title(&title, &result.app_name) {
        return truncate_chars(&title, 72);
    }

    let summary = card_summary(result);
    if let Some(title) = title_from_summary(&summary, &result.app_name) {
        return truncate_chars(&normalize_daily_fragment(&title), 72);
    }

    result.app_name.clone()
}

fn daily_activity_sample(result: &SearchResult) -> String {
    let summary = normalize_daily_fragment(&card_summary(result));
    if summary.is_empty() || is_low_signal_summary(&summary, &result.app_name) {
        return String::new();
    }

    let label = daily_activity_label(result).to_lowercase();
    if summary.to_lowercase() == label {
        return String::new();
    }

    truncate_chars(&summary.replace('"', "'"), 110)
}

fn daily_cluster_bullet(cluster: &DailyActivityCluster) -> String {
    let topic = if cluster.label.eq_ignore_ascii_case(&cluster.app_name) {
        "general activity".to_string()
    } else {
        format!("\"{}\"", cluster.label.replace('"', "'"))
    };

    let time_window = if cluster.first_ts == cluster.last_ts {
        format!("at {}", format_local_time(cluster.first_ts))
    } else {
        format!(
            "from {} to {}",
            format_local_time(cluster.first_ts),
            format_local_time(cluster.last_ts)
        )
    };

    let sample = cluster
        .samples
        .first()
        .map(|value| format!(", including \"{value}\""))
        .unwrap_or_default();

    format!(
        "- {}: {} {} around {}{}.",
        cluster.app_name,
        capture_count(cluster.count),
        time_window,
        topic,
        sample
    )
}

fn normalize_daily_fragment(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

fn capture_count(count: usize) -> String {
    if count == 1 {
        "1 capture".to_string()
    } else {
        format!("{count} captures")
    }
}

fn human_duration_minutes(minutes: i64) -> String {
    if minutes <= 1 {
        "about 1 minute".to_string()
    } else if minutes < 60 {
        format!("about {minutes} minutes")
    } else {
        let hours = minutes / 60;
        let rest = minutes % 60;
        if rest == 0 {
            format!("about {hours} hour{}", if hours == 1 { "" } else { "s" })
        } else {
            format!(
                "about {hours} hour{} {rest} minute{}",
                if hours == 1 { "" } else { "s" },
                if rest == 1 { "" } else { "s" }
            )
        }
    }
}

fn format_local_time(timestamp: i64) -> String {
    let raw = chrono::Local
        .timestamp_millis_opt(timestamp)
        .single()
        .unwrap_or_else(chrono::Local::now)
        .format("%I:%M %p")
        .to_string();
    raw.trim_start_matches('0').to_string()
}

fn daily_summary_day_label(target_day: chrono::NaiveDate) -> String {
    if target_day == chrono::Local::now().date_naive() {
        "today".to_string()
    } else {
        format!("on {}", target_day.format("%Y-%m-%d"))
    }
}

#[tauri::command]
pub async fn generate_daily_summary_for_date(
    state: State<'_, Arc<AppState>>,
    date_str: String,
) -> Result<String, String> {
    let target_day = chrono::NaiveDate::parse_from_str(&date_str, "%Y-%m-%d")
        .map_err(|e| format!("Invalid date format: {}", e))?;
    let start = target_day
        .and_hms_opt(0, 0, 0)
        .ok_or("Failed to create start time")?;
    let end = (target_day + chrono::Duration::days(1))
        .and_hms_opt(0, 0, 0)
        .ok_or("Failed to create end time")?;

    let start_ms = chrono::Local
        .from_local_datetime(&start)
        .earliest()
        .unwrap_or_else(|| chrono::Local.from_local_datetime(&start).latest().unwrap())
        .timestamp_millis();
    let end_ms = chrono::Local
        .from_local_datetime(&end)
        .earliest()
        .unwrap_or_else(|| chrono::Local.from_local_datetime(&end).latest().unwrap())
        .timestamp_millis()
        - 1;

    let records = state
        .store
        .get_search_results_in_range(start_ms, end_ms)
        .await
        .map_err(|e| e.to_string())?;
    let records = strip_internal_continuum_results(records);

    if records.is_empty() {
        return Ok("No memories recorded for this date.".to_string());
    }

    Ok(build_daily_activity_summary(
        &records,
        &daily_summary_day_label(target_day),
    ))
}

// ────────────────────────────────────────────────────────────────────────────
// Time Tracking
// ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct AppTimeEntry {
    pub app_name: String,
    pub duration_minutes: u32,
    pub capture_count: u32,
    pub last_seen: i64,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct TimeTrackingResult {
    pub date: String,
    pub total_captures: u32,
    pub breakdown: Vec<AppTimeEntry>,
}

/// Aggregate today's memory records into per-app time estimates.
///
/// Works by sorting each app's captures by timestamp and summing consecutive
/// inter-capture gaps (capped at 5 minutes so long idle periods don't bloat the total).
#[tauri::command]
pub async fn get_time_tracking(
    state: State<'_, Arc<AppState>>,
) -> Result<TimeTrackingResult, String> {
    use chrono::{Local, NaiveTime, TimeZone};
    use std::collections::HashMap;

    // Use a single clock snapshot so today_start_ms and now_ms are consistent.
    // Local::now() and Utc::now() both produce epoch milliseconds (timezone-independent),
    // but anchoring both to the same instant avoids any sub-second skew.
    let now_local = Local::now();
    let today = now_local.date_naive();
    let midnight = today.and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap());
    let today_start_ms = Local
        .from_local_datetime(&midnight)
        .earliest()
        .map(|dt| dt.timestamp_millis())
        .unwrap_or(0);
    let now_ms = now_local.timestamp_millis();

    let records = state
        .store
        .get_memories_in_range(today_start_ms, now_ms)
        .await
        .map_err(|e| e.to_string())?;

    // Group timestamps by app_name
    let mut app_timestamps: HashMap<String, Vec<i64>> = HashMap::new();
    for record in &records {
        app_timestamps
            .entry(record.app_name.clone())
            .or_default()
            .push(record.timestamp);
    }

    const MAX_GAP_MS: i64 = 5 * 60_000; // Cap idle gaps at 5 minutes
    const MIN_ACTIVITY_MS: i64 = 30_000; // Minimum 30 seconds credited per app

    let mut breakdown: Vec<AppTimeEntry> = app_timestamps
        .iter()
        .map(|(app, timestamps)| {
            let mut sorted = timestamps.clone();
            sorted.sort_unstable();

            let mut duration_ms: i64 = 0;
            for window in sorted.windows(2) {
                let gap = window[1] - window[0];
                duration_ms += gap.min(MAX_GAP_MS);
            }
            // Credit at least 30s for any app that had captures
            duration_ms = duration_ms.max(MIN_ACTIVITY_MS);

            AppTimeEntry {
                app_name: app.clone(),
                duration_minutes: ((duration_ms as f64) / 60_000.0).round() as u32,
                capture_count: sorted.len() as u32,
                last_seen: *sorted.last().unwrap_or(&0),
            }
        })
        .collect();

    breakdown.sort_by(|a, b| b.duration_minutes.cmp(&a.duration_minutes));

    Ok(TimeTrackingResult {
        date: today.format("%Y-%m-%d").to_string(),
        total_captures: records.len() as u32,
        breakdown,
    })
}

// ────────────────────────────────────────────────────────────────────────────
// Focus Mode
// ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct FocusStatus {
    pub task: Option<String>,
    pub is_active: bool,
    pub drift_count: u32,
}

/// Set or clear the current focus task.
///
/// When set, the capture loop embeds the task description once and compares
/// every incoming capture against it (cosine similarity). Three consecutive
/// off-task captures surface a ProactiveSuggestion drift alert.
#[tauri::command]
pub async fn set_focus_task(
    task: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<FocusStatus, String> {
    // Always clear embedding first so the capture loop never sees a stale
    // embedding paired with a new task (or vice-versa). The brief window where
    // embedding is None means the loop skips drift detection for at most one
    // capture cycle — an acceptable trade-off for consistency.
    *state.focus_task_embedding.write() = None;
    *state.focus_task.write() = task.clone();
    state.focus_drift_count.store(0, Ordering::Relaxed);

    if let Some(ref t) = task {
        let embedder = shared_embedder().ok();
        if let Some(embedding) = build_focus_task_embedding(t, embedder)? {
            *state.focus_task_embedding.write() = Some(embedding);
        } else {
            let status = embedding_runtime_status();
            tracing::info!(
                backend = %status.backend,
                degraded = status.degraded,
                detail = %status.detail,
                "Focus drift detection disabled because semantic embeddings are unavailable"
            );
        }
    }

    Ok(FocusStatus {
        task,
        is_active: state.focus_task.read().is_some(),
        drift_count: 0,
    })
}

pub(crate) fn build_focus_task_embedding(
    task: &str,
    embedder: Option<&Embedder>,
) -> Result<Option<Vec<f32>>, String> {
    if !matches!(
        embedder.map(|value| value.backend()),
        Some(EmbeddingBackend::Real)
    ) {
        return Ok(None);
    }

    embedder
        .and_then(|value| value.embed_batch(&[task.to_string()]).ok())
        .and_then(|mut embeddings| embeddings.drain(..).next())
        .map(Some)
        .ok_or_else(|| "Failed to build focus task embedding".to_string())
}

/// Return the current focus task and drift counter.
#[tauri::command]
pub fn get_focus_status(state: State<'_, Arc<AppState>>) -> Result<FocusStatus, String> {
    let task = state.focus_task.read().clone();
    let is_active = task.is_some();
    let drift_count = state.focus_drift_count.load(Ordering::Relaxed);
    Ok(FocusStatus {
        task,
        is_active,
        drift_count,
    })
}

/// Engine latency counters, capture counters, optional process RSS (macOS). No PII.
#[tauri::command]
pub async fn get_runtime_metrics(
    state: State<'_, Arc<AppState>>,
) -> Result<crate::telemetry::runtime_metrics::RuntimeMetricsSnapshot, String> {
    let emb = embedding_runtime_status();
    let embedding = crate::telemetry::runtime_metrics::EmbeddingMetricsSnapshot {
        backend: emb.backend,
        degraded: emb.degraded,
        detail: emb.detail,
        model_name: emb.model_name,
        dimension: emb.dimension,
        clip_session_loaded: crate::embedding::clip_session_loaded(),
        last_clip_infer_ms: crate::embedding::last_clip_infer_ms(),
    };
    let inference = crate::telemetry::runtime_metrics::InferenceMetricsSnapshot {
        ai_model_available: state.inner().ai_model_available(),
        ai_model_loaded: state.inner().ai_model_loaded(),
        loaded_model_id: state.inner().loaded_model_id(),
    };
    Ok(crate::telemetry::runtime_metrics::build_snapshot(
        state.inner(),
        embedding,
        inference,
    ))
}
