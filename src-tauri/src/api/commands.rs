//! Tauri command handlers

use crate::capture::{
    continuity_anchor_for_memory, eligible_for_story_merge, merge_memory_records_with_policy,
    passes_merge_threshold, score_memory_candidate,
};
use crate::config::AutofillConfig;
use crate::context_runtime;
use crate::embed::{embedding_runtime_status, Embedder, EmbeddingBackend};
use crate::memory_compaction::{
    best_embedding_text, best_snippet_embedding_text, best_support_embedding_texts,
    compact_memory_record_payload, is_low_signal_embedding, mean_pool_embeddings,
};
use crate::memory_quality::classify_storage_outcome;
use crate::privacy::Blocklist;
use crate::store::MemoryRecord;

use crate::mcp::{self, McpServerStatus};
use crate::meeting::{self, MeetingRecorderStatus, MeetingTranscript};

use crate::search::{
    rerank_results, HybridSearcher, MemoryCard, MemoryCardSynthesizer, QueryContext,
};
use crate::speech;
use crate::store::{MeetingSession, SearchResult, Stats, Task, TaskType};
use crate::telemetry::quality_logger::read_quality_events;
use crate::AppState;
use chrono::{TimeZone, Timelike};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use tokio::time::{timeout, Duration, Instant};

use genpdf::elements;
use genpdf::style;
use genpdf::Element;

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    pub time_filter: Option<String>,
    pub app_filter: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceTranscriptionResult {
    pub text: String,
    pub backend: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeechSynthesisResult {
    pub audio_path: String,
    pub voice_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryScoreBreakdown {
    pub specificity: f32,
    pub intent: f32,
    pub entity: f32,
    pub usefulness: f32,
    pub evidence: f32,
    pub ocr_noise: f32,
    pub graph_readiness: f32,
    pub retrieval_value: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryGraphSnapshot {
    pub nodes: Vec<serde_json::Value>,
    pub edges: Vec<serde_json::Value>,
    pub weak_evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryDebugInspector {
    pub memory_id: String,
    pub memory_context: String,
    pub project: String,
    pub topic: String,
    pub workflow: String,
    pub activity_type: String,
    pub user_intent: String,
    pub entities: Vec<String>,
    pub actions: Vec<String>,
    pub quality_scores: MemoryScoreBreakdown,
    pub grounding_confidence: f32,
    pub extraction_issues: Vec<String>,
    pub ocr_quality_stats: OcrQualityStats,
    pub embedding_diagnostics: EmbeddingDiagnostics,
    pub embedding_text: String,
    pub search_aliases: Vec<String>,
    pub raw_ocr_evidence: serde_json::Value,
    pub graph: MemoryGraphSnapshot,
    pub storage_outcome: String,
    pub quality_gate_reason: String,
    pub query_match_reasons: Vec<String>,
    pub related_knowledge_pages: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OcrQualityStats {
    pub total_lines: usize,
    pub kept_lines: usize,
    pub low_conf_lines: usize,
    pub dropped_noise_lines: usize,
    pub dropped_low_signal_lines: usize,
    pub avg_line_score: f32,
    pub ocr_confidence: f32,
    pub ocr_blocks: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EmbeddingDiagnostics {
    pub structured_prefix_ratio: f32,
    pub evidence_tail_chars: usize,
    pub dominated_by_raw_ocr: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QualityCountRow {
    pub label: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CaptureQualityDashboard {
    pub generated_at: i64,
    pub lookback_minutes: u32,
    pub signals_rows: usize,
    pub anomalies_rows: usize,
    pub malformed_signal_rows: usize,
    pub malformed_anomaly_rows: usize,
    pub stored_candidates: usize,
    pub skipped_low_signal: usize,
    pub skipped_noise: usize,
    pub avg_ocr_confidence: f32,
    pub grounding_confidence_lt_05: usize,
    pub grounding_confidence_lt_08: usize,
    pub top_anomalies: Vec<QualityCountRow>,
    pub top_apps: Vec<QualityCountRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryQualityFlag {
    pub memory_id: String,
    pub summary: String,
    pub app_name: String,
    pub timestamp: i64,
    pub storage_outcome: String,
    pub issues: Vec<String>,
    pub scores: MemoryScoreBreakdown,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryValidationReport {
    pub generated_at: i64,
    pub lookback_minutes: u32,
    pub total_memories: usize,
    pub flagged_memories: Vec<MemoryQualityFlag>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RebuildMemoryPreview {
    pub memory_id: String,
    pub before_memory_context: String,
    pub after_memory_context: String,
    pub before_embedding_text: String,
    pub after_embedding_text: String,
    pub before_aliases: Vec<String>,
    pub after_aliases: Vec<String>,
    pub before_storage_outcome: String,
    pub after_storage_outcome: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RebuildMemoryContextReport {
    pub dry_run: bool,
    pub start: i64,
    pub end: i64,
    pub scanned: usize,
    pub changed: usize,
    pub previews: Vec<RebuildMemoryPreview>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RetrievalEvalCase {
    pub category: String,
    pub query: String,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RetrievalEvalRow {
    pub category: String,
    pub query: String,
    pub top_1_relevant: bool,
    pub top_5_relevant: bool,
    pub matched_by_semantic: bool,
    pub matched_by_prefix: bool,
    pub matched_by_fuzzy: bool,
    pub matched_by_ngram: bool,
    pub matched_by_graph: bool,
    pub matched_by_alias: bool,
    pub query_expansion_terms: Vec<String>,
    pub bad_match_reason: Option<String>,
    pub top_memory_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RetrievalEvalReport {
    pub generated_at: i64,
    pub total_queries: usize,
    pub top1_hits: usize,
    pub top5_hits: usize,
    pub rows: Vec<RetrievalEvalRow>,
}

static SHARED_EMBEDDER: OnceLock<Result<Embedder, String>> = OnceLock::new();

const SYNTHESIS_TIMEOUT: Duration = Duration::from_millis(2400);
const MEMORY_GRAPH_LIMIT: usize = 1_500;
const TASK_LINK_SCAN_LIMIT: usize = 260;
const TASK_MEETING_LOOKBACK_DAYS: i64 = 14;
const TASK_MEMORY_BACKFILL_LIMIT: usize = 6;
const MEMORY_REPAIR_CHECKPOINT_VERSION: u32 = 4;
const MEMORY_REPAIR_SIMILARITY_SCAN_LIMIT: usize = 96;
const MEMORY_REPAIR_CHECKPOINT_ITEM_STEP: usize = 300;
const MEMORY_REPAIR_CHECKPOINT_MS: u64 = 12_000;
const STORAGE_RECLAIM_HEARTBEAT_ITEM_STEP: usize = 72;
const STORAGE_RECLAIM_HEARTBEAT_MS: u64 = 850;
const STORAGE_RECLAIM_EMBED_BATCH: usize = 48;
const MEMORY_DERIVED_CACHE_TTL_MS: i64 = 30_000;
static MEMORY_REPAIR_RUNNING: AtomicBool = AtomicBool::new(false);
static STORAGE_RECLAIM_RUNNING: AtomicBool = AtomicBool::new(false);

fn shared_embedder() -> Result<&'static Embedder, String> {
    match SHARED_EMBEDDER.get_or_init(Embedder::new) {
        Ok(embedder) => Ok(embedder),
        Err(err) => Err(err.clone()),
    }
}

fn shared_real_embedder() -> Result<&'static Embedder, String> {
    let embedder = shared_embedder()?;
    if matches!(embedder.backend(), EmbeddingBackend::Real) {
        return Ok(embedder);
    }

    let status = embedding_runtime_status();
    Err(format!(
        "Real embeddings are required before running continuity repair or storage reclaim. Current backend: {}{}{}",
        status.backend,
        if status.degraded { " (degraded)" } else { "" },
        if status.detail.is_empty() {
            String::new()
        } else {
            format!(" - {}", status.detail)
        }
    ))
}

async fn run_search_query(
    state: &AppState,
    query: &str,
    time_filter: Option<&str>,
    app_filter: Option<&str>,
    limit: usize,
) -> Result<Vec<SearchResult>, String> {
    let limit = limit.clamp(1, 50);

    if !state
        .store
        .has_memories()
        .await
        .map_err(|e| e.to_string())?
    {
        return Ok(Vec::new());
    }

    let search_config = {
        let config = state.config.read();
        config.search.clone()
    };

    let results = match shared_embedder() {
        Ok(embedder) => match HybridSearcher::search_hybrid_memories(
            &state.store,
            embedder,
            query,
            limit,
            time_filter,
            app_filter,
            &search_config,
        )
        .await
        .map_err(|err| err.to_string())
        {
            Ok(results) => results,
            Err(err) => {
                tracing::warn!(
                    "Hybrid search failed; falling back to keyword-only search: {}",
                    err
                );
                state
                    .store
                    .keyword_search(query, limit, time_filter, app_filter)
                    .await
                    .map_err(|e| e.to_string())?
            }
        },
        Err(err) => {
            tracing::warn!(
                "Semantic embedder unavailable for raw search; falling back to keyword-only: {}",
                err
            );
            state
                .store
                .keyword_search(query, limit, time_filter, app_filter)
                .await
                .map_err(|e| e.to_string())?
        }
    };

    Ok(strip_internal_fndr_results(results))
}

fn cache_is_fresh(computed_at_ms: i64) -> bool {
    let age_ms = chrono::Utc::now().timestamp_millis() - computed_at_ms;
    age_ms >= 0 && age_ms <= MEMORY_DERIVED_CACHE_TTL_MS
}

fn is_internal_fndr_result(result: &SearchResult) -> bool {
    Blocklist::is_internal_app(&result.app_name, result.bundle_id.as_deref())
}

fn strip_internal_fndr_results(mut results: Vec<SearchResult>) -> Vec<SearchResult> {
    results.retain(|result| !is_internal_fndr_result(result));
    results
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    let mut chars = input.chars();
    let head: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{head}...")
    } else {
        head
    }
}

fn card_domain(url: &str) -> Option<String> {
    let no_scheme = url.split("://").nth(1).unwrap_or(url);
    let host = no_scheme.split('/').next()?.trim();
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

fn is_low_signal_title(title: &str, app_name: &str) -> bool {
    let normalized = title.trim().to_lowercase();
    if normalized.is_empty() {
        return true;
    }

    let app = app_name.trim().to_lowercase();
    if normalized == app || normalized == format!("{app} activity") {
        return true;
    }

    let tokens = normalized.split_whitespace().count();
    if tokens <= 1 {
        return true;
    }

    matches!(
        normalized.as_str(),
        "codex"
            | "cursor"
            | "new chat"
            | "chat"
            | "activity"
            | "home"
            | "dashboard"
            | "new tab"
            | "google chrome"
            | "chrome"
            | "safari"
            | "firefox"
            | "terminal"
            | "finder"
            | "settings"
    )
}

fn is_low_signal_summary(summary: &str, app_name: &str) -> bool {
    let normalized = summary.trim().to_lowercase();
    if normalized.is_empty() {
        return true;
    }

    let app = app_name.trim().to_lowercase();
    if normalized == app {
        return true;
    }

    let words = normalized.split_whitespace().count();
    words <= 2
}

fn title_from_summary(summary: &str, app_name: &str) -> Option<String> {
    let trimmed = summary.trim().trim_end_matches('.');
    if trimmed.is_empty() {
        return None;
    }

    let cleaned = if let Some(rest) = trimmed.strip_prefix("Reviewed ") {
        rest.trim()
    } else if let Some(rest) = trimmed.strip_prefix("reviewed ") {
        rest.trim()
    } else {
        trimmed
    };

    if cleaned.is_empty() {
        return None;
    }

    let candidate = cleaned.to_string();
    if is_low_signal_title(&candidate, app_name) {
        None
    } else {
        Some(candidate)
    }
}

fn card_summary(result: &SearchResult) -> String {
    let display = result.display_summary.trim();
    let snippet = result.snippet.trim();
    let clean = result.clean_text.trim();

    let base = if !display.is_empty() && !is_low_signal_summary(display, &result.app_name) {
        display
    } else if !snippet.is_empty() && !is_low_signal_summary(snippet, &result.app_name) {
        snippet
    } else if !clean.is_empty() && !is_low_signal_summary(clean, &result.app_name) {
        clean
    } else if !snippet.is_empty() {
        snippet
    } else {
        clean
    };

    if base.is_empty() {
        format!("Captured activity in {}", result.app_name)
    } else {
        base.to_string()
    }
}

fn has_continuity_signal(result: &SearchResult) -> bool {
    result.snippet.contains(" • ") || result.clean_text.contains(" • ")
}

fn card_title(result: &SearchResult, summary: &str) -> String {
    let title = result.window_title.trim();
    if !title.is_empty() {
        let candidate = title.to_string();
        if !is_low_signal_title(&candidate, &result.app_name) {
            return candidate;
        }
    }

    if let Some(from_summary) = title_from_summary(summary, &result.app_name) {
        return from_summary;
    }

    if let Some(domain) = result.url.as_deref().and_then(card_domain) {
        return format!("{} · {}", result.app_name, domain);
    }

    format!("{} memory", result.app_name)
}

fn memory_card_from_result(result: SearchResult) -> MemoryCard {
    let memory_id = result.id.clone();
    let score = result.score;
    let app_name = result.app_name.clone();
    let window_title = result.window_title.clone();
    let url = result.url.clone();
    let summary = card_summary(&result);
    let title = card_title(&result, &summary);
    let mut context = Vec::new();
    if let Some(domain) = url.as_deref().and_then(card_domain) {
        context.push(format!("Site: {}", domain));
    }

    let fallback_snippet = summary.clone();
    let action = if result.url.is_some() {
        "Open source".to_string()
    } else {
        "Revisit context".to_string()
    };
    MemoryCard {
        id: memory_id.clone(),
        title,
        summary: summary.clone(),
        display_summary: if result.display_summary.trim().is_empty() {
            summary
        } else {
            result.display_summary.clone()
        },
        internal_context: result.internal_context.clone(),
        action,
        context,
        timestamp: result.timestamp,
        app_name,
        window_title,
        url,
        score,
        source_count: 1,
        continuity: has_continuity_signal(&result),
        raw_snippets: vec![fallback_snippet],
        evidence_ids: vec![memory_id],
        confidence: score.clamp(0.0, 1.0),
        anchor_coverage_score: result.anchor_coverage_score.clamp(0.0, 1.0),
        activity_type: result.activity_type.clone(),
        files_touched: result.files_touched.clone(),
        session_duration_mins: result.session_duration_mins,
    }
}

fn refine_memory_card_title(card: &mut MemoryCard) {
    if !is_low_signal_title(&card.title, &card.app_name) {
        return;
    }

    let window_title = card.window_title.trim();
    if !window_title.is_empty() && !is_low_signal_title(window_title, &card.app_name) {
        card.title = window_title.to_string();
        return;
    }

    if let Some(from_summary) = title_from_summary(&card.summary, &card.app_name) {
        card.title = from_summary;
        return;
    }

    if let Some(domain) = card.url.as_deref().and_then(card_domain) {
        card.title = format!("{} · {}", card.app_name, domain);
        return;
    }

    card.title = format!("{} memory", card.app_name);
}

fn refine_memory_card_titles(cards: &mut [MemoryCard]) {
    for card in cards {
        refine_memory_card_title(card);
    }
}

/// Search for memories
#[tauri::command]
pub async fn search(
    state: State<'_, Arc<AppState>>,
    query: String,
    time_filter: Option<String>,
    app_filter: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<SearchResult>, String> {
    run_search_query(
        state.inner(),
        &query,
        time_filter.as_deref(),
        app_filter.as_deref(),
        limit.unwrap_or(20),
    )
    .await
}

/// Search and return synthesized memory cards for UI rendering
#[tauri::command]
pub async fn search_memory_cards(
    state: State<'_, Arc<AppState>>,
    query: String,
    time_filter: Option<String>,
    app_filter: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<MemoryCard>, String> {
    let limit = limit.unwrap_or(20).clamp(1, 50);
    let started = Instant::now();
    tracing::info!(
        query = %query,
        time_filter = ?time_filter,
        app_filter = ?app_filter,
        limit,
        "search_memory_cards:start"
    );

    if !state
        .store
        .has_memories()
        .await
        .map_err(|e| e.to_string())?
    {
        tracing::info!("search_memory_cards:complete total_ms=0 cards=0");
        return Ok(Vec::new());
    }

    let memory_card_config = {
        let config = state.config.read();
        config.memory_cards.clone()
    };
    let fallback_cards = |raw_results: &[SearchResult]| {
        MemoryCardSynthesizer::deterministic_from_results(
            &query,
            raw_results,
            limit.min(memory_card_config.max_groups),
        )
    };

    let raw_limit = limit.max(18).min(50);
    let mut raw_results = run_search_query(
        state.inner(),
        &query,
        time_filter.as_deref(),
        app_filter.as_deref(),
        raw_limit,
    )
    .await?;
    raw_results.truncate(raw_limit);
    let query_context = QueryContext::from_query(&query);
    let (reranked, rerank_stats) = rerank_results(&query_context, raw_results);
    let mut raw_results = reranked;
    if rerank_stats.excluded_for_coverage > 0 {
        tracing::info!(
            excluded_for_coverage = rerank_stats.excluded_for_coverage,
            query = %query_context.raw_query,
            "search_memory_cards:coverage_gate"
        );
    }
    raw_results.truncate(raw_limit);
    tracing::info!(count = raw_results.len(), "search_memory_cards:rerank:done");
    if raw_results.is_empty() {
        tracing::info!(
            "search_memory_cards:complete total_ms={} cards=0",
            started.elapsed().as_millis()
        );
        return Ok(Vec::new());
    }

    // Never block live search on model loading. If inference isn't already warm,
    // synthesis falls back to deterministic card generation immediately.
    let inference = state.inner().inference_engine();

    tracing::info!("search_memory_cards:synthesis:start");
    let synthesis_future = MemoryCardSynthesizer::from_results_with_policy(
        inference.as_deref(),
        &query,
        &raw_results,
        memory_card_config.max_groups,
        memory_card_config.max_llm_groups,
        Duration::from_millis(memory_card_config.llm_timeout_ms),
    );
    let mut cards = match timeout(SYNTHESIS_TIMEOUT, synthesis_future).await {
        Ok(generated) => {
            tracing::info!(
                count = generated.len(),
                "search_memory_cards:synthesis:done"
            );
            generated
        }
        Err(_) => {
            tracing::warn!(
                timeout_ms = SYNTHESIS_TIMEOUT.as_millis(),
                "search_memory_cards:synthesis:timeout"
            );
            fallback_cards(&raw_results)
        }
    };

    if cards.is_empty() {
        cards = fallback_cards(&raw_results);
    }
    refine_memory_card_titles(&mut cards);
    cards.retain(|card| !Blocklist::is_internal_app(&card.app_name, None));
    cards.truncate(limit);
    tracing::info!(
        total_ms = started.elapsed().as_millis(),
        cards = cards.len(),
        "search_memory_cards:complete"
    );
    Ok(cards)
}

/// List memory cards in newest→oldest order for browsing.
#[tauri::command]
pub async fn list_memory_cards(
    state: State<'_, Arc<AppState>>,
    app_filter: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<MemoryCard>, String> {
    let limit = limit.unwrap_or(MEMORY_GRAPH_LIMIT).clamp(1, 2_000);
    let results = state
        .inner()
        .store
        .list_recent_results(limit, app_filter.as_deref())
        .await
        .map_err(|e| e.to_string())?;

    let mut cards: Vec<MemoryCard> = strip_internal_fndr_results(results)
        .into_iter()
        .map(memory_card_from_result)
        .collect();
    refine_memory_card_titles(&mut cards);
    Ok(cards)
}

#[tauri::command]
pub async fn delete_memory(
    state: State<'_, Arc<AppState>>,
    memory_id: String,
) -> Result<bool, String> {
    let existing = state
        .inner()
        .store
        .get_memory_by_id(&memory_id)
        .await
        .map_err(|e: Box<dyn std::error::Error>| e.to_string())?;

    let deleted = state
        .inner()
        .store
        .delete_memory_by_id(&memory_id)
        .await
        .map_err(|e: Box<dyn std::error::Error>| e.to_string())?;

    if deleted == 0 {
        return Ok(false);
    }

    state.invalidate_memory_derived_caches();

    if let Some(record) = existing {
        if let Some(path) = record.screenshot_path {
            if let Err(err) = std::fs::remove_file(&path) {
                tracing::warn!("Failed to delete screenshot artifact {}: {}", path, err);
            }
        }
    }

    tracing::info!("Deleted memory record {}", memory_id);
    Ok(true)
}

fn normalize_debug_text(value: &str) -> String {
    value
        .to_ascii_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch.is_ascii_whitespace() {
                ch
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn dedupe_trimmed_strings(values: impl IntoIterator<Item = String>, limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        let key = normalize_debug_text(trimmed);
        if key.is_empty() || !seen.insert(key) {
            continue;
        }
        out.push(trimmed.to_string());
        if out.len() >= limit {
            break;
        }
    }
    out
}

fn raw_evidence_json(raw_evidence: &str) -> Option<Value> {
    let trimmed = raw_evidence.trim();
    if trimmed.is_empty() {
        return None;
    }
    serde_json::from_str::<Value>(trimmed).ok()
}

fn derive_grounding_confidence(memory: &MemoryRecord, evidence: Option<&Value>) -> f32 {
    evidence
        .and_then(|value| {
            value
                .get("extraction_grounding_confidence")
                .and_then(Value::as_f64)
                .map(|value| value as f32)
        })
        .unwrap_or(memory.extraction_confidence.clamp(0.0, 1.0))
        .clamp(0.0, 1.0)
}

fn derive_extraction_issues(evidence: Option<&Value>) -> Vec<String> {
    evidence
        .and_then(|value| value.get("extraction_issues"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .take(24)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn derive_ocr_quality_stats(memory: &MemoryRecord, evidence: Option<&Value>) -> OcrQualityStats {
    let quality = evidence
        .and_then(|value| value.get("ocr_quality"))
        .cloned()
        .unwrap_or(Value::Null);
    OcrQualityStats {
        total_lines: quality
            .get("total_lines")
            .and_then(Value::as_u64)
            .unwrap_or(memory.ocr_block_count as u64) as usize,
        kept_lines: quality
            .get("kept_lines")
            .and_then(Value::as_u64)
            .unwrap_or(memory.ocr_block_count as u64) as usize,
        low_conf_lines: quality
            .get("low_conf_lines")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize,
        dropped_noise_lines: quality
            .get("dropped_noise_lines")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize,
        dropped_low_signal_lines: quality
            .get("dropped_low_signal_lines")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize,
        avg_line_score: quality
            .get("avg_line_score")
            .and_then(Value::as_f64)
            .unwrap_or(0.0) as f32,
        ocr_confidence: memory.ocr_confidence,
        ocr_blocks: memory.ocr_block_count,
    }
}

fn derive_embedding_diagnostics(memory: &MemoryRecord) -> EmbeddingDiagnostics {
    let embedding_norm = normalize_debug_text(&memory.embedding_text);
    let clean_norm = normalize_debug_text(&memory.clean_text);
    let evidence_index = embedding_norm
        .find("evidence ")
        .unwrap_or(embedding_norm.len());
    let prefix = &embedding_norm[..evidence_index];
    let structured_markers = [
        "intent", "project", "topic", "workflow", "context", "entities", "files",
    ];
    let marker_hits = structured_markers
        .iter()
        .filter(|marker| prefix.contains(*marker))
        .count();
    let structured_prefix_ratio =
        (marker_hits as f32 / structured_markers.len() as f32).clamp(0.0, 1.0);
    let clean_prefix = clean_norm.chars().take(42).collect::<String>();
    let dominated = embedding_norm.len() > 60
        && !clean_prefix.is_empty()
        && embedding_norm.starts_with(&clean_prefix);
    EmbeddingDiagnostics {
        structured_prefix_ratio,
        evidence_tail_chars: embedding_norm
            .split("evidence:")
            .nth(1)
            .map(str::trim)
            .map(str::len)
            .unwrap_or(0),
        dominated_by_raw_ocr: dominated,
    }
}

fn is_vague_memory_context(value: &str) -> bool {
    let normalized = normalize_debug_text(value);
    if normalized.split_whitespace().count() < 10 {
        return true;
    }
    let vague_markers = [
        "user engaged with",
        "low confidence",
        "ui elements",
        "window and saw",
        "general browsing",
        "some activity",
    ];
    vague_markers
        .iter()
        .any(|marker| normalized.contains(marker))
}

fn compose_rebuild_embedding_text(record: &MemoryRecord) -> String {
    let mut parts = Vec::new();
    if !record.memory_context.trim().is_empty() {
        parts.push(format!("context: {}", record.memory_context.trim()));
    }
    if !record.user_intent.trim().is_empty() {
        parts.push(format!("intent: {}", record.user_intent.trim()));
    }
    if !record.project.trim().is_empty() {
        parts.push(format!("project: {}", record.project.trim()));
    }
    if !record.topic.trim().is_empty() {
        parts.push(format!("topic: {}", record.topic.trim()));
    }
    if !record.workflow.trim().is_empty() {
        parts.push(format!("workflow: {}", record.workflow.trim()));
    }
    if !record.entities.is_empty() {
        parts.push(format!("entities: {}", record.entities.join(", ")));
    }
    if !record.files_touched.is_empty() {
        parts.push(format!("files: {}", record.files_touched.join(", ")));
    }
    if !record.decisions.is_empty() {
        parts.push(format!("decisions: {}", record.decisions.join("; ")));
    }
    if !record.errors.is_empty() {
        parts.push(format!("errors: {}", record.errors.join("; ")));
    }
    if !record.next_steps.is_empty() {
        parts.push(format!("next_steps: {}", record.next_steps.join("; ")));
    }
    if !record.clean_text.trim().is_empty() {
        parts.push(format!(
            "evidence: {}",
            truncate_chars(record.clean_text.trim(), 420)
        ));
    }
    truncate_chars(&parts.join(" | "), 2_200)
}

fn regenerate_search_aliases_basic(record: &MemoryRecord) -> Vec<String> {
    let mut values = Vec::new();
    values.push(record.project.clone());
    values.push(record.topic.clone());
    values.push(record.workflow.clone());
    values.push(record.user_intent.clone());
    values.extend(record.entities.clone());
    values.extend(record.files_touched.clone());
    values.extend(record.related_tools.clone());
    values.extend(record.decisions.clone());
    values.extend(record.errors.clone());
    if let Some(url) = record.url.as_deref() {
        values.push(url.to_string());
    }
    let mut aliases = dedupe_trimmed_strings(values, 64);
    if aliases.is_empty() {
        aliases = dedupe_trimmed_strings(
            vec![
                record.app_name.clone(),
                record.window_title.clone(),
                record.memory_context.clone(),
            ],
            64,
        );
    }
    aliases
}

fn derive_rebuild_memory_context(
    previous: Option<&MemoryRecord>,
    current: &MemoryRecord,
    next: Option<&MemoryRecord>,
) -> String {
    if !current.memory_context.trim().is_empty()
        && !is_vague_memory_context(&current.memory_context)
    {
        return current.memory_context.trim().to_string();
    }

    let mut clauses = Vec::new();
    let intent = if !current.user_intent.trim().is_empty() {
        current.user_intent.trim().to_string()
    } else {
        current.activity_type.trim().to_string()
    };
    if !intent.is_empty() {
        clauses.push(format!("The user was {}.", intent));
    }
    if !current.project.trim().is_empty() {
        clauses.push(format!(
            "This belonged to the {} project.",
            current.project.trim()
        ));
    } else if !current.topic.trim().is_empty() && current.topic != "unknown" {
        clauses.push(format!("Topic focus: {}.", current.topic.trim()));
    }

    let artifacts = dedupe_trimmed_strings(
        current
            .files_touched
            .iter()
            .cloned()
            .chain(current.entities.iter().cloned())
            .chain(current.related_tools.iter().cloned())
            .collect::<Vec<_>>(),
        5,
    );
    if !artifacts.is_empty() {
        clauses.push(format!("Key artifacts involved: {}.", artifacts.join(", ")));
    }

    let outcomes = dedupe_trimmed_strings(
        current
            .decisions
            .iter()
            .cloned()
            .chain(current.errors.iter().cloned())
            .chain(current.blockers.iter().cloned())
            .chain(current.todos.iter().cloned())
            .chain(current.results.iter().cloned())
            .chain(current.next_steps.iter().cloned())
            .collect::<Vec<_>>(),
        4,
    );
    if !outcomes.is_empty() {
        clauses.push(format!(
            "Important decisions, blockers, or results: {}.",
            outcomes.join("; ")
        ));
    }

    if clauses.is_empty() {
        clauses.push(format!(
            "The user was active in {} reviewing {}.",
            current.app_name,
            truncate_chars(&current.window_title, 120)
        ));
    }

    if let Some(prev) = previous {
        if prev.session_id == current.session_id && !prev.memory_context.trim().is_empty() {
            clauses.push(format!(
                "Earlier in this session: {}",
                truncate_chars(prev.memory_context.trim(), 160)
            ));
        }
    }
    if let Some(next) = next {
        if next.session_id == current.session_id && !next.memory_context.trim().is_empty() {
            clauses.push(format!(
                "Then it continued with: {}",
                truncate_chars(next.memory_context.trim(), 160)
            ));
        }
    }

    truncate_chars(&clauses.join(" "), 900)
}

fn classify_storage_outcome_with_config(
    record: &MemoryRecord,
    config: &crate::config::MemoryQualityConfig,
) -> String {
    classify_storage_outcome(record, config)
}

fn memory_quality_scores(memory: &MemoryRecord) -> MemoryScoreBreakdown {
    MemoryScoreBreakdown {
        specificity: memory.specificity_score,
        intent: memory.intent_score,
        entity: memory.entity_score,
        usefulness: memory.agent_usefulness_score,
        evidence: memory.evidence_confidence,
        ocr_noise: memory.ocr_noise_score,
        graph_readiness: memory.graph_readiness_score,
        retrieval_value: memory.retrieval_value_score,
    }
}

fn build_query_match_reasons(memory: &MemoryRecord, query: &str) -> Vec<String> {
    let normalized_query = normalize_debug_text(query);
    if normalized_query.is_empty() {
        return Vec::new();
    }
    let context = normalize_debug_text(&format!(
        "{} {} {} {} {}",
        memory.memory_context,
        memory.display_summary,
        memory.clean_text,
        memory.search_aliases.join(" "),
        memory.project
    ));
    let mut reasons = Vec::new();
    if context.contains(&normalized_query) {
        reasons.push("exact_query_phrase".to_string());
    }
    let query_tokens = normalized_query
        .split_whitespace()
        .filter(|token| token.len() > 1)
        .collect::<Vec<_>>();
    let mut matched_tokens = 0usize;
    for token in &query_tokens {
        if context.contains(token) {
            matched_tokens += 1;
        }
    }
    if !query_tokens.is_empty() && matched_tokens > 0 {
        reasons.push(format!(
            "token_overlap:{}/{}",
            matched_tokens,
            query_tokens.len()
        ));
    }
    if memory
        .search_aliases
        .iter()
        .any(|alias| normalize_debug_text(alias).contains(&normalized_query))
    {
        reasons.push("matched_alias".to_string());
    }
    if let Some(url) = memory.url.as_deref() {
        if normalize_debug_text(url).contains(&normalized_query) {
            reasons.push("matched_url".to_string());
        }
    }
    reasons
}

async fn build_memory_graph_snapshot(
    state: &AppState,
    memory: &MemoryRecord,
) -> Result<MemoryGraphSnapshot, String> {
    let nodes = state
        .store
        .get_all_nodes()
        .await
        .map_err(|e| e.to_string())?;
    let edges = state
        .store
        .get_all_edges()
        .await
        .map_err(|e| e.to_string())?;
    let memory_node_id = format!("memory:{}", memory.id);

    let selected_nodes = nodes
        .iter()
        .filter(|node| {
            node.id == memory_node_id
                || memory.graph_node_ids.iter().any(|id| id == &node.id)
                || node
                    .metadata
                    .get("memory_id")
                    .and_then(serde_json::Value::as_str)
                    .map(|value| value == memory.id)
                    .unwrap_or(false)
        })
        .map(|node| {
            serde_json::json!({
                "id": node.id,
                "type": format!("{:?}", node.node_type),
                "label": node.label,
                "created_at": node.created_at
            })
        })
        .collect::<Vec<_>>();

    let selected_edges = edges
        .iter()
        .filter(|edge| {
            edge.source == memory_node_id
                || edge.target == memory_node_id
                || memory.graph_edge_ids.iter().any(|id| id == &edge.id)
        })
        .map(|edge| {
            serde_json::json!({
                "id": edge.id,
                "type": format!("{:?}", edge.edge_type),
                "source": edge.source,
                "target": edge.target,
                "timestamp": edge.timestamp,
                "metadata": edge.metadata
            })
        })
        .collect::<Vec<_>>();

    let weak_evidence = selected_edges
        .iter()
        .filter_map(|edge| {
            let target = edge.get("target").and_then(serde_json::Value::as_str)?;
            let target_lower = target.to_ascii_lowercase();
            if target_lower.ends_with(":ui")
                || target_lower.ends_with(":chrome")
                || target_lower.ends_with(":window")
            {
                Some(format!("generic_target:{target}"))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    Ok(MemoryGraphSnapshot {
        nodes: selected_nodes,
        edges: selected_edges,
        weak_evidence,
    })
}

#[tauri::command]
pub async fn get_memory_debug_inspector(
    state: State<'_, Arc<AppState>>,
    memory_id: String,
    query: Option<String>,
) -> Result<MemoryDebugInspector, String> {
    let memory = state
        .inner()
        .store
        .get_memory_by_id(&memory_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Memory not found: {memory_id}"))?;
    let graph = build_memory_graph_snapshot(state.inner().as_ref(), &memory).await?;

    let actions = dedupe_trimmed_strings(
        memory
            .action_items
            .iter()
            .map(|item| item.text.clone())
            .chain(memory.decisions.iter().cloned())
            .chain(memory.errors.iter().cloned())
            .chain(memory.todos.iter().cloned())
            .chain(memory.blockers.iter().cloned())
            .chain(memory.results.iter().cloned())
            .chain(memory.next_steps.iter().cloned())
            .collect::<Vec<_>>(),
        16,
    );

    let entities = dedupe_trimmed_strings(
        memory
            .extracted_entities_structured
            .iter()
            .map(|item| item.text.clone())
            .chain(memory.entities.iter().cloned())
            .chain(memory.files_touched.iter().cloned())
            .collect::<Vec<_>>(),
        20,
    );
    let related_knowledge_pages = state
        .inner()
        .store
        .list_knowledge_pages(80, None, None)
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|page| page.supporting_memory_ids.iter().any(|id| id == &memory.id))
        .take(12)
        .map(|page| {
            serde_json::json!({
                "page_id": page.page_id,
                "page_type": page.page_type,
                "title": page.title,
                "stability": page.stability,
                "confidence_score": page.confidence_score
            })
        })
        .collect::<Vec<_>>();
    let evidence = raw_evidence_json(&memory.raw_evidence);
    let grounding_confidence = derive_grounding_confidence(&memory, evidence.as_ref());
    let extraction_issues = derive_extraction_issues(evidence.as_ref());
    let ocr_quality_stats = derive_ocr_quality_stats(&memory, evidence.as_ref());
    let embedding_diagnostics = derive_embedding_diagnostics(&memory);

    Ok(MemoryDebugInspector {
        memory_id: memory.id.clone(),
        memory_context: memory.memory_context.clone(),
        project: memory.project.clone(),
        topic: memory.topic.clone(),
        workflow: memory.workflow.clone(),
        activity_type: memory.activity_type.clone(),
        user_intent: memory.user_intent.clone(),
        entities,
        actions,
        quality_scores: memory_quality_scores(&memory),
        grounding_confidence,
        extraction_issues,
        ocr_quality_stats,
        embedding_diagnostics,
        embedding_text: memory.embedding_text.clone(),
        search_aliases: memory.search_aliases.clone(),
        raw_ocr_evidence: serde_json::json!({
            "text": truncate_chars(&memory.text, 1800),
            "clean_text": truncate_chars(&memory.clean_text, 1800),
            "internal_context": truncate_chars(&memory.internal_context, 1400),
            "raw_evidence": truncate_chars(&memory.raw_evidence, 2200),
            "parsed_raw_evidence": evidence,
        }),
        graph,
        storage_outcome: memory.storage_outcome.clone(),
        quality_gate_reason: memory.quality_gate_reason.clone(),
        query_match_reasons: query
            .as_deref()
            .map(|q| build_query_match_reasons(&memory, q))
            .unwrap_or_default(),
        related_knowledge_pages,
    })
}

#[tauri::command]
pub async fn evaluate_recent_memory_quality(
    state: State<'_, Arc<AppState>>,
    lookback_minutes: Option<u32>,
    limit: Option<usize>,
) -> Result<MemoryValidationReport, String> {
    let lookback = lookback_minutes.unwrap_or(180).clamp(5, 72 * 60);
    let cap = limit.unwrap_or(180).clamp(10, 1_500);
    let now = chrono::Utc::now().timestamp_millis();
    let start = now - chrono::Duration::minutes(lookback as i64).num_milliseconds();
    let rows = state
        .inner()
        .store
        .get_search_results_in_range(start, now)
        .await
        .map_err(|e| e.to_string())?;
    let scanned_total = rows.len().min(cap);
    let memory_quality_cfg = state.inner().config.read().memory_quality.clone();

    let mut flagged = Vec::new();
    for row in rows.into_iter().rev().take(cap) {
        let Some(memory) = state
            .inner()
            .store
            .get_memory_by_id(&row.id)
            .await
            .map_err(|e| e.to_string())?
        else {
            continue;
        };

        let graph = build_memory_graph_snapshot(state.inner().as_ref(), &memory).await?;
        let mut issues = Vec::new();
        if is_vague_memory_context(&memory.memory_context) {
            issues.push("vague_memory_context".to_string());
        }
        if memory.user_intent.trim().is_empty() {
            issues.push("missing_intent".to_string());
        }
        if memory.project.trim().is_empty()
            && (memory.topic.trim().is_empty() || memory.topic == "unknown")
        {
            issues.push("missing_project_or_topic".to_string());
        }
        if memory.entities.is_empty() && memory.files_touched.is_empty() {
            issues.push("missing_entities".to_string());
        }
        if memory.agent_usefulness_score < 0.60 {
            issues.push("low_agent_usefulness".to_string());
        }
        if memory.ocr_noise_score > 0.50 {
            issues.push("high_ocr_noise".to_string());
        }
        if memory.storage_outcome == "primary_memory_card"
            && classify_storage_outcome_with_config(&memory, &memory_quality_cfg)
                != "primary_memory_card"
        {
            issues.push("primary_should_be_low_quality_evidence".to_string());
        }
        if !graph.weak_evidence.is_empty() {
            issues.push("graph_events_with_weak_evidence".to_string());
        }
        let embedding_norm = normalize_debug_text(&memory.embedding_text);
        let clean_norm = normalize_debug_text(&memory.clean_text);
        let clean_prefix = clean_norm.chars().take(42).collect::<String>();
        if embedding_norm.len() > 60
            && !clean_prefix.is_empty()
            && embedding_norm.starts_with(&clean_prefix)
        {
            issues.push("embedding_text_dominated_by_raw_ocr".to_string());
        }
        if issues.is_empty() {
            continue;
        }
        flagged.push(MemoryQualityFlag {
            memory_id: memory.id.clone(),
            summary: if !memory.memory_context.trim().is_empty() {
                truncate_chars(&memory.memory_context, 180)
            } else {
                truncate_chars(&memory.display_summary, 180)
            },
            app_name: memory.app_name.clone(),
            timestamp: memory.timestamp,
            storage_outcome: memory.storage_outcome.clone(),
            issues,
            scores: memory_quality_scores(&memory),
        });
    }

    Ok(MemoryValidationReport {
        generated_at: now,
        lookback_minutes: lookback,
        total_memories: scanned_total,
        flagged_memories: flagged,
    })
}

fn event_timestamp_ms(row: &Value) -> Option<i64> {
    row.get("payload")
        .and_then(|payload| payload.get("timestamp_ms"))
        .and_then(Value::as_i64)
        .or_else(|| row.get("timestamp_ms").and_then(Value::as_i64))
}

fn top_rows(map: HashMap<String, usize>, cap: usize) -> Vec<QualityCountRow> {
    let mut rows = map
        .into_iter()
        .map(|(label, count)| QualityCountRow { label, count })
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.label.cmp(&b.label)));
    rows.truncate(cap.max(1));
    rows
}

#[tauri::command]
pub async fn get_capture_quality_dashboard(
    state: State<'_, Arc<AppState>>,
    lookback_minutes: Option<u32>,
    limit: Option<usize>,
) -> Result<CaptureQualityDashboard, String> {
    let lookback = lookback_minutes.unwrap_or(240).clamp(30, 7 * 24 * 60);
    let limit = limit.unwrap_or(800).clamp(40, 20_000);
    let now = chrono::Utc::now().timestamp_millis();
    let earliest = now - chrono::Duration::minutes(lookback as i64).num_milliseconds();

    let (signal_rows_raw, malformed_signal_rows) =
        read_quality_events(state.inner().app_data_dir.as_path(), "signals.jsonl", limit)?;
    let (anomaly_rows_raw, malformed_anomaly_rows) = read_quality_events(
        state.inner().app_data_dir.as_path(),
        "anomalies.jsonl",
        limit,
    )?;

    let signal_rows = signal_rows_raw
        .into_iter()
        .filter(|row| {
            event_timestamp_ms(row)
                .map(|ts| ts >= earliest)
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    let anomaly_rows = anomaly_rows_raw
        .into_iter()
        .filter(|row| {
            event_timestamp_ms(row)
                .map(|ts| ts >= earliest)
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();

    let mut stored_candidates = 0usize;
    let mut skipped_low_signal = 0usize;
    let mut skipped_noise = 0usize;
    let mut confidence_sum = 0.0f32;
    let mut confidence_count = 0usize;
    let mut app_counts: HashMap<String, usize> = HashMap::new();
    let mut anomaly_counts: HashMap<String, usize> = HashMap::new();
    let mut grounding_confidence_lt_05 = 0usize;
    let mut grounding_confidence_lt_08 = 0usize;

    for row in &signal_rows {
        let Some(payload) = row.get("payload") else {
            continue;
        };
        if let Some(outcome) = payload.get("stored_or_skipped").and_then(Value::as_str) {
            match outcome {
                "stored_candidate" => stored_candidates += 1,
                "skipped_low_signal" => skipped_low_signal += 1,
                "skipped_noise" => skipped_noise += 1,
                _ => {}
            }
        }
        if let Some(conf) = payload.get("ocr_confidence").and_then(Value::as_f64) {
            confidence_sum += conf as f32;
            confidence_count += 1;
        }
        if let Some(app) = payload.get("app_name").and_then(Value::as_str) {
            *app_counts.entry(app.to_string()).or_insert(0) += 1;
        }
        if let Some(grounding) = payload.get("grounding_confidence").and_then(Value::as_f64) {
            let grounding = grounding as f32;
            if grounding < 0.5 {
                grounding_confidence_lt_05 += 1;
            }
            if grounding < 0.8 {
                grounding_confidence_lt_08 += 1;
            }
        }
    }

    for row in &anomaly_rows {
        let Some(payload) = row.get("payload") else {
            continue;
        };
        if let Some(app) = payload.get("app_name").and_then(Value::as_str) {
            *app_counts.entry(app.to_string()).or_insert(0) += 1;
        }
        if let Some(labels) = payload.get("anomaly_labels").and_then(Value::as_array) {
            for label in labels.iter().filter_map(Value::as_str) {
                *anomaly_counts.entry(label.to_string()).or_insert(0) += 1;
            }
        }
        if let Some(grounding) = payload.get("grounding_confidence").and_then(Value::as_f64) {
            let grounding = grounding as f32;
            if grounding < 0.5 {
                grounding_confidence_lt_05 += 1;
            }
            if grounding < 0.8 {
                grounding_confidence_lt_08 += 1;
            }
        }
    }

    Ok(CaptureQualityDashboard {
        generated_at: now,
        lookback_minutes: lookback,
        signals_rows: signal_rows.len(),
        anomalies_rows: anomaly_rows.len(),
        malformed_signal_rows,
        malformed_anomaly_rows,
        stored_candidates,
        skipped_low_signal,
        skipped_noise,
        avg_ocr_confidence: if confidence_count == 0 {
            0.0
        } else {
            confidence_sum / confidence_count as f32
        },
        grounding_confidence_lt_05,
        grounding_confidence_lt_08,
        top_anomalies: top_rows(anomaly_counts, 8),
        top_apps: top_rows(app_counts, 8),
    })
}

#[tauri::command]
pub async fn rebuild_memory_context_for_range(
    state: State<'_, Arc<AppState>>,
    start: i64,
    end: i64,
    dry_run: bool,
) -> Result<RebuildMemoryContextReport, String> {
    if start > end {
        return Err("start must be <= end".to_string());
    }
    let mut all = state
        .inner()
        .store
        .list_all_memories()
        .await
        .map_err(|e| e.to_string())?;
    let quality_cfg = state.inner().config.read().memory_quality.clone();
    let mut previews = Vec::new();
    let mut changed = 0usize;

    for index in 0..all.len() {
        let ts = all[index].timestamp;
        if ts < start || ts > end {
            continue;
        }

        let previous = if index > 0 {
            Some(&all[index - 1])
        } else {
            None
        };
        let next = all.get(index + 1);
        let before = all[index].clone();
        let mut rebuilt = all[index].clone();
        rebuilt.memory_context = derive_rebuild_memory_context(previous, &rebuilt, next);
        rebuilt.embedding_text = compose_rebuild_embedding_text(&rebuilt);
        rebuilt.search_aliases = regenerate_search_aliases_basic(&rebuilt);
        if rebuilt.raw_evidence.trim().is_empty() {
            rebuilt.raw_evidence = serde_json::json!({
                "text": truncate_chars(&rebuilt.text, 1600),
                "clean_text": truncate_chars(&rebuilt.clean_text, 1600),
                "window_title": rebuilt.window_title,
                "app_name": rebuilt.app_name
            })
            .to_string();
        }
        rebuilt.storage_outcome = classify_storage_outcome_with_config(&rebuilt, &quality_cfg);

        let different = rebuilt.memory_context != before.memory_context
            || rebuilt.embedding_text != before.embedding_text
            || rebuilt.search_aliases != before.search_aliases
            || rebuilt.storage_outcome != before.storage_outcome;
        if !different {
            continue;
        }
        changed += 1;
        previews.push(RebuildMemoryPreview {
            memory_id: rebuilt.id.clone(),
            before_memory_context: before.memory_context.clone(),
            after_memory_context: rebuilt.memory_context.clone(),
            before_embedding_text: truncate_chars(&before.embedding_text, 240),
            after_embedding_text: truncate_chars(&rebuilt.embedding_text, 240),
            before_aliases: before.search_aliases.clone(),
            after_aliases: rebuilt.search_aliases.clone(),
            before_storage_outcome: before.storage_outcome.clone(),
            after_storage_outcome: rebuilt.storage_outcome.clone(),
        });
        all[index] = rebuilt;
    }

    if !dry_run && changed > 0 {
        state
            .inner()
            .store
            .replace_all_memories_preserving_ids(&all)
            .await
            .map_err(|e| e.to_string())?;
        let touched = all
            .iter()
            .filter(|memory| memory.timestamp >= start && memory.timestamp <= end)
            .cloned()
            .collect::<Vec<_>>();
        let _ = context_runtime::sync_memory_records(
            state.inner().as_ref(),
            &touched,
            Some("backfill"),
        )
        .await;
        state.inner().invalidate_memory_derived_caches();
    }

    Ok(RebuildMemoryContextReport {
        dry_run,
        start,
        end,
        scanned: previews.len(),
        changed,
        previews: previews.into_iter().take(80).collect(),
    })
}

fn result_relevant_for_query(result: &SearchResult, query_ctx: &QueryContext) -> bool {
    let haystack = normalize_debug_text(&format!(
        "{} {} {} {} {} {} {}",
        result.window_title,
        result.snippet,
        result.display_summary,
        result.memory_context,
        result.search_aliases.join(" "),
        result.project,
        result.topic
    ));
    if haystack.is_empty() {
        return false;
    }
    query_ctx
        .expanded_terms
        .iter()
        .filter(|term| term.len() > 1)
        .any(|term| haystack.contains(&normalize_debug_text(term)))
}

#[tauri::command]
pub async fn run_memory_retrieval_eval(
    state: State<'_, Arc<AppState>>,
) -> Result<RetrievalEvalReport, String> {
    let fixture = include_str!("fixtures/retrieval_eval_queries.json");
    let cases: Vec<RetrievalEvalCase> = serde_json::from_str(fixture)
        .map_err(|err| format!("Invalid retrieval eval fixture: {err}"))?;
    let now = chrono::Utc::now().timestamp_millis();
    let mut rows = Vec::new();
    let mut top1 = 0usize;
    let mut top5 = 0usize;

    for case in cases {
        let query_ctx = QueryContext::from_query(&case.query);
        let hits = run_search_query(state.inner().as_ref(), &case.query, None, None, 5).await?;
        let top_1_relevant = hits
            .first()
            .map(|hit| result_relevant_for_query(hit, &query_ctx))
            .unwrap_or(false);
        let top_5_relevant = hits
            .iter()
            .any(|hit| result_relevant_for_query(hit, &query_ctx));
        if top_1_relevant {
            top1 += 1;
        }
        if top_5_relevant {
            top5 += 1;
        }
        let top = hits.first();
        let top_blob = top
            .map(|hit| {
                normalize_debug_text(&format!(
                    "{} {} {}",
                    hit.window_title,
                    hit.snippet,
                    hit.search_aliases.join(" ")
                ))
            })
            .unwrap_or_default();
        let matched_by_alias = top
            .map(|hit| {
                hit.search_aliases.iter().any(|alias| {
                    let n = normalize_debug_text(alias);
                    !n.is_empty() && query_ctx.expanded_terms.iter().any(|term| n.contains(term))
                })
            })
            .unwrap_or(false);
        let matched_by_prefix = query_ctx
            .prefix_variants
            .iter()
            .any(|prefix| top_blob.contains(prefix));
        let matched_by_fuzzy = query_ctx
            .fuzzy_variants
            .iter()
            .any(|variant| top_blob.contains(variant));
        let matched_by_ngram = query_ctx
            .ngram_variants
            .iter()
            .take(6)
            .any(|gram| top_blob.contains(gram));
        let matched_by_graph = top
            .map(|hit| !hit.related_memory_ids.is_empty() || !hit.extracted_entities.is_empty())
            .unwrap_or(false);
        let matched_by_semantic = top.map(|hit| hit.score >= 0.52).unwrap_or(false);
        let bad_match_reason = if top_5_relevant {
            None
        } else if hits.is_empty() {
            Some("no_results".to_string())
        } else {
            Some("no_query_grounding_in_top_results".to_string())
        };

        rows.push(RetrievalEvalRow {
            category: case.category,
            query: case.query,
            top_1_relevant,
            top_5_relevant,
            matched_by_semantic,
            matched_by_prefix,
            matched_by_fuzzy,
            matched_by_ngram,
            matched_by_graph,
            matched_by_alias,
            query_expansion_terms: query_ctx.expanded_terms,
            bad_match_reason,
            top_memory_id: top.map(|hit| hit.id.clone()),
        });
    }

    Ok(RetrievalEvalReport {
        generated_at: now,
        total_queries: rows.len(),
        top1_hits: top1,
        top5_hits: top5,
        rows,
    })
}

/// Debug-only raw search path without MemoryCard synthesis.
#[tauri::command]
pub async fn search_raw_results(
    state: State<'_, Arc<AppState>>,
    query: String,
    time_filter: Option<String>,
    app_filter: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<SearchResult>, String> {
    search(state, query, time_filter, app_filter, limit).await
}

/// Summarize search results using AI
#[tauri::command]
pub async fn summarize_search(
    _state: State<'_, Arc<AppState>>,
    query: String,
    results_snippets: Vec<String>,
) -> Result<String, String> {
    if results_snippets.is_empty() {
        return Ok(String::new());
    }

    let evidence = parse_summary_evidence(&results_snippets);
    let summary = build_grounded_search_summary(&query, &evidence);
    Ok(summary)
}

#[derive(Debug, Clone)]
struct SummaryEvidence {
    score: f32,
    text: String,
}

fn parse_summary_evidence(snippets: &[String]) -> Vec<SummaryEvidence> {
    let mut evidence = Vec::new();
    for raw in snippets {
        let score = extract_bracket_value(raw, "score")
            .and_then(|value| value.parse::<f32>().ok())
            .unwrap_or(0.5);
        let text = strip_bracket_prefixes(raw);
        if text.is_empty() {
            continue;
        }
        evidence.push(SummaryEvidence { score, text });
    }
    evidence
}

fn extract_bracket_value(raw: &str, key: &str) -> Option<String> {
    let prefix = format!("[{}:", key);
    let start = raw.find(&prefix)?;
    let rest = &raw[start + prefix.len()..];
    let end = rest.find(']')?;
    let value = rest[..end].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn strip_bracket_prefixes(raw: &str) -> String {
    let mut remaining = raw.trim();
    while remaining.starts_with('[') {
        let Some(end) = remaining.find(']') else {
            break;
        };
        remaining = remaining[end + 1..].trim_start();
    }
    remaining.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn summary_terms(query: &str) -> Vec<String> {
    query
        .to_lowercase()
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|term| term.len() > 1)
        .filter(|term| !summary_stop_word(term))
        .map(|term| term.to_string())
        .collect()
}

fn summary_stop_word(term: &str) -> bool {
    matches!(
        term,
        "a" | "an"
            | "and"
            | "are"
            | "as"
            | "at"
            | "be"
            | "for"
            | "from"
            | "how"
            | "in"
            | "is"
            | "it"
            | "of"
            | "on"
            | "or"
            | "that"
            | "the"
            | "this"
            | "to"
            | "was"
            | "what"
            | "when"
            | "where"
            | "who"
            | "why"
            | "with"
    )
}

fn evidence_relevance(
    query_terms: &[String],
    query_numbers: &HashSet<String>,
    text: &str,
    score: f32,
) -> f32 {
    let normalized = text.to_lowercase();

    let coverage = if query_terms.is_empty() {
        0.5
    } else {
        query_terms
            .iter()
            .filter(|term| normalized.contains(term.as_str()))
            .count() as f32
            / query_terms.len() as f32
    };

    let number_overlap = if query_numbers.is_empty() {
        0.0
    } else if query_numbers
        .iter()
        .any(|number| normalized.contains(number.as_str()))
    {
        1.0
    } else {
        0.0
    };

    (coverage * 0.58 + score.clamp(0.0, 1.0) * 0.30 + number_overlap * 0.12).clamp(0.0, 1.0)
}

fn clean_summary_fragment(text: &str) -> String {
    truncate_chars(
        &text
            .replace('\n', " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .trim_matches('"')
            .trim_matches('\'')
            .trim()
            .to_string(),
        180,
    )
}

fn ensure_period(sentence: &str) -> String {
    let mut out = sentence.trim().trim_end_matches('.').to_string();
    if !out.ends_with('.') {
        out.push('.');
    }
    out
}

fn build_grounded_search_summary(query: &str, evidence: &[SummaryEvidence]) -> String {
    if evidence.is_empty() {
        return String::new();
    }

    let query_terms = summary_terms(query);
    let query_numbers = query_terms
        .iter()
        .filter(|term| term.chars().any(|ch| ch.is_ascii_digit()))
        .cloned()
        .collect::<HashSet<_>>();

    let mut scored = evidence
        .iter()
        .map(|item| {
            let relevance =
                evidence_relevance(&query_terms, &query_numbers, &item.text, item.score);
            (item, relevance)
        })
        .collect::<Vec<_>>();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let selected = scored
        .iter()
        .filter(|(_, relevance)| *relevance >= 0.22)
        .take(2)
        .collect::<Vec<_>>();

    if selected.len() < 2 {
        tracing::debug!(
            query = %query,
            selected = selected.len(),
            "summarize_search:suppressed_insufficient_evidence"
        );
        return String::new();
    }

    let mut fragments = Vec::new();
    let mut confidence = 0.0f32;
    for (item, relevance) in &selected {
        fragments.push(clean_summary_fragment(&item.text));
        confidence += *relevance;
    }
    confidence /= selected.len() as f32;

    let mut summary = ensure_period(
        fragments
            .first()
            .map(|text| text.as_str())
            .unwrap_or("Found related activity"),
    );
    if let Some(second) = fragments.get(1) {
        summary.push_str(" Then ");
        summary.push_str(&ensure_period(second));
    }

    if confidence < 0.36 {
        tracing::debug!(
            query = %query,
            confidence,
            "summarize_search:suppressed_low_confidence"
        );
        return String::new();
    }

    summary
}

/// Get capture status
#[tauri::command]
pub async fn get_status(state: State<'_, Arc<AppState>>) -> Result<CaptureStatus, String> {
    let embed_status = embedding_runtime_status();
    Ok(CaptureStatus {
        is_capturing: state.inner().is_capturing(),
        is_paused: state.inner().is_paused.load(Ordering::SeqCst),
        is_incognito: state.inner().is_incognito.load(Ordering::SeqCst),
        frames_captured: state.inner().frames_captured.load(Ordering::Relaxed),
        frames_dropped: state.inner().frames_dropped.load(Ordering::Relaxed),
        last_capture_time: state.inner().last_capture_time.load(Ordering::Relaxed),
        ai_model_available: state.inner().ai_model_available(),
        ai_model_loaded: state.inner().ai_model_loaded(),
        loaded_model_id: state.inner().loaded_model_id(),
        embedding_backend: embed_status.backend,
        embedding_degraded: embed_status.degraded,
        embedding_detail: embed_status.detail,
        embedding_model_name: embed_status.model_name,
        embedding_dimension: embed_status.dimension,
    })
}

/// Get MCP server status
#[tauri::command]
pub async fn get_mcp_server_status() -> Result<McpServerStatus, String> {
    Ok(mcp::status())
}

#[tauri::command]
pub async fn get_context_runtime_status(
    state: State<'_, Arc<AppState>>,
) -> Result<crate::store::ContextRuntimeStatus, String> {
    context_runtime::get_context_runtime_status(state.inner()).await
}

#[tauri::command]
pub async fn list_recent_context_packs(
    state: State<'_, Arc<AppState>>,
    limit: Option<usize>,
) -> Result<Vec<crate::store::ContextPack>, String> {
    context_runtime::list_recent_context_packs(state.inner(), limit.unwrap_or(8)).await
}

#[tauri::command]
pub async fn get_context_pack_detail(
    state: State<'_, Arc<AppState>>,
    pack_id: String,
) -> Result<Option<crate::store::ContextPack>, String> {
    context_runtime::get_context_pack_detail(state.inner(), &pack_id).await
}

#[tauri::command]
pub async fn fndr_subscribe(
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
pub async fn fndr_unsubscribe(
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

/// Get meeting recorder status
#[tauri::command]
pub async fn get_meeting_status() -> Result<MeetingRecorderStatus, String> {
    meeting::recorder_status()
}

/// Start a meeting recording session
#[tauri::command]
pub async fn start_meeting_recording(
    app: tauri::AppHandle,
    title: String,
    participants: Option<Vec<String>>,
    model: Option<String>,
) -> Result<MeetingRecorderStatus, String> {
    meeting::start_recording(Some(app), title, participants.unwrap_or_default(), model).await
}

/// Stop the active meeting recording session
#[tauri::command]
pub async fn stop_meeting_recording() -> Result<MeetingRecorderStatus, String> {
    meeting::stop_recording().await
}

/// List all local meeting sessions
#[tauri::command]
pub async fn list_meetings() -> Result<Vec<MeetingSession>, String> {
    meeting::list_meetings().await
}

/// Delete a local meeting session and its persisted artifacts
#[tauri::command]
pub async fn delete_meeting(meeting_id: String) -> Result<bool, String> {
    meeting::delete_meeting(&meeting_id).await
}

/// Get full transcript for a meeting
#[tauri::command]
pub async fn get_meeting_transcript(meeting_id: String) -> Result<MeetingTranscript, String> {
    meeting::get_meeting_transcript(&meeting_id).await
}

/// Re-run transcription on an existing meeting (useful after STT backend changes)
#[tauri::command]
pub async fn retranscribe_meeting(meeting_id: String) -> Result<(), String> {
    meeting::retranscribe_meeting(&meeting_id).await
}

/// Export a meeting transcript, summary, and action items to a PDF in the Downloads folder
#[tauri::command]
pub async fn export_meeting_pdf(
    _app: tauri::AppHandle,
    meeting_id: String,
) -> Result<String, String> {
    let transcript = meeting::get_meeting_transcript(&meeting_id).await?;
    let meeting = &transcript.meeting;

    // 1. Resolve Downloads folder
    let downloads_dir = dirs::download_dir()
        .ok_or_else(|| "Could not find Downloads folder on this system.".to_string())?;

    // 2. Prepare filename
    let safe_title = meeting
        .title
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ')
        .collect::<String>()
        .replace(' ', "_");

    let date_str = chrono::DateTime::from_timestamp(meeting.start_timestamp / 1000, 0)
        .map(|dt| dt.format("%Y%m%d_%H%M").to_string())
        .unwrap_or_else(|| meeting.id.clone());

    let filename = format!("FNDR_Meeting_{}_{}.pdf", safe_title, date_str);
    let target_path = downloads_dir.join(filename);

    // 3. Generate PDF
    // Use a common macOS font path. Arial is standard in Supplemental.
    let font_dir = std::path::Path::new("/System/Library/Fonts/Supplemental");

    // Load font variants manually since genpdf's from_files helper expects "Arial-Regular.ttf"
    // but macOS uses "Arial.ttf", "Arial Bold.ttf", etc.
    let regular = genpdf::fonts::FontData::load(font_dir.join("Arial.ttf"), None)
        .map_err(|e| format!("Failed to load 'Arial.ttf' from {:?}: {}", font_dir, e))?;
    let bold = genpdf::fonts::FontData::load(font_dir.join("Arial Bold.ttf"), None)
        .map_err(|e| format!("Failed to load 'Arial Bold.ttf' from {:?}: {}", font_dir, e))?;
    let italic =
        genpdf::fonts::FontData::load(font_dir.join("Arial Italic.ttf"), None).map_err(|e| {
            format!(
                "Failed to load 'Arial Italic.ttf' from {:?}: {}",
                font_dir, e
            )
        })?;
    let bold_italic = genpdf::fonts::FontData::load(font_dir.join("Arial Bold Italic.ttf"), None)
        .map_err(|e| {
        format!(
            "Failed to load 'Arial Bold Italic.ttf' from {:?}: {}",
            font_dir, e
        )
    })?;

    let font_family = genpdf::fonts::FontFamily {
        regular,
        bold,
        italic,
        bold_italic,
    };

    let mut doc = genpdf::Document::new(font_family);
    doc.set_title(format!("FNDR Meeting: {}", meeting.title));

    let mut decorator = genpdf::SimplePageDecorator::new();
    decorator.set_margins(18);
    doc.set_page_decorator(decorator);

    // Title & Header
    doc.push(
        elements::Text::new(&meeting.title).styled(style::Style::new().bold().with_font_size(20)),
    );
    let date_label = chrono::DateTime::from_timestamp(meeting.start_timestamp / 1000, 0)
        .map(|dt| dt.format("%B %d, %Y at %H:%M").to_string())
        .unwrap_or_else(|| "Unknown Date".to_string());
    doc.push(
        elements::Text::new(format!("Date: {}", date_label))
            .styled(style::Style::new().with_font_size(10)),
    );
    doc.push(elements::Break::new(1.5));

    // Breakdown Sections
    if let Some(breakdown) = &meeting.breakdown {
        if !breakdown.summary.is_empty() && !breakdown.summary.eq_ignore_ascii_case("none") {
            doc.push(
                elements::Text::new("Summary")
                    .styled(style::Style::new().bold().with_font_size(14)),
            );
            doc.push(elements::Paragraph::new(&breakdown.summary));
            doc.push(elements::Break::new(1.0));
        }

        let filtered_todos: Vec<_> = breakdown
            .todos
            .iter()
            .filter(|s| !s.trim().is_empty() && !s.eq_ignore_ascii_case("none"))
            .collect();
        if !filtered_todos.is_empty() {
            doc.push(
                elements::Text::new("To-dos").styled(style::Style::new().bold().with_font_size(14)),
            );
            let mut list = elements::UnorderedList::new();
            for item in filtered_todos {
                list.push(elements::Text::new(item));
            }
            doc.push(list);
            doc.push(elements::Break::new(1.0));
        }

        let filtered_reminders: Vec<_> = breakdown
            .reminders
            .iter()
            .filter(|s| !s.trim().is_empty() && !s.eq_ignore_ascii_case("none"))
            .collect();
        if !filtered_reminders.is_empty() {
            doc.push(
                elements::Text::new("Reminders")
                    .styled(style::Style::new().bold().with_font_size(14)),
            );
            let mut list = elements::UnorderedList::new();
            for item in filtered_reminders {
                list.push(elements::Text::new(item));
            }
            doc.push(list);
            doc.push(elements::Break::new(1.0));
        }

        let filtered_followups: Vec<_> = breakdown
            .followups
            .iter()
            .filter(|s| !s.trim().is_empty() && !s.eq_ignore_ascii_case("none"))
            .collect();
        if !filtered_followups.is_empty() {
            doc.push(
                elements::Text::new("Follow-ups")
                    .styled(style::Style::new().bold().with_font_size(14)),
            );
            let mut list = elements::UnorderedList::new();
            for item in filtered_followups {
                list.push(elements::Text::new(item));
            }
            doc.push(list);
            doc.push(elements::Break::new(1.0));
        }
    }

    // Full Transcript
    if !transcript.full_text.is_empty() {
        doc.push(elements::Break::new(0.5));
        doc.push(
            elements::Text::new("Full Transcript")
                .styled(style::Style::new().bold().with_font_size(14)),
        );
        doc.push(elements::Paragraph::new(&transcript.full_text));
    }

    // Save
    doc.render_to_file(&target_path)
        .map_err(|e| format!("Failed to generate PDF file: {}", e))?;

    Ok(target_path.to_string_lossy().to_string())
}

/// Export a daily summary text to a PDF in the Downloads folder
#[tauri::command]
pub async fn export_daily_summary_pdf(
    _app: tauri::AppHandle,
    date_str: String,
    summary_text: String,
) -> Result<String, String> {
    // 1. Resolve Downloads folder
    let downloads_dir = dirs::download_dir()
        .ok_or_else(|| "Could not find Downloads folder on this system.".to_string())?;

    // 2. Prepare filename
    let safe_date = date_str.replace('/', "-").replace(' ', "_");
    let filename = format!("FNDR_Daily_Summary_{}.pdf", safe_date);
    let target_path = downloads_dir.join(filename);

    // 3. Generate PDF
    // Use a common macOS font path. Arial is standard in Supplemental.
    let font_dir = std::path::Path::new("/System/Library/Fonts/Supplemental");

    let regular = genpdf::fonts::FontData::load(font_dir.join("Arial.ttf"), None)
        .map_err(|e| format!("Failed to load 'Arial.ttf' from {:?}: {}", font_dir, e))?;
    let bold = genpdf::fonts::FontData::load(font_dir.join("Arial Bold.ttf"), None)
        .map_err(|e| format!("Failed to load 'Arial Bold.ttf' from {:?}: {}", font_dir, e))?;
    let italic =
        genpdf::fonts::FontData::load(font_dir.join("Arial Italic.ttf"), None).map_err(|e| {
            format!(
                "Failed to load 'Arial Italic.ttf' from {:?}: {}",
                font_dir, e
            )
        })?;
    let bold_italic = genpdf::fonts::FontData::load(font_dir.join("Arial Bold Italic.ttf"), None)
        .map_err(|e| {
        format!(
            "Failed to load 'Arial Bold Italic.ttf' from {:?}: {}",
            font_dir, e
        )
    })?;

    let font_family = genpdf::fonts::FontFamily {
        regular,
        bold,
        italic,
        bold_italic,
    };

    let mut doc = genpdf::Document::new(font_family);
    doc.set_title(format!("FNDR Daily Summary: {}", date_str));

    let mut decorator = genpdf::SimplePageDecorator::new();
    decorator.set_margins(18);
    doc.set_page_decorator(decorator);

    // Title & Header
    doc.push(
        genpdf::elements::Text::new("FNDR Daily Summary")
            .styled(genpdf::style::Style::new().bold().with_font_size(20)),
    );
    doc.push(
        genpdf::elements::Text::new(format!("Date: {}", date_str))
            .styled(genpdf::style::Style::new().with_font_size(10)),
    );
    doc.push(genpdf::elements::Break::new(1.5));

    if !summary_text.is_empty() {
        let mut list = genpdf::elements::UnorderedList::new();
        for line in summary_text.split('\n') {
            let trim = line.trim();
            if !trim.is_empty() {
                // Remove existing bullet points if present as genpdf adds its own
                let content = if trim.starts_with("- ")
                    || trim.starts_with("* ")
                    || trim.starts_with("• ")
                {
                    trim[2..].trim()
                } else if trim.starts_with("-") || trim.starts_with("*") || trim.starts_with("•")
                {
                    trim[1..].trim()
                } else {
                    trim
                };
                list.push(genpdf::elements::Paragraph::new(content.to_string()));
            }
        }
        doc.push(list);
    } else {
        doc.push(genpdf::elements::Paragraph::new(
            "No activity captured for this date.",
        ));
    }

    // Save
    doc.render_to_file(&target_path)
        .map_err(|e| format!("Failed to generate PDF file: {}", e))?;

    Ok(target_path.to_string_lossy().to_string())
}

/// Open a PDF exported by FNDR from the user's Downloads folder.
#[tauri::command]
pub async fn open_exported_pdf(path: String) -> Result<(), String> {
    let downloads_dir = dirs::download_dir()
        .ok_or_else(|| "Could not find Downloads folder on this system.".to_string())?;
    let downloads_dir = downloads_dir
        .canonicalize()
        .map_err(|e| format!("Could not resolve Downloads folder: {}", e))?;
    let target_path = PathBuf::from(path);
    let target_path = target_path
        .canonicalize()
        .map_err(|e| format!("Could not find exported PDF: {}", e))?;

    if !target_path.starts_with(&downloads_dir) {
        return Err("FNDR can only open exported PDFs from your Downloads folder.".to_string());
    }

    let is_pdf = target_path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"));
    if !is_pdf {
        return Err("FNDR can only open exported PDF files.".to_string());
    }

    open_path_with_system(&target_path)
}

#[cfg(target_os = "macos")]
fn open_path_with_system(path: &Path) -> Result<(), String> {
    Command::new("open")
        .arg(path)
        .spawn()
        .map_err(|e| format!("Failed to open exported PDF: {}", e))?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn open_path_with_system(path: &Path) -> Result<(), String> {
    Command::new("cmd")
        .arg("/C")
        .arg("start")
        .arg("")
        .arg(path)
        .spawn()
        .map_err(|e| format!("Failed to open exported PDF: {}", e))?;
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_path_with_system(path: &Path) -> Result<(), String> {
    Command::new("xdg-open")
        .arg(path)
        .spawn()
        .map_err(|e| format!("Failed to open exported PDF: {}", e))?;
    Ok(())
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

/// Synthesize a short spoken response for the FNDR UI
#[tauri::command]
pub async fn speak_text(
    app: AppHandle,
    text: String,
    voice_id: Option<String>,
) -> Result<SpeechSynthesisResult, String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let voice_id = voice_id.unwrap_or_else(|| "tara".to_string());
    let audio_path = speech::synthesize_speech(&app_data_dir, &text, Some(&voice_id)).await?;

    Ok(SpeechSynthesisResult {
        audio_path: audio_path.to_string_lossy().to_string(),
        voice_id,
    })
}

/// Pause capture
#[tauri::command]
pub async fn pause_capture(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.inner().pause();
    Ok(())
}

/// Resume capture
#[tauri::command]
pub async fn resume_capture(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.inner().resume();
    Ok(())
}

/// Get blocklist
#[tauri::command]
pub async fn get_blocklist(state: State<'_, Arc<AppState>>) -> Result<Vec<String>, String> {
    let config = state.inner().config.read();
    Ok(config.blocklist.clone())
}

/// Set blocklist
#[tauri::command]
pub async fn set_blocklist(
    state: State<'_, Arc<AppState>>,
    apps: Vec<String>,
) -> Result<(), String> {
    let mut config = state.inner().config.write();
    config.blocklist = apps;
    config.blocklist = config
        .blocklist
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .fold(Vec::new(), |mut acc, value| {
            if !acc
                .iter()
                .any(|existing: &String| existing.eq_ignore_ascii_case(&value))
            {
                acc.push(value);
            }
            acc
        });
    config
        .save()
        .map_err(|e: Box<dyn std::error::Error>| e.to_string())?;
    Ok(())
}

/// Delete all data
#[tauri::command]
pub async fn delete_all_data(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    // 1. Clear memory records
    state
        .inner()
        .store
        .delete_all()
        .await
        .map_err(|e: Box<dyn std::error::Error>| e.to_string())?;
    state.invalidate_memory_derived_caches();

    // 2. Clear knowledge graph
    if let Err(e) = state.inner().graph.clear_all().await {
        tracing::warn!("Failed to clear graph store during delete_all: {}", e);
    }

    // 3. Delete persisted capture artifacts
    for artifact_dir in ["frames", "screenshots", "meetings"] {
        let path = state.inner().store.data_dir().join(artifact_dir);
        if path.exists() {
            if let Err(e) = std::fs::remove_dir_all(&path) {
                tracing::warn!("Failed to remove {} dir: {}", artifact_dir, e);
            }
        }
    }

    tracing::info!("All FNDR data deleted");
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

fn merge_bucket_for_anchor(anchor: Option<&str>, app_name: &str) -> &'static str {
    if let Some(anchor) = anchor {
        let lower = anchor.to_lowercase();
        if lower.starts_with("spotify:") || lower.contains("spotify") {
            return "spotify";
        }
        if lower.starts_with("youtube:") || lower.contains("youtube") {
            return "youtube";
        }
        if lower.starts_with("codex:") || lower.contains("codex") || lower.contains("cursor") {
            return "codex";
        }
        if lower.starts_with("discord:") || lower.contains("discord") {
            return "discord";
        }
        if lower.starts_with("gitlab:") || lower.contains("gitlab") {
            return "gitlab";
        }
        if lower.starts_with("antigravity:") || lower.contains("antigravity") {
            return "antigravity";
        }
    }

    let app = app_name.to_lowercase();
    if app.contains("spotify") {
        return "spotify";
    }
    if app.contains("youtube") {
        return "youtube";
    }
    if app.contains("codex") || app.contains("cursor") {
        return "codex";
    }
    if app.contains("discord") {
        return "discord";
    }
    if app.contains("gitlab") {
        return "gitlab";
    }
    if app.contains("antigravity") {
        return "antigravity";
    }
    "generic"
}

#[tauri::command]
pub async fn run_memory_repair_backfill(
    state: State<'_, Arc<AppState>>,
) -> Result<MemoryRepairSummary, String> {
    run_memory_repair_backfill_for_state(state.inner().clone()).await
}

async fn run_memory_repair_backfill_for_state(
    state: Arc<AppState>,
) -> Result<MemoryRepairSummary, String> {
    let embedder = shared_real_embedder()?;
    if MEMORY_REPAIR_RUNNING.swap(true, Ordering::AcqRel) {
        return Err("Memory continuity repair is already running".to_string());
    }
    struct MemoryRepairRunGuard;
    impl Drop for MemoryRepairRunGuard {
        fn drop(&mut self) {
            MEMORY_REPAIR_RUNNING.store(false, Ordering::Release);
        }
    }
    let _run_guard = MemoryRepairRunGuard;

    let should_resume_capture = !state.is_paused.load(Ordering::SeqCst);
    if should_resume_capture {
        state.pause();
    }
    struct CaptureResumeGuard {
        state: Arc<AppState>,
        should_resume: bool,
    }
    impl Drop for CaptureResumeGuard {
        fn drop(&mut self) {
            if self.should_resume {
                self.state.resume();
            }
        }
    }
    let _capture_resume_guard = CaptureResumeGuard {
        state: state.clone(),
        should_resume: should_resume_capture,
    };

    let progress_path = memory_repair_progress_path(state.as_ref());
    let checkpoint_path = memory_repair_checkpoint_path(state.as_ref());
    let mut all_memories = state
        .store
        .list_all_memories()
        .await
        .map_err(|e: Box<dyn std::error::Error>| e.to_string())?;

    if all_memories.is_empty() {
        let _ = std::fs::remove_file(&checkpoint_path);
        persist_memory_repair_progress(
            &progress_path,
            &MemoryRepairProgress {
                is_running: false,
                phase: "complete".to_string(),
                processed: 0,
                total: 0,
                merged_count: 0,
                anchor_merges: 0,
                timestamp_ms: chrono::Utc::now().timestamp_millis(),
            },
        );
        return Ok(MemoryRepairSummary {
            total_before: 0,
            total_after: 0,
            merged_count: 0,
            anchor_merges: 0,
            task_reference_updates: 0,
            screenshots_cleaned: 0,
            embeddings_refreshed: 0,
            chars_before: 0,
            chars_after: 0,
            chars_reclaimed: 0,
            spotify_merges: 0,
            youtube_merges: 0,
            codex_merges: 0,
            discord_merges: 0,
            gitlab_merges: 0,
            antigravity_merges: 0,
            app_merges: Vec::new(),
        });
    }

    all_memories.sort_by_key(|memory| memory.timestamp);
    let before_count = all_memories.len();
    let chars_before = all_memories
        .iter()
        .map(|memory| memory.text.chars().count() + memory.clean_text.chars().count())
        .sum::<usize>();
    let source_fingerprint = memory_repair_source_fingerprint(&all_memories);
    let source_first_id = all_memories
        .first()
        .map(|memory| memory.id.clone())
        .unwrap_or_default();
    let source_last_id = all_memories
        .last()
        .map(|memory| memory.id.clone())
        .unwrap_or_default();

    let before_screenshots: HashSet<String> = all_memories
        .iter()
        .filter_map(|memory| memory.screenshot_path.clone())
        .collect();

    let backfill_engine: Option<&Arc<crate::inference::InferenceEngine>> = None;

    let mut merged_memories: Vec<MemoryRecord> = Vec::with_capacity(before_count);
    let mut anchor_index: HashMap<String, usize> = HashMap::new();
    let mut app_index: HashMap<String, Vec<usize>> = HashMap::new();
    let mut id_redirect: HashMap<String, String> = HashMap::new();
    let mut processed = 0usize;
    let mut resumed_from_checkpoint = false;

    let mut merged_count = 0usize;
    let mut anchor_merges = 0usize;
    let mut embeddings_refreshed = 0usize;
    let mut spotify_merges = 0usize;
    let mut youtube_merges = 0usize;
    let mut codex_merges = 0usize;
    let mut discord_merges = 0usize;
    let mut gitlab_merges = 0usize;
    let mut antigravity_merges = 0usize;
    let mut app_merge_counts: HashMap<String, usize> = HashMap::new();

    if let Some(checkpoint) = load_memory_repair_checkpoint(&checkpoint_path) {
        let checkpoint_valid = (checkpoint.version == MEMORY_REPAIR_CHECKPOINT_VERSION
            || checkpoint.version == 1)
            && checkpoint.source_total == before_count
            && checkpoint.source_fingerprint == source_fingerprint
            && checkpoint.source_first_id == source_first_id
            && checkpoint.source_last_id == source_last_id
            && checkpoint.processed <= before_count
            && checkpoint.merged_memories.len() <= checkpoint.processed
            && checkpoint.id_redirect.len() <= checkpoint.processed;

        if checkpoint_valid {
            merged_memories = checkpoint.merged_memories;
            id_redirect = checkpoint.id_redirect;
            processed = checkpoint.processed;
            merged_count = checkpoint.merged_count;
            anchor_merges = checkpoint.anchor_merges;
            spotify_merges = checkpoint.spotify_merges;
            youtube_merges = checkpoint.youtube_merges;
            codex_merges = checkpoint.codex_merges;
            discord_merges = checkpoint.discord_merges;
            gitlab_merges = checkpoint.gitlab_merges;
            antigravity_merges = checkpoint.antigravity_merges;
            app_merge_counts = checkpoint.app_merge_counts;

            for (index, memory) in merged_memories.iter().enumerate() {
                if let Some(anchor) = continuity_anchor_for_memory(memory) {
                    anchor_index.insert(anchor, index);
                }
                app_index
                    .entry(memory.app_name.to_lowercase())
                    .or_default()
                    .push(index);
            }

            resumed_from_checkpoint = true;
            tracing::info!(
                "memory_repair_backfill: resumed from checkpoint at {}/{}",
                processed,
                before_count
            );
        } else {
            let _ = std::fs::remove_file(&checkpoint_path);
        }
    }

    persist_memory_repair_progress(
        &progress_path,
        &MemoryRepairProgress {
            is_running: true,
            phase: if resumed_from_checkpoint {
                "resuming".to_string()
            } else {
                "scanning".to_string()
            },
            processed,
            total: before_count,
            merged_count,
            anchor_merges,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
        },
    );

    let mut last_heartbeat = Instant::now();
    let heartbeat_interval = Duration::from_secs(1);
    let heartbeat_count_step = 75usize;
    let checkpoint_interval = Duration::from_millis(MEMORY_REPAIR_CHECKPOINT_MS);
    let mut last_checkpoint = Instant::now();

    for incoming in all_memories.into_iter().skip(processed) {
        processed += 1;
        let incoming_id = incoming.id.clone();
        let normalized_app = incoming.app_name.to_lowercase();
        let incoming_anchor = continuity_anchor_for_memory(&incoming);
        let mut merged_into_idx: Option<usize> = None;

        if eligible_for_story_merge(&incoming) {
            if let Some(anchor) = incoming_anchor.as_ref() {
                if let Some(index) = anchor_index.get(anchor).copied() {
                    if merged_memories
                        .get(index)
                        .map(|existing| existing.app_name == incoming.app_name)
                        .unwrap_or(false)
                    {
                        merged_into_idx = Some(index);
                        anchor_merges += 1;
                    }
                }
            }

            if merged_into_idx.is_none() {
                if let Some(candidates) = app_index.get(&normalized_app) {
                    let mut best: Option<(usize, f32)> = None;
                    for candidate_index in candidates
                        .iter()
                        .rev()
                        .take(MEMORY_REPAIR_SIMILARITY_SCAN_LIMIT)
                    {
                        let existing = &merged_memories[*candidate_index];
                        let score = score_memory_candidate(&incoming, existing);
                        if !passes_merge_threshold(score) {
                            continue;
                        }
                        if best
                            .as_ref()
                            .map(|(_, best_score)| score.score > *best_score)
                            .unwrap_or(true)
                        {
                            best = Some((*candidate_index, score.score));
                        }
                    }
                    merged_into_idx = best.map(|(index, _)| index);
                }
            }
        }

        if let Some(target_index) = merged_into_idx {
            let existing_id = merged_memories[target_index].id.clone();
            let merged = merge_memory_records_with_policy(
                merged_memories[target_index].clone(),
                incoming.clone(),
                Some(embedder),
                backfill_engine,
                true,
                false,
            )
            .await;
            merged_memories[target_index] = merged.clone();
            id_redirect.insert(incoming_id, existing_id);
            merged_count += 1;

            let merge_bucket =
                merge_bucket_for_anchor(incoming_anchor.as_deref(), &incoming.app_name);
            match merge_bucket {
                "spotify" => spotify_merges += 1,
                "youtube" => youtube_merges += 1,
                "codex" => codex_merges += 1,
                "discord" => discord_merges += 1,
                "gitlab" => gitlab_merges += 1,
                "antigravity" => antigravity_merges += 1,
                _ => {}
            }
            *app_merge_counts
                .entry(incoming.app_name.clone())
                .or_insert(0) += 1;

            if let Some(anchor) = continuity_anchor_for_memory(&merged) {
                anchor_index.insert(anchor, target_index);
            }
            if processed % heartbeat_count_step == 0
                || last_heartbeat.elapsed() >= heartbeat_interval
            {
                tracing::info!(
                    "memory_repair_backfill:progress processed={} total={} merged={} anchor_merges={}",
                    processed,
                    before_count,
                    merged_count,
                    anchor_merges
                );
                persist_memory_repair_progress(
                    &progress_path,
                    &MemoryRepairProgress {
                        is_running: true,
                        phase: "scanning".to_string(),
                        processed,
                        total: before_count,
                        merged_count,
                        anchor_merges,
                        timestamp_ms: chrono::Utc::now().timestamp_millis(),
                    },
                );
                last_heartbeat = Instant::now();
            }

            if processed % MEMORY_REPAIR_CHECKPOINT_ITEM_STEP == 0
                || last_checkpoint.elapsed() >= checkpoint_interval
            {
                persist_memory_repair_checkpoint(
                    &checkpoint_path,
                    &MemoryRepairCheckpoint {
                        version: MEMORY_REPAIR_CHECKPOINT_VERSION,
                        source_total: before_count,
                        source_fingerprint,
                        source_first_id: source_first_id.clone(),
                        source_last_id: source_last_id.clone(),
                        processed,
                        merged_memories: merged_memories.clone(),
                        id_redirect: id_redirect.clone(),
                        merged_count,
                        anchor_merges,
                        spotify_merges,
                        youtube_merges,
                        codex_merges,
                        discord_merges,
                        gitlab_merges,
                        antigravity_merges,
                        app_merge_counts: app_merge_counts.clone(),
                    },
                );
                last_checkpoint = Instant::now();
            }
            continue;
        }

        let index = merged_memories.len();
        if let Some(anchor) = incoming_anchor {
            anchor_index.insert(anchor, index);
        }
        app_index.entry(normalized_app).or_default().push(index);
        merged_memories.push(incoming);

        if processed % heartbeat_count_step == 0 || last_heartbeat.elapsed() >= heartbeat_interval {
            tracing::info!(
                "memory_repair_backfill:progress processed={} total={} merged={} anchor_merges={}",
                processed,
                before_count,
                merged_count,
                anchor_merges
            );
            persist_memory_repair_progress(
                &progress_path,
                &MemoryRepairProgress {
                    is_running: true,
                    phase: "scanning".to_string(),
                    processed,
                    total: before_count,
                    merged_count,
                    anchor_merges,
                    timestamp_ms: chrono::Utc::now().timestamp_millis(),
                },
            );
            last_heartbeat = Instant::now();
        }

        if processed % MEMORY_REPAIR_CHECKPOINT_ITEM_STEP == 0
            || last_checkpoint.elapsed() >= checkpoint_interval
        {
            persist_memory_repair_checkpoint(
                &checkpoint_path,
                &MemoryRepairCheckpoint {
                    version: MEMORY_REPAIR_CHECKPOINT_VERSION,
                    source_total: before_count,
                    source_fingerprint,
                    source_first_id: source_first_id.clone(),
                    source_last_id: source_last_id.clone(),
                    processed,
                    merged_memories: merged_memories.clone(),
                    id_redirect: id_redirect.clone(),
                    merged_count,
                    anchor_merges,
                    spotify_merges,
                    youtube_merges,
                    codex_merges,
                    discord_merges,
                    gitlab_merges,
                    antigravity_merges,
                    app_merge_counts: app_merge_counts.clone(),
                },
            );
            last_checkpoint = Instant::now();
        }
    }

    for memory in &mut merged_memories {
        if is_low_signal_embedding(&memory.embedding) {
            let text_input = best_embedding_text(memory);
            if !text_input.is_empty() {
                if let Ok(mut vectors) = embedder.embed_batch_with_context(&[(
                    memory.app_name.clone(),
                    memory.window_title.clone(),
                    text_input,
                )]) {
                    if let Some(vector) = vectors.pop() {
                        memory.embedding = vector;
                        embeddings_refreshed += 1;
                    }
                }
            }
        }

        if is_low_signal_embedding(&memory.snippet_embedding) {
            let snippet_input = best_snippet_embedding_text(memory);
            if !snippet_input.is_empty() {
                if let Ok(mut vectors) = embedder.embed_batch_with_context(&[(
                    memory.app_name.clone(),
                    memory.window_title.clone(),
                    snippet_input,
                )]) {
                    if let Some(vector) = vectors.pop() {
                        memory.snippet_embedding = vector;
                        embeddings_refreshed += 1;
                    }
                }
            }
        }

        if is_low_signal_embedding(&memory.support_embedding) {
            let support_inputs = best_support_embedding_texts(memory);
            if !support_inputs.is_empty() {
                let contexts = support_inputs
                    .into_iter()
                    .map(|text| (memory.app_name.clone(), memory.window_title.clone(), text))
                    .collect::<Vec<_>>();
                if let Ok(vectors) = embedder.embed_batch_with_context(&contexts) {
                    memory.support_embedding = mean_pool_embeddings(&vectors);
                    embeddings_refreshed += 1;
                }
            }
        }
    }

    let chars_after = merged_memories
        .iter()
        .map(compact_memory_record_payload)
        .map(|memory| memory.text.chars().count() + memory.clean_text.chars().count())
        .sum::<usize>();

    persist_memory_repair_progress(
        &progress_path,
        &MemoryRepairProgress {
            is_running: true,
            phase: "writing".to_string(),
            processed: before_count,
            total: before_count,
            merged_count,
            anchor_merges,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
        },
    );
    persist_memory_repair_checkpoint(
        &checkpoint_path,
        &MemoryRepairCheckpoint {
            version: MEMORY_REPAIR_CHECKPOINT_VERSION,
            source_total: before_count,
            source_fingerprint,
            source_first_id: source_first_id.clone(),
            source_last_id: source_last_id.clone(),
            processed: before_count,
            merged_memories: merged_memories.clone(),
            id_redirect: id_redirect.clone(),
            merged_count,
            anchor_merges,
            spotify_merges,
            youtube_merges,
            codex_merges,
            discord_merges,
            gitlab_merges,
            antigravity_merges,
            app_merge_counts: app_merge_counts.clone(),
        },
    );

    if let Err(err) = state.store.delete_all().await {
        persist_memory_repair_progress(
            &progress_path,
            &MemoryRepairProgress {
                is_running: false,
                phase: "error".to_string(),
                processed,
                total: before_count,
                merged_count,
                anchor_merges,
                timestamp_ms: chrono::Utc::now().timestamp_millis(),
            },
        );
        persist_memory_repair_checkpoint(
            &checkpoint_path,
            &MemoryRepairCheckpoint {
                version: MEMORY_REPAIR_CHECKPOINT_VERSION,
                source_total: before_count,
                source_fingerprint,
                source_first_id: source_first_id.clone(),
                source_last_id: source_last_id.clone(),
                processed,
                merged_memories,
                id_redirect,
                merged_count,
                anchor_merges,
                spotify_merges,
                youtube_merges,
                codex_merges,
                discord_merges,
                gitlab_merges,
                antigravity_merges,
                app_merge_counts,
            },
        );
        return Err(err.to_string());
    }
    state.invalidate_memory_derived_caches();
    if let Err(err) = state.store.add_batch_preserving_ids(&merged_memories).await {
        persist_memory_repair_progress(
            &progress_path,
            &MemoryRepairProgress {
                is_running: false,
                phase: "error".to_string(),
                processed,
                total: before_count,
                merged_count,
                anchor_merges,
                timestamp_ms: chrono::Utc::now().timestamp_millis(),
            },
        );
        persist_memory_repair_checkpoint(
            &checkpoint_path,
            &MemoryRepairCheckpoint {
                version: MEMORY_REPAIR_CHECKPOINT_VERSION,
                source_total: before_count,
                source_fingerprint,
                source_first_id: source_first_id.clone(),
                source_last_id: source_last_id.clone(),
                processed,
                merged_memories,
                id_redirect,
                merged_count,
                anchor_merges,
                spotify_merges,
                youtube_merges,
                codex_merges,
                discord_merges,
                gitlab_merges,
                antigravity_merges,
                app_merge_counts,
            },
        );
        return Err(err.to_string());
    }
    state.invalidate_memory_derived_caches();

    let after_screenshots: HashSet<String> = merged_memories
        .iter()
        .map(compact_memory_record_payload)
        .filter_map(|memory| memory.screenshot_path)
        .collect();
    let screenshots_cleaned = before_screenshots
        .difference(&after_screenshots)
        .filter(|path| std::fs::remove_file(path).is_ok())
        .count();

    let mut task_reference_updates = 0usize;
    let mut tasks = state.store.list_tasks().await.map_err(|e| e.to_string())?;
    for task in &mut tasks {
        if let Some(source_id) = task.source_memory_id.clone() {
            if let Some(new_id) = id_redirect.get(&source_id) {
                if new_id != &source_id {
                    task.source_memory_id = Some(new_id.clone());
                    task_reference_updates += 1;
                }
            }
        }

        if !task.linked_memory_ids.is_empty() {
            let before = task.linked_memory_ids.clone();
            let mut seen = HashSet::new();
            let rewritten: Vec<String> = before
                .iter()
                .map(|memory_id| {
                    id_redirect
                        .get(memory_id)
                        .cloned()
                        .unwrap_or_else(|| memory_id.clone())
                })
                .filter(|memory_id| seen.insert(memory_id.clone()))
                .collect();
            if rewritten != before {
                task_reference_updates += before
                    .iter()
                    .zip(rewritten.iter())
                    .filter(|(left, right)| left != right)
                    .count()
                    + before.len().saturating_sub(rewritten.len());
                task.linked_memory_ids = rewritten;
            }
        }
    }

    if task_reference_updates > 0 {
        if let Err(err) = state.store.upsert_tasks(&tasks).await {
            persist_memory_repair_progress(
                &progress_path,
                &MemoryRepairProgress {
                    is_running: false,
                    phase: "error".to_string(),
                    processed: before_count,
                    total: before_count,
                    merged_count,
                    anchor_merges,
                    timestamp_ms: chrono::Utc::now().timestamp_millis(),
                },
            );
            persist_memory_repair_checkpoint(
                &checkpoint_path,
                &MemoryRepairCheckpoint {
                    version: MEMORY_REPAIR_CHECKPOINT_VERSION,
                    source_total: before_count,
                    source_fingerprint,
                    source_first_id: source_first_id.clone(),
                    source_last_id: source_last_id.clone(),
                    processed: before_count,
                    merged_memories,
                    id_redirect,
                    merged_count,
                    anchor_merges,
                    spotify_merges,
                    youtube_merges,
                    codex_merges,
                    discord_merges,
                    gitlab_merges,
                    antigravity_merges,
                    app_merge_counts,
                },
            );
            return Err(err.to_string());
        }
    }

    persist_memory_repair_progress(
        &progress_path,
        &MemoryRepairProgress {
            is_running: false,
            phase: "complete".to_string(),
            processed: before_count,
            total: before_count,
            merged_count,
            anchor_merges,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
        },
    );
    let _ = std::fs::remove_file(&checkpoint_path);

    let mut app_merges: Vec<AppMergeCount> = app_merge_counts
        .into_iter()
        .map(|(app_name, merged)| AppMergeCount { app_name, merged })
        .collect();
    app_merges.sort_by(|left, right| right.merged.cmp(&left.merged));

    Ok(MemoryRepairSummary {
        total_before: before_count,
        total_after: merged_memories.len(),
        merged_count,
        anchor_merges,
        task_reference_updates,
        screenshots_cleaned,
        embeddings_refreshed,
        chars_before,
        chars_after,
        chars_reclaimed: chars_before.saturating_sub(chars_after),
        spotify_merges,
        youtube_merges,
        codex_merges,
        discord_merges,
        gitlab_merges,
        antigravity_merges,
        app_merges,
    })
}

// ========== Task Commands ==========

fn task_type_sort_key(task_type: &TaskType) -> u8 {
    match task_type {
        TaskType::Todo => 0,
        TaskType::Reminder => 1,
        TaskType::Followup => 2,
    }
}

fn parse_task_type(task_type: Option<&str>) -> TaskType {
    match task_type {
        Some("Reminder") => TaskType::Reminder,
        Some("Followup") => TaskType::Followup,
        _ => TaskType::Todo,
    }
}

fn normalize_task_text(value: &str) -> String {
    value
        .to_lowercase()
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn task_terms(title: &str) -> Vec<String> {
    normalize_task_text(title)
        .split_whitespace()
        .filter(|term| term.len() >= 3)
        .filter(|term| {
            !matches!(
                *term,
                "the"
                    | "and"
                    | "for"
                    | "with"
                    | "from"
                    | "that"
                    | "this"
                    | "todo"
                    | "task"
                    | "follow"
                    | "followup"
                    | "reminder"
            )
        })
        .map(|term| term.to_string())
        .collect()
}

fn is_manual_task(task: &Task) -> bool {
    task.source_app.eq_ignore_ascii_case("manual")
}

fn is_meeting_task(task: &Task) -> bool {
    task.source_app.starts_with("Meeting:")
}

fn is_memory_task(task: &Task) -> bool {
    task.source_app.starts_with("Memory:") || task.source_app.eq_ignore_ascii_case("auto")
}

fn task_has_supporting_context(task: &Task) -> bool {
    task.source_memory_id.is_some()
        || !task.linked_memory_ids.is_empty()
        || !task.linked_urls.is_empty()
}

fn is_low_signal_task_title(title: &str) -> bool {
    let normalized = normalize_task_text(title);
    if normalized.len() < 6 {
        return true;
    }
    if normalized.split_whitespace().count() < 2 {
        return true;
    }

    if matches!(
        normalized.as_str(),
        "todo"
            | "to do"
            | "task"
            | "follow up"
            | "followup"
            | "reminder"
            | "none"
            | "n a"
            | "check this"
            | "look into this"
            | "work on this"
    ) {
        return true;
    }

    let generic_prefixes = [
        "complete ",
        "remember ",
        "follow up ",
        "followup ",
        "work on ",
        "check ",
        "look into ",
    ];
    generic_prefixes
        .iter()
        .any(|prefix| normalized.starts_with(prefix) && normalized.split_whitespace().count() <= 3)
}

fn task_priority_score(task: &Task, now_ms: i64) -> i64 {
    let age_hours = ((now_ms - task.created_at).max(0) / 3_600_000) as i64;
    let recency_bonus = (96 - age_hours).clamp(0, 96);
    let memory_bonus = (task.linked_memory_ids.len().min(10) as i64) * 4;
    let url_bonus = (task.linked_urls.len().min(6) as i64) * 2;
    let due_bonus = if task.due_date.is_some() { 12 } else { 0 };
    let source_bonus = if is_manual_task(task) {
        22
    } else if is_meeting_task(task) {
        16
    } else if is_memory_task(task) {
        11
    } else {
        6
    };
    let context_penalty =
        if !task_has_supporting_context(task) && !is_manual_task(task) && !is_meeting_task(task) {
            18
        } else {
            0
        };
    let title_penalty = if is_low_signal_task_title(&task.title) {
        24
    } else {
        0
    };
    recency_bonus + memory_bonus + url_bonus + due_bonus + source_bonus
        - context_penalty
        - title_penalty
}

fn required_term_matches(terms: &[String]) -> usize {
    if terms.len() >= 4 {
        2
    } else {
        1
    }
}

fn update_task_links_from_memories(tasks: &mut [Task], recent_memories: &[SearchResult]) -> bool {
    let mut changed = false;

    for task in tasks
        .iter_mut()
        .filter(|t| !t.is_completed && !t.is_dismissed)
    {
        let terms = task_terms(&task.title);
        if terms.is_empty() {
            continue;
        }

        let mut known_memory_ids: HashSet<String> =
            task.linked_memory_ids.iter().cloned().collect();
        let mut known_urls: HashSet<String> = task.linked_urls.iter().cloned().collect();

        for memory in recent_memories.iter().take(TASK_LINK_SCAN_LIMIT) {
            let blob = format!(
                "{} {} {}",
                memory.snippet, memory.clean_text, memory.window_title
            );
            let normalized_blob = normalize_task_text(&blob);
            let matches = terms
                .iter()
                .filter(|term| normalized_blob.contains(term.as_str()))
                .count();
            let required = required_term_matches(&terms);
            if matches < required {
                continue;
            }

            if known_memory_ids.insert(memory.id.clone()) {
                task.linked_memory_ids.push(memory.id.clone());
                changed = true;
            }

            if let Some(url) = memory.url.clone() {
                if known_urls.insert(url.clone()) {
                    task.linked_urls.push(url);
                    changed = true;
                }
            }
        }

        task.linked_memory_ids.truncate(18);
        task.linked_urls.truncate(8);
    }

    changed
}

fn backfill_tasks_from_meetings(tasks: &mut Vec<Task>, meetings: &[MeetingSession]) -> bool {
    let mut changed = false;
    let now_ms = chrono::Utc::now().timestamp_millis();
    let cutoff = now_ms - (TASK_MEETING_LOOKBACK_DAYS * 24 * 60 * 60 * 1000);
    let mut dedupe = tasks
        .iter()
        .map(|task| {
            (
                normalize_task_text(&task.title),
                task_type_sort_key(&task.task_type),
            )
        })
        .collect::<HashSet<_>>();

    let mut ordered = meetings.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| {
        right
            .end_timestamp
            .unwrap_or(right.updated_at)
            .cmp(&left.end_timestamp.unwrap_or(left.updated_at))
    });

    for meeting in ordered {
        let event_ts = meeting.end_timestamp.unwrap_or(meeting.updated_at);
        if event_ts < cutoff {
            continue;
        }
        let Some(breakdown) = meeting.breakdown.as_ref() else {
            continue;
        };
        let source_app = format!("Meeting:{}", meeting.id);

        let mut add_items = |items: &[String], task_type: TaskType| {
            let type_key = task_type_sort_key(&task_type);
            for item in items {
                let title = item.trim();
                if title.is_empty() || is_low_signal_task_title(title) {
                    continue;
                }
                let dedupe_key = (normalize_task_text(title), type_key);
                if !dedupe.insert(dedupe_key) {
                    continue;
                }
                tasks.push(Task {
                    id: uuid::Uuid::new_v4().to_string(),
                    title: title.to_string(),
                    description: String::new(),
                    source_app: source_app.clone(),
                    source_memory_id: None,
                    created_at: event_ts,
                    due_date: None,
                    is_completed: false,
                    is_dismissed: false,
                    task_type: task_type.clone(),
                    linked_urls: Vec::new(),
                    linked_memory_ids: Vec::new(),
                });
                changed = true;
            }
        };

        add_items(&breakdown.todos, TaskType::Todo);
        add_items(&breakdown.reminders, TaskType::Reminder);
        add_items(&breakdown.followups, TaskType::Followup);
    }

    changed
}

#[derive(Debug, Clone)]
struct MemoryTaskCandidate {
    title: String,
    task_type: TaskType,
    score: i64,
    created_at: i64,
    source_app: String,
    source_memory_id: String,
    linked_urls: Vec<String>,
}

fn first_sentence(text: &str) -> String {
    text.split(['.', '!', '?'])
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .take(18)
        .collect::<Vec<_>>()
        .join(" ")
}

fn classify_task_type_from_text(text: &str) -> TaskType {
    let lower = text.to_lowercase();
    if [
        "follow up",
        "follow-up",
        "reply to",
        "reach out",
        "check in with",
        "ping ",
    ]
    .iter()
    .any(|cue| lower.contains(cue))
    {
        TaskType::Followup
    } else if [
        "tomorrow",
        "today",
        "tonight",
        "next week",
        "next month",
        "deadline",
        "due ",
        "monday",
        "tuesday",
        "wednesday",
        "thursday",
        "friday",
        "saturday",
        "sunday",
    ]
    .iter()
    .any(|cue| lower.contains(cue))
    {
        TaskType::Reminder
    } else {
        TaskType::Todo
    }
}

fn build_memory_task_candidate(memory: &SearchResult) -> Option<MemoryTaskCandidate> {
    if is_internal_fndr_result(memory) {
        return None;
    }

    let mut text = memory.snippet.trim().to_string();
    if text.is_empty() {
        text = memory.clean_text.trim().to_string();
    }
    if text.is_empty() {
        text = memory.window_title.trim().to_string();
    }
    if text.is_empty() {
        return None;
    }

    let sentence = first_sentence(&text);
    if sentence.is_empty() {
        return None;
    }

    let cleaned = sentence
        .trim()
        .trim_matches(|ch| ch == '"' || ch == '\'' || ch == '`')
        .trim_start_matches("TODO:")
        .trim_start_matches("To do:")
        .trim_start_matches("to do:")
        .trim()
        .to_string();
    if cleaned.is_empty() || is_low_signal_task_title(&cleaned) {
        return None;
    }

    let lower = cleaned.to_lowercase();
    let action_cues = [
        "need to",
        "should",
        "must",
        "todo",
        "to do",
        "action item",
        "send",
        "reply",
        "schedule",
        "book",
        "finish",
        "complete",
        "prepare",
        "submit",
        "review",
        "update",
        "fix",
        "call",
        "email",
        "draft",
        "plan",
        "confirm",
        "deploy",
        "ship",
        "follow up",
    ];
    let action_hits = action_cues
        .iter()
        .filter(|cue| lower.contains(*cue))
        .count() as i64;
    if action_hits == 0 {
        return None;
    }

    let reminder_hits = [
        "tomorrow",
        "today",
        "tonight",
        "next week",
        "next month",
        "deadline",
        "due ",
    ]
    .iter()
    .filter(|cue| lower.contains(*cue))
    .count() as i64;
    let followup_hits = [
        "follow up",
        "follow-up",
        "reply to",
        "reach out",
        "check in with",
    ]
    .iter()
    .filter(|cue| lower.contains(*cue))
    .count() as i64;
    let score = action_hits * 4 + reminder_hits * 3 + followup_hits * 4;
    if score < 4 {
        return None;
    }

    Some(MemoryTaskCandidate {
        title: cleaned,
        task_type: classify_task_type_from_text(&lower),
        score,
        created_at: memory.timestamp,
        source_app: format!("Memory:{}", memory.app_name),
        source_memory_id: memory.id.clone(),
        linked_urls: memory.url.clone().map(|url| vec![url]).unwrap_or_default(),
    })
}

fn backfill_tasks_from_memories(tasks: &mut Vec<Task>, recent_memories: &[SearchResult]) -> bool {
    let mut changed = false;
    let mut dedupe = tasks
        .iter()
        .map(|task| {
            (
                normalize_task_text(&task.title),
                task_type_sort_key(&task.task_type),
            )
        })
        .collect::<HashSet<_>>();

    let mut candidates = recent_memories
        .iter()
        .filter_map(build_memory_task_candidate)
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| right.created_at.cmp(&left.created_at))
    });

    for candidate in candidates.into_iter().take(TASK_MEMORY_BACKFILL_LIMIT) {
        let type_key = task_type_sort_key(&candidate.task_type);
        let dedupe_key = (normalize_task_text(&candidate.title), type_key);
        if !dedupe.insert(dedupe_key) {
            continue;
        }

        tasks.push(Task {
            id: uuid::Uuid::new_v4().to_string(),
            title: candidate.title,
            description: String::new(),
            source_app: candidate.source_app,
            source_memory_id: Some(candidate.source_memory_id.clone()),
            created_at: candidate.created_at,
            due_date: None,
            is_completed: false,
            is_dismissed: false,
            task_type: candidate.task_type.clone(),
            linked_urls: candidate.linked_urls,
            linked_memory_ids: vec![candidate.source_memory_id],
        });
        changed = true;
    }

    changed
}

fn dismiss_low_quality_auto_tasks(tasks: &mut [Task]) -> bool {
    let mut changed = false;

    for task in tasks
        .iter_mut()
        .filter(|task| !task.is_completed && !task.is_dismissed)
    {
        if is_manual_task(task) || is_meeting_task(task) {
            continue;
        }

        let stale_auto_seed = task.source_app.eq_ignore_ascii_case("auto");
        let weak_title = is_low_signal_task_title(&task.title);
        let missing_context = !task_has_supporting_context(task);
        if stale_auto_seed || weak_title || (is_memory_task(task) && missing_context) {
            task.is_dismissed = true;
            changed = true;
        }
    }

    changed
}

/// Add a new todo
#[tauri::command]
pub async fn add_todo(
    state: State<'_, Arc<AppState>>,
    title: String,
    task_type: Option<String>,
) -> Result<Task, String> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err("Task title cannot be empty.".to_string());
    }

    let parsed_task_type = parse_task_type(task_type.as_deref());

    let task = Task {
        id: uuid::Uuid::new_v4().to_string(),
        title: trimmed.to_string(),
        description: String::new(),
        source_app: "Manual".to_string(),
        source_memory_id: None,
        created_at: chrono::Utc::now().timestamp_millis(),
        due_date: None,
        is_completed: false,
        is_dismissed: false,
        task_type: parsed_task_type,
        linked_urls: Vec::new(),
        linked_memory_ids: Vec::new(),
    };

    let mut tasks = state.store.list_tasks().await.map_err(|e| e.to_string())?;
    tasks.push(task.clone());
    state
        .store
        .upsert_tasks(&tasks)
        .await
        .map_err(|e| e.to_string())?;

    Ok(task)
}

/// Get all active todos
#[tauri::command]
pub async fn get_todos(state: State<'_, Arc<AppState>>) -> Result<Vec<Task>, String> {
    let mut tasks = state.store.list_tasks().await.map_err(|e| e.to_string())?;
    let recent_memories = state
        .store
        .list_recent_results(TASK_LINK_SCAN_LIMIT, None)
        .await
        .map_err(|e| e.to_string())?;
    let meetings = state
        .store
        .list_meetings()
        .await
        .map_err(|e| e.to_string())?;

    let links_changed = update_task_links_from_memories(&mut tasks, &recent_memories);
    let memory_backfill_changed = backfill_tasks_from_memories(&mut tasks, &recent_memories);
    let meeting_backfill_changed = backfill_tasks_from_meetings(&mut tasks, &meetings);
    let cleanup_changed = dismiss_low_quality_auto_tasks(&mut tasks);
    if links_changed || memory_backfill_changed || meeting_backfill_changed || cleanup_changed {
        state
            .store
            .upsert_tasks(&tasks)
            .await
            .map_err(|e| e.to_string())?;
    }

    let mut visible = tasks
        .into_iter()
        .filter(|task| !task.is_completed && !task.is_dismissed)
        .filter(|task| !is_low_signal_task_title(&task.title))
        .filter(|task| {
            is_manual_task(task) || is_meeting_task(task) || task_has_supporting_context(task)
        })
        .collect::<Vec<_>>();
    let mut seen = HashSet::new();
    visible.retain(|task| {
        seen.insert((
            normalize_task_text(&task.title),
            task_type_sort_key(&task.task_type),
        ))
    });
    let now_ms = chrono::Utc::now().timestamp_millis();
    visible.sort_by(|left, right| {
        task_priority_score(right, now_ms)
            .cmp(&task_priority_score(left, now_ms))
            .then_with(|| right.created_at.cmp(&left.created_at))
            .then_with(|| {
                task_type_sort_key(&left.task_type).cmp(&task_type_sort_key(&right.task_type))
            })
    });
    Ok(visible)
}

/// Dismiss a task
#[tauri::command]
pub async fn dismiss_todo(
    state: State<'_, Arc<AppState>>,
    task_id: String,
) -> Result<bool, String> {
    let mut tasks = state.store.list_tasks().await.map_err(|e| e.to_string())?;
    if let Some(task) = tasks.iter_mut().find(|t| t.id == task_id) {
        task.is_dismissed = true;
        state
            .store
            .upsert_tasks(&tasks)
            .await
            .map_err(|e| e.to_string())?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Mark a task for execution
#[tauri::command]
pub async fn execute_todo(
    state: State<'_, Arc<AppState>>,
    task_id: String,
) -> Result<Task, String> {
    let tasks = state.store.list_tasks().await.map_err(|e| e.to_string())?;
    let task = tasks
        .into_iter()
        .find(|t| t.id == task_id)
        .ok_or_else(|| "Task not found".to_string())?;

    Ok(task)
}

/// Update an existing task's title and/or type
#[tauri::command]
pub async fn update_todo(
    state: State<'_, Arc<AppState>>,
    task_id: String,
    title: String,
    task_type: Option<String>,
) -> Result<Task, String> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err("Task title cannot be empty.".to_string());
    }

    let parsed_type = parse_task_type(task_type.as_deref());
    let mut tasks = state.store.list_tasks().await.map_err(|e| e.to_string())?;
    let task = tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .ok_or_else(|| "Task not found".to_string())?;

    task.title = trimmed.to_string();
    task.task_type = parsed_type;
    task.created_at = chrono::Utc::now().timestamp_millis();
    let updated = task.clone();

    state
        .store
        .upsert_tasks(&tasks)
        .await
        .map_err(|e| e.to_string())?;
    Ok(updated)
}

// ========== Agent Commands ==========

use parking_lot::Mutex as AgentMutex;
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock as AgentOnceLock;
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStatus {
    pub is_running: bool,
    pub task_title: Option<String>,
    pub last_message: Option<String>,
    pub status: String, // "idle" | "running" | "completed" | "error"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HermesAppContext {
    pub app_name: String,
    pub memory_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HermesMemoryDigest {
    pub title: String,
    pub app_name: String,
    pub summary: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HermesBridgeStatus {
    pub installed: bool,
    pub configured: bool,
    pub setup_complete: bool,
    pub gateway_running: bool,
    pub api_server_ready: bool,
    pub version: Option<String>,
    pub bundled_repo_available: bool,
    pub runtime_source: Option<String>,
    pub provider_kind: Option<String>,
    pub model_name: Option<String>,
    pub base_url: Option<String>,
    pub api_url: String,
    pub gateway_dir: String,
    pub home_dir: String,
    pub context_path: String,
    pub context_ready: bool,
    pub last_synced_at: Option<i64>,
    pub fndr_local_model_id: Option<String>,
    pub ollama_installed: bool,
    pub ollama_reachable: bool,
    pub ollama_models: Vec<String>,
    pub ollama_base_url: String,
    pub codex_cli_installed: bool,
    pub codex_logged_in: bool,
    pub codex_auth_path: String,
    pub profile_name: Option<String>,
    pub focus_task: Option<String>,
    pub recent_memory_count: u32,
    pub open_task_count: u32,
    /// True when Ollama is reachable and configured — chat works without Hermes CLI.
    pub direct_ollama_ready: bool,
    pub top_apps: Vec<HermesAppContext>,
    pub recent_memories: Vec<HermesMemoryDigest>,
    pub last_error: Option<String>,
    pub install_command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HermesSetupPayload {
    pub provider_kind: String,
    pub model_name: String,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HermesChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HermesChatReply {
    pub response_id: String,
    pub conversation_id: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRepairSummary {
    pub total_before: usize,
    pub total_after: usize,
    pub merged_count: usize,
    pub anchor_merges: usize,
    pub task_reference_updates: usize,
    pub screenshots_cleaned: usize,
    pub embeddings_refreshed: usize,
    pub chars_before: usize,
    pub chars_after: usize,
    pub chars_reclaimed: usize,
    pub spotify_merges: usize,
    pub youtube_merges: usize,
    pub codex_merges: usize,
    pub discord_merges: usize,
    pub gitlab_merges: usize,
    pub antigravity_merges: usize,
    pub app_merges: Vec<AppMergeCount>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageReclaimSummary {
    pub records_scanned: usize,
    pub records_rewritten: usize,
    pub screenshot_paths_cleared: usize,
    pub screenshot_files_deleted: usize,
    pub embeddings_refreshed: usize,
    pub snippet_embeddings_refreshed: usize,
    #[serde(default)]
    pub support_embeddings_refreshed: usize,
    pub chars_before: usize,
    pub chars_after: usize,
    pub chars_reclaimed: usize,
    pub bytes_before: u64,
    pub bytes_after: u64,
    pub bytes_reclaimed: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageHealth {
    pub memory_db_bytes: u64,
    pub frames_bytes: u64,
    pub models_bytes: u64,
    pub dev_build_cache_bytes: u64,
    pub runtime_total_bytes: u64,
    pub measured_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppMergeCount {
    pub app_name: String,
    pub merged: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRepairProgress {
    pub is_running: bool,
    pub phase: String,
    pub processed: usize,
    pub total: usize,
    pub merged_count: usize,
    pub anchor_merges: usize,
    pub timestamp_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageReclaimProgress {
    pub is_running: bool,
    pub phase: String,
    pub processed: usize,
    pub total: usize,
    pub records_rewritten: usize,
    pub screenshot_paths_cleared: usize,
    pub screenshot_files_deleted: usize,
    pub embeddings_refreshed: usize,
    pub snippet_embeddings_refreshed: usize,
    #[serde(default)]
    pub support_embeddings_refreshed: usize,
    pub timestamp_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MemoryRepairCheckpoint {
    version: u32,
    source_total: usize,
    source_fingerprint: u64,
    source_first_id: String,
    source_last_id: String,
    processed: usize,
    merged_memories: Vec<MemoryRecord>,
    id_redirect: HashMap<String, String>,
    merged_count: usize,
    anchor_merges: usize,
    spotify_merges: usize,
    youtube_merges: usize,
    codex_merges: usize,
    discord_merges: usize,
    gitlab_merges: usize,
    antigravity_merges: usize,
    app_merge_counts: HashMap<String, usize>,
}

fn memory_repair_source_fingerprint(memories: &[MemoryRecord]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for memory in memories {
        for byte in memory.id.as_bytes() {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        for byte in memory.timestamp.to_le_bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    hash
}

fn memory_repair_progress_path(state: &AppState) -> PathBuf {
    state.store.data_dir().join("memory_repair_progress.json")
}

fn memory_repair_checkpoint_path(state: &AppState) -> PathBuf {
    state.store.data_dir().join("memory_repair_checkpoint.json")
}

fn storage_reclaim_progress_path(state: &AppState) -> PathBuf {
    state.store.data_dir().join("storage_reclaim_progress.json")
}

fn persist_memory_repair_progress(path: &PathBuf, progress: &MemoryRepairProgress) {
    if let Ok(serialized) = serde_json::to_string_pretty(progress) {
        let _ = std::fs::write(path, serialized);
    }
}

fn persist_storage_reclaim_progress(path: &PathBuf, progress: &StorageReclaimProgress) {
    if let Ok(serialized) = serde_json::to_string_pretty(progress) {
        let _ = std::fs::write(path, serialized);
    }
}

fn persist_memory_repair_checkpoint(path: &PathBuf, checkpoint: &MemoryRepairCheckpoint) {
    let tmp = path.with_extension("json.tmp");
    if let Ok(serialized) = serde_json::to_string(checkpoint) {
        if std::fs::write(&tmp, serialized).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

fn load_memory_repair_checkpoint(path: &PathBuf) -> Option<MemoryRepairCheckpoint> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<MemoryRepairCheckpoint>(&content).ok()
}

fn recursive_size(path: &Path) -> u64 {
    if !path.exists() {
        return 0;
    }

    if path.is_file() {
        return std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    }

    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let child = entry.path();
            if child.is_dir() {
                stack.push(child);
            } else if let Ok(meta) = entry.metadata() {
                total = total.saturating_add(meta.len());
            }
        }
    }
    total
}

fn recursive_file_count(path: &Path) -> usize {
    if !path.exists() {
        return 0;
    }

    if path.is_file() {
        return 1;
    }

    let mut total = 0usize;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let child = entry.path();
            if child.is_dir() {
                stack.push(child);
            } else {
                total = total.saturating_add(1);
            }
        }
    }
    total
}

fn memory_payload_bytes(state: &AppState) -> u64 {
    recursive_size(
        &state
            .store
            .data_dir()
            .join("lancedb")
            .join("memories.lance"),
    )
    .saturating_add(recursive_size(&state.store.frames_dir()))
}

fn dev_build_cache_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target")
}

fn storage_health_for_state(state: &AppState) -> StorageHealth {
    let data_dir = state.store.data_dir();
    let memory_db_bytes = recursive_size(&data_dir.join("lancedb"));
    let frames_bytes = recursive_size(&state.store.frames_dir());
    let models_bytes = recursive_size(&data_dir.join("models"))
        .saturating_add(recursive_size(&data_dir.join("speech_models")));
    let runtime_total_bytes = recursive_size(&data_dir);
    let dev_build_cache_bytes = recursive_size(&dev_build_cache_dir());

    StorageHealth {
        memory_db_bytes,
        frames_bytes,
        models_bytes,
        dev_build_cache_bytes,
        runtime_total_bytes,
        measured_at_ms: chrono::Utc::now().timestamp_millis(),
    }
}

#[tauri::command]
pub async fn get_storage_health(state: State<'_, Arc<AppState>>) -> Result<StorageHealth, String> {
    Ok(storage_health_for_state(state.inner()))
}

#[tauri::command]
pub async fn clean_dev_build_cache(
    state: State<'_, Arc<AppState>>,
) -> Result<StorageHealth, String> {
    let target_dir = dev_build_cache_dir();
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if !target_dir.starts_with(&manifest_dir) || target_dir == manifest_dir {
        return Err("Refusing to clean an unexpected dev cache path.".to_string());
    }

    let before = recursive_size(&target_dir);
    let target_for_task = target_dir.clone();
    tokio::task::spawn_blocking(move || {
        if target_for_task.exists() {
            std::fs::remove_dir_all(&target_for_task)
        } else {
            Ok(())
        }
    })
    .await
    .map_err(|err| format!("Dev cache cleanup task failed: {err}"))?
    .map_err(|err| format!("Failed to remove dev build cache: {err}"))?;

    tracing::info!(
        bytes_before = before,
        path = %target_dir.display(),
        "dev_build_cache:cleaned"
    );

    Ok(storage_health_for_state(state.inner()))
}

#[tauri::command]
pub async fn get_memory_repair_progress(
    state: State<'_, Arc<AppState>>,
) -> Result<MemoryRepairProgress, String> {
    let path = memory_repair_progress_path(state.inner());
    if !path.exists() {
        return Ok(MemoryRepairProgress {
            is_running: false,
            phase: "idle".to_string(),
            processed: 0,
            total: 0,
            merged_count: 0,
            anchor_merges: 0,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
        });
    }

    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut progress: MemoryRepairProgress =
        serde_json::from_str(&content).map_err(|e| e.to_string())?;

    // If heartbeat is stale for over 2 minutes, mark as not running.
    if progress.is_running {
        let now_ms = chrono::Utc::now().timestamp_millis();
        if now_ms.saturating_sub(progress.timestamp_ms) > 120_000 {
            progress.is_running = false;
            progress.phase = "stale".to_string();
            progress.timestamp_ms = now_ms;
            persist_memory_repair_progress(&path, &progress);
        }
    }

    Ok(progress)
}

#[tauri::command]
pub async fn get_storage_reclaim_progress(
    state: State<'_, Arc<AppState>>,
) -> Result<StorageReclaimProgress, String> {
    let path = storage_reclaim_progress_path(state.inner());
    if !path.exists() {
        return Ok(StorageReclaimProgress {
            is_running: false,
            phase: "idle".to_string(),
            processed: 0,
            total: 0,
            records_rewritten: 0,
            screenshot_paths_cleared: 0,
            screenshot_files_deleted: 0,
            embeddings_refreshed: 0,
            snippet_embeddings_refreshed: 0,
            support_embeddings_refreshed: 0,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
        });
    }

    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut progress: StorageReclaimProgress =
        serde_json::from_str(&content).map_err(|e| e.to_string())?;

    if progress.is_running {
        let now_ms = chrono::Utc::now().timestamp_millis();
        if now_ms.saturating_sub(progress.timestamp_ms) > 120_000 {
            progress.is_running = false;
            progress.phase = "stale".to_string();
            progress.timestamp_ms = now_ms;
            persist_storage_reclaim_progress(&path, &progress);
        }
    }

    Ok(progress)
}

#[tauri::command]
pub async fn reclaim_memory_storage(
    state: State<'_, Arc<AppState>>,
) -> Result<StorageReclaimSummary, String> {
    reclaim_memory_storage_for_state(state.inner().clone(), true).await
}

pub async fn reclaim_memory_storage_silent(
    state: Arc<AppState>,
) -> Result<StorageReclaimSummary, String> {
    reclaim_memory_storage_for_state(state, false).await
}

async fn reclaim_memory_storage_for_state(
    state: Arc<AppState>,
    publish_progress: bool,
) -> Result<StorageReclaimSummary, String> {
    shared_real_embedder()?;
    if STORAGE_RECLAIM_RUNNING.swap(true, Ordering::AcqRel) {
        return Err("Storage reclaim is already running".to_string());
    }
    struct StorageReclaimRunGuard;
    impl Drop for StorageReclaimRunGuard {
        fn drop(&mut self) {
            STORAGE_RECLAIM_RUNNING.store(false, Ordering::Release);
        }
    }
    let _run_guard = StorageReclaimRunGuard;

    let progress_path = storage_reclaim_progress_path(state.as_ref());
    let mut progress = StorageReclaimProgress {
        is_running: true,
        phase: "starting".to_string(),
        processed: 0,
        total: 0,
        records_rewritten: 0,
        screenshot_paths_cleared: 0,
        screenshot_files_deleted: 0,
        embeddings_refreshed: 0,
        snippet_embeddings_refreshed: 0,
        support_embeddings_refreshed: 0,
        timestamp_ms: chrono::Utc::now().timestamp_millis(),
    };
    if publish_progress {
        persist_storage_reclaim_progress(&progress_path, &progress);
    }

    progress.phase = "repairing_prerequisite".to_string();
    progress.timestamp_ms = chrono::Utc::now().timestamp_millis();
    if publish_progress {
        persist_storage_reclaim_progress(&progress_path, &progress);
    }
    if let Err(err) = run_memory_repair_backfill_for_state(state.clone()).await {
        progress.is_running = false;
        progress.phase = "error".to_string();
        progress.timestamp_ms = chrono::Utc::now().timestamp_millis();
        if publish_progress {
            persist_storage_reclaim_progress(&progress_path, &progress);
        }
        return Err(err);
    }

    let should_resume_capture = !state.is_paused.load(Ordering::SeqCst);
    if should_resume_capture {
        state.pause();
    }
    struct CaptureResumeGuard {
        state: Arc<AppState>,
        should_resume: bool,
    }
    impl Drop for CaptureResumeGuard {
        fn drop(&mut self) {
            if self.should_resume {
                self.state.resume();
            }
        }
    }
    let _capture_resume_guard = CaptureResumeGuard {
        state: state.clone(),
        should_resume: should_resume_capture,
    };

    tracing::info!("storage_reclaim:start");
    progress.phase = "loading".to_string();
    progress.timestamp_ms = chrono::Utc::now().timestamp_millis();
    if publish_progress {
        persist_storage_reclaim_progress(&progress_path, &progress);
    }

    let reclaim_started = Instant::now();
    let bytes_before = memory_payload_bytes(state.as_ref());
    let embedder = shared_real_embedder()?;
    let memories = match state.store.list_all_memories().await {
        Ok(memories) => memories,
        Err(err) => {
            progress.is_running = false;
            progress.phase = "error".to_string();
            progress.timestamp_ms = chrono::Utc::now().timestamp_millis();
            if publish_progress {
                persist_storage_reclaim_progress(&progress_path, &progress);
            }
            return Err(err.to_string());
        }
    };

    let mut rewritten_memories = Vec::with_capacity(memories.len());
    let mut changed_flags = Vec::with_capacity(memories.len());
    let mut records_rewritten = 0usize;
    let mut screenshot_paths_cleared = 0usize;
    let mut screenshot_files_deleted = 0usize;
    let mut embeddings_refreshed = 0usize;
    let mut snippet_embeddings_refreshed = 0usize;
    let mut support_embeddings_refreshed = 0usize;
    let mut chars_before = 0usize;
    let mut chars_after = 0usize;
    let mut text_embedding_jobs: Vec<(usize, (String, String, String))> = Vec::new();
    let mut snippet_embedding_jobs: Vec<(usize, (String, String, String))> = Vec::new();
    let mut support_embedding_jobs: Vec<(usize, String, String, Vec<String>)> = Vec::new();

    let frames_dir = state.store.frames_dir();
    let mut external_screenshot_paths: HashSet<PathBuf> = HashSet::new();

    progress.phase = "compacting".to_string();
    progress.total = memories.len();
    progress.processed = 0;
    progress.timestamp_ms = chrono::Utc::now().timestamp_millis();
    if publish_progress {
        persist_storage_reclaim_progress(&progress_path, &progress);
    }
    let mut last_heartbeat = Instant::now();
    let heartbeat_interval = Duration::from_millis(STORAGE_RECLAIM_HEARTBEAT_MS);

    for memory in memories {
        chars_before = chars_before
            .saturating_add(memory.text.chars().count() + memory.clean_text.chars().count());

        let compacted = compact_memory_record_payload(&memory);
        let mut changed = compacted.text != memory.text
            || compacted.clean_text != memory.clean_text
            || compacted.screenshot_path != memory.screenshot_path;

        if let Some(path) = memory.screenshot_path.as_deref() {
            screenshot_paths_cleared = screenshot_paths_cleared.saturating_add(1);
            let screenshot_path = PathBuf::from(path);
            if !screenshot_path.starts_with(&frames_dir) {
                external_screenshot_paths.insert(screenshot_path);
            }
            changed = true;
        }

        if is_low_signal_embedding(&compacted.embedding) {
            let embedding_text = best_embedding_text(&memory);
            if !embedding_text.is_empty() {
                text_embedding_jobs.push((
                    rewritten_memories.len(),
                    (
                        memory.app_name.clone(),
                        memory.window_title.clone(),
                        embedding_text,
                    ),
                ));
            }
        }

        let snippet_input = best_snippet_embedding_text(&compacted);
        if !snippet_input.is_empty() {
            snippet_embedding_jobs.push((
                rewritten_memories.len(),
                (
                    compacted.app_name.clone(),
                    compacted.window_title.clone(),
                    snippet_input,
                ),
            ));
        }

        if is_low_signal_embedding(&compacted.support_embedding) {
            let support_inputs = best_support_embedding_texts(&compacted);
            if !support_inputs.is_empty() {
                support_embedding_jobs.push((
                    rewritten_memories.len(),
                    compacted.app_name.clone(),
                    compacted.window_title.clone(),
                    support_inputs,
                ));
            }
        }

        chars_after = chars_after
            .saturating_add(compacted.text.chars().count() + compacted.clean_text.chars().count());
        rewritten_memories.push(compacted);

        changed_flags.push(changed);
        if changed {
            records_rewritten = records_rewritten.saturating_add(1);
        }

        progress.processed = rewritten_memories.len();
        progress.records_rewritten = records_rewritten;
        progress.screenshot_paths_cleared = screenshot_paths_cleared;
        if progress.processed % STORAGE_RECLAIM_HEARTBEAT_ITEM_STEP == 0
            || last_heartbeat.elapsed() >= heartbeat_interval
        {
            progress.timestamp_ms = chrono::Utc::now().timestamp_millis();
            if publish_progress {
                persist_storage_reclaim_progress(&progress_path, &progress);
            }
            last_heartbeat = Instant::now();
        }
    }

    tracing::info!(
        "storage_reclaim:compacted records={} rewritten={} text_jobs={} snippet_jobs={} support_jobs={} elapsed_ms={}",
        rewritten_memories.len(),
        records_rewritten,
        text_embedding_jobs.len(),
        snippet_embedding_jobs.len(),
        support_embedding_jobs.len(),
        reclaim_started.elapsed().as_millis()
    );

    let total_embedding_jobs =
        text_embedding_jobs.len() + snippet_embedding_jobs.len() + support_embedding_jobs.len();
    if total_embedding_jobs > 0 {
        progress.phase = "embedding".to_string();
        progress.total = total_embedding_jobs;
        progress.processed = 0;
        progress.timestamp_ms = chrono::Utc::now().timestamp_millis();
        if publish_progress {
            persist_storage_reclaim_progress(&progress_path, &progress);
        }

        let mut embedded_jobs = 0usize;

        for chunk in text_embedding_jobs.chunks(STORAGE_RECLAIM_EMBED_BATCH) {
            let contexts = chunk
                .iter()
                .map(|(_, context)| context.clone())
                .collect::<Vec<_>>();
            match embedder.embed_batch_with_context(&contexts) {
                Ok(vectors) => {
                    let vector_count = vectors.len();
                    for ((record_index, _), vector) in chunk.iter().zip(vectors.into_iter()) {
                        rewritten_memories[*record_index].embedding = vector;
                        if !changed_flags[*record_index] {
                            changed_flags[*record_index] = true;
                            records_rewritten = records_rewritten.saturating_add(1);
                        }
                        embeddings_refreshed = embeddings_refreshed.saturating_add(1);
                    }
                    if vector_count != chunk.len() {
                        tracing::warn!(
                            "storage_reclaim:text embedding chunk mismatch vectors={} chunk={}",
                            vector_count,
                            chunk.len()
                        );
                    }
                }
                Err(err) => {
                    tracing::warn!(
                        "storage_reclaim:text embedding chunk failed ({} items): {}",
                        chunk.len(),
                        err
                    );
                }
            }

            embedded_jobs = embedded_jobs.saturating_add(chunk.len());
            progress.processed = embedded_jobs;
            progress.records_rewritten = records_rewritten;
            progress.embeddings_refreshed = embeddings_refreshed;
            progress.timestamp_ms = chrono::Utc::now().timestamp_millis();
            if publish_progress {
                persist_storage_reclaim_progress(&progress_path, &progress);
            }
        }

        for chunk in snippet_embedding_jobs.chunks(STORAGE_RECLAIM_EMBED_BATCH) {
            let contexts = chunk
                .iter()
                .map(|(_, context)| context.clone())
                .collect::<Vec<_>>();
            match embedder.embed_batch_with_context(&contexts) {
                Ok(vectors) => {
                    let vector_count = vectors.len();
                    for ((record_index, _), vector) in chunk.iter().zip(vectors.into_iter()) {
                        rewritten_memories[*record_index].snippet_embedding = vector;
                        if !changed_flags[*record_index] {
                            changed_flags[*record_index] = true;
                            records_rewritten = records_rewritten.saturating_add(1);
                        }
                        snippet_embeddings_refreshed =
                            snippet_embeddings_refreshed.saturating_add(1);
                    }
                    if vector_count != chunk.len() {
                        tracing::warn!(
                            "storage_reclaim:snippet embedding chunk mismatch vectors={} chunk={}",
                            vector_count,
                            chunk.len()
                        );
                    }
                }
                Err(err) => {
                    tracing::warn!(
                        "storage_reclaim:snippet embedding chunk failed ({} items): {}",
                        chunk.len(),
                        err
                    );
                }
            }

            embedded_jobs = embedded_jobs.saturating_add(chunk.len());
            progress.processed = embedded_jobs;
            progress.records_rewritten = records_rewritten;
            progress.snippet_embeddings_refreshed = snippet_embeddings_refreshed;
            progress.timestamp_ms = chrono::Utc::now().timestamp_millis();
            if publish_progress {
                persist_storage_reclaim_progress(&progress_path, &progress);
            }
        }

        for (record_index, app_name, window_title, support_inputs) in support_embedding_jobs {
            let contexts = support_inputs
                .into_iter()
                .map(|text| (app_name.clone(), window_title.clone(), text))
                .collect::<Vec<_>>();
            match embedder.embed_batch_with_context(&contexts) {
                Ok(vectors) => {
                    rewritten_memories[record_index].support_embedding =
                        mean_pool_embeddings(&vectors);
                    if !changed_flags[record_index] {
                        changed_flags[record_index] = true;
                        records_rewritten = records_rewritten.saturating_add(1);
                    }
                    support_embeddings_refreshed = support_embeddings_refreshed.saturating_add(1);
                }
                Err(err) => {
                    tracing::warn!(
                        "storage_reclaim:support embedding failed for {} inputs: {}",
                        contexts.len(),
                        err
                    );
                }
            }

            embedded_jobs = embedded_jobs.saturating_add(1);
            progress.processed = embedded_jobs;
            progress.records_rewritten = records_rewritten;
            progress.support_embeddings_refreshed = support_embeddings_refreshed;
            progress.timestamp_ms = chrono::Utc::now().timestamp_millis();
            if publish_progress {
                persist_storage_reclaim_progress(&progress_path, &progress);
            }
        }
    }

    if !external_screenshot_paths.is_empty() {
        progress.phase = "purging_external_files".to_string();
        progress.total = external_screenshot_paths.len();
        progress.processed = 0;
        progress.timestamp_ms = chrono::Utc::now().timestamp_millis();
        if publish_progress {
            persist_storage_reclaim_progress(&progress_path, &progress);
        }

        for path in external_screenshot_paths {
            if std::fs::remove_file(path).is_ok() {
                screenshot_files_deleted = screenshot_files_deleted.saturating_add(1);
            }
            progress.processed = progress.processed.saturating_add(1);
            progress.screenshot_files_deleted = screenshot_files_deleted;
            progress.timestamp_ms = chrono::Utc::now().timestamp_millis();
            if publish_progress {
                persist_storage_reclaim_progress(&progress_path, &progress);
            }
        }
    }

    progress.phase = "writing".to_string();
    progress.total = rewritten_memories.len();
    progress.processed = 0;
    progress.records_rewritten = records_rewritten;
    progress.embeddings_refreshed = embeddings_refreshed;
    progress.snippet_embeddings_refreshed = snippet_embeddings_refreshed;
    progress.support_embeddings_refreshed = support_embeddings_refreshed;
    progress.timestamp_ms = chrono::Utc::now().timestamp_millis();
    if publish_progress {
        persist_storage_reclaim_progress(&progress_path, &progress);
    }

    let write_result = if records_rewritten > 0 {
        state
            .store
            .replace_all_memories_preserving_ids(&rewritten_memories)
            .await
    } else {
        Ok(())
    };
    if let Err(err) = write_result {
        progress.is_running = false;
        progress.phase = "error".to_string();
        progress.timestamp_ms = chrono::Utc::now().timestamp_millis();
        if publish_progress {
            persist_storage_reclaim_progress(&progress_path, &progress);
        }
        return Err(err.to_string());
    }

    progress.processed = rewritten_memories.len();
    progress.timestamp_ms = chrono::Utc::now().timestamp_millis();
    if publish_progress {
        persist_storage_reclaim_progress(&progress_path, &progress);
    }

    progress.phase = "purging_frames".to_string();
    progress.total = 1;
    progress.processed = 0;
    progress.timestamp_ms = chrono::Utc::now().timestamp_millis();
    if publish_progress {
        persist_storage_reclaim_progress(&progress_path, &progress);
    }

    let frame_files_before = recursive_file_count(&frames_dir);
    if frames_dir.exists() && std::fs::remove_dir_all(&frames_dir).is_ok() {
        screenshot_files_deleted = screenshot_files_deleted.saturating_add(frame_files_before);
    }
    let _ = std::fs::create_dir_all(&frames_dir);

    progress.processed = 1;
    progress.screenshot_files_deleted = screenshot_files_deleted;
    progress.timestamp_ms = chrono::Utc::now().timestamp_millis();
    if publish_progress {
        persist_storage_reclaim_progress(&progress_path, &progress);
    }

    let bytes_after = memory_payload_bytes(state.as_ref());

    tracing::info!(
        "storage_reclaim:complete scanned={} rewritten={} screenshots_deleted={} bytes_reclaimed={} elapsed_ms={}",
        rewritten_memories.len(),
        records_rewritten,
        screenshot_files_deleted,
        bytes_before.saturating_sub(bytes_after),
        reclaim_started.elapsed().as_millis()
    );

    progress.is_running = false;
    progress.phase = "complete".to_string();
    progress.total = rewritten_memories.len();
    progress.processed = rewritten_memories.len();
    progress.records_rewritten = records_rewritten;
    progress.screenshot_paths_cleared = screenshot_paths_cleared;
    progress.screenshot_files_deleted = screenshot_files_deleted;
    progress.embeddings_refreshed = embeddings_refreshed;
    progress.snippet_embeddings_refreshed = snippet_embeddings_refreshed;
    progress.support_embeddings_refreshed = support_embeddings_refreshed;
    progress.timestamp_ms = chrono::Utc::now().timestamp_millis();
    if publish_progress {
        persist_storage_reclaim_progress(&progress_path, &progress);
    }

    Ok(StorageReclaimSummary {
        records_scanned: rewritten_memories.len(),
        records_rewritten,
        screenshot_paths_cleared,
        screenshot_files_deleted,
        embeddings_refreshed,
        snippet_embeddings_refreshed,
        support_embeddings_refreshed,
        chars_before,
        chars_after,
        chars_reclaimed: chars_before.saturating_sub(chars_after),
        bytes_before,
        bytes_after,
        bytes_reclaimed: bytes_before.saturating_sub(bytes_after),
    })
}

static AGENT_PROCESS: AgentOnceLock<AgentMutex<Option<Child>>> = AgentOnceLock::new();
static AGENT_STATUS: AgentOnceLock<AgentMutex<AgentStatus>> = AgentOnceLock::new();

fn get_agent_process() -> &'static AgentMutex<Option<Child>> {
    AGENT_PROCESS.get_or_init(|| AgentMutex::new(None))
}

fn get_agent_status_store() -> &'static AgentMutex<AgentStatus> {
    AGENT_STATUS.get_or_init(|| {
        AgentMutex::new(AgentStatus {
            is_running: false,
            task_title: None,
            last_message: None,
            status: "idle".to_string(),
        })
    })
}

#[derive(Debug, Deserialize)]
struct HermesOnboardingProfile {
    display_name: Option<String>,
    model_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HermesSetupRecord {
    provider_kind: String,
    model_name: String,
    #[serde(default)]
    base_url: Option<String>,
}

static HERMES_GATEWAY_PROCESS: AgentOnceLock<AgentMutex<Option<Child>>> = AgentOnceLock::new();
static HERMES_GATEWAY_ERROR: AgentOnceLock<AgentMutex<Option<String>>> = AgentOnceLock::new();
const HERMES_API_HOST: &str = "127.0.0.1";
const HERMES_API_PORT: u16 = 8742;
const OLLAMA_BASE_URL: &str = "http://127.0.0.1:11434/v1";

fn hermes_gateway_dir(state: &AppState) -> PathBuf {
    state.app_data_dir.join("hermes-gateway")
}

fn hermes_home_dir(state: &AppState) -> PathBuf {
    state.app_data_dir.join("hermes-home")
}

fn hermes_context_path(state: &AppState) -> PathBuf {
    hermes_gateway_dir(state).join("FNDR_CONTEXT.md")
}

fn hermes_project_context_path(state: &AppState) -> PathBuf {
    hermes_gateway_dir(state).join(".hermes.md")
}

fn hermes_gateway_readme_path(state: &AppState) -> PathBuf {
    hermes_gateway_dir(state).join("README.md")
}

fn hermes_env_path(state: &AppState) -> PathBuf {
    hermes_home_dir(state).join(".env")
}

fn hermes_config_path(state: &AppState) -> PathBuf {
    hermes_home_dir(state).join("config.yaml")
}

fn hermes_setup_record_path(state: &AppState) -> PathBuf {
    hermes_home_dir(state).join("fndr_setup.json")
}

fn hermes_soul_path(state: &AppState) -> PathBuf {
    hermes_home_dir(state).join("SOUL.md")
}

fn hermes_api_url() -> String {
    format!("http://{HERMES_API_HOST}:{HERMES_API_PORT}")
}

fn get_hermes_gateway_process() -> &'static AgentMutex<Option<Child>> {
    HERMES_GATEWAY_PROCESS.get_or_init(|| AgentMutex::new(None))
}

fn get_hermes_gateway_error_store() -> &'static AgentMutex<Option<String>> {
    HERMES_GATEWAY_ERROR.get_or_init(|| AgentMutex::new(None))
}

fn read_hermes_profile_name(state: &AppState) -> Option<String> {
    let path = state.app_data_dir.join("onboarding.json");
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<HermesOnboardingProfile>(&raw)
        .ok()?
        .display_name
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
}

fn read_fndr_local_model_id(state: &AppState) -> Option<String> {
    let path = state.app_data_dir.join("onboarding.json");
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<HermesOnboardingProfile>(&raw)
        .ok()?
        .model_id
        .map(|model_id| model_id.trim().to_string())
        .filter(|model_id| !model_id.is_empty())
}

#[derive(Debug, Clone)]
enum HermesLauncher {
    Bundled { python: PathBuf, script: PathBuf },
    System { executable: PathBuf },
}

impl HermesLauncher {
    fn command(&self) -> Command {
        match self {
            Self::Bundled { python, script } => {
                let mut command = Command::new(python);
                command.arg(script);
                command
            }
            Self::System { executable } => Command::new(executable),
        }
    }
}

#[derive(Debug, Clone)]
struct HermesRuntimeStatus {
    installed: bool,
    version: Option<String>,
    launcher: Option<HermesLauncher>,
    bundled_repo_path: Option<PathBuf>,
    runtime_source: Option<String>,
}

impl HermesRuntimeStatus {
    fn bundled_repo_available(&self) -> bool {
        self.bundled_repo_path.is_some()
    }
}

fn user_home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn hermes_runtime_root(state: &AppState) -> PathBuf {
    state.app_data_dir.join("hermes-runtime")
}

fn hermes_runtime_bin_dir(state: &AppState) -> PathBuf {
    hermes_runtime_root(state).join("bin")
}

fn hermes_runtime_python_dir(state: &AppState) -> PathBuf {
    hermes_runtime_root(state).join("python")
}

fn hermes_runtime_venv_dir(state: &AppState) -> PathBuf {
    hermes_runtime_root(state).join("venv")
}

fn hermes_runtime_python_path(state: &AppState) -> PathBuf {
    hermes_runtime_venv_dir(state).join("bin").join("python3")
}

fn hermes_uv_path(state: &AppState) -> PathBuf {
    hermes_runtime_bin_dir(state).join("uv")
}

fn is_hermes_repo(path: &Path) -> bool {
    path.join("pyproject.toml").exists() && path.join("hermes").exists()
}

fn resolve_bundled_hermes_repo() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Some(value) = std::env::var_os("FNDR_HERMES_REPO") {
        candidates.push(PathBuf::from(value));
    }

    let manifest_candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("hermes-agent");
    candidates.push(manifest_candidate);

    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("hermes-agent"));
    }

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            candidates.push(exe_dir.join("../Resources/hermes-agent"));
            for ancestor in exe_dir.ancestors().take(6) {
                candidates.push(ancestor.join("hermes-agent"));
            }
        }
    }

    candidates
        .into_iter()
        .find(|candidate| is_hermes_repo(candidate))
}

fn read_hermes_repo_version(repo_root: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(repo_root.join("pyproject.toml")).ok()?;
    let value = raw.parse::<toml::Value>().ok()?;
    value
        .get("project")
        .and_then(|project| project.get("version"))
        .and_then(|version| version.as_str())
        .map(str::to_string)
}

fn existing_executable_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

fn find_existing_executable(candidates: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    candidates.into_iter().find(|candidate| candidate.is_file())
}

fn common_executable_candidates(name: &str) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(home) = user_home_dir() {
        candidates.push(home.join(".local/bin").join(name));
        candidates.push(home.join(".cargo/bin").join(name));
        candidates.push(home.join(".npm-global/bin").join(name));
    }

    candidates.push(PathBuf::from("/opt/homebrew/bin").join(name));
    candidates.push(PathBuf::from("/usr/local/bin").join(name));
    candidates
}

fn detect_system_hermes_executable() -> Option<PathBuf> {
    existing_executable_path("hermes")
        .or_else(|| find_existing_executable(common_executable_candidates("hermes")))
}

fn detect_uv_executable(state: &AppState) -> Option<PathBuf> {
    let bundled_uv = hermes_uv_path(state);
    if bundled_uv.exists() {
        return Some(bundled_uv);
    }

    existing_executable_path("uv")
        .or_else(|| find_existing_executable(common_executable_candidates("uv")))
}

fn detect_ollama_executable() -> Option<PathBuf> {
    let mut candidates = common_executable_candidates("ollama");
    candidates.push(PathBuf::from(
        "/Applications/Ollama.app/Contents/Resources/ollama",
    ));
    existing_executable_path("ollama").or_else(|| find_existing_executable(candidates))
}

fn detect_codex_executable() -> Option<PathBuf> {
    existing_executable_path("codex")
        .or_else(|| find_existing_executable(common_executable_candidates("codex")))
}

fn version_from_output(output: &std::process::Output) -> Option<String> {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stdout.is_empty() {
        stderr.lines().next().map(str::to_string)
    } else {
        stdout.lines().next().map(str::to_string)
    }
}

fn command_failure_detail(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        "No diagnostic output was returned.".to_string()
    }
}

fn configure_uv_command(command: &mut Command, state: &AppState) -> Result<(), String> {
    let runtime_root = hermes_runtime_root(state);
    let xdg_cache_home = runtime_root.join("xdg-cache");
    let xdg_data_home = runtime_root.join("xdg-data");
    let xdg_config_home = runtime_root.join("xdg-config");
    let tool_dir = runtime_root.join("tools");
    let tool_bin_dir = hermes_runtime_bin_dir(state);
    let python_dir = hermes_runtime_python_dir(state);
    let project_env = hermes_runtime_venv_dir(state);

    for dir in [
        &runtime_root,
        &xdg_cache_home,
        &xdg_data_home,
        &xdg_config_home,
        &tool_dir,
        &tool_bin_dir,
        &python_dir,
    ] {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }

    command
        .env("XDG_CACHE_HOME", xdg_cache_home)
        .env("XDG_DATA_HOME", xdg_data_home)
        .env("XDG_CONFIG_HOME", xdg_config_home)
        .env("UV_TOOL_DIR", tool_dir)
        .env("UV_TOOL_BIN_DIR", tool_bin_dir)
        .env("UV_PYTHON_INSTALL_DIR", python_dir)
        .env("UV_PROJECT_ENVIRONMENT", project_env)
        .env("UV_NO_PROGRESS", "1");

    Ok(())
}

fn ensure_uv_available(state: &AppState) -> Result<PathBuf, String> {
    if let Some(path) = detect_uv_executable(state) {
        return Ok(path);
    }

    let install_dir = hermes_runtime_bin_dir(state);
    std::fs::create_dir_all(&install_dir).map_err(|e| e.to_string())?;

    let output = Command::new("sh")
        .arg("-lc")
        .arg("curl -LsSf https://astral.sh/uv/install.sh | sh")
        .env("UV_UNMANAGED_INSTALL", &install_dir)
        .env("UV_NO_MODIFY_PATH", "1")
        .output()
        .map_err(|e| format!("Failed to install uv for the bundled Hermes runtime: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "FNDR could not prepare its private Hermes runtime because uv failed to install. {}",
            command_failure_detail(&output)
        ));
    }

    detect_uv_executable(state).ok_or_else(|| {
        "uv installed successfully, but FNDR could not locate the resulting binary.".to_string()
    })
}

fn prepare_vendored_hermes_runtime(state: &AppState) -> Result<(), String> {
    let repo_root = resolve_bundled_hermes_repo().ok_or_else(|| {
        "FNDR could not find the vendored hermes-agent clone. Expected a bundled `hermes-agent/` directory."
            .to_string()
    })?;
    let uv = ensure_uv_available(state)?;
    let venv_dir = hermes_runtime_venv_dir(state);

    let mut venv_command = Command::new(&uv);
    venv_command
        .arg("venv")
        .arg(&venv_dir)
        .arg("--python")
        .arg("3.11");
    configure_uv_command(&mut venv_command, state)?;
    let venv_output = venv_command
        .current_dir(&repo_root)
        .output()
        .map_err(|e| format!("Failed to create the FNDR Hermes environment: {e}"))?;
    if !venv_output.status.success() {
        return Err(format!(
            "FNDR could not create the bundled Hermes environment. {}",
            command_failure_detail(&venv_output)
        ));
    }

    let mut sync_command = Command::new(&uv);
    sync_command.arg("sync").arg("--locked");
    for extra in ["messaging", "pty", "honcho", "mcp", "acp"] {
        sync_command.arg("--extra").arg(extra);
    }
    configure_uv_command(&mut sync_command, state)?;
    let sync_output = sync_command
        .current_dir(&repo_root)
        .output()
        .map_err(|e| format!("Failed to install Hermes dependencies for FNDR: {e}"))?;
    if !sync_output.status.success() {
        return Err(format!(
            "FNDR could not finish installing Hermes dependencies. {}",
            command_failure_detail(&sync_output)
        ));
    }

    if !hermes_runtime_python_path(state).exists() {
        return Err(
            "FNDR prepared the Hermes runtime, but the private Python interpreter is missing."
                .to_string(),
        );
    }

    Ok(())
}

fn detect_hermes_runtime(state: &AppState) -> HermesRuntimeStatus {
    let bundled_repo_path = resolve_bundled_hermes_repo();
    let bundled_version = bundled_repo_path
        .as_deref()
        .and_then(read_hermes_repo_version);

    if let Some(repo_root) = bundled_repo_path.clone() {
        let python = hermes_runtime_python_path(state);
        let script = repo_root.join("hermes");
        if python.exists() && script.exists() {
            return HermesRuntimeStatus {
                installed: true,
                version: bundled_version.clone(),
                launcher: Some(HermesLauncher::Bundled { python, script }),
                bundled_repo_path: Some(repo_root),
                runtime_source: Some("bundled".to_string()),
            };
        }
    }

    if let Some(executable) = detect_system_hermes_executable() {
        let mut command = Command::new(&executable);
        let output = command.arg("--version").output();
        if let Ok(output) = output {
            if output.status.success() {
                return HermesRuntimeStatus {
                    installed: true,
                    version: version_from_output(&output).or(bundled_version.clone()),
                    launcher: Some(HermesLauncher::System { executable }),
                    bundled_repo_path,
                    runtime_source: Some("system".to_string()),
                };
            }
        }
    }

    HermesRuntimeStatus {
        installed: false,
        version: bundled_version,
        launcher: None,
        bundled_repo_path,
        runtime_source: None,
    }
}

fn detect_ollama_installation() -> bool {
    detect_ollama_executable()
        .and_then(|executable| {
            Command::new(executable)
                .arg("--version")
                .output()
                .ok()
                .filter(|output| output.status.success())
        })
        .is_some()
}

fn parse_ollama_list_output(output: &str) -> Vec<String> {
    let mut models = output
        .lines()
        .skip(1)
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            trimmed
                .split_whitespace()
                .next()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.eq_ignore_ascii_case("name"))
        })
        .collect::<Vec<_>>();
    models.sort();
    models.dedup();
    models
}

async fn detect_ollama_state() -> (bool, bool, Vec<String>) {
    let installed = detect_ollama_installation();
    let mut reachable = false;
    let mut models: Vec<String> = Vec::new();

    if let Ok(response) = reqwest::Client::new()
        .get("http://127.0.0.1:11434/api/tags")
        .send()
        .await
    {
        if response.status().is_success() {
            reachable = true;
            if let Ok(json) = response.json::<serde_json::Value>().await {
                models = json
                    .get("models")
                    .and_then(|value| value.as_array())
                    .into_iter()
                    .flatten()
                    .filter_map(|item| item.get("name").and_then(|value| value.as_str()))
                    .map(str::to_string)
                    .collect();
            }
        }
    }

    if models.is_empty() && installed {
        if let Some(ollama) = detect_ollama_executable() {
            if let Ok(output) = Command::new(ollama).arg("list").output() {
                if output.status.success() {
                    reachable = true;
                    models = parse_ollama_list_output(&String::from_utf8_lossy(&output.stdout));
                }
            }
        }
    }

    models.sort();
    models.dedup();
    (installed, reachable, models)
}

fn codex_home_dir() -> PathBuf {
    if let Some(value) = std::env::var_os("CODEX_HOME") {
        return PathBuf::from(value);
    }

    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".codex")
}

fn codex_auth_path() -> PathBuf {
    codex_home_dir().join("auth.json")
}

fn detect_codex_state() -> (bool, bool, PathBuf) {
    let auth_path = codex_auth_path();
    let cli_installed = detect_codex_executable()
        .and_then(|executable| {
            Command::new(executable)
                .arg("--help")
                .output()
                .ok()
                .filter(|output| output.status.success())
        })
        .is_some();

    let logged_in = std::fs::read_to_string(&auth_path)
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .map(|json| {
            json.get("OPENAI_API_KEY")
                .and_then(|value| value.as_str())
                .is_some_and(|value| !value.trim().is_empty())
                || json
                    .get("tokens")
                    .and_then(|value| value.as_object())
                    .is_some_and(|tokens| !tokens.is_empty())
                || json
                    .get("tokens")
                    .and_then(|value| value.as_array())
                    .is_some_and(|tokens| !tokens.is_empty())
        })
        .unwrap_or(false);

    (cli_installed, logged_in, auth_path)
}

fn read_hermes_setup_record(state: &AppState) -> Option<HermesSetupRecord> {
    let raw = std::fs::read_to_string(hermes_setup_record_path(state)).ok()?;
    serde_json::from_str::<HermesSetupRecord>(&raw).ok()
}

fn persist_hermes_setup_files(state: &AppState, setup: &HermesSetupPayload) -> Result<(), String> {
    let home_dir = hermes_home_dir(state);
    std::fs::create_dir_all(&home_dir).map_err(|e| e.to_string())?;
    let api_key = setup
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let record = HermesSetupRecord {
        provider_kind: setup.provider_kind.trim().to_string(),
        model_name: setup.model_name.trim().to_string(),
        base_url: setup
            .base_url
            .as_ref()
            .map(|value| value.trim().to_string()),
    };

    let config_yaml = match record.provider_kind.as_str() {
        "ollama" => {
            let base_url = record
                .base_url
                .clone()
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| OLLAMA_BASE_URL.to_string());
            format!(
                "model:\n  provider: custom\n  default: {}\n  base_url: {}\n  context_length: 32768\n",
                toml::to_string(&record.model_name)
                    .map_err(|e| e.to_string())?
                    .trim(),
                toml::to_string(&base_url)
                    .map_err(|e| e.to_string())?
                    .trim(),
            )
        }
        "codex" => format!(
            "model:\n  provider: codex\n  default: {}\n",
            toml::to_string(&record.model_name)
                .map_err(|e| e.to_string())?
                .trim(),
        ),
        "custom" => {
            let base_url = record
                .base_url
                .clone()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "A base URL is required for a custom endpoint.".to_string())?;
            format!(
                "model:\n  provider: custom\n  default: {}\n  base_url: {}\n",
                toml::to_string(&record.model_name)
                    .map_err(|e| e.to_string())?
                    .trim(),
                toml::to_string(&base_url)
                    .map_err(|e| e.to_string())?
                    .trim(),
            )
        }
        _ => format!(
            "model:\n  provider: {}\n  default: {}\n",
            record.provider_kind,
            toml::to_string(&record.model_name)
                .map_err(|e| e.to_string())?
                .trim(),
        ),
    };

    let mut env_lines = vec![
        "API_SERVER_ENABLED=true".to_string(),
        format!("API_SERVER_HOST={HERMES_API_HOST}"),
        format!("API_SERVER_PORT={HERMES_API_PORT}"),
        format!("API_SERVER_KEY={}", uuid::Uuid::new_v4()),
        "API_SERVER_MODEL_NAME=hermes-agent".to_string(),
    ];

    match record.provider_kind.as_str() {
        "custom" | "ollama" => {
            if let Some(api_key) = api_key {
                env_lines.push(format!("OPENAI_API_KEY={api_key}"));
            }
        }
        "openrouter" => {
            if let Some(api_key) = api_key {
                env_lines.push(format!("OPENROUTER_API_KEY={api_key}"));
            }
        }
        _ => {}
    }

    let soul_md = r#"# FNDR Agent Identity

You are the native FNDR agent experience, powered by Hermes under the hood.

- Present yourself as FNDR's built-in agent unless the user asks how you are implemented.
- FNDR is the user's trusted interface and source of truth for personal context.
- Treat FNDR-provided memory, tasks, and focus context as private and read-only.
- Ask before destructive actions, external sends, purchases, or credential changes.
- Prefer helping with recall, planning, drafting, research, and safe computer-use assistance.
"#;

    let record_json = serde_json::to_string_pretty(&record).map_err(|e| e.to_string())?;
    std::fs::write(hermes_config_path(state), config_yaml).map_err(|e| e.to_string())?;
    std::fs::write(hermes_env_path(state), env_lines.join("\n") + "\n")
        .map_err(|e| e.to_string())?;
    std::fs::write(hermes_soul_path(state), soul_md).map_err(|e| e.to_string())?;
    std::fs::write(hermes_setup_record_path(state), record_json).map_err(|e| e.to_string())?;
    Ok(())
}

fn read_hermes_api_key(state: &AppState) -> Option<String> {
    let env_contents = std::fs::read_to_string(hermes_env_path(state)).ok()?;
    env_contents.lines().find_map(|line| {
        let value = line.strip_prefix("API_SERVER_KEY=")?;
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn update_hermes_gateway_runtime() -> (bool, Option<String>) {
    let mut process_guard = get_hermes_gateway_process().lock();
    if let Some(child) = process_guard.as_mut() {
        match child.try_wait() {
            Ok(Some(status)) => {
                let message = if status.success() {
                    "Hermes gateway exited.".to_string()
                } else {
                    format!("Hermes gateway exited with status {status}.")
                };
                *get_hermes_gateway_error_store().lock() = Some(message.clone());
                *process_guard = None;
                (false, Some(message))
            }
            Ok(None) => (true, get_hermes_gateway_error_store().lock().clone()),
            Err(err) => {
                let message = format!("Failed to inspect Hermes gateway: {err}");
                *get_hermes_gateway_error_store().lock() = Some(message.clone());
                *process_guard = None;
                (false, Some(message))
            }
        }
    } else {
        (false, get_hermes_gateway_error_store().lock().clone())
    }
}

async fn hermes_api_ready() -> bool {
    match reqwest::Client::new()
        .get(format!("{}/health", hermes_api_url()))
        .send()
        .await
    {
        Ok(response) => response.status().is_success(),
        Err(_) => false,
    }
}

fn file_modified_at_ms(path: &PathBuf) -> Option<i64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let duration = modified.duration_since(UNIX_EPOCH).ok()?;
    Some(duration.as_millis().min(i64::MAX as u128) as i64)
}

fn format_hermes_timestamp(timestamp_ms: i64) -> String {
    chrono::Local
        .timestamp_millis_opt(timestamp_ms)
        .single()
        .map(|dt| dt.format("%b %d, %Y at %I:%M %p").to_string())
        .unwrap_or_else(|| "Unknown time".to_string())
}

async fn build_hermes_bridge_status(state: &AppState) -> Result<HermesBridgeStatus, String> {
    let context_path = hermes_context_path(state);
    let home_dir = hermes_home_dir(state);
    let recent_results = state
        .store
        .list_recent_results(18, None)
        .await
        .map_err(|e| e.to_string())?;
    let mut recent_memories: Vec<MemoryCard> = strip_internal_fndr_results(recent_results)
        .into_iter()
        .map(memory_card_from_result)
        .collect();
    refine_memory_card_titles(&mut recent_memories);

    let mut app_counts: HashMap<String, usize> = HashMap::new();
    for memory in &recent_memories {
        *app_counts.entry(memory.app_name.clone()).or_insert(0) += 1;
    }

    let mut top_apps: Vec<HermesAppContext> = app_counts
        .into_iter()
        .map(|(app_name, memory_count)| HermesAppContext {
            app_name,
            memory_count: memory_count as u32,
        })
        .collect();
    top_apps.sort_by(|left, right| {
        right
            .memory_count
            .cmp(&left.memory_count)
            .then_with(|| left.app_name.cmp(&right.app_name))
    });
    top_apps.truncate(6);

    let recent_memories = recent_memories
        .into_iter()
        .take(6)
        .map(|memory| HermesMemoryDigest {
            title: memory.title,
            app_name: memory.app_name,
            summary: truncate_chars(&memory.summary, 180),
            timestamp: memory.timestamp,
        })
        .collect::<Vec<_>>();

    let open_task_count = state
        .store
        .list_tasks()
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|task| !task.is_completed && !task.is_dismissed)
        .count() as u32;

    let runtime = detect_hermes_runtime(state);
    let (ollama_installed, ollama_reachable, ollama_models) = detect_ollama_state().await;
    let (codex_cli_installed, codex_logged_in, codex_auth_path) = detect_codex_state();
    let setup = read_hermes_setup_record(state);
    let configured = setup.is_some();
    let (gateway_running, last_error) = update_hermes_gateway_runtime();
    let api_server_ready = if gateway_running {
        hermes_api_ready().await
    } else {
        false
    };

    // Direct Ollama mode: provider configured as ollama + Ollama reachable + has models.
    // This works without the Hermes CLI being installed at all.
    let direct_ollama_ready = setup
        .as_ref()
        .map(|s| s.provider_kind == "ollama")
        .unwrap_or(false)
        && ollama_reachable
        && !ollama_models.is_empty();

    let bundled_repo_available = runtime.bundled_repo_available();
    let runtime_source = runtime.runtime_source.clone();
    let version = runtime.version.clone();

    Ok(HermesBridgeStatus {
        installed: runtime.installed,
        configured,
        setup_complete: configured && (runtime.installed || direct_ollama_ready),
        gateway_running,
        api_server_ready,
        direct_ollama_ready,
        version,
        bundled_repo_available,
        runtime_source,
        provider_kind: setup.as_ref().map(|value| value.provider_kind.clone()),
        model_name: setup.as_ref().map(|value| value.model_name.clone()),
        base_url: setup.as_ref().and_then(|value| {
            if value.provider_kind == "ollama" {
                Some(
                    value
                        .base_url
                        .clone()
                        .unwrap_or_else(|| OLLAMA_BASE_URL.to_string()),
                )
            } else {
                value.base_url.clone()
            }
        }),
        api_url: hermes_api_url(),
        gateway_dir: hermes_gateway_dir(state).display().to_string(),
        home_dir: home_dir.display().to_string(),
        context_path: context_path.display().to_string(),
        context_ready: context_path.exists(),
        last_synced_at: file_modified_at_ms(&context_path),
        fndr_local_model_id: read_fndr_local_model_id(state),
        ollama_installed,
        ollama_reachable,
        ollama_models,
        ollama_base_url: OLLAMA_BASE_URL.to_string(),
        codex_cli_installed,
        codex_logged_in,
        codex_auth_path: codex_auth_path.display().to_string(),
        profile_name: read_hermes_profile_name(state),
        focus_task: state.focus_task.read().clone(),
        recent_memory_count: recent_memories.len() as u32,
        open_task_count,
        top_apps,
        recent_memories,
        last_error,
        install_command: if bundled_repo_available {
            "Prepare the bundled Hermes runtime inside FNDR.".to_string()
        } else {
            "curl -fsSL https://hermes-agent.nousresearch.com/install.sh | bash".to_string()
        },
    })
}

fn render_hermes_context_markdown(status: &HermesBridgeStatus) -> String {
    let profile_line = status
        .profile_name
        .as_deref()
        .map(|name| format!("- Preferred name: {name}"))
        .unwrap_or_else(|| "- Preferred name: not set in FNDR onboarding".to_string());
    let focus_line = status
        .focus_task
        .as_deref()
        .map(|task| format!("- Focus task: {task}"))
        .unwrap_or_else(|| "- Focus task: none currently pinned in FNDR".to_string());

    let app_lines = if status.top_apps.is_empty() {
        "- No recent app clusters captured yet.".to_string()
    } else {
        status
            .top_apps
            .iter()
            .map(|app| format!("- {} ({})", app.app_name, app.memory_count))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let memory_lines = if status.recent_memories.is_empty() {
        "- No recent memories are available yet.".to_string()
    } else {
        status
            .recent_memories
            .iter()
            .map(|memory| {
                format!(
                    "- {} [{}] {}\n  {}\n  {}",
                    memory.title,
                    memory.app_name,
                    format_hermes_timestamp(memory.timestamp),
                    memory.summary,
                    "Treat this as private user context from FNDR."
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        "# FNDR Hermes Gateway\n\n\
This workspace is generated by FNDR and should feel like part of FNDR, not a separate product.\n\n\
## Operating mode\n\n\
- FNDR is the source of truth for personal context.\n\
- Use the FNDR snapshot below to help the user with recall, planning, drafting, research, and safe computer-use support.\n\
- Be grounded: if the FNDR snapshot is missing or stale, ask the user to refresh it in FNDR instead of guessing.\n\
- Operate as FNDR's built-in agent experience unless the user asks about implementation details.\n\
- Ask for approval before sending messages, making purchases, changing credentials, or doing irreversible actions.\n\
- Treat FNDR context as read-only and privacy-sensitive.\n\n\
## FNDR snapshot\n\n\
{profile_line}\n\
{focus_line}\n\
- Open FNDR tasks: {}\n\
- Recent memory cards included: {}\n\n\
## Recent apps from FNDR\n\n\
{app_lines}\n\n\
## Recent memories from FNDR\n\n\
{memory_lines}\n",
        status.open_task_count, status.recent_memory_count
    )
}

fn render_hermes_gateway_readme(status: &HermesBridgeStatus) -> String {
    format!(
        "# FNDR Hermes Gateway\n\n\
FNDR generated this workspace so Hermes can operate with FNDR-curated context.\n\n\
Files:\n\
- `.hermes.md` keeps the FNDR-native operating instructions and latest snapshot.\n\
- `FNDR_CONTEXT.md` mirrors the same snapshot in a user-readable file.\n\n\
If the snapshot feels stale, refresh it from FNDR's Agent page.\n\n\
Gateway directory: {}\n",
        status.gateway_dir
    )
}

async fn sync_hermes_bridge_files(state: &AppState) -> Result<HermesBridgeStatus, String> {
    let mut status = build_hermes_bridge_status(state).await?;
    let gateway_dir = hermes_gateway_dir(state);
    std::fs::create_dir_all(&gateway_dir).map_err(|e| e.to_string())?;

    let context_markdown = match context_runtime::build_context_pack(
        state,
        context_runtime::ContextRequest {
            query: String::new(),
            agent_type: "chat_agent".to_string(),
            budget_tokens: 1200,
            session_id: Some("hermes-bridge".to_string()),
            active_files: Vec::new(),
            project: None,
        },
    )
    .await
    {
        Ok(pack) => {
            let profile_line = status
                .profile_name
                .as_deref()
                .map(|name| format!("- Preferred name: {name}"))
                .unwrap_or_else(|| "- Preferred name: not set in FNDR onboarding".to_string());
            let focus_line = status
                .focus_task
                .as_deref()
                .map(|task| format!("- Focus task: {task}"))
                .unwrap_or_else(|| "- Focus task: none currently pinned in FNDR".to_string());

            format!(
                "# FNDR Hermes Gateway\n\n\
This workspace is generated by FNDR and should feel like part of FNDR, not a separate product.\n\n\
{profile_line}\n\
{focus_line}\n\n\
{}",
                context_runtime::render_pack_markdown(&pack)
            )
        }
        Err(err) => {
            tracing::warn!("Falling back to legacy Hermes context snapshot: {}", err);
            render_hermes_context_markdown(&status)
        }
    };
    std::fs::write(hermes_project_context_path(state), &context_markdown)
        .map_err(|e| e.to_string())?;
    std::fs::write(hermes_context_path(state), &context_markdown).map_err(|e| e.to_string())?;
    std::fs::write(
        hermes_gateway_readme_path(state),
        render_hermes_gateway_readme(&status),
    )
    .map_err(|e| e.to_string())?;

    status.context_ready = true;
    status.last_synced_at = file_modified_at_ms(&hermes_context_path(state));
    Ok(status)
}

async fn wait_for_hermes_api(timeout_ms: u64) -> bool {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    while Instant::now() < deadline {
        if hermes_api_ready().await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    false
}

fn validate_hermes_gateway_prerequisites(status: &HermesBridgeStatus) -> Result<(), String> {
    if !status.installed {
        return Err(if status.bundled_repo_available {
            "FNDR has a bundled Hermes clone, but the private runtime is not prepared yet. Click Enable Agent in the FNDR Agent panel first."
                .to_string()
        } else {
            "Hermes is not installed yet.".to_string()
        });
    }
    if !status.configured {
        return Err("Finish FNDR Agent setup before starting the runtime.".to_string());
    }
    if status.provider_kind.as_deref() == Some("ollama") {
        if !status.ollama_installed {
            return Err(
                "Install Ollama on this Mac before starting the FNDR agent in Ollama mode."
                    .to_string(),
            );
        }
        if !status.ollama_reachable {
            return Err(
                "FNDR could not reach Ollama at http://127.0.0.1:11434. Open Ollama or run `ollama serve`, then try again."
                    .to_string(),
            );
        }
    }
    if status.provider_kind.as_deref() == Some("codex") && !status.codex_logged_in {
        return Err(
            "FNDR could not find an active Codex login for the agent runtime. Sign in to Codex on this Mac first."
                .to_string(),
        );
    }
    Ok(())
}

async fn ensure_hermes_gateway_ready(
    state: &AppState,
    timeout_ms: u64,
) -> Result<HermesBridgeStatus, String> {
    let status = sync_hermes_bridge_files(state).await?;
    validate_hermes_gateway_prerequisites(&status)?;

    if status.api_server_ready {
        return Ok(status);
    }

    let (running, _) = update_hermes_gateway_runtime();
    if !running {
        let launcher = detect_hermes_runtime(state)
            .launcher
            .ok_or_else(|| "FNDR could not resolve a Hermes runtime to launch.".to_string())?;
        let mut command = launcher.command();
        command
            .arg("gateway")
            .env("HERMES_HOME", hermes_home_dir(state))
            .current_dir(hermes_gateway_dir(state))
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        if status.codex_logged_in {
            command.env("CODEX_HOME", codex_home_dir());
        }

        let child = command
            .spawn()
            .map_err(|e| format!("Failed to start Hermes gateway: {e}"))?;

        *get_hermes_gateway_process().lock() = Some(child);
        *get_hermes_gateway_error_store().lock() = None;
    }

    if !wait_for_hermes_api(timeout_ms).await {
        let message =
            "Hermes gateway started, but the local API server did not come online in time."
                .to_string();
        *get_hermes_gateway_error_store().lock() = Some(message.clone());
        return Err(message);
    }

    let ready_status = build_hermes_bridge_status(state).await?;
    if ready_status.api_server_ready {
        Ok(ready_status)
    } else {
        Err(ready_status
            .last_error
            .clone()
            .unwrap_or_else(|| "Hermes gateway is still unavailable.".to_string()))
    }
}

#[tauri::command]
pub async fn get_hermes_bridge_status(
    state: State<'_, Arc<AppState>>,
) -> Result<HermesBridgeStatus, String> {
    build_hermes_bridge_status(state.inner()).await
}

#[tauri::command]
pub async fn sync_hermes_bridge_context(
    state: State<'_, Arc<AppState>>,
) -> Result<HermesBridgeStatus, String> {
    sync_hermes_bridge_files(state.inner()).await
}

#[tauri::command]
pub async fn install_hermes_bridge(
    state: State<'_, Arc<AppState>>,
) -> Result<HermesBridgeStatus, String> {
    let runtime = detect_hermes_runtime(state.inner());
    if runtime.installed {
        return build_hermes_bridge_status(state.inner()).await;
    }

    if runtime.bundled_repo_available() {
        prepare_vendored_hermes_runtime(state.inner())?;
    } else {
        let install_command = "curl -fsSL https://hermes-agent.nousresearch.com/install.sh | bash";
        let output = Command::new("sh")
            .arg("-lc")
            .arg(install_command)
            .output()
            .map_err(|e| format!("Failed to run Hermes installer: {e}"))?;

        if !output.status.success() {
            return Err(format!(
                "Hermes install failed. {}",
                command_failure_detail(&output)
            ));
        }
    }

    build_hermes_bridge_status(state.inner()).await
}

#[tauri::command]
pub async fn save_hermes_setup(
    state: State<'_, Arc<AppState>>,
    payload: HermesSetupPayload,
) -> Result<HermesBridgeStatus, String> {
    let provider_kind = payload.provider_kind.trim();
    let model_name = payload.model_name.trim();
    if model_name.is_empty() {
        return Err("Choose a model name for the FNDR agent.".to_string());
    }

    let api_key = payload
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    match provider_kind {
        "openrouter" => {
            if api_key.is_none() {
                return Err(
                    "An OpenRouter API key is required to finish FNDR Agent setup.".to_string(),
                );
            }
        }
        "custom" => {
            let has_base_url = payload
                .base_url
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty());
            if !has_base_url {
                return Err("A base URL is required for a custom endpoint.".to_string());
            }
        }
        "ollama" => {
            let (ollama_installed, _, _) = detect_ollama_state().await;
            if !ollama_installed {
                return Err(
                    "FNDR could not find Ollama on this Mac. Install Ollama first, then return to the Agent page."
                        .to_string(),
                );
            }
        }
        "codex" => {
            let (_, codex_logged_in, _) = detect_codex_state();
            if !codex_logged_in {
                return Err(
                    "FNDR could not find a local Codex login yet. Sign in to Codex on this Mac first, then choose Codex again."
                        .to_string(),
                );
            }
        }
        _ => {
            return Err(
                "FNDR currently supports agent setup via Ollama, Codex OAuth, OpenRouter, or a custom endpoint."
                    .to_string(),
            );
        }
    }

    persist_hermes_setup_files(state.inner(), &payload)?;
    {
        let mut process_guard = get_hermes_gateway_process().lock();
        if let Some(child) = process_guard.as_mut() {
            let _ = child.kill();
        }
        *process_guard = None;
    }
    *get_hermes_gateway_error_store().lock() = None;
    sync_hermes_bridge_files(state.inner()).await
}

#[tauri::command]
pub async fn start_hermes_gateway(
    state: State<'_, Arc<AppState>>,
) -> Result<HermesBridgeStatus, String> {
    ensure_hermes_gateway_ready(state.inner(), 12_000).await
}

#[tauri::command]
pub async fn stop_hermes_gateway(
    state: State<'_, Arc<AppState>>,
) -> Result<HermesBridgeStatus, String> {
    // Drop guards before the .await — parking_lot MutexGuard is !Send.
    {
        let mut process_guard = get_hermes_gateway_process().lock();
        if let Some(child) = process_guard.as_mut() {
            let _ = child.kill();
        }
        *process_guard = None;
        *get_hermes_gateway_error_store().lock() = None;
    }
    build_hermes_bridge_status(state.inner()).await
}

/// Direct chat with an Ollama model — no Hermes CLI required.
/// Works with any OpenAI-compatible base URL (Ollama's /v1 endpoint).
#[tauri::command]
pub async fn send_direct_chat(
    state: State<'_, Arc<AppState>>,
    messages: Vec<serde_json::Value>,
    input: String,
) -> Result<String, String> {
    let _ = sync_hermes_bridge_files(state.inner()).await?;
    let setup = read_hermes_setup_record(state.inner())
        .ok_or_else(|| "Configure a provider in FNDR's Agent page first.".to_string())?;

    let base_url = if setup.provider_kind == "ollama" {
        setup
            .base_url
            .clone()
            .filter(|u| !u.trim().is_empty())
            .unwrap_or_else(|| OLLAMA_BASE_URL.to_string())
    } else {
        return Err(
            "Direct chat is only available for Ollama. Use the Hermes gateway for other providers."
                .to_string(),
        );
    };

    // Build a system message with FNDR context
    let context_path = hermes_context_path(state.inner());
    let system_content = if context_path.exists() {
        std::fs::read_to_string(&context_path)
            .unwrap_or_else(|_| "You are a helpful assistant embedded in FNDR.".to_string())
    } else {
        "You are a helpful assistant embedded in FNDR, a privacy-first local memory app. Help the user with recall, planning, drafting, and research using context they provide.".to_string()
    };

    let mut all_messages: Vec<serde_json::Value> =
        vec![serde_json::json!({ "role": "system", "content": system_content })];
    all_messages.extend(messages);
    all_messages.push(serde_json::json!({ "role": "user", "content": input.trim() }));

    let request = serde_json::json!({
        "model": setup.model_name,
        "messages": all_messages,
        "stream": false,
    });

    let response = reqwest::Client::new()
        .post(format!(
            "{}/chat/completions",
            base_url.trim_end_matches('/')
        ))
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("Could not reach Ollama at {base_url}: {e}"))?;

    let status_code = response.status();
    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Ollama returned an unreadable response: {e}"))?;

    if !status_code.is_success() {
        return Err(json
            .get("error")
            .and_then(|v| v.get("message").or(Some(v)))
            .and_then(|v| v.as_str())
            .unwrap_or("Ollama request failed.")
            .to_string());
    }

    let content = json
        .get("choices")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|msg| msg.get("content"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    Ok(content)
}

#[tauri::command]
pub async fn send_hermes_message(
    state: State<'_, Arc<AppState>>,
    conversation_id: String,
    input: String,
) -> Result<HermesChatReply, String> {
    let status = ensure_hermes_gateway_ready(state.inner(), 12_000).await?;

    let api_key = read_hermes_api_key(state.inner())
        .ok_or_else(|| "FNDR could not read the Hermes API server key.".to_string())?;
    let input = input.trim();
    if input.is_empty() {
        return Err("Message cannot be empty.".to_string());
    }

    let instructions = "You are the native FNDR agent experience, powered by Hermes under the hood. Use FNDR's context files and private snapshot to help with planning, recall, drafting, research, and safe computer-use support. Ask before destructive actions, external messages, purchases, or credential changes.";
    let request_body = serde_json::json!({
        "model": "hermes-agent",
        "input": input,
        "conversation": conversation_id,
        "store": true,
        "instructions": instructions
    });

    let response = reqwest::Client::new()
        .post(format!("{}/v1/responses", status.api_url))
        .bearer_auth(api_key)
        .json(&request_body)
        .send()
        .await
        .map_err(|e| format!("Failed to reach the Hermes API server: {e}"))?;

    let status_code = response.status();
    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Hermes returned an unreadable response: {e}"))?;

    if !status_code.is_success() {
        return Err(json
            .get("error")
            .and_then(|value| value.get("message"))
            .and_then(|value| value.as_str())
            .or_else(|| json.get("detail").and_then(|value| value.as_str()))
            .unwrap_or("Hermes API request failed.")
            .to_string());
    }

    let response_id = json
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();

    let content = json
        .get("output")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    if item.get("type").and_then(|value| value.as_str()) == Some("message") {
                        item.get("content")
                            .and_then(|value| value.as_array())
                            .map(|parts| {
                                parts
                                    .iter()
                                    .filter_map(|part| {
                                        part.get("text").and_then(|value| value.as_str())
                                    })
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            })
                    } else {
                        None
                    }
                })
                .filter(|text| !text.trim().is_empty())
                .collect::<Vec<_>>()
                .join("\n\n")
        })
        .filter(|text| !text.trim().is_empty())
        .unwrap_or_else(|| {
            "Hermes completed the turn, but no assistant text was returned.".to_string()
        });

    Ok(HermesChatReply {
        response_id,
        conversation_id,
        content,
    })
}

/// Start the agent to execute a task
#[tauri::command]
pub async fn start_agent_task(
    task_title: String,
    context_urls: Option<Vec<String>>,
    context_notes: Option<Vec<String>>,
) -> Result<AgentStatus, String> {
    let mut process_guard = get_agent_process().lock();

    // Kill existing process if any
    if let Some(ref mut child) = *process_guard {
        let _ = child.kill();
    }

    // Find the agent runner script
    let sidecar_path = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .parent()
        .ok_or("No parent dir")?
        .join("../Resources/sidecar/agent_runner.py");

    let script_path = if sidecar_path.exists() {
        sidecar_path
    } else {
        // Fallback for development
        std::path::PathBuf::from("src-tauri/sidecar/agent_runner.py")
    };

    // Find the python executable in the virtual environment
    let venv_python = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .parent()
        .ok_or("No parent dir")?
        .join("../.venv/bin/python3");

    let python_exe = if venv_python.exists() {
        venv_python
    } else {
        // Fallback for development (assuming project root relative to execution)
        std::path::PathBuf::from(".venv/bin/python3")
    };

    let mut task_prompt = task_title.clone();
    if let Some(urls) = context_urls {
        if !urls.is_empty() {
            let url_context = urls
                .into_iter()
                .take(6)
                .map(|u| format!("- {}", u))
                .collect::<Vec<_>>()
                .join("\n");
            task_prompt.push_str("\n\nGround-truth URLs from memory graph:\n");
            task_prompt.push_str(&url_context);
        }
    }
    if let Some(notes) = context_notes {
        if !notes.is_empty() {
            task_prompt.push_str("\n\nMemory graph notes:\n");
            task_prompt.push_str(
                &notes
                    .into_iter()
                    .take(5)
                    .map(|n| format!("- {}", n))
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        }
    }

    // Start the agent process
    let child = Command::new(python_exe)
        .arg(&script_path)
        .arg(&task_prompt)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to start agent: {}", e))?;

    *process_guard = Some(child);

    // Update status
    let mut status = get_agent_status_store().lock();
    *status = AgentStatus {
        is_running: true,
        task_title: Some(task_title),
        last_message: Some("Agent started...".to_string()),
        status: "running".to_string(),
    };

    Ok(status.clone())
}

/// Get current agent status
#[tauri::command]
pub async fn get_agent_status() -> Result<AgentStatus, String> {
    let mut process_guard = get_agent_process().lock();
    let mut status = get_agent_status_store().lock();

    if let Some(ref mut child) = *process_guard {
        // Check if process is still running
        match child.try_wait() {
            Ok(Some(exit_status)) => {
                status.is_running = false;
                status.status = if exit_status.success() {
                    "completed".to_string()
                } else {
                    "error".to_string()
                };
            }
            Ok(None) => {
                // Still running, try to read output
                status.is_running = true;
            }
            Err(e) => {
                status.is_running = false;
                status.status = "error".to_string();
                status.last_message = Some(format!("Error: {}", e));
            }
        }
    }

    Ok(status.clone())
}

/// Stop the agent
#[tauri::command]
pub async fn stop_agent() -> Result<AgentStatus, String> {
    let mut process_guard = get_agent_process().lock();

    if let Some(ref mut child) = *process_guard {
        let _ = child.kill();
    }
    *process_guard = None;

    let mut status = get_agent_status_store().lock();
    *status = AgentStatus {
        is_running: false,
        task_title: status.task_title.clone(),
        last_message: Some("Agent stopped by user".to_string()),
        status: "idle".to_string(),
    };

    Ok(status.clone())
}

/// Generate a smart daily briefing paragraph using the local LLM.
/// `mode`: "morning" (actionable: what to focus on) or "evening" (recap + tomorrow).
/// Defaults to time-of-day detection when None.
#[tauri::command]
pub async fn generate_daily_briefing(
    state: State<'_, Arc<AppState>>,
    mode: Option<String>,
) -> Result<String, String> {
    // Detect mode from local hour if not specified
    let resolved_mode = mode.unwrap_or_else(|| {
        let hour = chrono::Local::now().hour();
        if hour >= 17 {
            "evening".to_string()
        } else {
            "morning".to_string()
        }
    });

    // Fetch the most recent cards (today + a few recent ones for context)
    let limit = 10usize;
    let results = state
        .store
        .list_recent_results(limit, None)
        .await
        .map_err(|e| e.to_string())?;

    let mut cards: Vec<MemoryCard> = strip_internal_fndr_results(results)
        .into_iter()
        .map(memory_card_from_result)
        .collect();
    refine_memory_card_titles(&mut cards);

    if cards.is_empty() {
        return Ok(String::new());
    }

    // Build compact per-card lines for the LLM context
    let card_lines: Vec<String> = cards
        .iter()
        .take(8)
        .map(|c| format!("- [{}] {}: {}", c.app_name, c.title, c.summary))
        .collect();

    // Grab inference engine
    let engine = {
        let guard = state.inference.read();
        guard.as_ref().map(Arc::clone)
    };

    let Some(engine) = engine else {
        return Ok(String::new());
    };

    let briefing = engine
        .generate_daily_briefing(&card_lines, &resolved_mode)
        .await;
    Ok(briefing)
}

/// Link all segments of a completed meeting to overlapping memory records via
/// `OccurredDuringAudio` edges. Called automatically when a meeting is stopped.
#[tauri::command]
pub async fn link_audio_to_memories(
    meeting_id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<usize, String> {
    let segments = crate::meeting::get_meeting_segments(&meeting_id).await;
    let count = segments.len();
    state
        .graph
        .link_audio_to_memories(&segments)
        .await
        .map_err(|e| e.to_string())?;
    Ok(count)
}

#[tauri::command]
pub fn get_fun_greeting(name: Option<String>) -> Result<String, String> {
    use rand::prelude::IndexedRandom;
    let base_name = name.unwrap_or_else(|| "there".to_string());

    let hour = chrono::Local::now().hour();

    let prefix = if hour >= 4 && hour < 12 {
        "Good Morning"
    } else if hour >= 12 && hour < 16 {
        "Good Afternoon"
    } else if hour >= 16 && hour < 20 {
        "Good Evening"
    } else {
        "Good Night"
    };

    let fun_suffixes = vec![
        "Ready to conquer the day?",
        "Let's dive into your memories.",
        "What are we exploring today?",
        "Time to make some magic happen.",
        "Welcome back to the matrix.",
        "Let's get productive.",
        "System fully operational.",
    ];

    let mut rng = rand::rng();
    let random_suffix = fun_suffixes.choose(&mut rng).unwrap_or(&"");

    Ok(format!("{}, {}! {}", prefix, base_name, random_suffix))
}

// ── Proactive Privacy Shield Commands ───────────────────────────────────────────

#[tauri::command]
pub async fn get_privacy_alerts(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<crate::PrivacyAlert>, String> {
    let pending = state.pending_privacy_alerts.read();
    Ok(pending.clone())
}

#[tauri::command]
pub async fn dismiss_privacy_alert(
    site: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let site_key = privacy_site_key(&site);
    {
        let mut pending = state.pending_privacy_alerts.write();
        pending.retain(|a| !privacy_site_matches(&a.domain_or_title, &site_key));
    }
    {
        let mut snoozed = state.snoozed_privacy_alerts.write();
        // Keep the in-memory cache aligned until the persisted dismissal is reloaded.
        let expire = chrono::Local::now().timestamp() + (30 * 24 * 60 * 60);
        snoozed.insert(site_key.clone(), expire);
    }
    {
        let mut config = state.config.write();
        push_unique_case_insensitive(&mut config.dismissed_privacy_alerts, site_key);
        config.save().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn add_to_blocklist(site: String, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let site_key = privacy_site_key(&site);

    // 1. Remove from pending alerts
    {
        let mut pending = state.pending_privacy_alerts.write();
        pending.retain(|a| !privacy_site_matches(&a.domain_or_title, &site_key));
    }

    // 2. Add to config blocklist
    {
        let mut config = state.config.write();
        if !config
            .blocklist
            .iter()
            .any(|b| b.eq_ignore_ascii_case(&site_key))
        {
            config.blocklist.push(site_key.clone());
        }
        config
            .dismissed_privacy_alerts
            .retain(|value| !privacy_site_matches(value, &site_key));
        config.save().map_err(|e| e.to_string())?;
    }

    // 3. Retroactively delete memories with this site if we grabbed it during the alert period
    if let Err(e) = state.store.delete_memories_by_domain(&site_key).await {
        tracing::error!(
            "Failed to retroactively delete memories for blocked site {}: {}",
            site_key,
            e
        );
    } else {
        state.invalidate_memory_derived_caches();
    }

    Ok(())
}

fn privacy_site_key(site: &str) -> String {
    Blocklist::context_key(Some(site), Some(site)).unwrap_or_else(|| site.trim().to_string())
}

fn privacy_site_matches(value: &str, site_key: &str) -> bool {
    if value.eq_ignore_ascii_case(site_key) {
        return true;
    }
    let site_values = vec![site_key.to_string()];
    let value_values = vec![value.to_string()];
    Blocklist::is_context_blocked(Some(value), Some(value), &site_values)
        || Blocklist::is_context_blocked(Some(site_key), Some(site_key), &value_values)
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

fn build_daily_activity_summary(records: &[SearchResult], day_label: &str) -> String {
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
            "- FNDR captured 1 memory {day_label} at {}.",
            format_local_time(first_ts)
        ));
    } else {
        lines.push(format!(
            "- FNDR captured {} {memory_word} across {} {day_label}, from {} to {}.",
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
    let records = strip_internal_fndr_results(records);

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

fn build_focus_task_embedding(
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

// ── Auto-fill commands ────────────────────────────────────────────────────────

const AUTOFILL_OVERLAY_LABEL: &str = "autofill-overlay";
const AUTOFILL_OVERLAY_WIDTH: f64 = 500.0;
const AUTOFILL_OVERLAY_HEIGHT: f64 = 430.0;
static AUTOFILL_OVERLAY_READY: once_cell::sync::Lazy<parking_lot::Mutex<bool>> =
    once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(false));
static PENDING_AUTOFILL_PAYLOAD: once_cell::sync::Lazy<
    parking_lot::Mutex<Option<serde_json::Value>>,
> = once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(None));

/// Return the logical (x, y) bottom-right position for the autofill overlay on
/// the monitor the mouse cursor currently occupies. Falls back to primary monitor.
fn find_cursor_monitor<R: tauri::Runtime>(app: &AppHandle<R>) -> Option<(f64, f64)> {
    use enigo::{Enigo, Mouse, Settings};

    let cursor_pos = Enigo::new(&Settings::default())
        .ok()
        .and_then(|e| e.location().ok());

    let monitors = app.available_monitors().ok()?;

    let active = if let Some((cx, cy)) = cursor_pos {
        monitors
            .iter()
            .find(|m| {
                let pos = m.position();
                let size = m.size();
                cx >= pos.x
                    && cy >= pos.y
                    && cx < pos.x + size.width as i32
                    && cy < pos.y + size.height as i32
            })
            .or_else(|| monitors.first())
    } else {
        monitors.first()
    }?;

    let scale = active.scale_factor();
    let size = active.size();
    let pos = active.position();
    let w = size.width as f64 / scale;
    let h = size.height as f64 / scale;
    let lx = pos.x as f64 / scale;
    let ly = pos.y as f64 / scale;
    Some((
        lx + w - AUTOFILL_OVERLAY_WIDTH - 24.0,
        ly + h - AUTOFILL_OVERLAY_HEIGHT - 40.0,
    ))
}

/// Pre-create the autofill overlay window at startup (hidden) so it is fully
/// loaded and the React event listener is mounted before the first hotkey press.
/// Called once from main.rs setup.
pub fn create_autofill_overlay_window<R: tauri::Runtime>(app: &AppHandle<R>) {
    let url = tauri::WebviewUrl::App("autofill.html".into());

    let (x, y) = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| {
            let size = m.size();
            let pos = m.position();
            let scale = m.scale_factor();
            // Both size and position are in physical pixels; divide by scale for logical coords.
            let w = size.width as f64 / scale;
            let h = size.height as f64 / scale;
            let lx = pos.x as f64 / scale;
            let ly = pos.y as f64 / scale;
            let x = lx + w - AUTOFILL_OVERLAY_WIDTH - 24.0;
            let y = ly + h - AUTOFILL_OVERLAY_HEIGHT - 40.0;
            tracing::info!(
                "autofill overlay: monitor {w}×{h} logical, scale={scale}, placing at ({x},{y})"
            );
            (x, y)
        })
        .unwrap_or((800.0, 400.0));

    match tauri::WebviewWindowBuilder::new(app, AUTOFILL_OVERLAY_LABEL, url)
        .title("FNDR Autofill")
        .inner_size(AUTOFILL_OVERLAY_WIDTH, AUTOFILL_OVERLAY_HEIGHT)
        .position(x, y)
        .decorations(false)
        .always_on_top(true)
        .resizable(false)
        .skip_taskbar(true)
        .shadow(false)
        .visible(false)
        .build()
    {
        Ok(_) => tracing::info!("autofill overlay window pre-created (hidden)"),
        Err(err) => tracing::warn!("failed to pre-create autofill overlay window: {err}"),
    }
}

#[tauri::command]
pub async fn set_autofill_overlay_ready(ready: bool) -> Option<serde_json::Value> {
    *AUTOFILL_OVERLAY_READY.lock() = ready;
    if ready {
        PENDING_AUTOFILL_PAYLOAD.lock().take()
    } else {
        None
    }
}

#[tauri::command]
pub async fn take_pending_autofill_payload() -> Option<serde_json::Value> {
    PENDING_AUTOFILL_PAYLOAD.lock().take()
}

#[tauri::command]
pub async fn show_autofill_overlay_window(app: AppHandle) -> Result<(), String> {
    let Some(win) = app.get_webview_window(AUTOFILL_OVERLAY_LABEL) else {
        return Err("Autofill overlay window is unavailable".to_string());
    };

    win.show().map_err(|err| err.to_string())?;
    win.set_focus().map_err(|err| err.to_string())?;
    Ok(())
}

pub fn register_autofill_shortcut<R: tauri::Runtime>(
    app: &AppHandle<R>,
    config: &AutofillConfig,
) -> Result<(), String> {
    if let Err(err) = app.global_shortcut().unregister_all() {
        tracing::debug!("autofill: failed clearing existing shortcuts: {err}");
    }

    if !config.enabled {
        tracing::info!("autofill: shortcut disabled in settings");
        return Ok(());
    }

    let shortcut: Shortcut = config
        .shortcut
        .parse()
        .map_err(|err| format!("Invalid auto-fill shortcut '{}': {err}", config.shortcut))?;

    let handle = app.clone();
    app.global_shortcut()
        .on_shortcut(shortcut, move |_app, _shortcut, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }

            tracing::info!("Auto-fill hotkey fired");
            let handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                // Capture the focused field before FNDR steals focus, otherwise we may
                // end up describing the overlay window instead of the target form input.
                let payload = match crate::accessibility::capture_focused_context() {
                    Ok(ctx) => {
                        tracing::info!(
                            "Auto-fill field context captured: label='{}' app='{}' window='{}'",
                            ctx.label,
                            ctx.app_name,
                            ctx.window_title
                        );
                        serde_json::to_value(&ctx).unwrap_or_default()
                    }
                    Err(err) => {
                        tracing::info!("Auto-fill context capture failed: {err}");
                        serde_json::json!({ "error": err })
                    }
                };

                *PENDING_AUTOFILL_PAYLOAD.lock() = Some(payload);

                // Reposition to the monitor the cursor is currently on before showing,
                // so the overlay appears near the user's active working context.
                let cursor_monitor = find_cursor_monitor(&handle);

                let (tx, rx) = tokio::sync::oneshot::channel::<()>();
                let h1 = handle.clone();
                let _ = handle.run_on_main_thread(move || {
                    if let Some(win) = h1.get_webview_window(AUTOFILL_OVERLAY_LABEL) {
                        // Reposition to the active monitor's bottom-right corner.
                        if let Some((x, y)) = cursor_monitor {
                            let _ = win.set_position(tauri::LogicalPosition::new(x, y));
                        }
                        tracing::info!("autofill: showing overlay window");
                        let _ = win.show();
                        let _ = win.set_focus();
                    } else {
                        tracing::warn!("autofill: overlay window not found at hotkey time");
                    }
                    let _ = tx.send(());
                });

                // Wait for show() to complete, then give WKWebView a beat to resume
                // before delivering the payload. Emit the actual captured payload so
                // the frontend can start resolution immediately without polling.
                let _ = rx.await;
                tokio::time::sleep(std::time::Duration::from_millis(120)).await;
                if *AUTOFILL_OVERLAY_READY.lock() {
                    // Emit the real payload directly — frontend handles FieldContext,
                    // error objects, and { scanning } objects the same way.
                    let payload_to_emit = PENDING_AUTOFILL_PAYLOAD.lock().clone();
                    if let Some(payload) = payload_to_emit {
                        let _ = handle.emit("autofill-triggered", payload);
                    } else {
                        let _ = handle.emit(
                            "autofill-triggered",
                            serde_json::json!({ "scanning": true, "message": "Preparing autofill" }),
                        );
                    }
                }
            });
        })
        .map_err(|err| err.to_string())
}

/// A single candidate memory value FNDR can inject into the active field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutofillCandidate {
    pub value: String,
    pub confidence: f32,
    pub match_reason: String,
    pub source_snippet: String,
    pub source_app: String,
    pub source_window_title: String,
    pub timestamp: i64,
    pub memory_id: String,
}

/// Result of resolving the active field against FNDR's memory store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutofillResolution {
    pub query: String,
    pub query_source: String,
    pub context_hint: String,
    pub candidates: Vec<AutofillCandidate>,
    pub auto_inject_threshold: f32,
    pub requires_confirmation: bool,
    pub used_ocr_fallback: bool,
}

#[derive(Debug, Clone)]
struct AutofillCandidateDraft {
    value: String,
    extraction_score: f32,
    match_reason: String,
    source_snippet: String,
    source_app: String,
    source_window_title: String,
    timestamp: i64,
    memory_id: String,
    search_score: f32,
    ocr_confidence: f32,
    noise_score: f32,
    context_alignment: f32,
}

fn normalize_autofill_phrase(input: &str) -> String {
    input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '#' {
                ch.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalized_tokens(input: &str) -> Vec<String> {
    normalize_autofill_phrase(input)
        .split_whitespace()
        .map(ToString::to_string)
        .collect()
}

fn push_unique_case_insensitive(values: &mut Vec<String>, value: impl Into<String>) {
    let value = value.into();
    let normalized = normalize_autofill_phrase(&value);
    if normalized.is_empty() {
        return;
    }
    if values
        .iter()
        .any(|existing| normalize_autofill_phrase(existing) == normalized)
    {
        return;
    }
    values.push(value);
}

fn field_aliases(query: &str) -> Vec<String> {
    const GROUPS: &[&[&str]] = &[
        &[
            "tax id",
            "tax identification number",
            "employer identification number",
            "employer id number",
            "ein",
            "tin",
            "federal tax id",
        ],
        &["policy number", "policy no", "policy #"],
        &[
            "member id",
            "member number",
            "member #",
            "subscriber id",
            "subscriber number",
        ],
        &["group number", "group no", "group #"],
        &["claim number", "claim no", "claim #"],
        &["phone", "phone number", "telephone", "mobile"],
        &["email", "email address"],
        &["date of birth", "birth date", "dob"],
        &["zip", "zip code", "postal code"],
        &["routing number", "routing #", "aba routing number"],
        &["account number", "account #"],
    ];

    let normalized = normalize_autofill_phrase(query);
    let mut aliases = Vec::new();
    push_unique_case_insensitive(&mut aliases, query.trim());

    for group in GROUPS {
        let matches_group = group.iter().any(|alias| {
            let alias_normalized = normalize_autofill_phrase(alias);
            normalized == alias_normalized
                || normalized.contains(&alias_normalized)
                || alias_normalized.contains(&normalized)
        });
        if matches_group {
            for alias in *group {
                push_unique_case_insensitive(&mut aliases, *alias);
            }
        }
    }

    if normalized.ends_with(" number") {
        let stem = normalized.trim_end_matches(" number").trim();
        if !stem.is_empty() {
            push_unique_case_insensitive(&mut aliases, format!("{stem} no"));
            push_unique_case_insensitive(&mut aliases, format!("{stem} #"));
        }
    }

    aliases.truncate(6);
    aliases
}

fn is_generic_context_token(token: &str) -> bool {
    matches!(
        token,
        "the"
            | "and"
            | "for"
            | "with"
            | "from"
            | "into"
            | "your"
            | "this"
            | "that"
            | "form"
            | "field"
            | "portal"
            | "screen"
            | "window"
            | "page"
            | "submit"
            | "cancel"
            | "save"
            | "continue"
            | "next"
            | "back"
            | "required"
            | "optional"
            | "section"
            | "information"
            | "details"
            | "value"
            | "google"
            | "chrome"
            | "safari"
            | "browser"
            | "preview"
            | "acrobat"
            | "adobe"
            | "microsoft"
            | "edge"
            | "firefox"
            | "brave"
    )
}

fn collect_context_terms(context: &crate::accessibility::FieldContext, query: &str) -> Vec<String> {
    let query_tokens = normalized_tokens(query).into_iter().collect::<HashSet<_>>();
    let mut counts: HashMap<String, usize> = HashMap::new();

    for text in [
        &context.window_title,
        &context.screen_context,
        &context.app_name,
    ] {
        for token in normalized_tokens(text) {
            if token.len() < 3 || query_tokens.contains(&token) || is_generic_context_token(&token)
            {
                continue;
            }
            *counts.entry(token).or_insert(0) += 1;
        }
    }

    let mut ranked = counts.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    ranked.into_iter().map(|(term, _)| term).take(6).collect()
}

fn build_autofill_search_query(
    context: &crate::accessibility::FieldContext,
    query: &str,
) -> String {
    let context_terms = collect_context_terms(context, query);
    let normalized = normalize_autofill_phrase(query);
    let token_count = normalized.split_whitespace().count();

    if token_count <= 2 && !context_terms.is_empty() {
        let anchor = context_terms
            .iter()
            .take(2)
            .cloned()
            .collect::<Vec<_>>()
            .join(" ");
        return format!("{query} {anchor}");
    }

    query.trim().to_string()
}

fn build_autofill_query(
    context: &crate::accessibility::FieldContext,
    query_override: Option<&str>,
) -> (String, String) {
    if let Some(query) = query_override
        .map(str::trim)
        .filter(|query| !query.is_empty())
    {
        return (query.to_string(), "manual".to_string());
    }

    if !context.label.trim().is_empty() {
        return (context.label.trim().to_string(), "label".to_string());
    }

    if !context.placeholder.trim().is_empty() {
        return (
            context.placeholder.trim().to_string(),
            "placeholder".to_string(),
        );
    }

    if !context.inferred_label.trim().is_empty() {
        return (context.inferred_label.trim().to_string(), "ocr".to_string());
    }

    (String::new(), "unknown".to_string())
}

fn context_hint(context: &crate::accessibility::FieldContext) -> String {
    if !context.screen_context.trim().is_empty() {
        return context.screen_context.clone();
    }

    if !context.window_title.trim().is_empty() {
        return context.window_title.trim().to_string();
    }

    context.app_name.trim().to_string()
}

fn split_tableish(line: &str) -> Vec<String> {
    let Some(regex) = regex::Regex::new(r"\s*\|\s*|\t+|\s{2,}").ok() else {
        return vec![line.trim().to_string()];
    };
    regex
        .split(line)
        .map(str::trim)
        .filter(|cell| !cell.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn sanitize_autofill_value(raw: &str) -> String {
    let compact = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let compact = compact
        .trim()
        .trim_matches(|ch: char| matches!(ch, ':' | ';' | ',' | '|' | '"' | '\'' | ' '))
        .to_string();

    if compact.contains("  ") {
        return compact
            .split("  ")
            .next()
            .unwrap_or(&compact)
            .trim()
            .to_string();
    }

    compact
}

fn looks_like_field_value(query: &str, raw: &str) -> bool {
    let value = sanitize_autofill_value(raw);
    let normalized_value = normalize_autofill_phrase(&value);
    let normalized_query = normalize_autofill_phrase(query);

    if value.is_empty() || value.len() > 160 || normalized_value == normalized_query {
        return false;
    }

    if value.starts_with("http://") || value.starts_with("https://") {
        return false;
    }

    let word_count = value.split_whitespace().count();
    let has_digits = value.chars().any(|ch| ch.is_ascii_digit());
    let has_letters = value.chars().any(|ch| ch.is_ascii_alphabetic());

    if normalized_query.contains("address") {
        return word_count <= 12;
    }

    if normalized_query.contains("email") {
        return value.contains('@') && value.contains('.');
    }

    if normalized_query.contains("phone") {
        return has_digits && value.len() <= 32;
    }

    if normalized_query.contains("date") || normalized_query.contains("dob") {
        return has_digits && value.len() <= 24;
    }

    if normalized_query.contains("number")
        || normalized_query.ends_with(" id")
        || normalized_query.contains("ein")
        || normalized_query.contains("routing")
        || normalized_query.contains("account")
    {
        return has_digits || value.contains('-');
    }

    word_count <= 8 && (has_digits || has_letters)
}

fn alias_matches(label_cell: &str, aliases: &[String]) -> bool {
    let normalized = normalize_autofill_phrase(label_cell);
    if normalized.is_empty() {
        return false;
    }

    let normalized_tokens = normalized.split_whitespace().collect::<HashSet<_>>();
    aliases.iter().any(|alias| {
        let alias_normalized = normalize_autofill_phrase(alias);
        if alias_normalized.is_empty() {
            return false;
        }

        if normalized == alias_normalized
            || normalized.contains(&alias_normalized)
            || alias_normalized.contains(&normalized)
        {
            return true;
        }

        let alias_tokens = alias_normalized.split_whitespace().collect::<Vec<_>>();
        if alias_tokens.is_empty() {
            return false;
        }

        let matched = alias_tokens
            .iter()
            .filter(|token| normalized_tokens.contains(**token))
            .count();

        if alias_tokens.len() == 1 {
            matched == 1
        } else {
            matched == alias_tokens.len()
        }
    })
}

fn extract_inline_value(line: &str, alias: &str) -> Option<String> {
    let pattern = format!(
        r"(?i)\b{}\b\s*(?:[:=#-]\s*|\s+)([^\n\r]{{1,160}})",
        regex::escape(alias)
    );
    let regex = regex::Regex::new(&pattern).ok()?;
    let captures = regex.captures(line)?;
    let raw = captures.get(1)?.as_str();
    let cells = split_tableish(raw);
    let value = if cells.len() > 1 { &cells[0] } else { raw };
    Some(sanitize_autofill_value(value))
}

fn extract_candidates_from_result(
    query: &str,
    aliases: &[String],
    result: &SearchResult,
    context_alignment: f32,
) -> Vec<AutofillCandidateDraft> {
    let text = if !result.clean_text.trim().is_empty() {
        result.clean_text.as_str()
    } else {
        result.text.as_str()
    };

    let lines = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    let mut drafts = Vec::new();
    let mut seen = HashSet::new();

    let mut push_draft =
        |value: String, extraction_score: f32, reason: &str, source_snippet: String| {
            let normalized = normalize_autofill_phrase(&value);
            if normalized.is_empty() || !seen.insert(normalized) {
                return;
            }
            drafts.push(AutofillCandidateDraft {
                value,
                extraction_score,
                match_reason: reason.to_string(),
                source_snippet,
                source_app: result.app_name.clone(),
                source_window_title: result.window_title.clone(),
                timestamp: result.timestamp,
                memory_id: result.id.clone(),
                search_score: result.score,
                ocr_confidence: result.ocr_confidence,
                noise_score: result.noise_score,
                context_alignment,
            });
        };

    for (index, line) in lines.iter().enumerate() {
        if let Some((lhs, rhs)) = line.split_once(':').or_else(|| line.split_once('=')) {
            if alias_matches(lhs, aliases) && looks_like_field_value(query, rhs) {
                push_draft(
                    sanitize_autofill_value(rhs),
                    0.97,
                    "Matched a labeled value in a remembered document",
                    line.clone(),
                );
            }
        }

        let cells = split_tableish(line);
        if cells.len() >= 2 {
            for window in cells.windows(2) {
                if alias_matches(&window[0], aliases) && looks_like_field_value(query, &window[1]) {
                    push_draft(
                        sanitize_autofill_value(&window[1]),
                        0.95,
                        "Matched a label-value pair in a remembered document",
                        line.clone(),
                    );
                }
            }
        }

        for alias in aliases {
            if let Some(value) = extract_inline_value(line, alias) {
                if looks_like_field_value(query, &value) {
                    push_draft(
                        value,
                        0.93,
                        "Matched an inline field label in OCR text",
                        line.clone(),
                    );
                }
            }

            if alias_matches(line, std::slice::from_ref(alias)) {
                if let Some(next_line) = lines.iter().skip(index + 1).find(|next| !next.is_empty())
                {
                    if looks_like_field_value(query, next_line) {
                        push_draft(
                            sanitize_autofill_value(next_line),
                            0.88,
                            "Matched a stacked field label and value",
                            format!("{line} / {next_line}"),
                        );
                    }
                }
            }
        }
    }

    for pair in lines.windows(2) {
        let headers = split_tableish(&pair[0]);
        let values = split_tableish(&pair[1]);
        if headers.len() >= 2 && headers.len() == values.len() {
            for (idx, header) in headers.iter().enumerate() {
                if alias_matches(header, aliases) && looks_like_field_value(query, &values[idx]) {
                    push_draft(
                        sanitize_autofill_value(&values[idx]),
                        0.86,
                        "Matched a value from a remembered table",
                        format!("{} / {}", pair[0], pair[1]),
                    );
                }
            }
        }
    }

    // Prose / free-form fallback: structured patterns missed the value.
    // Look for any alias term appearing in a line and extract the trailing value,
    // or take short standalone lines that look like the right type (e.g. bare IDs).
    drop(push_draft);
    if drafts.is_empty() {
        let mut fallback: Vec<AutofillCandidateDraft> = Vec::new();
        let mut fb_seen: HashSet<String> = HashSet::new();
        let mut push_fb =
            |value: String, extraction_score: f32, reason: &str, source_snippet: String| {
                let normalized = normalize_autofill_phrase(&value);
                if normalized.is_empty() || !fb_seen.insert(normalized) {
                    return;
                }
                fallback.push(AutofillCandidateDraft {
                    value,
                    extraction_score,
                    match_reason: reason.to_string(),
                    source_snippet,
                    source_app: result.app_name.clone(),
                    source_window_title: result.window_title.clone(),
                    timestamp: result.timestamp,
                    memory_id: result.id.clone(),
                    search_score: result.score,
                    ocr_confidence: result.ocr_confidence,
                    noise_score: result.noise_score,
                    context_alignment,
                });
            };
        for line in &lines {
            let lower = line.to_ascii_lowercase();
            for alias in aliases {
                let alias_lower = alias.to_ascii_lowercase();
                if lower.contains(&alias_lower) {
                    let after_alias =
                        &line[lower.find(&alias_lower).unwrap_or(0) + alias_lower.len()..];
                    let value: String = after_alias
                        .trim_start_matches(|c: char| matches!(c, ':' | '=' | ' ' | '\t'))
                        .split_whitespace()
                        .take(10)
                        .collect::<Vec<_>>()
                        .join(" ");
                    if looks_like_field_value(query, &value) {
                        push_fb(
                            sanitize_autofill_value(&value),
                            0.65,
                            "Found value near label in memory text",
                            line.clone(),
                        );
                    }
                }
            }

            // Bare structured value on its own line — e.g. "POL-88291-X" or "012-34-5678".
            let cells = split_tableish(line);
            if cells.len() == 1 && line.len() <= 64 && looks_like_field_value(query, line) {
                push_fb(
                    sanitize_autofill_value(line),
                    0.58,
                    "Remembered value matching this field type",
                    line.clone(),
                );
            }
        }
        drop(push_fb);
        drafts.extend(fallback);
    }

    drafts
}

fn context_alignment_score(
    context: &crate::accessibility::FieldContext,
    result: &SearchResult,
    query: &str,
) -> f32 {
    let context_terms = collect_context_terms(context, query);
    if context_terms.is_empty() {
        return 0.0;
    }

    let title_tokens = normalized_tokens(&format!(
        "{} {} {}",
        result.app_name,
        result.window_title,
        result.url.clone().unwrap_or_default()
    ))
    .into_iter()
    .collect::<HashSet<_>>();
    let body_tokens = normalized_tokens(&format!(
        "{} {}",
        truncate_chars(&result.clean_text, 500),
        truncate_chars(&result.snippet, 220)
    ))
    .into_iter()
    .collect::<HashSet<_>>();

    let title_hits = context_terms
        .iter()
        .filter(|term| title_tokens.contains(*term))
        .count() as f32;
    let body_hits = context_terms
        .iter()
        .filter(|term| body_tokens.contains(*term))
        .count() as f32;
    let total = context_terms.len() as f32;

    ((title_hits / total) * 0.68 + (body_hits / total) * 0.32).clamp(0.0, 1.0)
}

fn document_affinity(app_name: &str, window_title: &str) -> f32 {
    let app = app_name.to_ascii_lowercase();
    let window = window_title.to_ascii_lowercase();

    let mut score: f32 = 0.2;
    if [
        "preview", "acrobat", "excel", "numbers", "sheets", "word", "pages",
    ]
    .iter()
    .any(|needle| app.contains(needle))
    {
        score += 0.45;
    }
    if [
        ".pdf",
        ".xlsx",
        ".xls",
        ".csv",
        "statement",
        "invoice",
        "policy",
        "claim",
        "onboarding",
        "application",
        "tax",
        "form",
        "record",
        "spreadsheet",
        "sheet",
    ]
    .iter()
    .any(|needle| window.contains(needle))
    {
        score += 0.3;
    }
    score.clamp(0.0, 1.0)
}

fn value_shape_bonus(query: &str, value: &str) -> f32 {
    let normalized_query = normalize_autofill_phrase(query);
    if !(normalized_query.contains("number")
        || normalized_query.ends_with(" id")
        || normalized_query.contains("ein")
        || normalized_query.contains("routing")
        || normalized_query.contains("policy"))
    {
        return 0.0;
    }

    if value.chars().any(|ch| ch.is_ascii_digit())
        && value
            .chars()
            .any(|ch| ch.is_ascii_uppercase() || matches!(ch, '-' | '/'))
    {
        0.08
    } else if value.chars().any(|ch| ch.is_ascii_digit()) {
        0.04
    } else {
        0.0
    }
}

fn recency_score(timestamp: i64, lookback_days: u32) -> f32 {
    let age_ms = (chrono::Utc::now().timestamp_millis() - timestamp).max(0);
    let lookback_ms = (lookback_days.max(1) as i64) * 86_400_000;
    (1.0 - (age_ms as f32 / lookback_ms as f32)).clamp(0.0, 1.0)
}

fn rank_autofill_candidates(
    query: &str,
    drafts: Vec<AutofillCandidateDraft>,
    lookback_days: u32,
    max_candidates: usize,
) -> Vec<AutofillCandidate> {
    let mut best_by_value: HashMap<String, AutofillCandidate> = HashMap::new();

    for draft in drafts {
        let doc_score = document_affinity(&draft.source_app, &draft.source_window_title);
        let recency = recency_score(draft.timestamp, lookback_days);
        let shape_bonus = value_shape_bonus(query, &draft.value);
        let mut confidence = draft.search_score * 0.28
            + draft.extraction_score * 0.28
            + draft.context_alignment * 0.20
            + draft.ocr_confidence.clamp(0.0, 1.0) * 0.10
            + doc_score * 0.08
            + recency * 0.06
            + shape_bonus;
        confidence -= draft.noise_score.clamp(0.0, 1.0) * 0.08;
        confidence = confidence.clamp(0.0, 0.995);

        let candidate = AutofillCandidate {
            value: draft.value.clone(),
            confidence,
            match_reason: draft.match_reason,
            source_snippet: draft.source_snippet,
            source_app: draft.source_app,
            source_window_title: draft.source_window_title,
            timestamp: draft.timestamp,
            memory_id: draft.memory_id,
        };

        let key = normalize_autofill_phrase(&candidate.value);
        let should_replace = best_by_value
            .get(&key)
            .map(|existing| candidate.confidence > existing.confidence)
            .unwrap_or(true);
        if should_replace {
            best_by_value.insert(key, candidate);
        }
    }

    let mut candidates = best_by_value.into_values().collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .confidence
            .partial_cmp(&left.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| right.timestamp.cmp(&left.timestamp))
    });
    candidates.truncate(max_candidates.max(1));
    candidates
}

fn needs_autofill_confirmation(candidates: &[AutofillCandidate], auto_threshold: f32) -> bool {
    let Some(top) = candidates.first() else {
        return false;
    };

    if top.confidence < auto_threshold {
        return true;
    }

    candidates.get(1).is_some_and(|next| {
        next.confidence >= auto_threshold - 0.03 || (top.confidence - next.confidence) <= 0.05
    })
}

#[tauri::command]
pub async fn get_autofill_settings(
    state: State<'_, Arc<AppState>>,
) -> Result<AutofillConfig, String> {
    Ok(state.inner().config.read().autofill.clone().normalized())
}

#[tauri::command]
pub async fn set_autofill_settings(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    settings: AutofillConfig,
) -> Result<AutofillConfig, String> {
    let mut normalized = settings.normalized();
    let shortcut: Shortcut = normalized.shortcut.parse().map_err(|err| {
        format!(
            "Invalid auto-fill shortcut '{}': {err}",
            normalized.shortcut
        )
    })?;
    normalized.shortcut = shortcut.into_string();

    {
        let mut config = state.inner().config.write();
        config.autofill = normalized.clone();
        config
            .save()
            .map_err(|e: Box<dyn std::error::Error>| e.to_string())?;
    }

    register_autofill_shortcut(&app, &normalized)?;
    Ok(normalized)
}

#[tauri::command]
pub async fn resolve_autofill(
    state: State<'_, Arc<AppState>>,
    context: crate::accessibility::FieldContext,
    query_override: Option<String>,
) -> Result<AutofillResolution, String> {
    let settings = {
        let config = state.inner().config.read();
        config.autofill.clone().normalized()
    };
    let (query, query_source) = build_autofill_query(&context, query_override.as_deref());

    let mut resolution = AutofillResolution {
        query: query.clone(),
        query_source: query_source.clone(),
        context_hint: context_hint(&context),
        candidates: Vec::new(),
        auto_inject_threshold: settings.auto_inject_threshold,
        requires_confirmation: false,
        used_ocr_fallback: query_source == "ocr",
    };

    if query.trim().is_empty() {
        return Ok(resolution);
    }

    let time_filter = format!("{}d", settings.lookback_days);
    let aliases = field_aliases(&query);
    let search_query = build_autofill_search_query(&context, &query);
    let results = run_search_query(
        state.inner(),
        &search_query,
        Some(time_filter.as_str()),
        None,
        settings.max_candidates.max(4) * 3,
    )
    .await?;

    let mut drafts = Vec::new();
    for result in &results {
        let alignment = context_alignment_score(&context, result, &query);
        drafts.extend(extract_candidates_from_result(
            &query, &aliases, result, alignment,
        ));
    }

    resolution.candidates = rank_autofill_candidates(
        &query,
        drafts,
        settings.lookback_days,
        settings.max_candidates,
    );
    resolution.requires_confirmation =
        needs_autofill_confirmation(&resolution.candidates, settings.auto_inject_threshold);

    Ok(resolution)
}

#[tauri::command]
pub async fn inject_text(
    _app: AppHandle,
    state: State<'_, Arc<AppState>>,
    text: String,
) -> Result<(), String> {
    // Do NOT hide the overlay here. The frontend shows "injecting" → "done" / "error"
    // toast states that are only visible if the window stays open during injection.
    // The frontend calls dismissAutofill() after the SUCCESS_TOAST_MS / ERROR_TOAST_MS delay.
    //
    // Move all blocking work (osascript activation, enigo CGEvent posting) off the Tokio thread.
    // CGEvent APIs on macOS can crash or silently fail when called from async runtime threads.
    let prefer_typed = state.inner().config.read().autofill.prefer_typed_injection;
    tokio::task::spawn_blocking(move || {
        crate::accessibility::restore_target_app_focus();
        std::thread::sleep(std::time::Duration::from_millis(60));
        crate::accessibility::inject_text_into_field(&text, prefer_typed)
    })
    .await
    .map_err(|e| format!("Injection task failed to join: {e}"))?
}

/// Hide the autofill overlay window.
#[tauri::command]
pub async fn dismiss_autofill(app: AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window(AUTOFILL_OVERLAY_LABEL) {
        win.hide().map_err(|e| e.to_string())?;
    }
    PENDING_AUTOFILL_PAYLOAD.lock().take();
    crate::accessibility::restore_target_app_focus();
    Ok(())
}

#[tauri::command]
pub async fn quick_setup_ollama(
    state: State<'_, Arc<AppState>>,
) -> Result<HermesBridgeStatus, String> {
    let (installed, reachable, models) = detect_ollama_state().await;
    if !installed {
        return Err("Ollama is not installed on this Mac.".to_string());
    }
    if !reachable {
        return Err(
            "FNDR could not reach Ollama. Make sure Ollama is running (`ollama serve`)."
                .to_string(),
        );
    }
    if models.is_empty() {
        return Err(
            "No Ollama models found. Pull a model first: `ollama pull llama3.2` or `ollama pull qwen2.5-coder`.".to_string(),
        );
    }

    let best_model = models
        .iter()
        .find(|m| {
            let l = m.to_lowercase();
            l.contains("llama3")
                || l.contains("llama-3")
                || l.contains("qwen2.5")
                || l.contains("mistral")
                || l.contains("gemma")
        })
        .or_else(|| models.first())
        .cloned()
        .unwrap_or_else(|| models[0].clone());

    let payload = HermesSetupPayload {
        provider_kind: "ollama".to_string(),
        model_name: best_model,
        api_key: None,
        base_url: Some(OLLAMA_BASE_URL.to_string()),
    };

    persist_hermes_setup_files(state.inner(), &payload)?;
    {
        let mut process_guard = get_hermes_gateway_process().lock();
        if let Some(child) = process_guard.as_mut() {
            let _ = child.kill();
        }
        *process_guard = None;
    }
    *get_hermes_gateway_error_store().lock() = None;
    sync_hermes_bridge_files(state.inner()).await
}

#[cfg(test)]
mod daily_summary_tests {
    use super::*;

    fn result(
        id: &str,
        timestamp: i64,
        app_name: &str,
        window_title: &str,
        snippet: &str,
        url: Option<&str>,
    ) -> SearchResult {
        SearchResult {
            id: id.to_string(),
            timestamp,
            app_name: app_name.to_string(),
            bundle_id: None,
            window_title: window_title.to_string(),
            session_id: "test-session".to_string(),
            text: snippet.to_string(),
            clean_text: snippet.to_string(),
            ocr_confidence: 0.95,
            ocr_block_count: 4,
            snippet: snippet.to_string(),
            summary_source: "fallback".to_string(),
            noise_score: 0.02,
            session_key: "test-session-key".to_string(),
            lexical_shadow: String::new(),
            score: 1.0,
            screenshot_path: None,
            url: url.map(str::to_string),
            decay_score: 1.0,
            ..Default::default()
        }
    }

    #[test]
    fn daily_summary_is_grounded_for_short_capture_span() {
        let start = chrono::Utc::now().timestamp_millis();
        let records = vec![
            result(
                "1",
                start,
                "VS Code",
                "MemoryCardsPanel.tsx",
                "Investigated why all memory cards were not loading.",
                None,
            ),
            result(
                "2",
                start + 10 * 60_000,
                "Discord",
                "FNDR team chat",
                "Checked a short team follow-up.",
                None,
            ),
            result(
                "3",
                start + 30 * 60_000,
                "VS Code",
                "MemoryCardsPanel.tsx",
                "Tested the all-app memory browse flow.",
                None,
            ),
        ];

        let summary = build_daily_activity_summary(&records, "today");
        let lines = summary.lines().collect::<Vec<_>>();

        assert!(summary.contains("about 30 minutes today"));
        assert!(summary.contains("VS Code"));
        assert!(summary.contains("Discord"));
        assert!(!summary.contains("GitLab"));
        assert!(
            lines.len() <= 4,
            "short spans should not force 6-8 bullets: {summary}"
        );
    }

    #[test]
    fn autofill_aliases_expand_common_synonyms() {
        let aliases = field_aliases("Tax ID");

        assert!(aliases
            .iter()
            .any(|alias| alias.eq_ignore_ascii_case("ein")));
        assert!(aliases
            .iter()
            .any(|alias| alias.eq_ignore_ascii_case("employer identification number")));
    }

    #[test]
    fn autofill_extracts_inline_and_table_values() {
        let record = result(
            "autofill-1",
            chrono::Utc::now().timestamp_millis(),
            "Preview",
            "StateFarm_Statement.pdf",
            "Policy Number: POL-88291-X\nGroup Number  8821\nMember Name  Jane Doe",
            None,
        );

        let inline = extract_candidates_from_result(
            "Policy Number",
            &field_aliases("Policy Number"),
            &record,
            1.0,
        );
        assert!(
            inline
                .iter()
                .any(|candidate| candidate.value == "POL-88291-X"),
            "expected inline label-value extraction, got: {inline:?}"
        );

        let table = extract_candidates_from_result(
            "Group Number",
            &field_aliases("Group Number"),
            &record,
            1.0,
        );
        assert!(
            table.iter().any(|candidate| candidate.value == "8821"),
            "expected table-style extraction, got: {table:?}"
        );
    }

    #[test]
    fn autofill_requires_confirmation_when_candidates_are_close() {
        let candidates = vec![
            AutofillCandidate {
                value: "POL-111".to_string(),
                confidence: 0.94,
                match_reason: "Top".to_string(),
                source_snippet: "".to_string(),
                source_app: "Preview".to_string(),
                source_window_title: "A.pdf".to_string(),
                timestamp: 1,
                memory_id: "1".to_string(),
            },
            AutofillCandidate {
                value: "POL-222".to_string(),
                confidence: 0.91,
                match_reason: "Close".to_string(),
                source_snippet: "".to_string(),
                source_app: "Preview".to_string(),
                source_window_title: "B.pdf".to_string(),
                timestamp: 2,
                memory_id: "2".to_string(),
            },
        ];

        assert!(needs_autofill_confirmation(&candidates, 0.90));
    }

    #[test]
    fn focus_task_embedding_is_disabled_without_semantic_backend() {
        assert_eq!(
            build_focus_task_embedding("Finish quarterly planning", None).expect("focus embedding"),
            None
        );
    }

    #[test]
    fn focus_task_embedding_is_present_for_real_backend_when_available() {
        let Ok(embedder) = Embedder::new() else {
            return;
        };
        if !matches!(embedder.backend(), EmbeddingBackend::Real) {
            return;
        }

        let embedding = build_focus_task_embedding("Finish quarterly planning", Some(&embedder))
            .expect("focus embedding");
        assert!(embedding.is_some());
        assert!(embedding
            .as_ref()
            .is_some_and(|vector| vector.iter().any(|value| *value != 0.0)));
    }

    #[test]
    fn list_card_builder_preserves_activity_files_and_duration() {
        let mut source = result(
            "list-1",
            chrono::Utc::now().timestamp_millis(),
            "VS Code",
            "capture/mod.rs",
            "Updated structured extraction validation",
            None,
        );
        source.activity_type = "coding".to_string();
        source.files_touched = vec![
            "src-tauri/src/capture/mod.rs".to_string(),
            "src-tauri/src/api/commands.rs".to_string(),
        ];
        source.session_duration_mins = 37;

        let card = memory_card_from_result(source);
        assert_eq!(card.activity_type, "coding");
        assert_eq!(card.files_touched.len(), 2);
        assert_eq!(card.session_duration_mins, 37);
    }

    #[test]
    fn api_storage_classifier_matches_shared_classifier() {
        let config = crate::memory_quality::default_memory_quality_config();
        let record = MemoryRecord {
            specificity_score: 0.33,
            intent_score: 0.78,
            agent_usefulness_score: 0.64,
            evidence_confidence: 0.28,
            ocr_noise_score: 0.25,
            dedup_fingerprint: "fndr:debug:ocr".to_string(),
            ..Default::default()
        };

        let api_outcome = classify_storage_outcome_with_config(&record, &config);
        let shared_outcome = crate::memory_quality::classify_storage_outcome(&record, &config);
        assert_eq!(api_outcome, shared_outcome);
    }
}
