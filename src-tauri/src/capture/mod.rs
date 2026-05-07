//! Capture pipeline
//!
//! Samples the foreground screen, blocks private contexts before OCR, extracts
//! Apple Vision text, embeds cleaned chunks, and batches memory records into
//! LanceDB.

pub mod clipboard;
mod dedupe;
pub(crate) mod macos;
pub mod permissions;
mod sampling;
pub mod text_cleanup;

pub use dedupe::PerceptualHasher;
pub use sampling::AdaptiveSampler;

/// Convenience wrapper: return just the frontmost app name on macOS.
/// Used by the proactive notification system outside the capture crate.
pub fn macos_frontmost_app_name() -> Option<String> {
    let ctx = macos::get_frontmost_app_info();
    if ctx.app_name == "Unknown" {
        None
    } else {
        Some(ctx.app_name)
    }
}

use crate::config::{DEFAULT_CAPTURE_EMBEDDING_CACHE_SIZE, DEFAULT_IMAGE_EMBEDDING_DIM};
use crate::context_runtime;
use crate::embed::{Embedder, EmbeddingBackend, EMBEDDING_DIM};
use crate::inference::StructuredMemoryExtraction;
use crate::memory_compaction::{
    build_lexical_shadow, compact_summary_embedding_text, mean_pool_embeddings,
    support_embedding_texts,
};
use crate::memory_quality::{deterministic_dedup_fingerprint, is_supported_dedup_fingerprint};
use crate::ocr::{OcrEngine, RecognizedText};
use crate::privacy::Blocklist;
use crate::store::{MemoryRecord, SearchResult, Task, TaskType};
use crate::summariser::narration_filter::clean_or_fallback_display_summary;
use crate::tasks::parse_tasks_from_llm_response;
use crate::telemetry::quality_logger::append_quality_event;
use crate::AppState;
use chrono::{Local, Timelike};
use regex::Regex;
use serde_json::json;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Default)]
struct SemanticDedupWindow {
    seen_at_ms: HashMap<u64, i64>,
}

impl SemanticDedupWindow {
    fn should_skip(&mut self, signature: u64, now_ms: i64, window_ms: i64) -> bool {
        self.seen_at_ms
            .retain(|_, seen_at| now_ms.saturating_sub(*seen_at) <= window_ms);

        if let Some(last_seen) = self.seen_at_ms.get(&signature).copied() {
            if now_ms.saturating_sub(last_seen) <= window_ms {
                self.seen_at_ms.insert(signature, now_ms);
                return true;
            }
        }

        self.seen_at_ms.insert(signature, now_ms);
        false
    }
}

struct EmbeddingMemo {
    capacity: usize,
    order: VecDeque<String>,
    values: HashMap<String, Vec<f32>>,
}

impl EmbeddingMemo {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            order: VecDeque::with_capacity(capacity),
            values: HashMap::with_capacity(capacity),
        }
    }

    fn get(&self, key: &str) -> Option<Vec<f32>> {
        self.values.get(key).cloned()
    }

    fn insert(&mut self, key: String, value: Vec<f32>) {
        if self.values.contains_key(&key) {
            return;
        }
        if self.order.len() >= self.capacity.max(1) {
            if let Some(evicted) = self.order.pop_front() {
                self.values.remove(&evicted);
            }
        }
        self.order.push_back(key.clone());
        self.values.insert(key, value);
    }
}

pub(crate) fn should_skip_capture_context(
    app_name: &str,
    bundle_id: Option<&str>,
    window_title: &str,
    url: Option<&str>,
    blocklist: &[String],
) -> bool {
    Blocklist::is_internal_app(app_name, bundle_id)
        || Blocklist::is_blocked(app_name, blocklist)
        || Blocklist::is_context_blocked(url, Some(window_title), blocklist)
}

fn extract_ocr_text(app_name: &str, ocr_result: &RecognizedText) -> text_cleanup::HighSignalText {
    text_cleanup::build_high_signal_text_for_app(app_name, &ocr_result.text)
}

fn normalize_evidence_text(value: &str) -> String {
    value
        .to_ascii_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn compute_window_title_hash(url: Option<&str>, window_title: &str, timestamp_ms: i64) -> String {
    let mut hasher = DefaultHasher::new();
    url.unwrap_or_default().hash(&mut hasher);
    window_title.hash(&mut hasher);
    timestamp_ms.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptureSurfacePolicy {
    Normal,
    UrlOnly,
    SkipFrame,
}

fn classify_capture_surface_policy(
    app_name: &str,
    window_title: &str,
    url: Option<&str>,
) -> CaptureSurfacePolicy {
    if !is_browser_app(app_name) {
        return CaptureSurfacePolicy::Normal;
    }
    let Some(url) = url else {
        return CaptureSurfacePolicy::Normal;
    };
    let lower_url = url.to_ascii_lowercase();
    let lower_title = window_title.to_ascii_lowercase();
    let domain = extract_domain(url).unwrap_or_default().to_ascii_lowercase();
    let path_query = lower_url
        .split("://")
        .nth(1)
        .unwrap_or(lower_url.as_str())
        .split_once('/')
        .map(|(_, rest)| format!("/{}", rest))
        .unwrap_or_else(|| "/".to_string());

    if is_skip_surface(&domain, &path_query, &lower_title) {
        return CaptureSurfacePolicy::SkipFrame;
    }
    if is_url_only_surface(&domain, &path_query, &lower_title) {
        return CaptureSurfacePolicy::UrlOnly;
    }
    CaptureSurfacePolicy::Normal
}

fn is_browser_app(app_name: &str) -> bool {
    let app = app_name.to_ascii_lowercase();
    matches!(
        app.as_str(),
        value if value.contains("chrome")
            || value.contains("safari")
            || value.contains("firefox")
            || value.contains("arc")
            || value.contains("edge")
            || value.contains("brave")
            || value.contains("opera")
    )
}

fn is_skip_surface(domain: &str, path_query: &str, title: &str) -> bool {
    if domain.contains("youtube.com")
        && (path_query.starts_with("/results?")
            || path_query.starts_with("/feed/")
            || path_query.starts_with("/hashtag/")
            || path_query.contains("search_query="))
    {
        return true;
    }
    if domain.contains("google.") && path_query.starts_with("/search?") && path_query.contains("q=")
    {
        return true;
    }
    if domain.contains("bing.com") && path_query.starts_with("/search?") {
        return true;
    }
    if domain.contains("duckduckgo.com") && path_query.contains("q=") {
        return true;
    }
    if (domain == "x.com" || domain.ends_with(".x.com") || domain.contains("twitter.com"))
        && (path_query.starts_with("/home")
            || path_query.starts_with("/explore")
            || path_query.starts_with("/search"))
    {
        return true;
    }
    if domain.contains("linkedin.com")
        && (path_query.starts_with("/feed") || path_query.starts_with("/search/results"))
    {
        return true;
    }
    title.contains("new tab") || title.contains("start page")
}

fn is_url_only_surface(domain: &str, path_query: &str, title: &str) -> bool {
    if domain.contains("youtube.com")
        && ((path_query.starts_with("/@")
            || path_query.starts_with("/channel/")
            || path_query.starts_with("/c/")
            || path_query.starts_with("/user/"))
            && path_query.contains("/videos"))
    {
        return true;
    }
    if domain.contains("youtube.com")
        && (path_query.starts_with("/@")
            || path_query.starts_with("/channel/")
            || path_query.starts_with("/c/")
            || path_query.starts_with("/user/"))
    {
        return true;
    }
    if domain.contains("reddit.com")
        && (path_query.starts_with("/r/")
            || path_query.starts_with("/search")
            || path_query.starts_with("/best")
            || path_query.starts_with("/top"))
    {
        return true;
    }
    if (domain == "x.com" || domain.ends_with(".x.com") || domain.contains("twitter.com"))
        && path_query.starts_with("/@")
    {
        return true;
    }
    title.contains("search results") || title.contains("videos - youtube")
}

fn entity_regex() -> &'static Regex {
    static ENTITY_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    ENTITY_RE.get_or_init(|| {
        Regex::new(r"\b(?:[A-Z][a-z0-9]+(?:\s+[A-Z][a-z0-9]+){0,2}|[A-Z]{2,}(?:\s+[A-Z]{2,})?)\b")
            .expect("valid entity regex")
    })
}

fn lightweight_entities_from_text(text: &str) -> Vec<String> {
    const STOP_ENTITIES: &[&str] = &[
        "The",
        "This",
        "That",
        "And",
        "For",
        "With",
        "From",
        "You",
        "Your",
        "Google Chrome",
        "Safari",
        "YouTube",
        "Page",
        "Menu",
        "Home",
        "Search",
        "Results",
    ];
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for cap in entity_regex().find_iter(text) {
        let value = cap.as_str().trim();
        if value.len() < 3 || value.len() > 48 {
            continue;
        }
        if STOP_ENTITIES
            .iter()
            .any(|item| item.eq_ignore_ascii_case(value))
        {
            continue;
        }
        let key = value.to_ascii_lowercase();
        if seen.insert(key) {
            out.push(value.to_string());
        }
        if out.len() >= 12 {
            break;
        }
    }
    out
}

fn build_structured_from_browser_semantics(
    _app_name: &str,
    window_title: &str,
    url: Option<&str>,
    semantic: &macos::BrowserSemanticContent,
) -> Option<StructuredMemoryExtraction> {
    if !semantic.has_signal() {
        return None;
    }
    let content_text = semantic.content_text();
    if content_text.trim().is_empty() {
        return None;
    }
    let mut entities = lightweight_entities_from_text(&content_text);
    if let Some(domain) = url.and_then(extract_domain) {
        if !entities
            .iter()
            .any(|value| value.eq_ignore_ascii_case(&domain))
        {
            entities.push(domain);
        }
    }
    let topic = if !semantic.h1.trim().is_empty() {
        semantic.h1.trim().to_string()
    } else if !window_title.trim().is_empty() {
        window_title.trim().to_string()
    } else {
        semantic.title.trim().to_string()
    };
    let mut memory_context = String::new();
    if !topic.is_empty() {
        memory_context.push_str(&topic);
    }
    if !semantic.meta_description.trim().is_empty() {
        if !memory_context.is_empty() {
            memory_context.push_str(". ");
        }
        memory_context.push_str(semantic.meta_description.trim());
    }
    if memory_context.trim().is_empty() {
        memory_context = content_text
            .split_terminator(['.', '!', '?'])
            .next()
            .unwrap_or_default()
            .trim()
            .to_string();
    }
    Some(StructuredMemoryExtraction {
        activity_type: "research".to_string(),
        project: String::new(),
        topic,
        memory_context,
        workflow: "researching".to_string(),
        user_intent: "researching".to_string(),
        entities,
        search_aliases: Vec::new(),
        confidence: semantic.content_signal_score.clamp(0.35, 0.90),
        dedup_fingerprint: String::new(),
        ..Default::default()
    })
}

fn field_supported_by_evidence(field: &str, evidence_norm: &str) -> bool {
    let normalized_field = normalize_evidence_text(field);
    let terms = normalized_field
        .split_whitespace()
        .filter(|term| term.len() >= 3)
        .filter(|term| !matches!(*term, "unknown" | "none" | "null"))
        .collect::<Vec<_>>();
    if terms.is_empty() {
        return true;
    }
    let matched = terms
        .iter()
        .filter(|term| evidence_norm.contains(**term))
        .count();
    let ratio = matched as f32 / terms.len() as f32;
    matched >= 1 && ratio >= 0.34
}

fn strip_unsupported_values(
    values: &mut Vec<String>,
    evidence_norm: &str,
    issues: &mut Vec<String>,
    issue_label: &str,
) {
    let mut kept = Vec::new();
    let mut removed = 0usize;
    for value in values.iter() {
        if field_supported_by_evidence(value, evidence_norm) {
            kept.push(value.clone());
        } else {
            removed += 1;
        }
    }
    if removed > 0 {
        issues.push(format!("{issue_label}:{removed}"));
    }
    *values = kept;
}

fn validate_structured_memory_extraction(
    extraction: &mut StructuredMemoryExtraction,
    app_name: &str,
    window_title: &str,
    clean_text: &str,
) -> (f32, Vec<String>) {
    let evidence_norm = normalize_evidence_text(&format!("{app_name} {window_title} {clean_text}"));
    let mut issues = Vec::new();
    let mut supported = 0usize;
    let mut total = 0usize;

    let mut maybe_scrub = |value: &mut String, label: &str| {
        if value.trim().is_empty() {
            return;
        }
        total += 1;
        if field_supported_by_evidence(value, &evidence_norm) {
            supported += 1;
        } else {
            issues.push(format!("unsupported_{label}"));
            if extraction.confidence < 0.8 {
                value.clear();
            }
        }
    };

    maybe_scrub(&mut extraction.project, "project");
    maybe_scrub(&mut extraction.topic, "topic");
    maybe_scrub(&mut extraction.workflow, "workflow");
    maybe_scrub(&mut extraction.user_intent, "intent");
    maybe_scrub(&mut extraction.memory_context, "memory_context");
    maybe_scrub(&mut extraction.outcome, "outcome");

    total += extraction.entities.len()
        + extraction.files_touched.len()
        + extraction.search_aliases.len();
    strip_unsupported_values(
        &mut extraction.entities,
        &evidence_norm,
        &mut issues,
        "unsupported_entities",
    );
    strip_unsupported_values(
        &mut extraction.files_touched,
        &evidence_norm,
        &mut issues,
        "unsupported_files",
    );
    strip_unsupported_values(
        &mut extraction.search_aliases,
        &evidence_norm,
        &mut issues,
        "unsupported_aliases",
    );

    supported += extraction.entities.len()
        + extraction.files_touched.len()
        + extraction.search_aliases.len();
    if !is_supported_dedup_fingerprint(&extraction.dedup_fingerprint) {
        if !extraction.dedup_fingerprint.trim().is_empty() {
            issues.push("unsupported_dedup_fingerprint".to_string());
        }
        extraction.dedup_fingerprint.clear();
    }

    let support_ratio = if total == 0 {
        0.0
    } else {
        supported as f32 / total as f32
    };
    let grounding_confidence =
        (support_ratio * 0.72 + extraction.confidence.clamp(0.0, 1.0) * 0.28).clamp(0.0, 1.0);

    if grounding_confidence < 0.55 {
        issues.push("structured_fields_weakly_grounded".to_string());
    }
    if grounding_confidence < 0.8 {
        issues.push("possible_ungrounded_extraction".to_string());
    }

    extraction.confidence = extraction.confidence.clamp(0.0, 1.0);
    (grounding_confidence, issues)
}

fn build_grounded_memory_context(
    extraction: Option<&StructuredMemoryExtraction>,
    app_name: &str,
    window_title: &str,
    clean_text: &str,
    display_summary: &str,
) -> String {
    if let Some(mem) = extraction {
        if !mem.memory_context.trim().is_empty() {
            return mem.memory_context.trim().to_string();
        }
        let mut parts = Vec::new();
        if !mem.user_intent.trim().is_empty() {
            parts.push(format!("You were {}.", mem.user_intent.trim()));
        }
        if !mem.project.trim().is_empty() {
            parts.push(format!("This was work on {}.", mem.project.trim()));
        } else if !mem.topic.trim().is_empty() {
            parts.push(format!("Topic: {}.", mem.topic.trim()));
        }
        if !mem.files_touched.is_empty() {
            parts.push(format!(
                "Files involved: {}.",
                mem.files_touched
                    .iter()
                    .take(4)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !mem.next_steps.is_empty() {
            parts.push(format!(
                "Next steps: {}.",
                mem.next_steps
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
        if !parts.is_empty() {
            return parts.join(" ");
        }
    }

    let fallback = text_cleanup::concise_fallback_snippet(app_name, window_title, clean_text);
    if !fallback.trim().is_empty() {
        fallback
    } else if !display_summary.trim().is_empty() {
        display_summary.trim().to_string()
    } else {
        clean_text.chars().take(240).collect::<String>()
    }
}

fn compose_primary_embedding_text(
    extraction: Option<&StructuredMemoryExtraction>,
    app_name: &str,
    window_title: &str,
    memory_context: &str,
    display_summary: &str,
    clean_text: &str,
    lexical_shadow: &str,
) -> String {
    let mut segments = Vec::new();
    if let Some(mem) = extraction {
        if !mem.user_intent.trim().is_empty() {
            segments.push(format!("intent: {}", mem.user_intent.trim()));
        }
        if !mem.project.trim().is_empty() {
            segments.push(format!("project: {}", mem.project.trim()));
        }
        if !mem.topic.trim().is_empty() {
            segments.push(format!("topic: {}", mem.topic.trim()));
        }
        if !mem.workflow.trim().is_empty() {
            segments.push(format!("workflow: {}", mem.workflow.trim()));
        }
        if !mem.entities.is_empty() {
            segments.push(format!(
                "entities: {}",
                mem.entities
                    .iter()
                    .take(8)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !mem.files_touched.is_empty() {
            segments.push(format!(
                "files: {}",
                mem.files_touched
                    .iter()
                    .take(8)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !mem.results.is_empty() {
            segments.push(format!(
                "results: {}",
                mem.results
                    .iter()
                    .take(4)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
    }
    if !memory_context.trim().is_empty() {
        segments.push(format!("context: {}", memory_context.trim()));
    }
    if !display_summary.trim().is_empty() {
        segments.push(format!("summary: {}", display_summary.trim()));
    }
    segments.push(format!(
        "app: {} | window: {}",
        app_name.trim(),
        window_title.trim()
    ));
    let evidence_tail = clean_text.chars().take(320).collect::<String>();
    if !evidence_tail.trim().is_empty() {
        segments.push(format!("evidence: {}", evidence_tail.trim()));
    }
    let shadow = lexical_shadow.chars().take(220).collect::<String>();
    if !shadow.trim().is_empty() {
        segments.push(format!("shadow: {}", shadow.trim()));
    }
    segments.join("\n")
}

fn weighted_primary_embedding(primary: &[f32], snippet: &[f32], support: &[f32]) -> Vec<f32> {
    let dim = primary
        .len()
        .max(snippet.len())
        .max(support.len())
        .max(EMBEDDING_DIM);
    let mut out = vec![0.0f32; dim];
    for i in 0..dim {
        let p = primary.get(i).copied().unwrap_or(0.0);
        let s = snippet.get(i).copied().unwrap_or(0.0);
        let u = support.get(i).copied().unwrap_or(0.0);
        out[i] = p * 0.62 + s * 0.23 + u * 0.15;
    }
    normalize_embedding_vector(&mut out);
    out
}

fn normalize_embedding_vector(vector: &mut [f32]) {
    let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm <= 1e-6 {
        return;
    }
    for value in vector {
        *value /= norm;
    }
}

fn emit_capture_quality_signal(state: &AppState, payload: serde_json::Value) {
    let _ = append_quality_event(
        state.app_data_dir.as_path(),
        "signals.jsonl",
        &json!({
            "kind": "Capture",
            "payload": payload
        }),
    );
}

fn emit_extraction_quality_anomaly(state: &AppState, payload: serde_json::Value) {
    let _ = append_quality_event(
        state.app_data_dir.as_path(),
        "anomalies.jsonl",
        &json!({
            "kind": "Extraction",
            "payload": payload
        }),
    );
}

/// Run the main capture loop
pub async fn run_capture_loop(state: Arc<AppState>) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("Initializing capture pipeline...");

    // Initialize components
    let mut hasher = PerceptualHasher::new();
    let sampler = AdaptiveSampler::new();
    let ocr = OcrEngine::new()?;
    let text_embedder = match Embedder::new() {
        Ok(embedder) => Some(embedder),
        Err(err) => {
            tracing::warn!("Semantic embeddings unavailable in capture loop: {}", err);
            None
        }
    };
    let initial_capture_config = state.config.read().capture_pipeline.clone();

    // Batch buffer
    let mut batch: Vec<MemoryRecord> = Vec::new();
    let mut continuity_index: HashMap<String, String> = HashMap::new();
    let mut last_flush = Instant::now();

    // Force capture timer
    let mut last_forced_capture = Instant::now();

    // Semantic dedup window suppresses repeated unchanged content bursts.
    let mut semantic_window = SemanticDedupWindow::default();
    let mut embedding_memo = EmbeddingMemo::new(
        initial_capture_config
            .embedding_cache_size
            .max(DEFAULT_CAPTURE_EMBEDDING_CACHE_SIZE),
    );

    tracing::info!("Capture loop started");

    loop {
        let config = state.config.read().clone();
        let flush_interval = Duration::from_secs(config.capture_pipeline.flush_interval_secs);
        let max_batch_size = config.capture_pipeline.max_batch_size;

        // Flush batch if needed
        let should_flush = batch.len() >= max_batch_size || last_flush.elapsed() >= flush_interval;
        if should_flush && !batch.is_empty() {
            batch.retain(|record| {
                !Blocklist::is_blocked(&record.app_name, &config.blocklist)
                    && !Blocklist::is_context_blocked(
                        record.url.as_deref(),
                        Some(&record.window_title),
                        &config.blocklist,
                    )
            });
            if batch.is_empty() {
                purge_capture_artifacts(state.store.frames_dir());
                last_flush = Instant::now();
                continue;
            }

            let flush_start = Instant::now();
            if let Err(e) = state.store.add_batch(&batch).await {
                tracing::error!("Failed to flush batch: {}", e);
            } else {
                if let Err(err) =
                    context_runtime::sync_memory_records(state.as_ref(), &batch, Some("screen"))
                        .await
                {
                    tracing::warn!("Context runtime batch sync failed: {}", err);
                }
                purge_capture_artifacts(state.store.frames_dir());
                state.invalidate_memory_derived_caches();
                tracing::info!(
                    "Flushed {} records in {:?}",
                    batch.len(),
                    flush_start.elapsed()
                );
            }
            batch.clear();
            last_flush = Instant::now();
        }

        // Check if paused
        if !state.is_capturing() {
            tokio::time::sleep(Duration::from_millis(500)).await;
            continue;
        }

        // Calculate sleep duration based on FPS
        let fps = sampler.get_current_fps(&config);
        if fps <= 0.0 {
            tokio::time::sleep(Duration::from_secs(1)).await;
            continue;
        }
        let sleep_duration = Duration::from_secs_f64(1.0 / fps);

        // Get active application info
        let app_context = macos::get_frontmost_app_info();
        let app_name = app_context.app_name.clone();
        let window_title = app_context.window_title.clone();
        let force_capture =
            last_forced_capture.elapsed().as_secs() >= config.forced_capture_interval;

        let url = macos::get_browser_url(&app_name);
        if let Some(ref u) = url {
            tracing::info!("Frontmost browser URL: {}", u);
        }

        if should_skip_capture_context(
            &app_name,
            app_context.bundle_id.as_deref(),
            &window_title,
            url.as_deref(),
            &config.blocklist,
        ) {
            tracing::debug!(
                "Skipping capture for blocklisted context: app='{}' title='{}' url={:?}",
                app_name,
                window_title,
                url
            );
            tokio::time::sleep(sleep_duration).await;
            continue;
        }

        let surface_policy =
            classify_capture_surface_policy(&app_name, &window_title, url.as_deref());
        if surface_policy == CaptureSurfacePolicy::SkipFrame {
            emit_capture_quality_signal(
                state.as_ref(),
                json!({
                    "timestamp_ms": chrono::Utc::now().timestamp_millis(),
                    "app_name": app_name,
                    "bundle_id": app_context.bundle_id.clone(),
                    "domain": url.as_deref().and_then(extract_domain).unwrap_or_default(),
                    "surface_policy": "skip_frame",
                    "low_signal": true,
                    "stored_or_skipped": "skipped_surface_policy",
                }),
            );
            tokio::time::sleep(sleep_duration).await;
            continue;
        }
        if surface_policy == CaptureSurfacePolicy::UrlOnly {
            let mut h = DefaultHasher::new();
            app_name.hash(&mut h);
            window_title.hash(&mut h);
            url.hash(&mut h);
            let semantic_hash = h.finish();
            let now_ms = chrono::Utc::now().timestamp_millis();
            if semantic_window.should_skip(
                semantic_hash,
                now_ms,
                config.capture_pipeline.semantic_dedup_window_ms,
            ) && !force_capture
            {
                tokio::time::sleep(sleep_duration).await;
                continue;
            }
            let now = Local::now();
            let session_key = build_session_key(&app_name, &window_title, url.as_deref());
            let session_id = build_session_id(
                &now,
                &app_name,
                app_context.bundle_id.as_deref(),
                &session_key,
            );
            let domain = url
                .as_deref()
                .and_then(extract_domain)
                .unwrap_or_else(|| "unknown_domain".to_string());
            let snippet = if !window_title.trim().is_empty() {
                window_title.trim().to_string()
            } else {
                format!("Visited {}", domain)
            };
            let memory_context = format!("URL-only surface capture for {} at {}", domain, snippet);
            let mut record = MemoryRecord {
                id: uuid::Uuid::new_v4().to_string(),
                timestamp: now.timestamp_millis(),
                day_bucket: now.format("%Y-%m-%d").to_string(),
                app_name: app_name.clone(),
                bundle_id: app_context.bundle_id.clone(),
                window_title: window_title.clone(),
                session_id,
                text: String::new(),
                clean_text: String::new(),
                ocr_confidence: 0.0,
                ocr_block_count: 0,
                snippet: snippet.clone(),
                display_summary: snippet.clone(),
                internal_context: memory_context.clone(),
                summary_source: "url_only".to_string(),
                noise_score: 0.0,
                session_key,
                lexical_shadow: build_lexical_shadow(&window_title, &snippet, "", url.as_deref()),
                embedding: vec![0.0; EMBEDDING_DIM],
                image_embedding: vec![0.0; DEFAULT_IMAGE_EMBEDDING_DIM],
                screenshot_path: None,
                url: url.clone(),
                snippet_embedding: vec![0.0; EMBEDDING_DIM],
                support_embedding: vec![0.0; EMBEDDING_DIM],
                decay_score: 1.0,
                last_accessed_at: 0,
                timestamp_start: now.timestamp_millis(),
                timestamp_end: now.timestamp_millis(),
                source_type: "browser_url_only".to_string(),
                topic: "navigation_surface".to_string(),
                workflow: "browsing".to_string(),
                user_intent: "navigating".to_string(),
                memory_context: memory_context.clone(),
                raw_evidence: json!({
                    "surface_policy": "url_only",
                    "timestamp_ms": now.timestamp_millis(),
                    "app_name": app_name,
                    "window_title": window_title,
                    "url": url,
                })
                .to_string(),
                schema_version: 2,
                activity_type: "browsing".to_string(),
                embedding_text: format!("url: {} | title: {}", domain, snippet),
                embedding_model: "bge-large-en-v1.5".to_string(),
                embedding_dim: EMBEDDING_DIM as u32,
                ..Default::default()
            };
            record.dedup_fingerprint =
                deterministic_dedup_fingerprint(&record, Some(&record.memory_context));
            batch.push(record);
            if force_capture {
                last_forced_capture = Instant::now();
            }
            emit_capture_quality_signal(
                state.as_ref(),
                json!({
                    "timestamp_ms": now.timestamp_millis(),
                    "app_name": app_name,
                    "bundle_id": app_context.bundle_id.clone(),
                    "domain": domain,
                    "surface_policy": "url_only",
                    "low_signal": false,
                    "stored_or_skipped": "stored_url_only_surface",
                    "grounding_confidence": 0.0
                }),
            );
            state.frames_captured.fetch_add(1, Ordering::Relaxed);
            state
                .last_capture_time
                .store(now.timestamp_millis() as u64, Ordering::Relaxed);
            tokio::time::sleep(sleep_duration).await;
            continue;
        }

        // Capture screen
        let capture_result = macos::capture_screen();
        let image_data = match capture_result {
            Ok(data) => data,
            Err(e) => {
                tracing::warn!("Screen capture failed: {}", e);
                tokio::time::sleep(sleep_duration).await;
                continue;
            }
        };

        // Deduplication check
        let is_duplicate = hasher.is_duplicate(&image_data, config.dedupe_threshold);

        if is_duplicate && !force_capture {
            state.frames_dropped.fetch_add(1, Ordering::Relaxed);
            tokio::time::sleep(sleep_duration).await;
            continue;
        }

        tracing::info!("Processing new frame from {}", app_name);

        if force_capture {
            last_forced_capture = Instant::now();
        }

        let semantic_page = if surface_policy == CaptureSurfacePolicy::Normal {
            macos::get_browser_semantic_content(&app_name)
        } else {
            None
        };
        if let Some(page) = semantic_page.as_ref() {
            if page.nav_ratio > 0.58 && page.content_signal_score < 0.18 {
                emit_capture_quality_signal(
                    state.as_ref(),
                    json!({
                        "timestamp_ms": chrono::Utc::now().timestamp_millis(),
                        "app_name": app_name,
                        "bundle_id": app_context.bundle_id.clone(),
                        "domain": url.as_deref().and_then(extract_domain).unwrap_or_default(),
                        "surface_policy": "skip_frame",
                        "source_kind": "browser_semantic",
                        "semantic_nav_ratio": page.nav_ratio,
                        "semantic_content_score": page.content_signal_score,
                        "stored_or_skipped": "skipped_surface_policy",
                        "low_signal": true
                    }),
                );
                tokio::time::sleep(sleep_duration).await;
                continue;
            }
        }
        let mut source_kind = "ocr";
        let mut source_low_signal = false;
        let ocr_start = Instant::now();
        let (text, qwen_cleaned_text, capture_quality, observed_confidence, observed_block_count) =
            if let Some(semantic) = semantic_page.as_ref().filter(|page| page.has_signal()) {
                source_kind = "browser_semantic";
                let semantic_text = semantic.content_text();
                let high_signal =
                    text_cleanup::build_high_signal_text_for_app(&app_name, &semantic_text);
                let mut stats = high_signal.stats.clone();
                if stats.total_lines == 0 {
                    stats.total_lines = 1;
                }
                if stats.kept_lines == 0 && !high_signal.text.trim().is_empty() {
                    stats.kept_lines = 1;
                }
                stats.low_conf_lines = 0;
                stats.avg_line_score = (0.68 + semantic.content_signal_score * 0.28
                    - semantic.nav_ratio * 0.18)
                    .clamp(0.0, 1.0);
                (
                    high_signal.text.clone(),
                    high_signal.text,
                    stats,
                    (0.62 + semantic.content_signal_score * 0.30 - semantic.nav_ratio * 0.12)
                        .clamp(0.0, 1.0),
                    high_signal.stats.kept_lines.max(1),
                )
            } else {
                let (ocr_result, qwen_cleaned) = match ocr.recognize_with_metadata(&image_data) {
                    Ok(result) => result,
                    Err(e) => {
                        tracing::warn!("OCR failed: {}", e);
                        tokio::time::sleep(sleep_duration).await;
                        continue;
                    }
                };
                source_low_signal = ocr_result.is_low_signal(config.min_text_length);
                let high_signal = extract_ocr_text(&app_name, &ocr_result);
                (
                    high_signal.text.clone(),
                    qwen_cleaned,
                    high_signal.stats,
                    ocr_result.confidence,
                    ocr_result.block_count,
                )
            };
        let ocr_latency = ocr_start.elapsed();
        tracing::info!(
            "Capture text source={} chars={} latency_ms={} confidence={:.2} blocks={}",
            source_kind,
            text.len(),
            ocr_latency.as_millis(),
            observed_confidence,
            observed_block_count
        );

        // Skip if source output is too weak/noisy to improve recall.
        if source_low_signal {
            emit_capture_quality_signal(
                state.as_ref(),
                json!({
                    "timestamp_ms": chrono::Utc::now().timestamp_millis(),
                    "app_name": app_name,
                    "bundle_id": app_context.bundle_id.clone(),
                    "ocr_confidence": observed_confidence,
                    "ocr_block_count": observed_block_count,
                    "clean_text_len": text.len(),
                    "noise_score": 1.0,
                    "low_signal": true,
                    "stored_or_skipped": "skipped_low_signal",
                    "source_kind": source_kind,
                }),
            );
            tokio::time::sleep(sleep_duration).await;
            continue;
        }
        if text.len() < config.min_text_length {
            emit_capture_quality_signal(
                state.as_ref(),
                json!({
                    "timestamp_ms": chrono::Utc::now().timestamp_millis(),
                    "app_name": app_name,
                    "bundle_id": app_context.bundle_id.clone(),
                    "ocr_confidence": observed_confidence,
                    "ocr_block_count": observed_block_count,
                    "clean_text_len": text.len(),
                    "noise_score": 1.0,
                    "low_signal": true,
                    "stored_or_skipped": "skipped_low_signal",
                    "source_kind": source_kind,
                    "quality_stats": {
                        "total_lines": capture_quality.total_lines,
                        "kept_lines": capture_quality.kept_lines,
                        "low_conf_lines": capture_quality.low_conf_lines,
                        "dropped_noise_lines": capture_quality.dropped_noise_lines,
                        "dropped_low_signal_lines": capture_quality.dropped_low_signal_lines,
                        "avg_line_score": capture_quality.avg_line_score
                    }
                }),
            );
            tokio::time::sleep(sleep_duration).await;
            continue;
        }
        let noise_score = text_cleanup::estimate_noise_score(&app_name, &text);
        let keep_ratio = if capture_quality.total_lines == 0 {
            0.0
        } else {
            capture_quality.kept_lines as f32 / capture_quality.total_lines as f32
        };
        if capture_quality.avg_line_score < 0.30 || (keep_ratio < 0.12 && text.len() < 220) {
            emit_capture_quality_signal(
                state.as_ref(),
                json!({
                    "timestamp_ms": chrono::Utc::now().timestamp_millis(),
                    "app_name": app_name,
                    "bundle_id": app_context.bundle_id.clone(),
                    "ocr_confidence": observed_confidence,
                    "ocr_block_count": observed_block_count,
                    "clean_text_len": text.len(),
                    "noise_score": noise_score,
                    "low_signal": true,
                    "stored_or_skipped": "skipped_low_signal",
                    "source_kind": source_kind,
                    "quality_stats": {
                        "total_lines": capture_quality.total_lines,
                        "kept_lines": capture_quality.kept_lines,
                        "avg_line_score": capture_quality.avg_line_score,
                        "keep_ratio": keep_ratio
                    }
                }),
            );
            tokio::time::sleep(sleep_duration).await;
            continue;
        }
        if noise_score > config.capture_pipeline.noise_skip_threshold {
            emit_capture_quality_signal(
                state.as_ref(),
                json!({
                    "timestamp_ms": chrono::Utc::now().timestamp_millis(),
                    "app_name": app_name,
                    "bundle_id": app_context.bundle_id.clone(),
                    "ocr_confidence": observed_confidence,
                    "ocr_block_count": observed_block_count,
                    "clean_text_len": text.len(),
                    "noise_score": noise_score,
                    "low_signal": false,
                    "stored_or_skipped": "skipped_noise",
                    "source_kind": source_kind,
                    "quality_stats": {
                        "total_lines": capture_quality.total_lines,
                        "kept_lines": capture_quality.kept_lines,
                        "avg_line_score": capture_quality.avg_line_score
                    }
                }),
            );
            tokio::time::sleep(sleep_duration).await;
            continue;
        }

        // ── Semantic dedup ────────────────────────────────────────────────
        // Hash (app_name, window_title, clean_text). If the hash is
        // identical to the previous frame, the user is staring at the
        // same content (blinking cursor, ticking clock, etc.).  Skip the
        // entire LLM → VLM → embedding pipeline to save CPU/battery.
        {
            let mut h = DefaultHasher::new();
            app_name.hash(&mut h);
            window_title.hash(&mut h);
            text.hash(&mut h);
            let semantic_hash = h.finish();
            let now_ms = chrono::Utc::now().timestamp_millis();
            if semantic_window.should_skip(
                semantic_hash,
                now_ms,
                config.capture_pipeline.semantic_dedup_window_ms,
            ) && !force_capture
            {
                tracing::debug!("Semantic dedup: identical content, skipping pipeline");
                state.frames_dropped.fetch_add(1, Ordering::Relaxed);
                tokio::time::sleep(sleep_duration).await;
                continue;
            }
        }

        // Summarize each persisted memory with the local AI model when available.
        let engine = if let Some(engine) = state.inference_engine() {
            Some(engine)
        } else {
            match state.ensure_inference_engine().await {
                Ok(engine) => engine,
                Err(err) => {
                    tracing::warn!(
                        "Failed to initialize inference engine in capture loop: {}",
                        err
                    );
                    None
                }
            }
        };

        let browser_structured_seed = semantic_page.as_ref().and_then(|page| {
            build_structured_from_browser_semantics(&app_name, &window_title, url.as_deref(), page)
        });
        let mut structured_memory = if let Some(engine) = engine.as_ref() {
            engine
                .extract_structured_memory(&app_name, &window_title, &qwen_cleaned_text)
                .await
        } else {
            None
        };
        if structured_memory.is_none() {
            structured_memory = browser_structured_seed.clone();
        } else if let (Some(existing), Some(seed)) =
            (structured_memory.as_mut(), browser_structured_seed.as_ref())
        {
            if existing.memory_context.trim().is_empty() {
                existing.memory_context = seed.memory_context.clone();
            }
            if existing.topic.trim().is_empty() {
                existing.topic = seed.topic.clone();
            }
            if existing.user_intent.trim().is_empty() {
                existing.user_intent = seed.user_intent.clone();
            }
            if existing.activity_type.trim().is_empty() {
                existing.activity_type = seed.activity_type.clone();
            }
            if existing.entities.is_empty() {
                existing.entities = seed.entities.clone();
            }
            if existing.confidence < 0.55 {
                existing.confidence = existing
                    .confidence
                    .max((seed.confidence * 0.9).clamp(0.0, 1.0));
            }
        }
        let (extraction_grounding_confidence, extraction_issues) =
            if let Some(memory) = structured_memory.as_mut() {
                validate_structured_memory_extraction(memory, &app_name, &window_title, &text)
            } else {
                (0.0, vec!["structured_extraction_unavailable".to_string()])
            };
        if !extraction_issues.is_empty() {
            emit_extraction_quality_anomaly(
                state.as_ref(),
                json!({
                    "timestamp_ms": chrono::Utc::now().timestamp_millis(),
                    "record_id": uuid::Uuid::new_v4().to_string(),
                    "schema_version": 2,
                    "activity_type": structured_memory
                        .as_ref()
                        .map(|m| m.activity_type.clone())
                        .unwrap_or_else(|| "unknown".to_string()),
                    "project_present": structured_memory
                        .as_ref()
                        .map(|m| !m.project.trim().is_empty())
                        .unwrap_or(false),
                    "summary_present": structured_memory
                        .as_ref()
                        .map(|m| !m.memory_context.trim().is_empty())
                        .unwrap_or(false),
                    "files_touched_count": structured_memory
                        .as_ref()
                        .map(|m| m.files_touched.len())
                        .unwrap_or(0),
                    "symbols_changed_count": structured_memory
                        .as_ref()
                        .map(|m| m.symbols_changed.len())
                        .unwrap_or(0),
                    "errors_count": structured_memory
                        .as_ref()
                        .map(|m| m.errors.len())
                        .unwrap_or(0),
                    "next_steps_count": structured_memory
                        .as_ref()
                        .map(|m| m.next_steps.len())
                        .unwrap_or(0),
                    "extraction_confidence": structured_memory
                        .as_ref()
                        .map(|m| m.confidence)
                        .unwrap_or(0.0),
                    "parse_failed": structured_memory.is_none(),
                    "grounding_confidence": extraction_grounding_confidence,
                    "context_conflict": structured_memory
                        .as_ref()
                        .map(|m| m.memory_context.trim().is_empty() && !m.topic.trim().is_empty())
                        .unwrap_or(false),
                    "anomaly_labels": extraction_issues,
                    "app_name": app_name
                }),
            );
        }
        if extraction_grounding_confidence <= 0.10 {
            emit_capture_quality_signal(
                state.as_ref(),
                json!({
                    "timestamp_ms": chrono::Utc::now().timestamp_millis(),
                    "app_name": app_name,
                    "bundle_id": app_context.bundle_id.clone(),
                    "domain": url.as_deref().and_then(extract_domain).unwrap_or_default(),
                    "ocr_confidence": observed_confidence,
                    "ocr_block_count": observed_block_count,
                    "clean_text_len": text.len(),
                    "noise_score": noise_score,
                    "low_signal": false,
                    "surface_policy": "normal",
                    "source_kind": source_kind,
                    "stored_or_skipped": "skipped_grounding_gate",
                    "grounding_confidence": extraction_grounding_confidence,
                    "quality_stats": {
                        "total_lines": capture_quality.total_lines,
                        "kept_lines": capture_quality.kept_lines,
                        "low_conf_lines": capture_quality.low_conf_lines,
                        "dropped_noise_lines": capture_quality.dropped_noise_lines,
                        "dropped_low_signal_lines": capture_quality.dropped_low_signal_lines,
                        "avg_line_score": capture_quality.avg_line_score,
                    }
                }),
            );
            tokio::time::sleep(sleep_duration).await;
            continue;
        }

        let vlm_analysis: Option<String> = None;

        let (final_snippet, summary_source) = if let Some(ref mem) = structured_memory {
            let candidate = if !mem.memory_context.trim().is_empty() {
                mem.memory_context
                    .split_terminator(['.', '!', '?'])
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .to_string()
            } else {
                mem.topic.trim().to_string()
            };
            (
                candidate,
                if source_kind == "browser_semantic" {
                    "browser_semantic".to_string()
                } else {
                    "llm".to_string()
                },
            )
        } else if let Some(ref vlm_text) = vlm_analysis {
            (vlm_text.clone(), "vlm".to_string())
        } else {
            let fallback = text_cleanup::concise_fallback_snippet(&app_name, &window_title, &text);
            if fallback.is_empty() {
                (
                    text.chars().take(140).collect::<String>(),
                    "fallback".to_string(),
                )
            } else {
                (fallback, "fallback".to_string())
            }
        };

        let now = Local::now();
        let (display_summary, narration_filtered) = clean_or_fallback_display_summary(
            &final_snippet,
            &window_title,
            url.as_deref(),
            now.timestamp_millis(),
        );
        if narration_filtered {
            tracing::debug!(
                app = %app_name,
                title = %window_title,
                "capture_pipeline:display_summary_narration_filter_hit"
            );
        }
        let internal_context = build_grounded_memory_context(
            structured_memory.as_ref(),
            &app_name,
            &window_title,
            &text,
            &display_summary,
        );

        // --- Proactive Privacy Check ---
        if Blocklist::is_sensitive_context(url.as_deref(), Some(&window_title)) {
            let alert_key = Blocklist::context_key(url.as_deref(), Some(&window_title))
                .unwrap_or_else(|| window_title.clone());

            let is_dismissed = Blocklist::is_context_blocked(
                url.as_deref(),
                Some(&window_title),
                &config.dismissed_privacy_alerts,
            );
            let is_snoozed = {
                let snoozed = state.snoozed_privacy_alerts.read();
                if let Some(&expire_time) = snoozed.get(&alert_key) {
                    now.timestamp() < expire_time
                } else {
                    false
                }
            };

            if !is_dismissed && !is_snoozed {
                let mut pending = state.pending_privacy_alerts.write();
                if !pending.iter().any(|a| a.domain_or_title == alert_key) {
                    pending.push(crate::PrivacyAlert {
                        id: uuid::Uuid::new_v4().to_string(),
                        domain_or_title: alert_key,
                        detected_at: now.timestamp_millis(),
                    });
                    tracing::info!("Surfaced proactive privacy alert for sensitive context");
                }
            }
        }

        let session_key = build_session_key(&app_name, &window_title, url.as_deref());

        // Enrich clean_text with VLM metadata when available.
        let enriched_clean_text = if let Some(ref vlm_text) = vlm_analysis {
            merge_story_text(&text, vlm_text, 7000)
        } else {
            text.clone()
        };
        let lexical_shadow = build_lexical_shadow(
            &window_title,
            &display_summary,
            &enriched_clean_text,
            url.as_deref(),
        );
        let primary_embed_input = compose_primary_embedding_text(
            structured_memory.as_ref(),
            &app_name,
            &window_title,
            &internal_context,
            &display_summary,
            &enriched_clean_text,
            &lexical_shadow,
        );
        let snippet_embed_input = compact_summary_embedding_text(
            &summary_source,
            &display_summary,
            &enriched_clean_text,
            &lexical_shadow,
        );
        let support_texts = support_embedding_texts(
            &app_name,
            &window_title,
            &enriched_clean_text,
            &lexical_shadow,
        );

        let mut embedding_inputs = vec![primary_embed_input.clone(), snippet_embed_input.clone()];
        embedding_inputs.extend(support_texts.iter().cloned());
        let semantic_embeddings_available = semantic_embeddings_enabled(text_embedder.as_ref());
        let embed_start = Instant::now();
        let embedding_vectors = embed_text_inputs_with_memo(
            text_embedder.as_ref(),
            &mut embedding_memo,
            &app_name,
            &window_title,
            &embedding_inputs,
        );
        let embed_latency = embed_start.elapsed();
        let primary_embedding = embedding_vectors
            .first()
            .cloned()
            .unwrap_or_else(|| vec![0.0; EMBEDDING_DIM]);
        let snippet_embedding = embedding_vectors
            .get(1)
            .cloned()
            .unwrap_or_else(|| vec![0.0; EMBEDDING_DIM]);
        let support_embedding = if embedding_vectors.len() > 2 {
            mean_pool_embeddings(&embedding_vectors[2..])
        } else {
            vec![0.0; EMBEDDING_DIM]
        };
        let text_embedding =
            weighted_primary_embedding(&primary_embedding, &snippet_embedding, &support_embedding);
        *state.last_embedding.write() = if semantic_embeddings_available {
            text_embedding.clone()
        } else {
            Vec::new()
        };
        tracing::info!(
            app = %app_name,
            ocr_ms = ocr_latency.as_millis(),
            embed_ms = embed_latency.as_millis(),
            support_chunks = support_texts.len(),
            semantic_embeddings_available,
            "capture_pipeline:distilled_frame"
        );

        // ── Focus Mode drift detection ────────────────────────────────────────
        // Mirrors CC's context-similarity approach: embed the focus task once,
        // then compare every incoming capture. 3 consecutive off-task captures
        // surfaces a ProactiveSuggestion that the frontend can toast.
        if semantic_embeddings_available {
            let focus_emb_opt = state.focus_task_embedding.read().clone();
            if let Some(ref focus_emb) = focus_emb_opt {
                let sim = cosine_similarity(&text_embedding, focus_emb);
                if sim < config.capture_pipeline.focus_drift_similarity_threshold {
                    let prev = state
                        .focus_drift_count
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if prev + 1 >= config.capture_pipeline.focus_drift_capture_count {
                        state
                            .focus_drift_count
                            .store(0, std::sync::atomic::Ordering::Relaxed);
                        let task_title = state.focus_task.read().clone().unwrap_or_default();
                        let suggestion = crate::ProactiveSuggestion {
                            memory_id: "focus_drift".to_string(),
                            snippet: format!(
                                "You've been off-task for a while. Your focus: \"{}\"",
                                task_title
                            ),
                            similarity: sim,
                            task_title: Some(task_title),
                        };
                        let _ = state.proactive_tx.send(Some(suggestion));
                    }
                } else {
                    state
                        .focus_drift_count
                        .store(0, std::sync::atomic::Ordering::Relaxed);
                }
            }
        } else {
            state
                .focus_drift_count
                .store(0, std::sync::atomic::Ordering::Relaxed);
        }

        let mut dedup_fingerprint = structured_memory
            .as_ref()
            .map(|m| m.dedup_fingerprint.trim().to_string())
            .unwrap_or_default();
        if !is_supported_dedup_fingerprint(&dedup_fingerprint) {
            dedup_fingerprint = deterministic_dedup_fingerprint(
                &MemoryRecord {
                    app_name: app_name.clone(),
                    window_title: window_title.clone(),
                    project: structured_memory
                        .as_ref()
                        .map(|m| m.project.clone())
                        .unwrap_or_default(),
                    topic: structured_memory
                        .as_ref()
                        .map(|m| m.topic.clone())
                        .unwrap_or_default(),
                    activity_type: structured_memory
                        .as_ref()
                        .map(|m| m.activity_type.clone())
                        .unwrap_or_default(),
                    url: url.clone(),
                    clean_text: enriched_clean_text.clone(),
                    ..Default::default()
                },
                Some(&internal_context),
            );
        }

        let raw_evidence_payload = json!({
            "timestamp_ms": now.timestamp_millis(),
            "app_name": app_name.clone(),
            "window_title": window_title.clone(),
            "url": url.clone(),
            "source_kind": source_kind,
            "ocr_confidence": observed_confidence,
            "ocr_block_count": observed_block_count,
            "semantic_page": semantic_page.as_ref().map(|page| {
                json!({
                    "title": page.title.chars().take(200).collect::<String>(),
                    "h1": page.h1.chars().take(220).collect::<String>(),
                    "meta_description": page.meta_description.chars().take(280).collect::<String>(),
                    "nav_ratio": page.nav_ratio,
                    "content_signal_score": page.content_signal_score,
                })
            }),
            "ocr_quality": {
                "total_lines": capture_quality.total_lines,
                "kept_lines": capture_quality.kept_lines,
                "low_conf_lines": capture_quality.low_conf_lines,
                "dropped_noise_lines": capture_quality.dropped_noise_lines,
                "dropped_low_signal_lines": capture_quality.dropped_low_signal_lines,
                "avg_line_score": capture_quality.avg_line_score,
            },
            "extraction_grounding_confidence": extraction_grounding_confidence,
            "extraction_issues": extraction_issues.clone(),
            "primary_embed_input": primary_embed_input.chars().take(900).collect::<String>(),
            "internal_context": internal_context.chars().take(700).collect::<String>(),
            "clean_text_excerpt": enriched_clean_text.chars().take(700).collect::<String>(),
        })
        .to_string();

        let clean_text_len = enriched_clean_text.len();
        let session_id = build_session_id(
            &now,
            &app_name,
            app_context.bundle_id.as_deref(),
            &session_key,
        );
        let record = MemoryRecord {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: now.timestamp_millis(),
            day_bucket: now.format("%Y-%m-%d").to_string(),
            app_name: app_name.clone(),
            bundle_id: app_context.bundle_id.clone(),
            window_title: window_title.clone(),
            session_id,
            text: String::new(),
            clean_text: enriched_clean_text,
            ocr_confidence: observed_confidence,
            ocr_block_count: observed_block_count as u32,
            snippet: display_summary.clone(),
            display_summary: display_summary.clone(),
            internal_context,
            summary_source,
            noise_score,
            session_key,
            lexical_shadow,
            embedding: text_embedding,
            image_embedding: vec![0.0; DEFAULT_IMAGE_EMBEDDING_DIM],
            screenshot_path: None,
            url: url.clone(),
            snippet_embedding,
            support_embedding,
            decay_score: 1.0,
            last_accessed_at: 0,
            timestamp_start: now.timestamp_millis(),
            timestamp_end: now.timestamp_millis(),
            source_type: if url.is_some() {
                "browser".to_string()
            } else {
                "screen".to_string()
            },
            topic: structured_memory
                .as_ref()
                .map(|m| {
                    if m.topic.trim().is_empty() {
                        "unknown".to_string()
                    } else {
                        m.topic.clone()
                    }
                })
                .unwrap_or_else(|| "unknown".to_string()),
            workflow: structured_memory
                .as_ref()
                .map(|m| {
                    if m.workflow.trim().is_empty() {
                        "unknown".to_string()
                    } else {
                        m.workflow.clone()
                    }
                })
                .unwrap_or_else(|| "unknown".to_string()),
            user_intent: structured_memory
                .as_ref()
                .map(|m| {
                    if m.user_intent.trim().is_empty() {
                        m.activity_type.clone()
                    } else {
                        m.user_intent.clone()
                    }
                })
                .unwrap_or_default(),
            intent_analysis: crate::store::IntentAnalysis::default(),
            memory_context: structured_memory
                .as_ref()
                .map(|m| m.memory_context.clone())
                .unwrap_or_else(|| display_summary.clone()),
            commands: structured_memory
                .as_ref()
                .map(|m| m.commands.clone())
                .unwrap_or_default(),
            blockers: structured_memory
                .as_ref()
                .map(|m| m.blockers.clone())
                .unwrap_or_default(),
            todos: structured_memory
                .as_ref()
                .map(|m| m.todos.clone())
                .unwrap_or_default(),
            open_questions: structured_memory
                .as_ref()
                .map(|m| m.open_questions.clone())
                .unwrap_or_default(),
            results: structured_memory
                .as_ref()
                .map(|m| m.results.clone())
                .unwrap_or_default(),
            related_tools: Vec::new(),
            related_agents: Vec::new(),
            related_projects: Vec::new(),
            raw_evidence: raw_evidence_payload,
            search_aliases: structured_memory
                .as_ref()
                .map(|m| m.search_aliases.clone())
                .unwrap_or_default(),
            related_memory_ids: Vec::new(),
            graph_node_ids: Vec::new(),
            graph_edge_ids: Vec::new(),
            project_confidence: 0.0,
            topic_confidence: 0.0,
            workflow_confidence: 0.0,
            project_evidence: Vec::new(),
            related_project_ids: Vec::new(),
            evidence_confidence: observed_confidence,
            confidence_score: 0.0,
            importance_score: 0.0,
            specificity_score: 0.0,
            intent_score: 0.0,
            entity_score: 0.0,
            agent_usefulness_score: 0.0,
            ocr_noise_score: noise_score,
            graph_readiness_score: 0.0,
            retrieval_value_score: 0.0,
            storage_outcome: String::new(),
            quality_gate_reason: String::new(),
            extracted_entities_structured: Vec::new(),
            action_items: Vec::new(),

            // V2 Fields
            schema_version: 2,
            activity_type: structured_memory
                .as_ref()
                .map(|m| m.activity_type.clone())
                .unwrap_or_default(),
            files_touched: structured_memory
                .as_ref()
                .map(|m| m.files_touched.clone())
                .unwrap_or_default(),
            symbols_changed: structured_memory
                .as_ref()
                .map(|m| m.symbols_changed.clone())
                .unwrap_or_default(),
            session_duration_mins: 0,
            project: structured_memory
                .as_ref()
                .map(|m| m.project.clone())
                .unwrap_or_default(),
            tags: structured_memory
                .as_ref()
                .map(|m| m.tags.clone())
                .unwrap_or_default(),
            entities: structured_memory
                .as_ref()
                .map(|m| m.entities.clone())
                .unwrap_or_default(),
            decisions: structured_memory
                .as_ref()
                .map(|m| m.decisions.clone())
                .unwrap_or_default(),
            errors: structured_memory
                .as_ref()
                .map(|m| m.errors.clone())
                .unwrap_or_default(),
            next_steps: structured_memory
                .as_ref()
                .map(|m| m.next_steps.clone())
                .unwrap_or_default(),
            git_stats: structured_memory
                .as_ref()
                .map(|m| crate::store::schema::GitStats {
                    added: m.git_stats.added,
                    removed: m.git_stats.removed,
                    commits: m.git_stats.commits,
                }),
            outcome: structured_memory
                .as_ref()
                .map(|m| m.outcome.clone())
                .unwrap_or_default(),
            extraction_confidence: structured_memory
                .as_ref()
                .map(|m| m.confidence)
                .unwrap_or(0.0),
            anchor_coverage_score: 0.0,
            content_hash: String::new(),
            dedup_fingerprint: structured_memory
                .as_ref()
                .map(|_| dedup_fingerprint.clone())
                .unwrap_or(dedup_fingerprint),
            embedding_text: primary_embed_input,
            embedding_model: "bge-large-en-v1.5".to_string(), // Default assumption, actual model set in pipeline
            embedding_dim: EMBEDDING_DIM as u32,
            is_consolidated: false,
            is_soft_deleted: false,
            parent_id: None,
            related_ids: Vec::new(),
            consolidated_from: Vec::new(),
        };
        let incoming_record_id = record.id.clone();
        let merged_or_new = match merge_or_append_memory_record(
            state.as_ref(),
            &mut batch,
            &mut continuity_index,
            record.clone(),
            text_embedder.as_ref(),
            engine.as_ref(),
        )
        .await
        {
            Ok(merged) => merged,
            Err(err) => {
                tracing::warn!("Memory continuity merge failed for {}: {}", record.id, err);
                batch.push(record.clone());
                record
            }
        };
        if merged_or_new.id != incoming_record_id {
            emit_extraction_quality_anomaly(
                state.as_ref(),
                json!({
                    "timestamp_ms": now.timestamp_millis(),
                    "app_name": app_name,
                    "schema_version": 2,
                    "record_id": incoming_record_id,
                    "merged_into_id": merged_or_new.id,
                    "grounding_confidence": extraction_grounding_confidence,
                    "source_kind": source_kind,
                    "anomaly_labels": ["merge_audit_event"],
                }),
            );
        }
        // Fire-and-forget: auto-link to a task cluster based on embedding similarity.
        if semantic_embeddings_available {
            let record_clone = merged_or_new.clone();
            let cluster_store = state.store.clone();
            tauri::async_runtime::spawn(async move {
                let graph = crate::graph::GraphStore::new(cluster_store);
                if let Err(e) = graph.auto_link_to_task(&record_clone).await {
                    tracing::debug!("Auto task link: {e}");
                }
            });
        }

        if let Err(err) =
            maybe_create_tasks_from_memory(state.as_ref(), &merged_or_new, engine.as_ref()).await
        {
            tracing::debug!("Auto task extraction skipped: {}", err);
        }

        emit_capture_quality_signal(
            state.as_ref(),
            json!({
                "timestamp_ms": now.timestamp_millis(),
                "app_name": app_name,
                "bundle_id": app_context.bundle_id.clone(),
                "domain": url.as_deref().and_then(extract_domain).unwrap_or_default(),
                "window_title_hash": compute_window_title_hash(url.as_deref(), &window_title, now.timestamp_millis()),
                "ocr_confidence": observed_confidence,
                "ocr_block_count": observed_block_count,
                "clean_text_len": clean_text_len,
                "noise_score": noise_score,
                "low_signal": false,
                "stored_or_skipped": "stored_candidate",
                "source_kind": source_kind,
                "grounding_confidence": extraction_grounding_confidence,
                "semantic_nav_ratio": semantic_page.as_ref().map(|page| page.nav_ratio).unwrap_or(0.0),
                "semantic_content_score": semantic_page.as_ref().map(|page| page.content_signal_score).unwrap_or(0.0),
                "quality_stats": {
                    "total_lines": capture_quality.total_lines,
                    "kept_lines": capture_quality.kept_lines,
                    "low_conf_lines": capture_quality.low_conf_lines,
                    "dropped_noise_lines": capture_quality.dropped_noise_lines,
                    "dropped_low_signal_lines": capture_quality.dropped_low_signal_lines,
                    "avg_line_score": capture_quality.avg_line_score,
                }
            }),
        );

        state.frames_captured.fetch_add(1, Ordering::Relaxed);
        state
            .last_capture_time
            .store(now.timestamp_millis() as u64, Ordering::Relaxed);

        // Drop image data immediately (important for memory)
        drop(image_data);

        tokio::time::sleep(sleep_duration).await;
    }
}

fn embed_text_inputs_with_memo(
    text_embedder: Option<&Embedder>,
    memo: &mut EmbeddingMemo,
    app_name: &str,
    window_title: &str,
    texts: &[String],
) -> Vec<Vec<f32>> {
    if !semantic_embeddings_enabled(text_embedder) {
        return vec![vec![0.0; EMBEDDING_DIM]; texts.len()];
    }

    let Some(text_embedder) = text_embedder else {
        return vec![vec![0.0; EMBEDDING_DIM]; texts.len()];
    };

    let mut out: Vec<Option<Vec<f32>>> = vec![None; texts.len()];
    let mut missing = Vec::new();
    let mut missing_positions = Vec::new();
    let mut missing_dedup: HashMap<String, usize> = HashMap::new();
    let app_key = app_name.trim().to_lowercase();
    let title_key = window_title.trim().to_lowercase();

    for (idx, text) in texts.iter().enumerate() {
        let text_key = text.trim().to_string();
        if text_key.is_empty() {
            out[idx] = Some(vec![0.0; EMBEDDING_DIM]);
            continue;
        }
        let key = format!("{app_key}|||{title_key}|||{text_key}");

        if let Some(cached) = memo.get(&key) {
            out[idx] = Some(cached);
            continue;
        }

        if let Some(unique_idx) = missing_dedup.get(&key).copied() {
            missing_positions.push((idx, unique_idx));
            continue;
        }

        let unique_idx = missing.len();
        missing_dedup.insert(key.clone(), unique_idx);
        missing_positions.push((idx, unique_idx));
        missing.push((key, text_key));
    }

    if !missing.is_empty() {
        let contextual_inputs = missing
            .iter()
            .map(|(_, text)| (app_name.to_string(), window_title.to_string(), text.clone()))
            .collect::<Vec<_>>();
        if let Ok(vectors) = text_embedder.embed_batch_with_context(&contextual_inputs) {
            for ((memo_key, _), vector) in missing.iter().cloned().zip(vectors.iter().cloned()) {
                memo.insert(memo_key, vector);
            }
            for (idx, unique_idx) in missing_positions {
                out[idx] = Some(
                    vectors
                        .get(unique_idx)
                        .cloned()
                        .unwrap_or_else(|| vec![0.0; EMBEDDING_DIM]),
                );
            }
        }
    }

    out.into_iter()
        .map(|maybe| maybe.unwrap_or_else(|| vec![0.0; EMBEDDING_DIM]))
        .collect()
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct MergeScore {
    pub score: f32,
    pub lexical: f32,
    pub vector: f32,
    pub anchor_match: bool,
}

async fn merge_or_append_memory_record(
    state: &AppState,
    batch: &mut Vec<MemoryRecord>,
    continuity_index: &mut HashMap<String, String>,
    incoming: MemoryRecord,
    text_embedder: Option<&Embedder>,
    engine: Option<&Arc<crate::inference::InferenceEngine>>,
) -> Result<MemoryRecord, String> {
    if !eligible_for_story_merge(&incoming) {
        if let Some(anchor) = continuity_anchor_for_memory(&incoming) {
            continuity_index.insert(anchor, incoming.id.clone());
        }
        batch.push(incoming.clone());
        return Ok(incoming);
    }

    let incoming_anchor = continuity_anchor_for_memory(&incoming);
    let incoming_id = incoming.id.clone();
    let semantic_merge_enabled = semantic_embeddings_enabled(text_embedder);

    if let Some(anchor) = incoming_anchor.as_ref() {
        if let Some(anchor_id) = continuity_index.get(anchor).cloned() {
            if let Some(batch_idx) = batch.iter().position(|record| record.id == anchor_id) {
                let merged = merge_memory_records(
                    batch[batch_idx].clone(),
                    incoming.clone(),
                    text_embedder,
                    engine,
                )
                .await;
                tracing::info!(
                    "Merged memory {} into in-flight continuity card {} via anchor {}",
                    incoming.id,
                    merged.id,
                    anchor
                );
                if merged.screenshot_path != incoming.screenshot_path {
                    cleanup_screenshot_path(incoming.screenshot_path.clone());
                }
                batch[batch_idx] = merged.clone();
                continuity_index.insert(anchor.clone(), merged.id.clone());
                return Ok(merged);
            }

            if let Some(existing) = state
                .store
                .get_memory_by_id(&anchor_id)
                .await
                .map_err(|e| e.to_string())?
            {
                let merged =
                    merge_memory_records(existing.clone(), incoming.clone(), text_embedder, engine)
                        .await;
                tracing::info!(
                    "Merged memory {} into persisted continuity card {} via anchor {}",
                    incoming.id,
                    merged.id,
                    anchor
                );
                state
                    .store
                    .delete_memory_by_id(&existing.id)
                    .await
                    .map_err(|e| e.to_string())?;
                state.invalidate_memory_derived_caches();
                state
                    .store
                    .add_batch(&[merged.clone()])
                    .await
                    .map_err(|e| e.to_string())?;
                if let Err(err) =
                    context_runtime::sync_memory_record(state, &merged, Some("screen")).await
                {
                    tracing::warn!("Context runtime merge sync failed: {}", err);
                }
                state.invalidate_memory_derived_caches();
                if merged.screenshot_path != incoming.screenshot_path {
                    cleanup_screenshot_path(incoming.screenshot_path.clone());
                }
                continuity_index.insert(anchor.clone(), merged.id.clone());
                return Ok(merged);
            }
        }
    }

    if semantic_merge_enabled {
        if let Some(batch_idx) = best_batch_merge_target(batch, &incoming) {
            let merged = merge_memory_records(
                batch[batch_idx].clone(),
                incoming.clone(),
                text_embedder,
                engine,
            )
            .await;
            tracing::info!(
                "Merged memory {} into in-flight continuity card {} via similarity score",
                incoming.id,
                merged.id
            );
            if merged.screenshot_path != incoming.screenshot_path {
                cleanup_screenshot_path(incoming.screenshot_path.clone());
            }
            batch[batch_idx] = merged.clone();
            if let Some(anchor) = incoming_anchor.as_ref() {
                continuity_index.insert(anchor.clone(), merged.id.clone());
            }
            return Ok(merged);
        }

        if let Some(existing) = best_persisted_merge_target(state, &incoming).await? {
            let merged =
                merge_memory_records(existing.clone(), incoming.clone(), text_embedder, engine)
                    .await;
            tracing::info!(
                "Merged memory {} into persisted continuity card {} via similarity score",
                incoming.id,
                merged.id
            );
            state
                .store
                .delete_memory_by_id(&existing.id)
                .await
                .map_err(|e| e.to_string())?;
            state.invalidate_memory_derived_caches();
            state
                .store
                .add_batch(&[merged.clone()])
                .await
                .map_err(|e| e.to_string())?;
            if let Err(err) =
                context_runtime::sync_memory_record(state, &merged, Some("screen")).await
            {
                tracing::warn!("Context runtime merge sync failed: {}", err);
            }
            state.invalidate_memory_derived_caches();
            if merged.screenshot_path != incoming.screenshot_path {
                cleanup_screenshot_path(incoming.screenshot_path.clone());
            }
            if let Some(anchor) = continuity_anchor_for_memory(&merged) {
                continuity_index.insert(anchor, merged.id.clone());
            }
            return Ok(merged);
        }
    } else {
        if let Some(batch_idx) = best_batch_lexical_merge_target(batch, &incoming) {
            let merged = merge_memory_records(
                batch[batch_idx].clone(),
                incoming.clone(),
                text_embedder,
                engine,
            )
            .await;
            tracing::info!(
                "Merged memory {} into in-flight continuity card {} via lexical fallback",
                incoming.id,
                merged.id
            );
            if merged.screenshot_path != incoming.screenshot_path {
                cleanup_screenshot_path(incoming.screenshot_path.clone());
            }
            batch[batch_idx] = merged.clone();
            if let Some(anchor) = incoming_anchor.as_ref() {
                continuity_index.insert(anchor.clone(), merged.id.clone());
            }
            return Ok(merged);
        }

        if let Some(existing) = best_persisted_lexical_merge_target(state, &incoming).await? {
            let merged =
                merge_memory_records(existing.clone(), incoming.clone(), text_embedder, engine)
                    .await;
            tracing::info!(
                "Merged memory {} into persisted continuity card {} via lexical fallback",
                incoming.id,
                merged.id
            );
            state
                .store
                .delete_memory_by_id(&existing.id)
                .await
                .map_err(|e| e.to_string())?;
            state
                .store
                .add_batch(&[merged.clone()])
                .await
                .map_err(|e| e.to_string())?;
            if let Err(err) =
                context_runtime::sync_memory_record(state, &merged, Some("screen")).await
            {
                tracing::warn!("Context runtime merge sync failed: {}", err);
            }
            if merged.screenshot_path != incoming.screenshot_path {
                cleanup_screenshot_path(incoming.screenshot_path.clone());
            }
            if let Some(anchor) = continuity_anchor_for_memory(&merged) {
                continuity_index.insert(anchor, merged.id.clone());
            }
            return Ok(merged);
        }
    }

    if let Some(anchor) = incoming_anchor {
        continuity_index.insert(anchor, incoming_id);
    }
    batch.push(incoming.clone());
    Ok(incoming)
}

pub(crate) fn eligible_for_story_merge(record: &MemoryRecord) -> bool {
    record.clean_text.trim().len() >= 36 || record.snippet.trim().len() >= 18
}

fn best_batch_merge_target(batch: &[MemoryRecord], incoming: &MemoryRecord) -> Option<usize> {
    let mut best: Option<(usize, MergeScore)> = None;
    for (index, candidate) in batch.iter().enumerate() {
        let scored = score_memory_candidate(incoming, candidate);
        if incoming.app_name != candidate.app_name
            && !allows_cross_app_merge_from_memory(incoming, candidate, scored)
        {
            continue;
        }
        if !passes_merge_threshold(scored) {
            continue;
        }
        if best
            .as_ref()
            .map(|(_, prev)| scored.score > prev.score)
            .unwrap_or(true)
        {
            best = Some((index, scored));
        }
    }

    best.map(|(index, _)| index)
}

fn best_batch_lexical_merge_target(
    batch: &[MemoryRecord],
    incoming: &MemoryRecord,
) -> Option<usize> {
    let mut best: Option<(usize, MergeScore)> = None;
    for (index, candidate) in batch.iter().enumerate() {
        if incoming.app_name != candidate.app_name {
            continue;
        }
        if !is_cross_app_merge_window(incoming.timestamp, candidate.timestamp) {
            continue;
        }
        let scored = score_memory_candidate_lexical(incoming, candidate);
        if !passes_lexical_merge_threshold(scored) {
            continue;
        }
        if best
            .as_ref()
            .map(|(_, prev)| scored.score > prev.score)
            .unwrap_or(true)
        {
            best = Some((index, scored));
        }
    }

    best.map(|(index, _)| index)
}

async fn best_persisted_merge_target(
    state: &AppState,
    incoming: &MemoryRecord,
) -> Result<Option<MemoryRecord>, String> {
    let same_app_candidates = state
        .store
        .vector_search(
            &incoming.embedding,
            24,
            Some("7d"),
            Some(&incoming.app_name),
        )
        .await
        .map_err(|e| e.to_string())?;

    let best_same_app = same_app_candidates
        .iter()
        .filter(|candidate| candidate.id != incoming.id)
        .filter_map(|candidate| {
            let scored = score_search_candidate(incoming, candidate);
            if !passes_merge_threshold(scored) {
                return None;
            }
            Some((candidate.id.clone(), scored.score))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1));

    if let Some((best_id, _)) = best_same_app {
        return state
            .store
            .get_memory_by_id(&best_id)
            .await
            .map_err(|e| e.to_string());
    }

    let cross_app_candidates = state
        .store
        .vector_search(&incoming.embedding, 32, Some("24h"), None)
        .await
        .map_err(|e| e.to_string())?;

    let best_cross_app = cross_app_candidates
        .iter()
        .filter(|candidate| candidate.id != incoming.id)
        .filter(|candidate| candidate.app_name != incoming.app_name)
        .filter_map(|candidate| {
            let scored = score_search_candidate(incoming, candidate);
            if !passes_merge_threshold(scored) {
                return None;
            }
            if !allows_cross_app_merge_from_search(incoming, candidate, scored) {
                return None;
            }
            Some((candidate.id.clone(), scored.score))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1));

    if let Some((best_id, _)) = best_cross_app {
        return state
            .store
            .get_memory_by_id(&best_id)
            .await
            .map_err(|e| e.to_string());
    }
    Ok(None)
}

async fn best_persisted_lexical_merge_target(
    state: &AppState,
    incoming: &MemoryRecord,
) -> Result<Option<MemoryRecord>, String> {
    let query = lexical_merge_query(incoming);
    if query.is_empty() {
        return Ok(None);
    }

    let candidates = state
        .store
        .keyword_search(&query, 36, Some("24h"), Some(&incoming.app_name))
        .await
        .map_err(|e| e.to_string())?;

    let best = candidates
        .iter()
        .filter(|candidate| candidate.id != incoming.id)
        .filter_map(|candidate| {
            let scored = score_search_candidate_lexical(incoming, candidate);
            if !passes_lexical_merge_threshold(scored) {
                return None;
            }
            Some((candidate.id.clone(), scored.score))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1));

    if let Some((best_id, _)) = best {
        state
            .store
            .get_memory_by_id(&best_id)
            .await
            .map_err(|e| e.to_string())
    } else {
        Ok(None)
    }
}

pub(crate) async fn merge_memory_records(
    existing: MemoryRecord,
    incoming: MemoryRecord,
    text_embedder: Option<&Embedder>,
    engine: Option<&Arc<crate::inference::InferenceEngine>>,
) -> MemoryRecord {
    merge_memory_records_with_policy(existing, incoming, text_embedder, engine, true, true).await
}

pub(crate) async fn merge_memory_records_with_policy(
    existing: MemoryRecord,
    incoming: MemoryRecord,
    text_embedder: Option<&Embedder>,
    engine: Option<&Arc<crate::inference::InferenceEngine>>,
    recompute_embedding: bool,
    allow_llm_summary: bool,
) -> MemoryRecord {
    let merged_clean_text = merge_story_text(&existing.clean_text, &incoming.clean_text, 6400);
    let snippet_fallback = merge_story_text(&existing.snippet, &incoming.snippet, 260);
    let llm_snippet = if allow_llm_summary {
        if let Some(model) = engine {
            let generated = model
                .summarize_memory_node(
                    &incoming.app_name,
                    &incoming.window_title,
                    &merged_clean_text,
                )
                .await;
            if generated.trim().is_empty() {
                None
            } else {
                Some(generated)
            }
        } else {
            None
        }
    } else {
        None
    };

    let merged_snippet = llm_snippet.unwrap_or_else(|| snippet_fallback.clone());
    let merged_summary_source = if snippet_fallback.trim().is_empty() {
        existing.summary_source.clone()
    } else if merged_snippet == snippet_fallback {
        "fallback".to_string()
    } else {
        "llm".to_string()
    };
    let merged_window_title = choose_story_title(&existing.window_title, &incoming.window_title);
    let merged_url = incoming.url.clone().or(existing.url.clone());
    let merged_timestamp = incoming.timestamp.max(existing.timestamp);
    let (merged_display_summary, filtered_narration) = clean_or_fallback_display_summary(
        &merged_snippet,
        &merged_window_title,
        merged_url.as_deref(),
        merged_timestamp,
    );
    if filtered_narration {
        tracing::debug!(
            existing_id = %existing.id,
            incoming_id = %incoming.id,
            "capture_merge:display_summary_narration_filter_hit"
        );
    }
    let merged_internal_context = merge_story_text(
        &prefer_non_empty(&existing.internal_context, &existing.clean_text),
        &prefer_non_empty(&incoming.internal_context, &incoming.clean_text),
        6400,
    );
    let merged_lexical_shadow = build_lexical_shadow(
        &merged_window_title,
        &merged_display_summary,
        &format!(
            "{}\n{}\n{}",
            merged_clean_text, existing.lexical_shadow, incoming.lexical_shadow
        ),
        merged_url.as_deref(),
    );
    let compact_snippet_text = compact_summary_embedding_text(
        &merged_summary_source,
        &merged_display_summary,
        &merged_clean_text,
        &merged_lexical_shadow,
    );
    let support_texts = support_embedding_texts(
        &incoming.app_name,
        &merged_window_title,
        &merged_clean_text,
        &merged_lexical_shadow,
    );

    let merged_embedding = if recompute_embedding && semantic_embeddings_enabled(text_embedder) {
        text_embedder
            .and_then(|embedder| {
                embedder
                    .embed_batch_with_context(&[(
                        incoming.app_name.clone(),
                        merged_window_title.clone(),
                        merged_clean_text.clone(),
                    )])
                    .ok()
                    .and_then(|mut vectors| vectors.drain(..).next())
            })
            .unwrap_or_else(|| existing.embedding.clone())
    } else {
        existing.embedding.clone()
    };

    let merged_snippet_embedding =
        if recompute_embedding && semantic_embeddings_enabled(text_embedder) {
            text_embedder
                .and_then(|embedder| {
                    embedder
                        .embed_batch_with_context(&[(
                            incoming.app_name.clone(),
                            merged_window_title.clone(),
                            compact_snippet_text.clone(),
                        )])
                        .ok()
                        .and_then(|mut vectors| vectors.drain(..).next())
                })
                .unwrap_or_else(|| existing.snippet_embedding.clone())
        } else {
            existing.snippet_embedding.clone()
        };

    let merged_support_embedding = if recompute_embedding
        && semantic_embeddings_enabled(text_embedder)
        && !support_texts.is_empty()
    {
        let contexts = support_texts
            .iter()
            .map(|text| {
                (
                    incoming.app_name.clone(),
                    merged_window_title.clone(),
                    text.clone(),
                )
            })
            .collect::<Vec<_>>();
        text_embedder
            .and_then(|embedder| {
                embedder
                    .embed_batch_with_context(&contexts)
                    .ok()
                    .map(|vectors| mean_pool_embeddings(&vectors))
            })
            .unwrap_or_else(|| existing.support_embedding.clone())
    } else {
        existing.support_embedding.clone()
    };

    let schema_version = existing
        .schema_version
        .max(incoming.schema_version)
        .max(MemoryRecord::default().schema_version);
    let activity_type = prefer_non_empty(&incoming.activity_type, &existing.activity_type);
    let files_touched = merge_string_lists(&existing.files_touched, &incoming.files_touched);
    let symbols_changed = merge_string_lists(&existing.symbols_changed, &incoming.symbols_changed);
    let session_duration_mins = existing
        .session_duration_mins
        .max(incoming.session_duration_mins);
    let project = prefer_non_empty(&incoming.project, &existing.project);
    let tags = merge_string_lists(&existing.tags, &incoming.tags);
    let entities = merge_string_lists(&existing.entities, &incoming.entities);
    let decisions = merge_string_lists(&existing.decisions, &incoming.decisions);
    let errors = merge_string_lists(&existing.errors, &incoming.errors);
    let next_steps = merge_string_lists(&existing.next_steps, &incoming.next_steps);
    let git_stats = incoming.git_stats.clone().or(existing.git_stats.clone());
    let outcome = prefer_non_empty(&incoming.outcome, &existing.outcome);
    let extraction_confidence = existing
        .extraction_confidence
        .max(incoming.extraction_confidence);
    let dedup_fingerprint =
        prefer_non_empty(&incoming.dedup_fingerprint, &existing.dedup_fingerprint);
    let embedding_text = prefer_non_empty(&incoming.embedding_text, &existing.embedding_text);
    let embedding_model = prefer_non_empty(&incoming.embedding_model, &existing.embedding_model);
    let embedding_dim = incoming
        .embedding_dim
        .max(existing.embedding_dim)
        .max(EMBEDDING_DIM as u32);
    let parent_id = incoming.parent_id.clone().or(existing.parent_id.clone());
    let related_ids = merge_string_lists(&existing.related_ids, &incoming.related_ids);
    let consolidated_from =
        merge_string_lists(&existing.consolidated_from, &incoming.consolidated_from);

    MemoryRecord {
        id: existing.id.clone(),
        timestamp: merged_timestamp,
        day_bucket: incoming.day_bucket.clone(),
        app_name: incoming.app_name.clone(),
        bundle_id: incoming.bundle_id.clone().or(existing.bundle_id.clone()),
        window_title: merged_window_title,
        session_id: existing.session_id.clone(),
        text: String::new(),
        clean_text: merged_clean_text,
        ocr_confidence: existing.ocr_confidence.max(incoming.ocr_confidence),
        ocr_block_count: existing.ocr_block_count.max(incoming.ocr_block_count),
        snippet: merged_display_summary.clone(),
        display_summary: merged_display_summary,
        internal_context: merged_internal_context,
        summary_source: merged_summary_source,
        noise_score: ((existing.noise_score + incoming.noise_score) / 2.0).clamp(0.0, 1.0),
        session_key: choose_story_title(&existing.session_key, &incoming.session_key),
        lexical_shadow: merged_lexical_shadow,
        embedding: merged_embedding,
        image_embedding: incoming.image_embedding.clone(),
        screenshot_path: existing
            .screenshot_path
            .clone()
            .or(incoming.screenshot_path.clone()),
        url: merged_url,
        snippet_embedding: merged_snippet_embedding,
        support_embedding: merged_support_embedding,
        decay_score: existing.decay_score.max(incoming.decay_score),
        last_accessed_at: existing.last_accessed_at.max(incoming.last_accessed_at),
        timestamp_start: if existing.timestamp_start > 0 && incoming.timestamp_start > 0 {
            existing.timestamp_start.min(incoming.timestamp_start)
        } else {
            existing.timestamp.min(incoming.timestamp)
        },
        timestamp_end: if existing.timestamp_end > 0 && incoming.timestamp_end > 0 {
            existing.timestamp_end.max(incoming.timestamp_end)
        } else {
            existing.timestamp.max(incoming.timestamp)
        },
        source_type: prefer_non_empty(&incoming.source_type, &existing.source_type),
        topic: prefer_non_empty(&incoming.topic, &existing.topic),
        workflow: prefer_non_empty(&incoming.workflow, &existing.workflow),
        user_intent: prefer_non_empty(&incoming.user_intent, &existing.user_intent),
        intent_analysis: if incoming.intent_analysis.confidence
            >= existing.intent_analysis.confidence
        {
            incoming.intent_analysis.clone()
        } else {
            existing.intent_analysis.clone()
        },
        memory_context: prefer_non_empty(&incoming.memory_context, &existing.memory_context),
        commands: merge_string_lists(&existing.commands, &incoming.commands),
        blockers: merge_string_lists(&existing.blockers, &incoming.blockers),
        todos: merge_string_lists(&existing.todos, &incoming.todos),
        open_questions: merge_string_lists(&existing.open_questions, &incoming.open_questions),
        results: merge_string_lists(&existing.results, &incoming.results),
        related_tools: merge_string_lists(&existing.related_tools, &incoming.related_tools),
        related_agents: merge_string_lists(&existing.related_agents, &incoming.related_agents),
        related_projects: merge_string_lists(
            &existing.related_projects,
            &incoming.related_projects,
        ),
        raw_evidence: prefer_non_empty(&incoming.raw_evidence, &existing.raw_evidence),
        search_aliases: merge_string_lists(&existing.search_aliases, &incoming.search_aliases),
        related_memory_ids: merge_string_lists(
            &existing.related_memory_ids,
            &incoming.related_memory_ids,
        ),
        graph_node_ids: merge_string_lists(&existing.graph_node_ids, &incoming.graph_node_ids),
        graph_edge_ids: merge_string_lists(&existing.graph_edge_ids, &incoming.graph_edge_ids),
        project_confidence: existing.project_confidence.max(incoming.project_confidence),
        topic_confidence: existing.topic_confidence.max(incoming.topic_confidence),
        workflow_confidence: existing
            .workflow_confidence
            .max(incoming.workflow_confidence),
        project_evidence: merge_string_lists(
            &existing.project_evidence,
            &incoming.project_evidence,
        ),
        related_project_ids: merge_string_lists(
            &existing.related_project_ids,
            &incoming.related_project_ids,
        ),
        evidence_confidence: existing
            .evidence_confidence
            .max(incoming.evidence_confidence),
        confidence_score: existing.confidence_score.max(incoming.confidence_score),
        importance_score: existing.importance_score.max(incoming.importance_score),
        specificity_score: existing.specificity_score.max(incoming.specificity_score),
        intent_score: existing.intent_score.max(incoming.intent_score),
        entity_score: existing.entity_score.max(incoming.entity_score),
        agent_usefulness_score: existing
            .agent_usefulness_score
            .max(incoming.agent_usefulness_score),
        ocr_noise_score: existing.ocr_noise_score.max(incoming.ocr_noise_score),
        graph_readiness_score: existing
            .graph_readiness_score
            .max(incoming.graph_readiness_score),
        retrieval_value_score: existing
            .retrieval_value_score
            .max(incoming.retrieval_value_score),
        storage_outcome: prefer_non_empty(&incoming.storage_outcome, &existing.storage_outcome),
        quality_gate_reason: prefer_non_empty(
            &incoming.quality_gate_reason,
            &existing.quality_gate_reason,
        ),
        extracted_entities_structured: if incoming.extracted_entities_structured.len()
            >= existing.extracted_entities_structured.len()
        {
            incoming.extracted_entities_structured.clone()
        } else {
            existing.extracted_entities_structured.clone()
        },
        action_items: if incoming.action_items.len() >= existing.action_items.len() {
            incoming.action_items.clone()
        } else {
            existing.action_items.clone()
        },
        schema_version,
        activity_type,
        files_touched,
        symbols_changed,
        session_duration_mins,
        project,
        tags,
        entities,
        decisions,
        errors,
        next_steps,
        git_stats,
        outcome,
        extraction_confidence,
        anchor_coverage_score: existing
            .anchor_coverage_score
            .max(incoming.anchor_coverage_score),
        content_hash: prefer_non_empty(&incoming.content_hash, &existing.content_hash),
        dedup_fingerprint,
        embedding_text,
        embedding_model,
        embedding_dim,
        is_consolidated: existing.is_consolidated || incoming.is_consolidated,
        is_soft_deleted: existing.is_soft_deleted || incoming.is_soft_deleted,
        parent_id,
        related_ids,
        consolidated_from,
    }
}

fn prefer_non_empty(incoming: &str, existing: &str) -> String {
    let incoming_trim = incoming.trim();
    if !incoming_trim.is_empty() {
        incoming_trim.to_string()
    } else {
        existing.trim().to_string()
    }
}

fn merge_string_lists(existing: &[String], incoming: &[String]) -> Vec<String> {
    let mut merged = Vec::with_capacity(existing.len() + incoming.len());
    for value in existing.iter().chain(incoming.iter()) {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !merged
            .iter()
            .any(|item: &String| item.eq_ignore_ascii_case(trimmed))
        {
            merged.push(trimmed.to_string());
        }
    }
    merged
}

fn semantic_embeddings_enabled(text_embedder: Option<&Embedder>) -> bool {
    matches!(
        text_embedder.map(|embedder| embedder.backend()),
        Some(EmbeddingBackend::Real)
    )
}

fn choose_story_title(existing: &str, incoming: &str) -> String {
    let existing_trim = existing.trim();
    let incoming_trim = incoming.trim();
    if existing_trim.is_empty() {
        return incoming_trim.to_string();
    }
    if incoming_trim.is_empty() {
        return existing_trim.to_string();
    }
    if incoming_trim.len() > existing_trim.len() {
        incoming_trim.to_string()
    } else {
        existing_trim.to_string()
    }
}

fn merge_story_text(existing: &str, incoming: &str, max_chars: usize) -> String {
    let existing_trim = existing.trim();
    let incoming_trim = incoming.trim();
    if existing_trim.is_empty() {
        return trim_chars(incoming_trim, max_chars);
    }
    if incoming_trim.is_empty() {
        return trim_chars(existing_trim, max_chars);
    }

    let normalized_existing = normalize_text_for_overlap(existing_trim);
    let normalized_incoming = normalize_text_for_overlap(incoming_trim);
    if normalized_existing.contains(&normalized_incoming) {
        return trim_chars(existing_trim, max_chars);
    }
    if normalized_incoming.contains(&normalized_existing) {
        return trim_chars(incoming_trim, max_chars);
    }

    let mut merged = existing_trim.to_string();
    let mut merged_norm = normalized_existing;
    for segment in incoming_trim
        .split(['\n', '.', '!', '?', ';'])
        .map(str::trim)
        .filter(|segment| segment.len() >= 12)
    {
        let normalized_segment = normalize_text_for_overlap(segment);
        if normalized_segment.is_empty() || merged_norm.contains(&normalized_segment) {
            continue;
        }
        merged.push_str(" • ");
        merged.push_str(segment);
        merged_norm.push(' ');
        merged_norm.push_str(&normalized_segment);
        if merged.chars().count() >= max_chars {
            break;
        }
    }
    trim_chars(&merged, max_chars)
}

fn score_search_candidate(incoming: &MemoryRecord, candidate: &SearchResult) -> MergeScore {
    let snippet_similarity = token_overlap(&incoming.snippet, &candidate.snippet);
    let title_similarity = token_overlap(&incoming.window_title, &candidate.window_title);
    let text_similarity = token_overlap(
        &trim_chars(&incoming.clean_text, 1000),
        &trim_chars(&candidate.clean_text, 1000),
    );
    let shadow_similarity = token_overlap(&incoming.lexical_shadow, &candidate.lexical_shadow);
    let lexical = snippet_similarity * 0.42
        + title_similarity * 0.26
        + text_similarity * 0.2
        + shadow_similarity * 0.12;
    let vector = candidate.score.clamp(0.0, 1.0);

    let anchor_match = continuity_anchor_for_memory(incoming)
        .zip(continuity_anchor_for_search_result(candidate))
        .map(|(left, right)| left == right)
        .unwrap_or(false);

    let same_domain = incoming
        .url
        .as_deref()
        .and_then(extract_domain)
        .zip(candidate.url.as_deref().and_then(extract_domain))
        .map(|(left, right)| left == right)
        .unwrap_or(false);

    let mut score = vector * 0.5 + lexical * 0.42;
    if same_domain {
        score += 0.08;
    }
    if anchor_match {
        score += 0.32;
    }

    MergeScore {
        score,
        lexical,
        vector,
        anchor_match,
    }
}

pub(crate) fn score_memory_candidate(
    incoming: &MemoryRecord,
    candidate: &MemoryRecord,
) -> MergeScore {
    let snippet_similarity = token_overlap(&incoming.snippet, &candidate.snippet);
    let title_similarity = token_overlap(&incoming.window_title, &candidate.window_title);
    let text_similarity = token_overlap(
        &trim_chars(&incoming.clean_text, 1000),
        &trim_chars(&candidate.clean_text, 1000),
    );
    let shadow_similarity = token_overlap(&incoming.lexical_shadow, &candidate.lexical_shadow);
    let lexical = snippet_similarity * 0.42
        + title_similarity * 0.26
        + text_similarity * 0.2
        + shadow_similarity * 0.12;
    let vector = cosine_similarity(&incoming.embedding, &candidate.embedding).clamp(0.0, 1.0);

    let anchor_match = continuity_anchor_for_memory(incoming)
        .zip(continuity_anchor_for_memory(candidate))
        .map(|(left, right)| left == right)
        .unwrap_or(false);

    let same_domain = incoming
        .url
        .as_deref()
        .and_then(extract_domain)
        .zip(candidate.url.as_deref().and_then(extract_domain))
        .map(|(left, right)| left == right)
        .unwrap_or(false);

    let mut score = vector * 0.5 + lexical * 0.42;
    if same_domain {
        score += 0.08;
    }
    if anchor_match {
        score += 0.32;
    }

    MergeScore {
        score,
        lexical,
        vector,
        anchor_match,
    }
}

fn score_search_candidate_lexical(incoming: &MemoryRecord, candidate: &SearchResult) -> MergeScore {
    let base = score_search_candidate(incoming, candidate);
    let same_url = matching_effective_url(incoming.url.as_deref(), candidate.url.as_deref());
    let same_domain = same_domain(incoming.url.as_deref(), candidate.url.as_deref());

    let mut score = base.lexical * 0.9;
    if same_domain {
        score += 0.08;
    }
    if same_url {
        score += 0.14;
    }
    if base.anchor_match {
        score += 0.24;
    }

    MergeScore {
        score,
        lexical: base.lexical,
        vector: base.vector,
        anchor_match: base.anchor_match,
    }
}

fn score_memory_candidate_lexical(incoming: &MemoryRecord, candidate: &MemoryRecord) -> MergeScore {
    let base = score_memory_candidate(incoming, candidate);
    let same_url = matching_effective_url(incoming.url.as_deref(), candidate.url.as_deref());
    let same_domain = same_domain(incoming.url.as_deref(), candidate.url.as_deref());

    let mut score = base.lexical * 0.9;
    if same_domain {
        score += 0.08;
    }
    if same_url {
        score += 0.14;
    }
    if base.anchor_match {
        score += 0.24;
    }

    MergeScore {
        score,
        lexical: base.lexical,
        vector: base.vector,
        anchor_match: base.anchor_match,
    }
}

pub(crate) fn passes_merge_threshold(score: MergeScore) -> bool {
    if score.anchor_match {
        return score.score >= 0.58 && score.lexical >= 0.18;
    }
    if score.lexical >= 0.72 && score.score >= 0.80 {
        return true;
    }
    score.score >= 0.86 && score.vector >= 0.82 && score.lexical >= 0.28
}

fn passes_lexical_merge_threshold(score: MergeScore) -> bool {
    if score.anchor_match {
        return score.lexical >= 0.24 && score.score >= 0.46;
    }
    score.lexical >= 0.66 && score.score >= 0.74
}

fn lexical_merge_query(record: &MemoryRecord) -> String {
    let text = format!(
        "{} {} {} {}",
        record.window_title,
        record.snippet,
        trim_chars(&record.clean_text, 500),
        record.lexical_shadow
    );
    text.split_whitespace()
        .take(48)
        .collect::<Vec<_>>()
        .join(" ")
}

fn allows_cross_app_merge_from_memory(
    incoming: &MemoryRecord,
    candidate: &MemoryRecord,
    score: MergeScore,
) -> bool {
    if !is_cross_app_merge_window(incoming.timestamp, candidate.timestamp) {
        return false;
    }
    if matching_effective_url(incoming.url.as_deref(), candidate.url.as_deref()) {
        return true;
    }
    if !same_domain(incoming.url.as_deref(), candidate.url.as_deref()) {
        return false;
    }
    (score.anchor_match && score.lexical >= 0.52) || (score.vector >= 0.93 && score.lexical >= 0.70)
}

fn allows_cross_app_merge_from_search(
    incoming: &MemoryRecord,
    candidate: &SearchResult,
    score: MergeScore,
) -> bool {
    if !is_cross_app_merge_window(incoming.timestamp, candidate.timestamp) {
        return false;
    }
    if matching_effective_url(incoming.url.as_deref(), candidate.url.as_deref()) {
        return true;
    }
    if !same_domain(incoming.url.as_deref(), candidate.url.as_deref()) {
        return false;
    }
    (score.anchor_match && score.lexical >= 0.52) || (score.vector >= 0.93 && score.lexical >= 0.70)
}

fn is_cross_app_merge_window(left_ts: i64, right_ts: i64) -> bool {
    (left_ts - right_ts).abs() <= 45 * 60 * 1000
}

fn matching_effective_url(left: Option<&str>, right: Option<&str>) -> bool {
    let Some(left) = left else {
        return false;
    };
    let Some(right) = right else {
        return false;
    };
    normalize_url_for_merge(left) == normalize_url_for_merge(right)
}

fn normalize_url_for_merge(raw: &str) -> String {
    let lowered = raw.trim().to_lowercase();
    if lowered.is_empty() {
        return String::new();
    }
    let no_scheme = lowered
        .strip_prefix("https://")
        .or_else(|| lowered.strip_prefix("http://"))
        .unwrap_or(&lowered);
    let no_query = no_scheme.split('?').next().unwrap_or(no_scheme);
    let no_fragment = no_query.split('#').next().unwrap_or(no_query);
    no_fragment.trim_end_matches('/').to_string()
}

fn same_domain(left: Option<&str>, right: Option<&str>) -> bool {
    left.and_then(extract_domain)
        .zip(right.and_then(extract_domain))
        .map(|(l, r)| l.eq_ignore_ascii_case(&r))
        .unwrap_or(false)
}

pub(crate) fn continuity_anchor_for_memory(record: &MemoryRecord) -> Option<String> {
    continuity_anchor(
        &record.app_name,
        record.url.as_deref(),
        &record.window_title,
        &record.snippet,
    )
}

fn continuity_anchor_for_search_result(result: &SearchResult) -> Option<String> {
    continuity_anchor(
        &result.app_name,
        result.url.as_deref(),
        &result.window_title,
        &result.snippet,
    )
}

fn continuity_anchor(
    app_name: &str,
    url: Option<&str>,
    window_title: &str,
    snippet: &str,
) -> Option<String> {
    if let Some(raw_url) = url {
        if let Some(domain) = extract_domain(raw_url) {
            let domain_key = domain.to_lowercase();
            let path = extract_first_path_segments(raw_url, 3).unwrap_or_default();
            if !path.is_empty() {
                return Some(format!("url:{domain_key}:{path}"));
            }
            if !domain_key.is_empty() {
                return Some(format!("url:{domain_key}"));
            }
        }
    }

    let app_key = normalize_app_key(app_name);

    let generic_title = normalize_anchor_text(window_title);
    if generic_title.len() >= 8 {
        return Some(format!("app:{app_key}:title:{generic_title}"));
    }

    let generic_snippet = normalize_anchor_text(snippet);
    if generic_snippet.len() >= 10 {
        return Some(format!("app:{app_key}:snippet:{generic_snippet}"));
    }

    None
}

fn extract_first_path_segments(url: &str, count: usize) -> Option<String> {
    let without_scheme = url.split("://").nth(1).unwrap_or(url);
    let mut parts = without_scheme.split('/');
    let _host = parts.next()?;
    let segments: Vec<String> = parts
        .filter(|part| !part.trim().is_empty())
        .map(|part| part.trim().to_lowercase())
        .take(count)
        .collect();
    if segments.is_empty() {
        None
    } else {
        Some(segments.join("/"))
    }
}

fn normalize_app_key(app_name: &str) -> String {
    let normalized = app_name
        .to_lowercase()
        .chars()
        .map(|ch| if ch.is_alphanumeric() { ch } else { '_' })
        .collect::<String>();
    let cleaned = normalized
        .split('_')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    if cleaned.is_empty() {
        "unknown".to_string()
    } else {
        cleaned
    }
}

fn normalize_anchor_text(text: &str) -> String {
    text.to_lowercase()
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| token.len() > 2)
        .filter(|token| !is_generic_stop_word(token))
        .take(8)
        .collect::<Vec<_>>()
        .join("_")
}

fn token_overlap(left: &str, right: &str) -> f32 {
    let left_tokens = tokenize(left);
    let right_tokens = tokenize(right);
    if left_tokens.is_empty() || right_tokens.is_empty() {
        return 0.0;
    }

    let intersection = left_tokens.intersection(&right_tokens).count() as f32;
    let union = left_tokens.union(&right_tokens).count() as f32;
    if union == 0.0 {
        0.0
    } else {
        intersection / union
    }
}

fn tokenize(text: &str) -> HashSet<String> {
    text.to_lowercase()
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| token.len() > 2)
        .filter(|token| !is_generic_stop_word(token))
        .map(|token| token.to_string())
        .collect()
}

fn is_generic_stop_word(token: &str) -> bool {
    matches!(
        token,
        "the"
            | "and"
            | "for"
            | "with"
            | "this"
            | "that"
            | "from"
            | "your"
            | "you"
            | "are"
            | "was"
            | "were"
            | "have"
            | "has"
            | "into"
            | "about"
            | "after"
            | "before"
            | "then"
            | "just"
            | "there"
            | "here"
            | "user"
            | "app"
            | "window"
            | "tab"
            | "page"
            | "open"
            | "opened"
            | "search"
            | "searched"
            | "www"
            | "http"
            | "https"
            | "com"
    )
}

fn cosine_similarity(left: &[f32], right: &[f32]) -> f32 {
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let len = left.len().min(right.len());
    if len == 0 {
        return 0.0;
    }

    let mut dot = 0.0;
    let mut left_norm = 0.0;
    let mut right_norm = 0.0;
    for index in 0..len {
        let a = left[index];
        let b = right[index];
        dot += a * b;
        left_norm += a * a;
        right_norm += b * b;
    }

    if left_norm <= f32::EPSILON || right_norm <= f32::EPSILON {
        return 0.0;
    }

    dot / (left_norm.sqrt() * right_norm.sqrt())
}

fn normalize_text_for_overlap(text: &str) -> String {
    text.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn trim_chars(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect::<String>()
}

fn cleanup_screenshot_path(path: Option<String>) {
    let Some(path) = path else {
        return;
    };
    let _ = std::fs::remove_file(path);
}

fn purge_capture_artifacts(frames_dir: PathBuf) {
    if frames_dir.exists() {
        if let Err(err) = std::fs::remove_dir_all(&frames_dir) {
            tracing::debug!("Capture artifact purge skipped: {}", err);
            return;
        }
    }
    let _ = std::fs::create_dir_all(frames_dir);
}

async fn maybe_create_tasks_from_memory(
    state: &AppState,
    record: &MemoryRecord,
    engine: Option<&Arc<crate::inference::InferenceEngine>>,
) -> Result<(), String> {
    // Only run task extraction for summarized memories to keep precision high.
    if !record.summary_source.eq_ignore_ascii_case("llm") {
        return Ok(());
    }

    let Some(engine) = engine else {
        return Ok(());
    };

    if record.snippet.trim().len() < 16 {
        return Ok(());
    }

    let extraction_input = format!(
        "APP: {}\nWINDOW: {}\nSUMMARY: {}\nTEXT: {}",
        record.app_name,
        record.window_title,
        record.snippet,
        record.clean_text.chars().take(800).collect::<String>()
    );
    let raw = engine.extract_todos(&extraction_input).await;
    if raw.trim().is_empty() {
        return Ok(());
    }

    let mut parsed = parse_tasks_from_llm_response(&raw, &record.app_name);
    if parsed.is_empty() {
        return Ok(());
    }

    let mut all_tasks = state.store.list_tasks().await.map_err(|e| e.to_string())?;
    let mut active_keys: HashSet<(String, String)> = all_tasks
        .iter()
        .filter(|task| !task.is_completed && !task.is_dismissed)
        .map(|task| {
            (
                task.title.trim().to_lowercase(),
                task_type_key(&task.task_type).to_string(),
            )
        })
        .collect();

    let source_app = format!("Memory:{}", record.app_name);
    let mut changed = false;
    for task in parsed.iter_mut() {
        let normalized_title = task.title.trim().to_lowercase();
        if normalized_title.len() < 4 {
            continue;
        }

        let type_key = task_type_key(&task.task_type).to_string();
        let dedupe_key = (normalized_title, type_key);
        if active_keys.contains(&dedupe_key) {
            continue;
        }
        active_keys.insert(dedupe_key);

        task.id = uuid::Uuid::new_v4().to_string();
        task.created_at = record.timestamp;
        task.source_app = source_app.clone();
        task.source_memory_id = Some(record.id.clone());
        task.linked_memory_ids = vec![record.id.clone()];
        task.linked_urls = record.url.clone().map(|u| vec![u]).unwrap_or_default();

        all_tasks.push(Task {
            id: task.id.clone(),
            title: task.title.clone(),
            description: task.description.clone(),
            source_app: task.source_app.clone(),
            source_memory_id: task.source_memory_id.clone(),
            created_at: task.created_at,
            due_date: task.due_date,
            is_completed: false,
            is_dismissed: false,
            task_type: task.task_type.clone(),
            linked_urls: task.linked_urls.clone(),
            linked_memory_ids: task.linked_memory_ids.clone(),
        });
        changed = true;
    }

    if !changed {
        return Ok(());
    }

    state
        .store
        .upsert_tasks(&all_tasks)
        .await
        .map_err(|e| e.to_string())?;

    // Link created tasks into the graph for task-memory navigation.
    for task in all_tasks.iter().rev().take(8) {
        if task
            .source_memory_id
            .as_ref()
            .map(|id| id == &record.id)
            .unwrap_or(false)
        {
            if let Err(err) = state.graph.link_task(task).await {
                tracing::warn!("Failed linking auto-created task in graph: {}", err);
            }
        }
    }

    Ok(())
}

fn task_type_key(task_type: &TaskType) -> &'static str {
    match task_type {
        TaskType::Todo => "todo",
        TaskType::Reminder => "reminder",
        TaskType::Followup => "followup",
    }
}

fn build_session_key(app_name: &str, window_title: &str, url: Option<&str>) -> String {
    let app = app_name.trim().to_lowercase().replace(' ', "_");
    let title = window_title
        .trim()
        .to_lowercase()
        .chars()
        .filter(|ch| ch.is_alphanumeric() || *ch == ' ')
        .collect::<String>()
        .split_whitespace()
        .take(5)
        .collect::<Vec<_>>()
        .join("_");
    let domain = url
        .and_then(extract_domain)
        .unwrap_or_default()
        .replace('.', "_");

    if !domain.is_empty() {
        format!("{}:{}:{}", app, domain, title)
    } else {
        format!("{}:{}", app, title)
    }
}

fn build_session_id(
    now: &chrono::DateTime<Local>,
    app_name: &str,
    bundle_id: Option<&str>,
    session_key: &str,
) -> String {
    let app = bundle_id
        .unwrap_or(app_name)
        .to_ascii_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    let slot = ((now.hour() * 60 + now.minute()) / 30).min(47);
    let session_anchor = session_key
        .split(':')
        .nth(1)
        .unwrap_or("general")
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    format!(
        "{}-{}-{}-s{:02}",
        now.format("%Y%m%d"),
        app,
        session_anchor,
        slot
    )
}

fn extract_domain(url: &str) -> Option<String> {
    let without_scheme = url.split("://").nth(1).unwrap_or(url);
    let host = without_scheme.split('/').next()?.trim();
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn merge_test_record(id: &str) -> MemoryRecord {
        MemoryRecord {
            id: id.to_string(),
            timestamp: 1,
            day_bucket: "2026-05-01".to_string(),
            app_name: "Google Chrome".to_string(),
            bundle_id: Some("com.google.Chrome".to_string()),
            window_title: "Hacker News".to_string(),
            session_id: "20260501-com.google.Chrome".to_string(),
            text: String::new(),
            clean_text: "Hacker News page text about AI budgets and developer tooling.".to_string(),
            ocr_confidence: 0.8,
            ocr_block_count: 12,
            snippet: "Browsed Hacker News.".to_string(),
            summary_source: "llm".to_string(),
            noise_score: 0.0,
            session_key: "google_chrome:news.ycombinator.com".to_string(),
            lexical_shadow: "Hacker News AI budgets developer tooling".to_string(),
            embedding: vec![0.1; EMBEDDING_DIM],
            image_embedding: vec![0.0; DEFAULT_IMAGE_EMBEDDING_DIM],
            screenshot_path: None,
            url: Some("https://news.ycombinator.com".to_string()),
            snippet_embedding: vec![0.2; EMBEDDING_DIM],
            support_embedding: vec![0.3; EMBEDDING_DIM],
            decay_score: 1.0,
            last_accessed_at: 0,
            ..Default::default()
        }
    }

    #[test]
    fn privacy_exclusion_blocks_capture_before_ocr() {
        let blocklist = vec!["1Password".to_string(), "bank.example.com".to_string()];

        assert!(should_skip_capture_context(
            "1Password",
            Some("com.1password.1password"),
            "Vault",
            None,
            &blocklist,
        ));
        assert!(should_skip_capture_context(
            "Chrome",
            Some("com.google.Chrome"),
            "Account overview",
            Some("https://bank.example.com/accounts"),
            &blocklist,
        ));
        assert!(!should_skip_capture_context(
            "Chrome",
            Some("com.google.Chrome"),
            "FNDR architecture notes",
            Some("https://docs.example.com/fndr"),
            &blocklist,
        ));
    }

    #[test]
    fn surface_policy_skips_known_navigation_results_pages() {
        let policy = classify_capture_surface_policy(
            "Google Chrome",
            "Search results - YouTube",
            Some("https://www.youtube.com/results?search_query=screenpipe"),
        );
        assert_eq!(policy, CaptureSurfacePolicy::SkipFrame);
    }

    #[test]
    fn surface_policy_uses_url_only_for_channel_listing_pages() {
        let policy = classify_capture_surface_policy(
            "Google Chrome",
            "screen_pipe - YouTube",
            Some("https://www.youtube.com/@screen_pipe/videos"),
        );
        assert_eq!(policy, CaptureSurfacePolicy::UrlOnly);
    }

    #[test]
    fn surface_policy_allows_normal_article_capture() {
        let policy = classify_capture_surface_policy(
            "Google Chrome",
            "Screenpipe Architecture Deep Dive",
            Some("https://docs.screenpi.pe/architecture/memory-cards"),
        );
        assert_eq!(policy, CaptureSurfacePolicy::Normal);
    }

    #[test]
    fn session_id_is_sub_day_and_domain_anchored() {
        let now = chrono::TimeZone::with_ymd_and_hms(&Local, 2026, 5, 6, 21, 46, 0)
            .single()
            .expect("local datetime");
        let id = build_session_id(
            &now,
            "Google Chrome",
            Some("com.google.Chrome"),
            "google_chrome:youtube_com:screenpipe",
        );
        assert!(id.starts_with("20260506-com.google.chrome-youtube_com-s"));
    }

    #[test]
    fn semantic_structured_fallback_builds_grounded_fields() {
        let semantic = macos::BrowserSemanticContent {
            title: "Screenpipe architecture deep dive".to_string(),
            meta_description: "A walkthrough of memory card indexing and retrieval.".to_string(),
            h1: "Screenpipe architecture deep dive".to_string(),
            article_excerpt:
                "Screenpipe memory cards connect OCR cleanup, retrieval ranking, and context grounding."
                    .to_string(),
            nav_ratio: 0.08,
            content_signal_score: 0.84,
        };
        let extraction = build_structured_from_browser_semantics(
            "Google Chrome",
            "Screenpipe architecture deep dive",
            Some("https://docs.screenpi.pe/architecture"),
            &semantic,
        )
        .expect("semantic extraction");

        assert!(!extraction.topic.trim().is_empty());
        assert!(!extraction.memory_context.trim().is_empty());
        assert!(extraction.confidence >= 0.35);
        assert!(!extraction.entities.is_empty());
    }

    #[test]
    fn lightweight_entities_extracts_named_tokens() {
        let entities = lightweight_entities_from_text(
            "Screenpipe integrates Obsidian, Apple Intelligence, and Toggl reports.",
        );
        assert!(entities
            .iter()
            .any(|e| e.eq_ignore_ascii_case("Screenpipe")));
        assert!(entities.iter().any(|e| e.eq_ignore_ascii_case("Obsidian")));
    }

    #[tokio::test]
    async fn merge_preserves_v2_metadata_when_incoming_is_sparse() {
        let mut existing = merge_test_record("existing");
        existing.schema_version = 2;
        existing.activity_type = "research".to_string();
        existing.tags = vec!["ai".to_string(), "tooling".to_string()];
        existing.entities = vec!["Hacker News".to_string()];
        existing.decisions = vec!["Track browser research sessions".to_string()];
        existing.project = "FNDR".to_string();
        existing.outcome = "in_progress".to_string();
        existing.extraction_confidence = 0.9;
        existing.dedup_fingerprint = "hn_research".to_string();
        existing.embedding_model = "bge-large-en-v1.5".to_string();
        existing.embedding_dim = EMBEDDING_DIM as u32;

        let mut incoming = merge_test_record("incoming");
        incoming.timestamp = 2;
        incoming.clean_text = "More Hacker News page text about Claude Code.".to_string();
        incoming.snippet = "Continued browsing Hacker News.".to_string();
        incoming.schema_version = 0;
        incoming.activity_type = String::new();
        incoming.tags = Vec::new();
        incoming.entities = Vec::new();
        incoming.decisions = Vec::new();
        incoming.project = String::new();
        incoming.outcome = String::new();
        incoming.extraction_confidence = 0.0;
        incoming.dedup_fingerprint = String::new();
        incoming.embedding_model = String::new();
        incoming.embedding_dim = 0;

        let merged =
            merge_memory_records_with_policy(existing, incoming, None, None, false, false).await;

        assert_eq!(merged.schema_version, 2);
        assert_eq!(merged.activity_type, "research");
        assert_eq!(merged.project, "FNDR");
        assert_eq!(merged.tags, vec!["ai".to_string(), "tooling".to_string()]);
        assert_eq!(merged.entities, vec!["Hacker News".to_string()]);
        assert_eq!(
            merged.decisions,
            vec!["Track browser research sessions".to_string()]
        );
        assert_eq!(merged.outcome, "in_progress");
        assert_eq!(merged.extraction_confidence, 0.9);
        assert_eq!(merged.dedup_fingerprint, "hn_research");
        assert_eq!(merged.embedding_model, "bge-large-en-v1.5");
        assert_eq!(merged.embedding_dim, EMBEDDING_DIM as u32);
    }

    #[tokio::test]
    async fn merge_unions_v2_list_metadata_without_duplicates() {
        let mut existing = merge_test_record("existing");
        existing.tags = vec!["AI".to_string(), "tooling".to_string()];
        existing.files_touched = vec!["src-tauri/src/store/lance_store.rs".to_string()];

        let mut incoming = merge_test_record("incoming");
        incoming.tags = vec!["ai".to_string(), "browser".to_string()];
        incoming.files_touched = vec![
            "src-tauri/src/store/lance_store.rs".to_string(),
            "src-tauri/src/capture/mod.rs".to_string(),
        ];

        let merged =
            merge_memory_records_with_policy(existing, incoming, None, None, false, false).await;

        assert_eq!(
            merged.tags,
            vec![
                "AI".to_string(),
                "tooling".to_string(),
                "browser".to_string()
            ]
        );
        assert_eq!(
            merged.files_touched,
            vec![
                "src-tauri/src/store/lance_store.rs".to_string(),
                "src-tauri/src/capture/mod.rs".to_string()
            ]
        );
    }

    #[test]
    fn extraction_validator_strips_unsupported_fields_when_low_confidence() {
        let mut extraction = StructuredMemoryExtraction {
            confidence: 0.42,
            project: "Skunkworks".to_string(),
            user_intent: "Finalize launch budget".to_string(),
            topic: "Revenue model".to_string(),
            files_touched: vec!["src-tauri/src/capture/mod.rs".to_string()],
            entities: vec!["Jane Doe".to_string()],
            dedup_fingerprint: "bad fingerprint ###".to_string(),
            ..Default::default()
        };

        let (grounding, issues) = validate_structured_memory_extraction(
            &mut extraction,
            "Google Chrome",
            "Random docs page",
            "Navigation links and generic toolbar labels",
        );

        assert!(grounding < 0.55);
        assert!(issues
            .iter()
            .any(|item| item == "structured_fields_weakly_grounded"));
        assert!(issues
            .iter()
            .any(|item| item == "possible_ungrounded_extraction"));
        assert!(issues
            .iter()
            .any(|item| item == "unsupported_dedup_fingerprint"));
        assert!(extraction.project.is_empty());
        assert!(extraction.user_intent.is_empty());
        assert!(extraction.topic.is_empty());
        assert!(extraction.files_touched.is_empty());
        assert!(extraction.entities.is_empty());
        assert!(extraction.dedup_fingerprint.is_empty());
    }

    #[test]
    fn extraction_validator_keeps_supported_fields_when_grounded() {
        let mut extraction = StructuredMemoryExtraction {
            confidence: 0.91,
            project: "FNDR".to_string(),
            user_intent: "Improve memory card search ranking".to_string(),
            topic: "ranking quality".to_string(),
            files_touched: vec!["src-tauri/src/search/memory_cards.rs".to_string()],
            entities: vec!["MemoryCardSynthesizer".to_string()],
            dedup_fingerprint: "fndr:ranking:memory_cards".to_string(),
            ..Default::default()
        };

        let (grounding, issues) = validate_structured_memory_extraction(
            &mut extraction,
            "Codex",
            "memory_cards.rs",
            "Improved memory card search ranking quality in src-tauri/src/search/memory_cards.rs using MemoryCardSynthesizer",
        );

        assert!(grounding > 0.80);
        assert!(!issues
            .iter()
            .any(|item| item == "possible_ungrounded_extraction"));
        assert_eq!(extraction.project, "FNDR");
        assert_eq!(extraction.user_intent, "Improve memory card search ranking");
        assert_eq!(extraction.files_touched.len(), 1);
        assert_eq!(extraction.entities.len(), 1);
        assert_eq!(extraction.dedup_fingerprint, "fndr:ranking:memory_cards");
    }

    #[test]
    fn primary_embedding_text_is_structured_first_with_capped_evidence() {
        let extraction = StructuredMemoryExtraction {
            user_intent: "Refactor OCR scoring".to_string(),
            project: "FNDR".to_string(),
            topic: "capture quality".to_string(),
            entities: vec!["Apple Vision".to_string()],
            files_touched: vec!["src-tauri/src/capture/text_cleanup.rs".to_string()],
            results: vec!["Reduced low-signal capture writes".to_string()],
            ..Default::default()
        };
        let long_text = "evidence ".repeat(120);
        let text = compose_primary_embedding_text(
            Some(&extraction),
            "Codex",
            "capture/text_cleanup.rs",
            "Improved OCR cleanup and grounding checks.",
            "Implemented structured-first embedding context.",
            &long_text,
            "ocr cleanup grounding quality",
        );

        assert!(text.contains("intent: Refactor OCR scoring"));
        assert!(text.contains("project: FNDR"));
        assert!(text.contains("files: src-tauri/src/capture/text_cleanup.rs"));
        assert!(text.contains("evidence: "));
        assert!(!text.contains(&"evidence ".repeat(80)));
    }

    #[test]
    fn weighted_primary_embedding_prefers_primary_vector() {
        let merged = weighted_primary_embedding(&[1.0, 0.0], &[0.0, 1.0], &[0.0, 1.0]);
        assert!(
            merged[0] > merged[1],
            "primary component should dominate weighted output"
        );
        let norm = (merged.iter().map(|value| value * value).sum::<f32>()).sqrt();
        assert!((norm - 1.0).abs() < 1e-3);
    }
}
