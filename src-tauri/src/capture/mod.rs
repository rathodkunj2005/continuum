//! Capture pipeline
//!
//! Samples the foreground screen, blocks private contexts before OCR, extracts
//! Apple Vision text, embeds cleaned chunks, and batches memory records into
//! LanceDB.

mod admission;
pub mod clipboard;
mod dedupe;
pub mod entity_extractor;
pub(crate) mod macos;
pub mod permissions;
mod sampling;
pub mod text_cleanup;

use admission::{classify_capture_surface_policy, CaptureSurfacePolicy};
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

use crate::config::{
    CapturePipelineConfig, DEFAULT_CAPTURE_EMBEDDING_CACHE_SIZE, DEFAULT_IMAGE_EMBEDDING_DIM,
};
use crate::context_runtime;
use crate::embedding::{embed_imported_image, Embedder, EmbeddingBackend, EMBEDDING_DIM};
use crate::inference::vlm_router::{should_run_vlm, VlmRouteDecision, VlmRouteInput};
use crate::inference::{
    compose_import_memory_context, compose_import_memory_context_with_title,
    extract_image_semantics, ImageImportSource, StructuredMemoryExtraction,
};
use crate::memory::reopen::build_reopen_target;
use crate::memory_compaction::{
    build_lexical_shadow, build_lexical_shadow_with_aliases, compact_summary_embedding_text,
    mean_pool_embeddings, support_embedding_texts,
};
use crate::memory_quality::{deterministic_dedup_fingerprint, is_supported_dedup_fingerprint};
use crate::models;
use crate::ocr::{OcrEngine, RecognizedText};
use crate::privacy::Blocklist;
use crate::storage::{MemoryRecord, SearchResult, Task, TaskType};
use crate::summariser::narration_filter::clean_or_fallback_display_summary;
use crate::tasks::parse_tasks_from_llm_response;
use crate::telemetry::quality_logger::append_quality_event;
use crate::telemetry::runtime_metrics;
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

/// Per-session adaptive admission for the visual-narrative path.
///
/// Tracks recent CLIP image vectors and the count of visual-only admits in
/// the current session. A candidate frame is admitted iff
/// `novelty(candidate) >= base + alpha * admitted` (capped at `ceiling`),
/// so the gate self-throttles as more frames from the same scene/session
/// land. Resets whenever the session key changes (typically when the
/// frontmost app, window title, or URL changes).
#[derive(Default)]
struct VisualNoveltyTracker {
    session_key: String,
    recent: VecDeque<Vec<f32>>,
    admitted: u32,
}

impl VisualNoveltyTracker {
    fn reset_for(&mut self, session_key: &str) {
        if self.session_key != session_key {
            self.session_key.clear();
            self.session_key.push_str(session_key);
            self.recent.clear();
            self.admitted = 0;
        }
    }

    /// `1.0 - max(cosine_similarity)` against the ring; `1.0` when the
    /// ring is empty so the first frame is always considered fully novel.
    fn novelty(&self, vec: &[f32]) -> f32 {
        if self.recent.is_empty() {
            return 1.0;
        }
        let mut max_sim = -1.0_f32;
        for r in &self.recent {
            let s = cosine_similarity(vec, r);
            if s > max_sim {
                max_sim = s;
            }
        }
        (1.0 - max_sim).clamp(0.0, 1.0)
    }

    fn adaptive_threshold(&self, base: f32, alpha: f32, ceiling: f32) -> f32 {
        (base + alpha * self.admitted as f32).clamp(base, ceiling)
    }

    fn admit(&mut self, vec: Vec<f32>, capacity: usize) {
        let cap = capacity.max(1);
        while self.recent.len() >= cap {
            self.recent.pop_front();
        }
        self.recent.push_back(vec);
        self.admitted = self.admitted.saturating_add(1);
    }
}

/// Outcome of the visual-admission gate. `Admitted` carries the freshly
/// computed CLIP vector so the downstream composer reuses it on the
/// `MemoryRecord` instead of running CLIP twice.
#[derive(Debug)]
enum VisualAdmissionOutcome {
    Admitted { image_vec: Vec<f32>, novelty: f32 },
    SkippedSmall { width: u32, height: u32 },
    SkippedNovelty { novelty: f32, threshold: f32 },
    Failed(String),
}

const VISUAL_UNGROUNDED_LOW_RAM_REASON: &str = "visual_capture_ungrounded_low_ram";

fn capture_pixel_vlm_route(
    config: &crate::config::Config,
    app_data_dir: &std::path::Path,
    ocr_text_len: usize,
    ocr_confidence: f32,
    ocr_block_count: usize,
    visual_signal: bool,
    is_duplicate: bool,
    system_pressure_skip: bool,
    host_supports_qwen_vlm: bool,
    calls_remaining: u32,
) -> VlmRouteDecision {
    let model_id = models::configured_vlm_model_id(config);
    let vlm_available = models::pixel_vlm_available(model_id.as_deref(), Some(app_data_dir));
    should_run_vlm(&VlmRouteInput {
        ocr_text_len,
        ocr_confidence,
        ocr_block_count,
        visual_signal,
        is_duplicate,
        system_pressure_skip,
        host_supports_qwen_vlm,
        vlm_enabled: config.use_vlm,
        vlm_available,
        vlm_calls_remaining: calls_remaining,
        vlm_timeout_secs: config.vlm_timeout_secs,
        _phantom: std::marker::PhantomData,
    })
}

fn should_skip_ungrounded_low_ram_visual_capture(
    vlm_route: &VlmRouteDecision,
    observed_text_len: usize,
    observed_confidence: f32,
    observed_block_count: usize,
    min_text_length: usize,
) -> bool {
    // Only drop truly ungrounded visual-only frames on low-RAM hosts. Thin OCR
    // (below the main pipeline gate) can still ground an LLM/fusion fallback —
    // `source_low_signal` must not force a hard skip when chars are present.
    vlm_route.fallback_reason() == Some("vlm_blocked_low_ram")
        && (observed_text_len < min_text_length
            || (observed_block_count == 0 && observed_confidence <= 0.05))
}

/// First half of the visual-narrative path: decode the screen PNG once,
/// reject undersized frames, compute the CLIP embedding, and ask the
/// tracker whether this frame is novel enough to admit. CLIP is run in
/// `spawn_blocking` so the async loop stays responsive.
async fn try_admit_visual_capture(
    image_data: &[u8],
    tracker: &VisualNoveltyTracker,
    cfg: &CapturePipelineConfig,
    models_dir: &PathBuf,
) -> VisualAdmissionOutcome {
    let bytes = image_data.to_vec();
    let models_dir = models_dir.clone();
    let min_dim = cfg.visual_admission_min_image_dim;
    let decode_and_embed =
        tokio::task::spawn_blocking(move || -> Result<(Vec<f32>, u32, u32), String> {
            use image::GenericImageView;
            let dynamic =
                image::load_from_memory(&bytes).map_err(|e| format!("decode capture png: {e}"))?;
            let (w, h) = dynamic.dimensions();
            if w < min_dim || h < min_dim {
                return Ok((Vec::new(), w, h));
            }
            let vec = embed_imported_image(&dynamic, &models_dir)?;
            Ok((vec, w, h))
        })
        .await;

    let (image_vec, width, height) = match decode_and_embed {
        Ok(Ok(tuple)) => tuple,
        Ok(Err(err)) => return VisualAdmissionOutcome::Failed(err),
        Err(err) => return VisualAdmissionOutcome::Failed(format!("clip join: {err}")),
    };
    if image_vec.is_empty() {
        return VisualAdmissionOutcome::SkippedSmall { width, height };
    }

    let novelty = tracker.novelty(&image_vec);
    let threshold = tracker.adaptive_threshold(
        cfg.visual_novelty_base,
        cfg.visual_novelty_alpha,
        cfg.visual_novelty_ceiling,
    );
    if novelty >= threshold {
        VisualAdmissionOutcome::Admitted { image_vec, novelty }
    } else {
        VisualAdmissionOutcome::SkippedNovelty { novelty, threshold }
    }
}

#[allow(clippy::too_many_arguments)]
async fn compose_visual_capture_record(
    state: &AppState,
    text_embedder: Option<&Embedder>,
    embedding_memo: &mut EmbeddingMemo,
    image_data: Vec<u8>,
    image_vec: Vec<f32>,
    app_name: &str,
    bundle_id: Option<&str>,
    window_title: &str,
    url: Option<&str>,
    observed_text: &str,
    observed_text_len: usize,
    observed_confidence: f32,
    observed_block_count: usize,
    novelty: f32,
) -> Result<MemoryRecord, String> {
    let now = Local::now();
    let synthetic_filename = format!(
        "{}_{}.png",
        sanitize_visual_filename_token(app_name),
        now.timestamp_millis()
    );

    // System-pressure and host-size throttle: under high pressure, or on
    // machines below the VLM RAM floor, skip MTMD and fall back to the
    // OCR/window grounded path.
    let config = state.config.read().clone();
    let host_supports_qwen_vlm = crate::telemetry::system_metrics::host_supports_lightweight_vlm();
    let (skip_vlm, skip_reason) =
        crate::telemetry::system_metrics::pressure_recommends_skipping_heavy_models();
    let vlm_route = capture_pixel_vlm_route(
        &config,
        state.app_data_dir.as_path(),
        observed_text_len,
        observed_confidence,
        observed_block_count,
        true,
        false,
        skip_vlm,
        host_supports_qwen_vlm,
        config.vlm_max_calls_per_minute,
    );

    // Helper: try the LLM-on-OCR fallback when Llama is loaded. Returns
    // None if no engine is available or the structured extraction returns
    // nothing useful. Stays inside the existing async context (already
    // serialized by the model pipeline lock the caller holds).
    let llm_engine = state.inference_engine();
    let trimmed_observed = observed_text.trim();
    let llm_fallback_context = if trimmed_observed.is_empty() {
        format!(
            "App: {app_name}\nWindow: {window_title}\n(visual-only frame; OCR was below the storage gate)"
        )
    } else {
        format!(
            "App: {app_name}\nWindow: {window_title}\nOCR excerpt:\n{}",
            trimmed_observed.chars().take(4000).collect::<String>()
        )
    };
    let try_llm_fallback = || async {
        let engine = llm_engine.clone()?;
        engine
            .extract_structured_memory(app_name, window_title, &llm_fallback_context)
            .await
            .map(|s| crate::inference::insight_from_structured(&s))
    };

    // Removed ungrounded low-RAM visual capture gate: store captures even without OCR/VLM grounding

    let insight = if !vlm_route.runs_pixel_vlm() {
        let reason = vlm_route
            .fallback_reason()
            .unwrap_or_else(|| vlm_route.label());
        tracing::info!(
            app = %app_name,
            "compose_visual_capture_record: skipping VLM ({reason}); pressure_reason={skip_reason}; trying LLM-on-OCR fallback"
        );
        if let Some(i) = try_llm_fallback().await {
            i
        } else {
            crate::inference::insight_from_ocr_only(
                &synthetic_filename,
                Some(app_name),
                Some(window_title),
                "",
            )
        }
    } else {
        // Run the same VLM path Meta-glasses imports use. Visual narrative
        // is its sole signal; OCR is skipped (the gate fired *because* OCR
        // was thin), so we don't pass an OCR appendix.
        match extract_image_semantics(
            image_data,
            &synthetic_filename,
            ImageImportSource::ScreenCapture,
            state.app_data_dir.clone(),
        )
        .await
        {
            Ok(i) => i,
            Err(e) => {
                tracing::warn!(
                    app = %app_name,
                    "compose_visual_capture_record: VLM failed ({e}); trying LLM-on-OCR fallback"
                );
                if let Some(i) = try_llm_fallback().await {
                    i
                } else {
                    crate::inference::insight_from_ocr_only(
                        &synthetic_filename,
                        Some(app_name),
                        Some(window_title),
                        "",
                    )
                }
            }
        }
    };

    let composed = compose_import_memory_context_with_title(
        &synthetic_filename,
        &insight,
        None,
        ImageImportSource::ScreenCapture,
        Some(window_title),
    );

    let session_key = build_session_key(app_name, window_title, url);
    let session_id = build_session_id(&now, app_name, bundle_id, &session_key);
    let display_summary = if !insight.summary_short.trim().is_empty() {
        insight
            .summary_short
            .trim()
            .chars()
            .take(200)
            .collect::<String>()
    } else {
        composed
            .memory_context
            .chars()
            .take(160)
            .collect::<String>()
    };

    // Fold synthesis-derived concept terms (search_aliases + topic_categories)
    // into the lexical shadow so keyword search can hit them even when raw OCR
    // doesn't contain those terms. E.g. "sport" finds cricket captures.
    let mut shadow_extras: Vec<&str> = Vec::new();
    for v in composed
        .search_aliases
        .iter()
        .chain(composed.topic_categories.iter())
    {
        shadow_extras.push(v.as_str());
    }
    let lexical_shadow = build_lexical_shadow_with_aliases(
        app_name,
        &display_summary,
        &composed.memory_context,
        url,
        &shadow_extras,
    );
    let compact_summary = compact_summary_embedding_text(
        "visual_capture",
        &display_summary,
        &composed.memory_context,
        &lexical_shadow,
    );
    let support_texts = support_embedding_texts(
        app_name,
        window_title,
        &composed.memory_context,
        &lexical_shadow,
    );

    let mut embedding_inputs = vec![composed.embedding_text.clone(), compact_summary.clone()];
    embedding_inputs.extend(support_texts.iter().cloned());
    let vectors = embed_text_inputs_with_memo(
        text_embedder,
        embedding_memo,
        app_name,
        window_title,
        &embedding_inputs,
    );
    let primary = vectors
        .first()
        .cloned()
        .unwrap_or_else(|| vec![0.0; EMBEDDING_DIM]);
    let snippet_embedding = vectors
        .get(1)
        .cloned()
        .unwrap_or_else(|| vec![0.0; EMBEDDING_DIM]);
    let support_embedding = if vectors.len() > 2 {
        mean_pool_embeddings(&vectors[2..])
    } else {
        vec![0.0; EMBEDDING_DIM]
    };
    let text_embedding =
        weighted_primary_embedding(&primary, &snippet_embedding, &support_embedding);

    let raw_evidence = json!({
        "source_kind": "visual_capture",
        "vision_model_id": insight.model_id,
        "semantic_confidence": insight.confidence,
        "visual_admission_novelty": novelty,
        "vlm_route": vlm_route.label(),
        "vlm_block_reason": vlm_route.fallback_reason(),
        "host_supports_vlm": host_supports_qwen_vlm,
        "pressure_reason": skip_reason,
        "app_name": app_name,
        "window_title": window_title,
        "url": url,
        "synthetic_filename": synthetic_filename,
        "timestamp_ms": now.timestamp_millis(),
    })
    .to_string();

    let topic = if !composed.topic.trim().is_empty() {
        composed.topic.clone()
    } else {
        "unknown".to_string()
    };
    let user_intent = if !composed.user_intent.trim().is_empty() {
        composed.user_intent.clone()
    } else {
        composed.activity_type.clone()
    };

    let mut record = MemoryRecord {
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: now.timestamp_millis(),
        day_bucket: now.format("%Y-%m-%d").to_string(),
        app_name: app_name.to_string(),
        bundle_id: bundle_id.map(str::to_string),
        window_title: window_title.to_string(),
        session_id,
        text: String::new(),
        clean_text: composed.memory_context.clone(),
        ocr_confidence: observed_confidence,
        ocr_block_count: observed_block_count.min(u32::MAX as usize) as u32,
        snippet: display_summary.clone(),
        display_summary: display_summary.clone(),
        internal_context: composed.memory_context.clone(),
        summary_source: "visual_capture".to_string(),
        noise_score: 0.0,
        session_key,
        lexical_shadow,
        embedding: text_embedding,
        image_embedding: image_vec.clone(),
        screenshot_path: None,
        url: url.map(str::to_string),
        snippet_embedding,
        support_embedding,
        decay_score: 1.0,
        last_accessed_at: 0,
        timestamp_start: now.timestamp_millis(),
        timestamp_end: now.timestamp_millis(),
        source_type: if url.is_some() {
            "browser_visual".to_string()
        } else {
            "screen_visual".to_string()
        },
        topic,
        workflow: "unknown".to_string(),
        user_intent,
        memory_context: composed.memory_context.clone(),
        raw_evidence,
        search_aliases: composed.search_aliases.clone(),
        activity_type: composed.activity_type.clone(),
        entities: insight.entities.clone(),
        tags: insight.topics.clone(),
        embedding_text: composed.embedding_text.clone(),
        embedding_model: "all-MiniLM-L6-v2".to_string(),
        embedding_dim: EMBEDDING_DIM as u32,
        evidence_confidence: insight.confidence,
        extraction_confidence: insight.confidence,
        synthesis_branch: "vlm".to_string(),
        topic_categories: composed.topic_categories.clone(),
        insight_what_happened: composed.insight_what_happened.clone(),
        insight_why_mattered: composed.insight_why_mattered.clone(),
        insight_card_confidence: insight.confidence,
        schema_version: 2,
        ..Default::default()
    };
    record.dedup_fingerprint =
        deterministic_dedup_fingerprint(&record, Some(&record.memory_context));
    Ok(record)
}

/// Make the synthetic VLM-input filename stable but human-readable. The
/// filename is only used for analytics/telemetry; the VLM itself does not
/// read pixels from disk, just from memory.
fn sanitize_visual_filename_token(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let trimmed = cleaned.trim_matches('_');
    if trimmed.is_empty() {
        "screen".to_string()
    } else {
        trimmed.chars().take(48).collect()
    }
}

pub(crate) fn capture_context_skip_reason(
    app_name: &str,
    bundle_id: Option<&str>,
    window_title: &str,
    url: Option<&str>,
    blocklist: &[String],
) -> Option<crate::SkipReason> {
    if Blocklist::is_internal_app(app_name, bundle_id) {
        return Some(crate::SkipReason::SelfApp);
    }
    if Blocklist::is_blocked(app_name, blocklist) {
        return Some(crate::SkipReason::Blocklist);
    }
    if Blocklist::is_context_blocked(url, Some(window_title), blocklist) {
        return Some(crate::SkipReason::Blocklist);
    }
    None
}

pub(crate) fn should_skip_capture_context(
    app_name: &str,
    bundle_id: Option<&str>,
    window_title: &str,
    url: Option<&str>,
    blocklist: &[String],
) -> bool {
    capture_context_skip_reason(app_name, bundle_id, window_title, url, blocklist).is_some()
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
        synthesis_branch: "browser_semantic".to_string(),
        ..Default::default()
    })
}

#[derive(Debug, Clone)]
struct SemanticFusionDraft {
    extraction: StructuredMemoryExtraction,
    sources: Vec<&'static str>,
    reason: &'static str,
}

fn clean_file_reference(token: &str) -> String {
    token
        .trim_matches(|ch: char| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    ',' | ';' | '(' | ')' | '[' | ']' | '{' | '}' | '"' | '\'' | '`' | '•'
                )
        })
        .trim_end_matches(|ch: char| matches!(ch, ':' | '.' | ')' | ']'))
        .to_string()
}

fn looks_like_file_reference(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    const EXTENSIONS: &[&str] = &[
        ".md", ".rs", ".ts", ".tsx", ".js", ".jsx", ".json", ".toml", ".yaml", ".yml", ".css",
        ".html", ".py", ".swift", ".sh",
    ];
    EXTENSIONS
        .iter()
        .any(|ext| lower.ends_with(ext) || lower.contains(&format!("{ext}:")))
}

fn extract_file_references(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for raw in text.split_whitespace() {
        let mut token = clean_file_reference(raw);
        if let Some((head, _line)) = token.rsplit_once(':') {
            if looks_like_file_reference(head) {
                token = clean_file_reference(head);
            }
        }
        if !looks_like_file_reference(&token) {
            continue;
        }
        let key = token.to_ascii_lowercase();
        if seen.insert(key) {
            out.push(token);
            if out.len() >= 12 {
                break;
            }
        }
    }
    out
}

fn merge_unique_strings(existing: &mut Vec<String>, incoming: impl IntoIterator<Item = String>) {
    let mut seen: HashSet<String> = existing
        .iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .collect();
    for value in incoming {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        if seen.insert(trimmed.to_ascii_lowercase()) {
            existing.push(trimmed.to_string());
        }
    }
}

fn infer_review_activity(clean_text: &str) -> (&'static str, &'static str, &'static str) {
    let lower = clean_text.to_ascii_lowercase();
    if lower.contains("error") || lower.contains("failed") || lower.contains("debug") {
        ("debugging", "debugging", "debugging visible issue context")
    } else if lower.contains("todo")
        || lower.contains("planned")
        || lower.contains("roadmap")
        || lower.contains("implemented")
        || lower.contains("docs")
        || lower.contains("design")
    {
        (
            "reviewing",
            "reviewing",
            "reviewing implementation status and supporting context",
        )
    } else {
        ("reviewing", "reviewing", "reviewing visible screen context")
    }
}

fn build_low_ram_semantic_fusion(
    app_name: &str,
    window_title: &str,
    url: Option<&str>,
    clean_text: &str,
    semantic_page: Option<&macos::BrowserSemanticContent>,
    capture_quality: &text_cleanup::CaptureQualityStats,
    source_kind: &str,
) -> Option<SemanticFusionDraft> {
    let text = clean_text.trim();
    if text.len() < 80 && semantic_page.map(|page| !page.has_signal()).unwrap_or(true) {
        return None;
    }

    let spans = text_cleanup::rank_salient_spans(text, app_name);
    let salient = spans
        .iter()
        .filter(|span| span.score >= 0.30)
        .take(3)
        .map(|span| span.text.clone())
        .collect::<Vec<_>>();
    let files = extract_file_references(text);

    let semantic_title = semantic_page
        .and_then(|page| {
            [
                page.h1.as_str(),
                page.title.as_str(),
                page.meta_description.as_str(),
            ]
            .into_iter()
            .map(str::trim)
            .find(|value| !value.is_empty())
            .map(str::to_string)
        })
        .unwrap_or_default();

    let topic = if !semantic_title.trim().is_empty() {
        semantic_title.chars().take(120).collect::<String>()
    } else if !files.is_empty() {
        files
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(" and ")
    } else if let Some(first) = salient.first() {
        first.chars().take(120).collect::<String>()
    } else if !window_title.trim().is_empty() {
        window_title.trim().chars().take(120).collect::<String>()
    } else {
        app_name.trim().chars().take(120).collect::<String>()
    };

    let (activity, workflow, user_intent) = infer_review_activity(text);
    let subject = if !files.is_empty() {
        format!(
            "visible files {}",
            files.iter().take(4).cloned().collect::<Vec<_>>().join(", ")
        )
    } else if !topic.trim().is_empty() {
        topic.clone()
    } else {
        window_title.trim().to_string()
    };

    let mut sentences = Vec::new();
    let surface = if !window_title.trim().is_empty() {
        format!("{} in {}", app_name.trim(), window_title.trim())
    } else {
        app_name.trim().to_string()
    };
    sentences.push(format!("You were reviewing {subject} on {surface}."));
    if let Some(domain) = url.and_then(extract_domain) {
        sentences.push(format!("The visible page was from {domain}."));
    }
    if !semantic_title.trim().is_empty() && !sentences.join(" ").contains(&semantic_title) {
        sentences.push(format!("Browser context: {semantic_title}."));
    }
    let lower_text = text.to_ascii_lowercase();
    if lower_text.contains("planned")
        || lower_text.contains("implemented")
        || lower_text.contains("roadmap")
        || lower_text.contains("design")
        || lower_text.contains("docs")
    {
        sentences.push(
            "The visible context was about implementation status, docs, or roadmap items."
                .to_string(),
        );
    }

    let keep_ratio = if capture_quality.total_lines == 0 {
        0.0
    } else {
        capture_quality.kept_lines as f32 / capture_quality.total_lines as f32
    };
    let avg_span_score = if spans.is_empty() {
        0.0
    } else {
        spans.iter().take(3).map(|span| span.score).sum::<f32>() / spans.len().min(3) as f32
    };
    let semantic_score = semantic_page
        .map(|page| page.content_signal_score)
        .unwrap_or(0.0);
    let confidence =
        (0.58 + avg_span_score * 0.16 + semantic_score * 0.12 + keep_ratio.clamp(0.0, 1.0) * 0.08)
            .clamp(0.60, 0.86);

    let mut entities = Vec::new();
    merge_unique_strings(&mut entities, files.iter().cloned());
    if !semantic_title.trim().is_empty() {
        merge_unique_strings(&mut entities, [semantic_title.clone()]);
    }
    if let Some(domain) = url.and_then(extract_domain) {
        merge_unique_strings(&mut entities, [domain]);
    }

    let mut aliases = Vec::new();
    merge_unique_strings(&mut aliases, files.iter().cloned());
    merge_unique_strings(&mut aliases, [topic.clone()]);

    let mut sources = vec!["ocr_salient_spans", "app_window"];
    if !files.is_empty() {
        sources.push("file_references");
    }
    if semantic_page.is_some() {
        sources.push("browser_semantic");
    }
    if source_kind == "browser_semantic" {
        sources.push("browser_text_source");
    }

    Some(SemanticFusionDraft {
        extraction: StructuredMemoryExtraction {
            activity_type: activity.to_string(),
            project: String::new(),
            topic,
            memory_context: sentences.join(" "),
            workflow: workflow.to_string(),
            user_intent: user_intent.to_string(),
            files_touched: files,
            entities,
            search_aliases: aliases,
            confidence,
            dedup_fingerprint: String::new(),
            synthesis_branch: "fallback".to_string(),
            ..Default::default()
        },
        sources,
        reason: "low_ram_deterministic_semantic_fusion",
    })
}

fn semantic_layout_diagnostics(
    clean_text: &str,
    capture_quality: &text_cleanup::CaptureQualityStats,
) -> serde_json::Value {
    let visible_lines = clean_text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let checkbox_like_lines = visible_lines
        .iter()
        .filter(|line| {
            let lower = line.to_ascii_lowercase();
            lower.starts_with("☐")
                || lower.starts_with("[ ]")
                || lower.starts_with("- [ ]")
                || lower.contains("checkbox")
        })
        .count();
    let heading_like_lines = visible_lines
        .iter()
        .filter(|line| {
            let trimmed = line.trim_start_matches(['#', '*', '-']).trim();
            !trimmed.is_empty()
                && trimmed.len() <= 90
                && trimmed
                    .chars()
                    .filter(|ch| ch.is_alphabetic())
                    .take(1)
                    .next()
                    .is_some()
                && trimmed
                    .split_whitespace()
                    .filter(|word| word.chars().next().map(char::is_uppercase).unwrap_or(false))
                    .count()
                    >= 2
        })
        .count();
    let file_reference_count = extract_file_references(clean_text).len();
    let split_pane_likelihood = ((file_reference_count.min(8) as f32 * 0.10)
        + (checkbox_like_lines.min(8) as f32 * 0.05)
        + (heading_like_lines.min(8) as f32 * 0.04)
        + if visible_lines.len() >= 18 { 0.18 } else { 0.0 })
    .clamp(0.0, 1.0);

    json!({
        "line_count": visible_lines.len(),
        "file_reference_count": file_reference_count,
        "heading_like_lines": heading_like_lines,
        "checkbox_like_lines": checkbox_like_lines,
        "split_pane_likelihood": split_pane_likelihood,
        "line_confidence": {
            "low_conf_lines": capture_quality.low_conf_lines,
            "kept_lines": capture_quality.kept_lines,
            "avg_line_score": capture_quality.avg_line_score,
        }
    })
}

fn apply_semantic_fusion(
    structured_memory: &mut Option<StructuredMemoryExtraction>,
    fusion: SemanticFusionDraft,
    replace_core_fields: bool,
) -> serde_json::Value {
    if let Some(existing) = structured_memory.as_mut() {
        if replace_core_fields || existing.memory_context.trim().is_empty() {
            existing.memory_context = fusion.extraction.memory_context.clone();
        }
        if replace_core_fields || existing.topic.trim().is_empty() || existing.topic == "unknown" {
            existing.topic = fusion.extraction.topic.clone();
        }
        if replace_core_fields || existing.workflow.trim().is_empty() {
            existing.workflow = fusion.extraction.workflow.clone();
        }
        if replace_core_fields || existing.user_intent.trim().is_empty() {
            existing.user_intent = fusion.extraction.user_intent.clone();
        }
        if replace_core_fields || existing.activity_type.trim().is_empty() {
            existing.activity_type = fusion.extraction.activity_type.clone();
        }
        merge_unique_strings(
            &mut existing.files_touched,
            fusion.extraction.files_touched.clone(),
        );
        merge_unique_strings(&mut existing.entities, fusion.extraction.entities.clone());
        merge_unique_strings(
            &mut existing.search_aliases,
            fusion.extraction.search_aliases.clone(),
        );
        existing.confidence = existing.confidence.max(fusion.extraction.confidence);
    } else {
        *structured_memory = Some(fusion.extraction.clone());
    }

    json!({
        "applied": true,
        "reason": fusion.reason,
        "sources": fusion.sources,
        "topic": structured_memory.as_ref().map(|m| m.topic.clone()).unwrap_or_default(),
        "confidence": structured_memory.as_ref().map(|m| m.confidence).unwrap_or(0.0),
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

/// Reset a structured-extraction field when the model echoed an entire
/// enum vocabulary instead of choosing one value (e.g.
/// `coding|debugging|reviewing_agent_output|...`). `|` never belongs in a
/// single-label human-readable field, so we drop it and flag the issue.
/// Structural rule — independent of model name or vocabulary.
fn clear_if_multi_option(value: &mut String, issues: &mut Vec<String>, field: &str) {
    if value.contains('|') {
        issues.push(format!("{field}_multi_option_dump"));
        value.clear();
    }
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

    clear_if_multi_option(&mut extraction.activity_type, &mut issues, "activity_type");
    clear_if_multi_option(&mut extraction.topic, &mut issues, "topic");
    clear_if_multi_option(&mut extraction.workflow, &mut issues, "workflow");
    clear_if_multi_option(&mut extraction.user_intent, &mut issues, "user_intent");

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

/// True when the LLM narrative already mentions the bulk of the topic's
/// content tokens, so re-emitting a "Topic:" preamble would be redundant
/// noise. Token overlap on lowercased ascii-alnum words, ≥60% threshold.
/// Stopwords + tokens shorter than 3 chars are ignored so single-letter
/// or articles don't dominate the ratio.
fn narrative_mentions(narrative: Option<&str>, topic: &str) -> bool {
    let Some(narrative) = narrative else {
        return false;
    };
    let topic_tokens: Vec<String> = topic
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .filter(|t| t.len() >= 3)
        .map(|t| t.to_string())
        .collect();
    if topic_tokens.is_empty() {
        return false;
    }
    let narrative_lower = narrative.to_ascii_lowercase();
    let hits = topic_tokens
        .iter()
        .filter(|tok| narrative_lower.contains(tok.as_str()))
        .count();
    (hits as f32 / topic_tokens.len() as f32) >= 0.6
}

/// Pick a deterministic semantic anchor for the durable memory context.
/// Priority: structured topic → first salient span head → window-title noun
/// phrase. No app names — fully content-derived.
fn pick_semantic_center(
    extraction: Option<&StructuredMemoryExtraction>,
    app_name: &str,
    window_title: &str,
    clean_text: &str,
) -> String {
    if let Some(mem) = extraction {
        let topic = mem.topic.trim();
        if !topic.is_empty() && topic.to_ascii_lowercase() != "unknown" {
            return topic.to_string();
        }
    }
    let spans = text_cleanup::rank_salient_spans(clean_text, app_name);
    if let Some(top) = spans.first() {
        let trimmed = top
            .text
            .split_terminator(['.', '!', '?', '\n'])
            .next()
            .unwrap_or(&top.text)
            .trim();
        if !trimmed.is_empty() {
            return trimmed.chars().take(120).collect::<String>();
        }
    }
    let title = window_title.trim();
    if !title.is_empty() {
        return title.chars().take(120).collect();
    }
    String::new()
}

/// Compose a human-readable continuity footer.
fn build_continuation_footer(prior_chain: &[crate::storage::SearchResult]) -> String {
    let mut lines: Vec<String> = Vec::new();
    if let Some(prev) = prior_chain.first() {
        let head: String = prev
            .memory_context
            .trim()
            .split('\n')
            .next()
            .unwrap_or("")
            .chars()
            .take(80)
            .collect();
        if !head.trim().is_empty() {
            lines.push(format!(
                "This continues earlier related work: {}.",
                head.trim()
            ));
        } else {
            lines.push("This continues earlier related work from the same session.".to_string());
        }
    }
    lines.join("\n")
}

/// Pad short contexts with grounded structured fields. Pure helper; no I/O and
/// no raw OCR tail copying into durable `memory_context`.
fn pad_with_structured(
    base: &str,
    extraction: Option<&StructuredMemoryExtraction>,
    app_name: &str,
    clean_text: &str,
    min_chars: usize,
) -> String {
    if base.chars().count() >= min_chars {
        return base.to_string();
    }
    let mut out = base.to_string();
    let mut extras: Vec<String> = Vec::new();
    let base_norm = normalize_text_for_overlap(base);
    if let Some(mem) = extraction {
        let topic_norm = normalize_text_for_overlap(mem.topic.trim());
        if !mem.topic.trim().is_empty()
            && mem.topic.trim().to_ascii_lowercase() != "unknown"
            && (topic_norm.is_empty() || !base_norm.contains(&topic_norm))
        {
            extras.push(format!("Topic: {}", mem.topic.trim()));
        }
        if !mem.user_intent.trim().is_empty() {
            extras.push(format!("Intent: {}", mem.user_intent.trim()));
        }
        if !mem.workflow.trim().is_empty() && mem.workflow.trim().to_ascii_lowercase() != "unknown"
        {
            extras.push(format!("Workflow: {}", mem.workflow.trim()));
        }
        if !mem.files_touched.is_empty() {
            extras.push(format!(
                "Files: {}",
                mem.files_touched
                    .iter()
                    .take(4)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !mem.entities.is_empty() {
            extras.push(format!(
                "Entities: {}",
                mem.entities
                    .iter()
                    .take(4)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !mem.decisions.is_empty() {
            extras.push(format!(
                "Decisions: {}",
                mem.decisions
                    .iter()
                    .take(2)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
        if !mem.results.is_empty() {
            extras.push(format!(
                "Results: {}",
                mem.results
                    .iter()
                    .take(2)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
        if !mem.next_steps.is_empty() {
            extras.push(format!(
                "Next: {}",
                mem.next_steps
                    .iter()
                    .take(2)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
    }
    for extra in extras {
        if !out.is_empty() {
            out.push_str("\n");
        }
        out.push_str(&extra);
        if out.chars().count() >= min_chars {
            return out;
        }
    }
    if out.chars().count() < min_chars {
        let surface = if app_name.trim().is_empty() {
            String::new()
        } else {
            format!("Source app: {}", app_name.trim())
        };
        if !surface.trim().is_empty() && !out.contains(&surface) {
            if !out.is_empty() {
                out.push_str("\n");
            }
            out.push_str(&surface);
        }
    }
    let _ = clean_text;
    out
}

/// Capture-time durable `memory_context`. Composes three sections (what /
/// state / where) bounded by config min/max chars, embeds an optional
/// continuation pointer to the prior card, and falls back gracefully when
/// structured extraction is absent.
pub(crate) fn build_durable_memory_context(
    extraction: Option<&StructuredMemoryExtraction>,
    app_name: &str,
    window_title: &str,
    clean_text: &str,
    display_summary: &str,
    _bundle_id: Option<&str>,
    _url: Option<&str>,
    prior_chain: &[crate::storage::SearchResult],
    config: &crate::config::MemoryQualityConfig,
) -> String {
    let center = pick_semantic_center(extraction, app_name, window_title, clean_text);

    // Narrative-first: the LLM's free-form `memory_context` is the most
    // human-readable description we have and is what humans/agents want to
    // see in retrieval surfaces (iOS handoff, OpenClaw, etc.). We lead with
    // it and only fall back to structured "Topic:/You were/Activity:" lines
    // when no narrative is present. Topic: is appended only if it adds
    // tokens the narrative does not already cover.
    let narrative: Option<String> = extraction.and_then(|m| {
        let trimmed = m.memory_context.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    });
    let mut what_lines: Vec<String> = Vec::new();
    if let Some(ref n) = narrative {
        what_lines.push(n.clone());
    }
    if !center.is_empty() && !narrative_mentions(narrative.as_deref(), &center) {
        what_lines.push(format!("Topic: {}", center));
    }
    if narrative.is_none() {
        if let Some(mem) = extraction {
            let intent = mem.user_intent.trim();
            if !intent.is_empty() {
                what_lines.push(format!("You were {}.", intent));
            } else if !mem.activity_type.trim().is_empty()
                && mem.activity_type.trim().to_ascii_lowercase() != "unknown"
            {
                what_lines.push(format!("Activity: {}.", mem.activity_type.trim()));
            }
        }
    }
    if what_lines.is_empty() && !display_summary.trim().is_empty() {
        what_lines.push(display_summary.trim().to_string());
    }
    let what_section = what_lines.join("\n");

    let why_section = if let Some(mem) = extraction {
        let mut bits: Vec<String> = Vec::new();
        if !mem.decisions.is_empty() {
            bits.push(format!(
                "Decisions: {}",
                mem.decisions
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
        if !mem.errors.is_empty() {
            bits.push(format!(
                "Errors: {}",
                mem.errors
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
        if !mem.blockers.is_empty() {
            bits.push(format!(
                "Blockers: {}",
                mem.blockers
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
        if !mem.next_steps.is_empty() {
            bits.push(format!(
                "Next: {}",
                mem.next_steps
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
        if !mem.results.is_empty() {
            bits.push(format!(
                "Results: {}",
                mem.results
                    .iter()
                    .take(2)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
        bits.join("\n")
    } else {
        String::new()
    };

    let where_section = build_continuation_footer(prior_chain);

    let mut sections: Vec<String> = Vec::new();
    if !what_section.trim().is_empty() {
        sections.push(what_section);
    }
    if !why_section.trim().is_empty() {
        sections.push(why_section);
    }
    if !where_section.trim().is_empty() {
        sections.push(where_section);
    }

    let min_chars = config.memory_context_min_chars as usize;
    let max_chars = config.memory_context_max_chars as usize;
    let mut combined = sections.join("\n\n");
    if combined.chars().count() < min_chars {
        combined = pad_with_structured(&combined, extraction, app_name, clean_text, min_chars);
    }
    if combined.chars().count() > max_chars {
        let mut truncated: String = combined.chars().take(max_chars.saturating_sub(3)).collect();
        truncated.push_str("...");
        combined = truncated;
    }
    combined
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
    }
    // The durable memory_context is now the strongest single retrieval signal
    // — promote it ahead of entities/files/results so the embedding tower
    // anchors on synthesis rather than enumerations.
    if !memory_context.trim().is_empty() {
        segments.push(format!("context: {}", memory_context.trim()));
    }
    if let Some(mem) = extraction {
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
        if !mem.decisions.is_empty() {
            segments.push(format!(
                "decisions: {}",
                mem.decisions
                    .iter()
                    .take(4)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
        if !mem.next_steps.is_empty() {
            segments.push(format!(
                "next: {}",
                mem.next_steps
                    .iter()
                    .take(4)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
        if !mem.search_aliases.is_empty() {
            segments.push(format!(
                "aliases: {}",
                mem.search_aliases
                    .iter()
                    .take(8)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    if !display_summary.trim().is_empty() {
        segments.push(format!("summary: {}", display_summary.trim()));
    }
    segments.push(format!(
        "app: {} | window: {}",
        app_name.trim(),
        window_title.trim()
    ));
    let _ = (clean_text, lexical_shadow);
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
    // CLIP image embedding model lives next to the BGE assets; resolved once per
    // process. The first stored frame absorbs the ~80-200 ms session load; every
    // subsequent embed is ~30-80 ms on Apple Silicon CPU.
    let models_dir = models::models_dir(state.app_data_dir.as_path());
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
    let mut batch_outcomes: Vec<crate::StoreOutcome> = Vec::new();
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
    // Adaptive admission state for the visual-narrative path (frames with
    // thin OCR but informative pixels). Resets per session key.
    let mut visual_tracker = VisualNoveltyTracker::default();

    tracing::info!("Capture loop started");

    loop {
        let config = state.config.read().clone();
        let flush_interval = Duration::from_secs(config.capture_pipeline.flush_interval_secs);
        let max_batch_size = config.capture_pipeline.max_batch_size;

        // Flush batch if needed
        let should_flush = batch.len() >= max_batch_size || last_flush.elapsed() >= flush_interval;
        if should_flush && !batch.is_empty() {
            let pre_filter_count = batch.len();
            // Filter batch and outcomes in lockstep so their indices stay
            // aligned. A plain `retain` + `truncate` would keep the first N
            // outcomes regardless of which records were removed.
            let keep: Vec<bool> = batch
                .iter()
                .map(|record| {
                    !Blocklist::is_blocked(&record.app_name, &config.blocklist)
                        && !Blocklist::is_context_blocked(
                            record.url.as_deref(),
                            Some(&record.window_title),
                            &config.blocklist,
                        )
                })
                .collect();
            {
                let mut keep_iter = keep.iter().copied();
                batch_outcomes.retain(|_| keep_iter.next().unwrap_or(false));
            }
            {
                let mut keep_iter = keep.iter().copied();
                batch.retain(|_| keep_iter.next().unwrap_or(false));
            }
            if batch.is_empty() {
                purge_capture_artifacts(state.store.frames_dir());
                last_flush = Instant::now();
                continue;
            }

            let flush_start = Instant::now();
            match state.store.add_batch_and_get_count(&batch).await {
                Ok(inserted_count) => {
                    if inserted_count > 0 {
                        for outcome in batch_outcomes.iter() {
                            state.capture_stats.record_store(*outcome);
                        }
                        if let Err(err) = context_runtime::sync_memory_records(
                            state.as_ref(),
                            &batch,
                            Some("screen"),
                        )
                        .await
                        {
                            tracing::warn!("Context runtime batch sync failed: {}", err);
                        }
                        for rec in batch.iter() {
                            state.enqueue_graph_from_flushed_memory(rec);
                        }
                        if let Err(err) =
                            crate::ipc::commands::commit_graph_updates_now(state.clone()).await
                        {
                            tracing::debug!("immediate graph commit skipped: {}", err);
                        }
                    }
                    purge_capture_artifacts(state.store.frames_dir());
                    state.invalidate_memory_derived_caches();
                    let flush_ms = flush_start.elapsed().as_millis() as u64;
                    runtime_metrics::record_ms("capture.flush_ms", flush_ms);
                    if inserted_count > 0 {
                        tracing::info!(
                            "Flushed: attempted {} records, inserted {} in {:?}",
                            batch.len(),
                            inserted_count,
                            std::time::Duration::from_millis(flush_ms)
                        );
                    } else {
                        tracing::debug!(
                            "Batch flush skipped: all {} records filtered/deduped during storage",
                            batch.len()
                        );
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to flush batch: {}", e);
                }
            }
            batch.clear();
            batch_outcomes.clear();
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

        // We're past the "paused" / "deep idle" gates and intend to look at a
        // frame this tick — count it for the storage-rate denominator.
        state.capture_stats.record_evaluated();

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

        if let Some(reason) = capture_context_skip_reason(
            &app_name,
            app_context.bundle_id.as_deref(),
            &window_title,
            url.as_deref(),
            &config.blocklist,
        ) {
            tracing::debug!(
                "Skipping capture: reason={:?} app='{}' title='{}' url={:?}",
                reason,
                app_name,
                window_title,
                url
            );
            state.capture_stats.record_skip(reason, &app_name);
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
            state
                .capture_stats
                .record_skip(crate::SkipReason::SurfacePolicy, &app_name);
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
                state
                    .capture_stats
                    .record_skip(crate::SkipReason::SemanticDup, &app_name);
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
            let reopen_target = build_reopen_target(
                url.as_deref(),
                None,
                app_context.bundle_id.as_deref(),
                &app_name,
                now.timestamp_millis(),
            );
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
                reopen_kind: reopen_target.kind,
                reopen_url: reopen_target.url,
                reopen_file_path: reopen_target.file_path,
                reopen_app_bundle_id: reopen_target.app_bundle_id,
                reopen_app_name: reopen_target.app_name,
                reopen_app_deep_link: reopen_target.app_deep_link,
                reopen_captured_at_ms: reopen_target.captured_at_ms,
                reopen_confidence: reopen_target.confidence,
                reopen_validation_status: reopen_target.validation_status,
                schema_version: 2,
                activity_type: "browsing".to_string(),
                embedding_text: format!("url: {} | title: {}", domain, snippet),
                embedding_model: "all-MiniLM-L6-v2".to_string(),
                embedding_dim: EMBEDDING_DIM as u32,
                synthesis_branch: "url_only".to_string(),
                ..Default::default()
            };
            record.dedup_fingerprint =
                deterministic_dedup_fingerprint(&record, Some(&record.memory_context));
            batch.push(record);
            batch_outcomes.push(crate::StoreOutcome::UrlOnly);
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
                state
                    .capture_stats
                    .record_skip(crate::SkipReason::ScreenCaptureFailed, &app_name);
                tokio::time::sleep(sleep_duration).await;
                continue;
            }
        };

        // Deduplication check
        let is_duplicate = hasher.is_duplicate(&image_data, config.dedupe_threshold);

        if is_duplicate && !force_capture {
            state.frames_dropped.fetch_add(1, Ordering::Relaxed);
            state
                .capture_stats
                .record_skip(crate::SkipReason::PerceptualDup, &app_name);
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
                state
                    .capture_stats
                    .record_skip(crate::SkipReason::SurfacePolicy, &app_name);
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
                        state
                            .capture_stats
                            .record_skip(crate::SkipReason::OcrFailed, &app_name);
                        tokio::time::sleep(sleep_duration).await;
                        continue;
                    }
                };
                // DEBUG: Log OCR pipeline filtering to diagnose zero-confidence issues
                tracing::debug!(
                    "OCR raw result [{}]: confidence={:.3}, blocks={}, text_len={}, stats={{kept_lines={}, dropped={}, low_conf={}}}",
                    app_name,
                    ocr_result.confidence,
                    ocr_result.block_count,
                    ocr_result.text.len(),
                    ocr_result.ocr_stats.lines_used,
                    ocr_result.ocr_stats.lines_dropped,
                    ocr_result.ocr_stats.low_conf_count
                );
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

        // Skip if the text source is too weak/noisy to drive the OCR-narrative
        // pipeline. Before continuing, attempt the visual-narrative path: a
        // frame with thin OCR can still be visually informative (video,
        // image-heavy pages, design tools). The visual-admission gate is
        // adaptive — its threshold rises with each prior visual-only admit
        // in the current session — so the vault never gets flooded with
        // near-duplicate frames from the same scene.
        if source_low_signal || text.len() < config.min_text_length {
            let session_key_visual = build_session_key(&app_name, &window_title, url.as_deref());
            visual_tracker.reset_for(&session_key_visual);
            let outcome = try_admit_visual_capture(
                &image_data,
                &visual_tracker,
                &config.capture_pipeline,
                &models_dir,
            )
            .await;
            match outcome {
                VisualAdmissionOutcome::Admitted { image_vec, novelty } => {
                    // Hold the global model pipeline lock for the whole
                    // visual-narrative path: VLM + BGE batch + (lookups).
                    // Same invariant as the OCR pipeline above.
                    let _visual_guard = state.model_pipeline_lock.lock().await;
                    match compose_visual_capture_record(
                        state.as_ref(),
                        text_embedder.as_ref(),
                        &mut embedding_memo,
                        image_data.clone(),
                        image_vec.clone(),
                        &app_name,
                        app_context.bundle_id.as_deref(),
                        &window_title,
                        url.as_deref(),
                        &text,
                        text.len(),
                        observed_confidence,
                        observed_block_count,
                        novelty,
                    )
                    .await
                    {
                        Ok(record) => {
                            visual_tracker.admit(
                                image_vec,
                                config.capture_pipeline.visual_novelty_ring_capacity,
                            );
                            batch.push(record);
                            batch_outcomes.push(crate::StoreOutcome::VisualPath);
                            state.frames_captured.fetch_add(1, Ordering::Relaxed);
                            state.last_capture_time.store(
                                chrono::Utc::now().timestamp_millis() as u64,
                                Ordering::Relaxed,
                            );
                            emit_capture_quality_signal(
                                state.as_ref(),
                                json!({
                                    "timestamp_ms": chrono::Utc::now().timestamp_millis(),
                                    "app_name": app_name,
                                    "bundle_id": app_context.bundle_id.clone(),
                                    "ocr_confidence": observed_confidence,
                                    "ocr_block_count": observed_block_count,
                                    "clean_text_len": text.len(),
                                    "noise_score": 0.0,
                                    "low_signal": false,
                                    "stored_or_skipped": "stored_visual_capture",
                                    "source_kind": "visual_capture",
                                    "visual_admission_novelty": novelty,
                                    "visual_admission_threshold": visual_tracker
                                        .adaptive_threshold(
                                            config.capture_pipeline.visual_novelty_base,
                                            config.capture_pipeline.visual_novelty_alpha,
                                            config.capture_pipeline.visual_novelty_ceiling,
                                        ),
                                }),
                            );
                        }
                        Err(err) => {
                            tracing::warn!(
                                "visual-admission: VLM composition failed for {}: {}",
                                app_name,
                                err
                            );
                            let stored_or_skipped = if err == VISUAL_UNGROUNDED_LOW_RAM_REASON {
                                "skipped_visual_ungrounded_low_ram"
                            } else {
                                "skipped_visual_vlm_failed"
                            };
                            emit_capture_quality_signal(
                                state.as_ref(),
                                json!({
                                    "timestamp_ms": chrono::Utc::now().timestamp_millis(),
                                    "app_name": app_name,
                                    "bundle_id": app_context.bundle_id.clone(),
                                    "stored_or_skipped": stored_or_skipped,
                                    "source_kind": "visual_capture",
                                    "reason": err,
                                }),
                            );
                            state
                                .capture_stats
                                .record_skip(crate::SkipReason::VisualComposeFailed, &app_name);
                        }
                    }
                }
                VisualAdmissionOutcome::SkippedSmall { width, height } => {
                    emit_capture_quality_signal(
                        state.as_ref(),
                        json!({
                            "timestamp_ms": chrono::Utc::now().timestamp_millis(),
                            "app_name": app_name,
                            "bundle_id": app_context.bundle_id.clone(),
                            "stored_or_skipped": "skipped_visual_small_dim",
                            "source_kind": source_kind,
                            "width": width,
                            "height": height,
                        }),
                    );
                    state
                        .capture_stats
                        .record_skip(crate::SkipReason::VisualSmall, &app_name);
                }
                VisualAdmissionOutcome::SkippedNovelty { novelty, threshold } => {
                    emit_capture_quality_signal(
                        state.as_ref(),
                        json!({
                            "timestamp_ms": chrono::Utc::now().timestamp_millis(),
                            "app_name": app_name,
                            "bundle_id": app_context.bundle_id.clone(),
                            "stored_or_skipped": "skipped_visual_low_novelty",
                            "source_kind": source_kind,
                            "visual_admission_novelty": novelty,
                            "visual_admission_threshold": threshold,
                        }),
                    );
                    state
                        .capture_stats
                        .record_skip(crate::SkipReason::VisualNovelty, &app_name);
                }
                VisualAdmissionOutcome::Failed(err) => {
                    tracing::debug!("visual-admission: gate failed for {}: {}", app_name, err);
                    emit_capture_quality_signal(
                        state.as_ref(),
                        json!({
                            "timestamp_ms": chrono::Utc::now().timestamp_millis(),
                            "app_name": app_name,
                            "bundle_id": app_context.bundle_id.clone(),
                            "stored_or_skipped": "skipped_visual_failed",
                            "source_kind": source_kind,
                            "reason": err,
                        }),
                    );
                    state
                        .capture_stats
                        .record_skip(crate::SkipReason::VisualComposeFailed, &app_name);
                }
            }
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
            state
                .capture_stats
                .record_skip(crate::SkipReason::LowSignalText, &app_name);
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
            state
                .capture_stats
                .record_skip(crate::SkipReason::Noise, &app_name);
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
                state
                    .capture_stats
                    .record_skip(crate::SkipReason::SemanticDup, &app_name);
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

        // ── Pause capture, run all heavy model work serialized ────────────────
        // Acquire the global model pipeline lock for the duration of LLM +
        // text-embedding + CLIP. This guarantees the Metal/CoreML backend
        // sees one tenant at a time — the capture loop, glasses_import IPC,
        // and any other model consumer take turns, which keeps RSS even
        // and prevents the `mtmd eval chunks: -3` failure mode the user
        // reported when multiple model engines hit Metal concurrently.
        let _pipeline_guard = state.model_pipeline_lock.lock().await;

        let mut structured_memory = if let Some(engine) = engine.as_ref() {
            let mut s = engine
                .extract_structured_memory(&app_name, &window_title, &qwen_cleaned_text)
                .await;
            if let Some(ref mut extraction) = s {
                if extraction.synthesis_branch.is_empty() {
                    extraction.synthesis_branch = "llm".to_string();
                }
            }
            s
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
        let (mut extraction_grounding_confidence, mut extraction_issues) =
            if let Some(memory) = structured_memory.as_mut() {
                validate_structured_memory_extraction(memory, &app_name, &window_title, &text)
            } else {
                (0.0, vec!["structured_extraction_unavailable".to_string()])
            };
        let mut semantic_fusion_diagnostics = json!({
            "applied": false,
            "reason": null,
            "sources": [],
        });
        let needs_deterministic_fusion = structured_memory.is_none()
            || extraction_grounding_confidence < 0.55
            || extraction_issues
                .iter()
                .any(|issue| issue == "structured_fields_weakly_grounded");
        if needs_deterministic_fusion {
            if let Some(fusion) = build_low_ram_semantic_fusion(
                &app_name,
                &window_title,
                url.as_deref(),
                &text,
                semantic_page.as_ref(),
                &capture_quality,
                source_kind,
            ) {
                semantic_fusion_diagnostics = apply_semantic_fusion(
                    &mut structured_memory,
                    fusion,
                    extraction_grounding_confidence < 0.55,
                );
                let validated = if let Some(memory) = structured_memory.as_mut() {
                    validate_structured_memory_extraction(memory, &app_name, &window_title, &text)
                } else {
                    (0.0, vec!["structured_extraction_unavailable".to_string()])
                };
                extraction_grounding_confidence = validated.0.max(extraction_grounding_confidence);
                extraction_issues = validated.1;
            }
        }
        // Count "this memory's structured fields are likely hallucinated"
        // signals. Two or more of these stacked with grounding < 0.5 means
        // the row would land as a Willow/Shottr-style polluted card —
        // skip storage outright so neither retrieval nor the iOS/OpenClaw
        // surfaces see it.
        //
        // `possible_ungrounded_extraction` is intentionally excluded: it fires
        // at grounding < 0.80 which is too broad for a 1B model that often
        // uses semantically-correct but lexically-different wording. Including
        // it caused (grounding < 0.55) to always produce stacked_critical = 2,
        // dropping every frame the LLM touched. The effective gate is now:
        // both weakly-grounded AND an unsupported hallucinated outcome field.
        let stacked_critical_extraction_issues = extraction_issues
            .iter()
            .filter(|issue| {
                matches!(
                    issue.as_str(),
                    "structured_fields_weakly_grounded" | "unsupported_outcome"
                )
            })
            .count();
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
                    "anomaly_labels": extraction_issues.clone(),
                    "app_name": app_name
                }),
            );
        }
        let drop_due_to_stacked_issues =
            stacked_critical_extraction_issues >= 2 && extraction_grounding_confidence < 0.50;
        // Degraded-but-store: if structured extraction failed (parse error,
        // 1B model misshape, missing engine) but the OCR itself is strong
        // — long, decent confidence, and not noisy — we'd rather emit a
        // text-grounded memory tagged as `extraction_parse_failed` than
        // drop it entirely. Without this, every "JSON wasn't a valid
        // StructuredMemoryExtraction" log silently kills an otherwise good
        // frame at the grounding gate. The strict gate still applies to
        // weak OCR so we don't pollute the vault with empty rows.
        let ocr_grounded_enough = structured_memory.is_none()
            && text.len() >= 220
            && observed_confidence >= 0.45
            && noise_score <= config.capture_pipeline.noise_skip_threshold
            && capture_quality.avg_line_score >= 0.30
            && !drop_due_to_stacked_issues;
        // Text-heavy override: when OCR evidence is rich (many chars + blocks,
        // screen-typical confidence), admit the frame even if structured extraction
        // produced a low grounding score. Prevents high-value developer-context
        // frames from being silently dropped when the 1B model mis-shapes JSON.
        let text_heavy_override = should_text_heavy_override(
            text.len(),
            observed_confidence,
            observed_block_count,
            noise_score,
            config.capture_pipeline.noise_skip_threshold,
            drop_due_to_stacked_issues,
        );
        if (extraction_grounding_confidence <= 0.10 && !ocr_grounded_enough && !text_heavy_override)
            || drop_due_to_stacked_issues
        {
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
                    "stored_or_skipped": if drop_due_to_stacked_issues {
                        "skipped_stacked_extraction_issues"
                    } else {
                        "skipped_grounding_gate"
                    },
                    "grounding_confidence": extraction_grounding_confidence,
                    "stacked_critical_extraction_issues": stacked_critical_extraction_issues,
                    "extraction_issues": extraction_issues,
                    "text_heavy_override": text_heavy_override,
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
            state.capture_stats.record_skip(
                if drop_due_to_stacked_issues {
                    crate::SkipReason::StackedExtraction
                } else {
                    crate::SkipReason::Grounding
                },
                &app_name,
            );
            tokio::time::sleep(sleep_duration).await;
            continue;
        }
        // Reaching here with `ocr_grounded_enough` means we're using the
        // OCR text + window/app context as the durable signal instead of
        // a structured LLM extraction. Downstream code branches on
        // `structured_memory.is_none()` and already has fallbacks for
        // snippet/internal_context, so we just tag the path for telemetry.
        let extraction_parse_failed_degraded = structured_memory.is_none() && ocr_grounded_enough;

        // OCR-narrative branch: structured extraction is the durable
        // signal. The visual-narrative branch (low-OCR frames) ran
        // earlier with its own VLM and never reaches this point, so the
        // historical `vlm_analysis: Option<String> = None` stub used to
        // be dead code and has been removed.
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
        } else {
            let fallback = text_cleanup::concise_fallback_snippet(&app_name, &window_title, &text);
            // Tag degraded-path stores so retrieval / debug surfaces can
            // tell apart "no LLM available, OCR-only summary" from a
            // generic fallback (no engine, no OCR worth using).
            let source_label = if extraction_parse_failed_degraded {
                "extraction_parse_failed"
            } else {
                "fallback"
            };
            if fallback.is_empty() {
                (
                    text.chars().take(140).collect::<String>(),
                    source_label.to_string(),
                )
            } else {
                (fallback, source_label.to_string())
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

        // Fetch up to 3 recent same-session-or-project records to chain the
        // new durable memory_context to its predecessors. Failures here are
        // non-fatal — capture proceeds with an empty prior chain.
        let prior_chain = {
            let session_id_for_chain = build_session_id(
                &Local::now(),
                &app_name,
                app_context.bundle_id.as_deref(),
                &build_session_key(&app_name, &window_title, url.as_deref()),
            );
            let project_for_chain = structured_memory
                .as_ref()
                .map(|m| m.project.clone())
                .unwrap_or_default();
            match state
                .store
                .list_recent_by_session_or_project(
                    &session_id_for_chain,
                    &project_for_chain,
                    None,
                    3,
                )
                .await
            {
                Ok(rows) => rows,
                Err(err) => {
                    tracing::debug!("memory_context:prior_chain_fetch_skipped err={err}");
                    Vec::new()
                }
            }
        };
        let durable_memory_context = build_durable_memory_context(
            structured_memory.as_ref(),
            &app_name,
            &window_title,
            &text,
            &display_summary,
            app_context.bundle_id.as_deref(),
            url.as_deref(),
            &prior_chain,
            &config.memory_quality,
        );
        let reopen_target = build_reopen_target(
            url.as_deref(),
            structured_memory
                .as_ref()
                .and_then(|memory| memory.files_touched.first().map(String::as_str)),
            app_context.bundle_id.as_deref(),
            &app_name,
            now.timestamp_millis(),
        );
        let related_memory_ids_from_chain = prior_chain
            .iter()
            .map(|row| row.id.clone())
            .take(3)
            .collect::<Vec<_>>();

        // Hierarchical linking: set parent_id to the most recent prior memory
        // in the chain. `prior_chain` was already fetched by session OR project,
        // so any first() match implies a real session/project relationship.
        // Additional 1-hour recency gate avoids forced linking across stale chains.
        let parent_id_from_chain: Option<String> = prior_chain
            .first()
            .filter(|prior| {
                let gap_ms = (now.timestamp_millis() - prior.timestamp).max(0);
                gap_ms < 60 * 60 * 1000
            })
            .map(|prior| prior.id.clone());

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

        let enriched_clean_text = text.clone();
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
            &durable_memory_context,
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

        // ── CLIP image embedding ──────────────────────────────────────────────
        // Compute a 512-d L2-normalized image vector from the same screen pixels
        // OCR consumed. Stored alongside text embeddings so future image-to-image
        // retrieval can find visually similar screens. The text pipeline above is
        // the source of truth; on any CLIP failure we fall back to a zero vector
        // so retrieval/storage stay healthy.
        let (image_embedding, clip_embedding_status) = {
            let bytes = image_data.clone();
            let models_dir = models_dir.clone();
            let clip_start = Instant::now();
            let join = tokio::task::spawn_blocking(move || -> Result<Vec<f32>, String> {
                let dynamic = image::load_from_memory(&bytes)
                    .map_err(|e| format!("decode capture png: {e}"))?;
                embed_imported_image(&dynamic, &models_dir)
            })
            .await;
            match join {
                Ok(Ok(vec)) => {
                    tracing::debug!(
                        app = %app_name,
                        clip_ms = clip_start.elapsed().as_millis(),
                        "capture_pipeline:image_embedding_ok"
                    );
                    (vec, "ok".to_string())
                }
                Ok(Err(e)) => {
                    tracing::warn!(
                        "CLIP image embedding skipped for capture: {e}; storing zero vector"
                    );
                    (
                        vec![0.0f32; DEFAULT_IMAGE_EMBEDDING_DIM],
                        format!("error:{e}"),
                    )
                }
                Err(e) => {
                    tracing::warn!("CLIP image embedding join failed: {e}; storing zero vector");
                    (
                        vec![0.0f32; DEFAULT_IMAGE_EMBEDDING_DIM],
                        format!("join_error:{e}"),
                    )
                }
            }
        };
        let host_supports_qwen_vlm =
            crate::telemetry::system_metrics::host_supports_lightweight_vlm();
        let (vlm_pressure_skip, vlm_pressure_reason) =
            crate::telemetry::system_metrics::pressure_recommends_skipping_heavy_models();
        let text_capture_vlm_route = capture_pixel_vlm_route(
            &config,
            state.app_data_dir.as_path(),
            text.len(),
            observed_confidence,
            observed_block_count,
            clip_embedding_status == "ok",
            false,
            vlm_pressure_skip,
            host_supports_qwen_vlm,
            config.vlm_max_calls_per_minute,
        );

        // Release the model pipeline lock now that LLM + text-embed + CLIP
        // have all completed for this frame. Downstream Focus Mode / storage
        // / graph work doesn't touch the heavy model surfaces.
        drop(_pipeline_guard);

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
            "semantic_layout": semantic_layout_diagnostics(&enriched_clean_text, &capture_quality),
            "semantic_fusion": semantic_fusion_diagnostics.clone(),
            "fusion_sources": semantic_fusion_diagnostics
                .get("sources")
                .cloned()
                .unwrap_or_else(|| json!([])),
            "vlm_route": text_capture_vlm_route.label(),
            "vlm_block_reason": text_capture_vlm_route.fallback_reason(),
            "host_supports_vlm": host_supports_qwen_vlm,
            "pressure_reason": vlm_pressure_reason,
            "clip_embedding_status": clip_embedding_status,
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
            image_embedding,
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
            intent_analysis: crate::storage::IntentAnalysis::default(),
            memory_context: durable_memory_context.clone(),
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
            reopen_kind: reopen_target.kind,
            reopen_url: reopen_target.url,
            reopen_file_path: reopen_target.file_path,
            reopen_app_bundle_id: reopen_target.app_bundle_id,
            reopen_app_name: reopen_target.app_name,
            reopen_app_deep_link: reopen_target.app_deep_link,
            reopen_captured_at_ms: reopen_target.captured_at_ms,
            reopen_confidence: reopen_target.confidence,
            reopen_validation_status: reopen_target.validation_status,
            search_aliases: structured_memory
                .as_ref()
                .map(|m| m.search_aliases.clone())
                .unwrap_or_default(),
            related_memory_ids: related_memory_ids_from_chain,
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
                .map(|m| crate::storage::schema::GitStats {
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
            embedding_model: "all-MiniLM-L6-v2".to_string(), // Default assumption, actual model set in pipeline
            embedding_dim: EMBEDDING_DIM as u32,
            enrichment_status: String::new(),
            fallback_reason: None,
            raw_screenshot_stored: false,
            is_consolidated: false,
            is_soft_deleted: false,
            parent_id: parent_id_from_chain,
            related_ids: Vec::new(),
            consolidated_from: Vec::new(),
            synthesis_branch: structured_memory
                .as_ref()
                .map(|m| m.synthesis_branch.clone())
                .unwrap_or_else(|| "fallback".to_string()),
            topic_categories: structured_memory
                .as_ref()
                .map(|m| m.topic_categories.clone())
                .unwrap_or_default(),
            insight_what_happened: structured_memory
                .as_ref()
                .filter(|m| !m.insight_what_happened.is_empty())
                .map(|m| m.insight_what_happened.clone())
                .unwrap_or_default(),
            insight_why_mattered: structured_memory
                .as_ref()
                .filter(|m| !m.insight_why_mattered.is_empty())
                .map(|m| m.insight_why_mattered.clone())
                .unwrap_or_default(),
            insight_what_changed: String::new(),
            insight_context_thread: String::new(),
            insight_spans_json: String::new(),
            insight_card_confidence: 0.0,
        };
        let incoming_record_id = record.id.clone();
        let batch_size_before = batch.len();
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
            Ok(merged) => {
                let batch_size_after = batch.len();
                if batch_size_after > batch_size_before {
                    batch_outcomes.push(crate::StoreOutcome::OcrPath);
                }
                merged
            }
            Err(err) => {
                tracing::warn!("Memory continuity merge failed for {}: {}", record.id, err);
                batch.push(record.clone());
                batch_outcomes.push(crate::StoreOutcome::OcrPath);
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
        if extraction_parse_failed_degraded {
            tracing::debug!(
                app = %app_name,
                "stored OCR-grounded memory via degraded path (LLM structured extraction unavailable)"
            );
        }
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
    let synthesis_branch = prefer_non_empty(&incoming.synthesis_branch, &existing.synthesis_branch);
    let topic_categories =
        merge_string_lists(&existing.topic_categories, &incoming.topic_categories);
    let insight_what_happened = prefer_non_empty(
        &incoming.insight_what_happened,
        &existing.insight_what_happened,
    );
    let insight_why_mattered = prefer_non_empty(
        &incoming.insight_why_mattered,
        &existing.insight_why_mattered,
    );
    let insight_what_changed = prefer_non_empty(
        &incoming.insight_what_changed,
        &existing.insight_what_changed,
    );
    let insight_context_thread = prefer_non_empty(
        &incoming.insight_context_thread,
        &existing.insight_context_thread,
    );
    let insight_spans_json =
        prefer_non_empty(&incoming.insight_spans_json, &existing.insight_spans_json);
    let insight_card_confidence = existing
        .insight_card_confidence
        .max(incoming.insight_card_confidence);

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
        reopen_kind: if incoming.reopen_kind != crate::memory::reopen::ReopenKind::Unknown {
            incoming.reopen_kind.clone()
        } else {
            existing.reopen_kind.clone()
        },
        reopen_url: incoming.reopen_url.clone().or(existing.reopen_url.clone()),
        reopen_file_path: incoming
            .reopen_file_path
            .clone()
            .or(existing.reopen_file_path.clone()),
        reopen_app_bundle_id: incoming
            .reopen_app_bundle_id
            .clone()
            .or(existing.reopen_app_bundle_id.clone()),
        reopen_app_name: incoming
            .reopen_app_name
            .clone()
            .or(existing.reopen_app_name.clone()),
        reopen_app_deep_link: incoming
            .reopen_app_deep_link
            .clone()
            .or(existing.reopen_app_deep_link.clone()),
        reopen_captured_at_ms: if incoming.reopen_captured_at_ms > 0 {
            incoming.reopen_captured_at_ms
        } else {
            existing.reopen_captured_at_ms
        },
        reopen_confidence: existing.reopen_confidence.max(incoming.reopen_confidence),
        reopen_validation_status: if incoming.reopen_validation_status
            != crate::memory::reopen::ReopenValidationStatus::Unchecked
        {
            incoming.reopen_validation_status.clone()
        } else {
            existing.reopen_validation_status.clone()
        },
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
        enrichment_status: prefer_non_empty(
            &incoming.enrichment_status,
            &existing.enrichment_status,
        ),
        fallback_reason: incoming
            .fallback_reason
            .clone()
            .or_else(|| existing.fallback_reason.clone()),
        raw_screenshot_stored: existing.raw_screenshot_stored || incoming.raw_screenshot_stored,
        is_consolidated: existing.is_consolidated || incoming.is_consolidated,
        is_soft_deleted: existing.is_soft_deleted || incoming.is_soft_deleted,
        parent_id,
        related_ids,
        consolidated_from,
        synthesis_branch,
        topic_categories,
        insight_what_happened,
        insight_why_mattered,
        insight_what_changed,
        insight_context_thread,
        insight_spans_json,
        insight_card_confidence,
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

/// Returns true when the OCR volume alone justifies admitting the frame,
/// bypassing a low extraction_grounding_confidence score.
///
/// Conditions that still block admission:
/// - `drop_due_to_stacked` is true (≥2 critical extraction failures)
/// - `noise_score` exceeds the pipeline threshold
/// - `text_volume_qualifies` returns false (too short / low confidence)
fn should_text_heavy_override(
    text_len: usize,
    observed_confidence: f32,
    observed_block_count: usize,
    noise_score: f32,
    noise_threshold: f32,
    drop_due_to_stacked: bool,
) -> bool {
    !drop_due_to_stacked
        && noise_score <= noise_threshold
        && crate::ocr::text_volume_qualifies(text_len, observed_confidence, observed_block_count)
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
    fn self_app_skip_is_distinct_from_user_blocklist() {
        let blocklist: Vec<String> = Vec::new();
        assert_eq!(
            capture_context_skip_reason(
                "FNDR",
                Some("com.fndr.desktop"),
                "Settings",
                None,
                &blocklist,
            ),
            Some(crate::SkipReason::SelfApp)
        );
        assert_eq!(
            capture_context_skip_reason(
                "Finder",
                Some("com.apple.finder"),
                "Desktop",
                None,
                &blocklist,
            ),
            None
        );
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
    fn low_ram_semantic_fusion_builds_codex_docs_memory_without_topic_scaffold() {
        let raw = r#"
Push latest changes
README.md
DESIGN_DIRECTION.md
Removed untracked planning/artifact files and folders.
Still planned but not implemented (from current committed docs):
1. Optional Qwen3-VL + mmproj photo-import vision path is documented as optional setup, not baseline behavior yet (README.md:202).
2. Image-aware retrieval is explicitly future (CLIP vector stored now, richer retrieval later) (README.md:129).
3. Future graph enrichment runtime is noted as future/additive in design direction (DESIGN_DIRECTION.md:106).
Future Roadmap
Near Term
Advanced idle detection
Medium Term
Semantic timeline (group by topic, not just time)
Activity patterns and insights dashboard
"#;
        let quality = text_cleanup::CaptureQualityStats {
            total_lines: 51,
            kept_lines: 50,
            low_conf_lines: 51,
            dropped_noise_lines: 0,
            dropped_low_signal_lines: 1,
            avg_line_score: 0.58,
        };

        let fusion = build_low_ram_semantic_fusion(
            "Codex",
            "Push latest changes",
            None,
            raw,
            None,
            &quality,
            "ocr",
        )
        .expect("fusion should build from OCR, file refs, and app/window context");

        assert!(fusion.extraction.memory_context.contains("README.md"));
        assert!(fusion
            .extraction
            .memory_context
            .contains("DESIGN_DIRECTION.md"));
        assert!(fusion
            .extraction
            .user_intent
            .contains("implementation status"));
        assert!(!fusion.extraction.memory_context.starts_with("Topic:"));
        assert!(!fusion
            .extraction
            .search_aliases
            .iter()
            .any(|alias| alias.contains("tspbn")));
        assert!(fusion
            .extraction
            .files_touched
            .iter()
            .any(|file| file == "README.md"));
        assert!(fusion
            .extraction
            .files_touched
            .iter()
            .any(|file| file == "DESIGN_DIRECTION.md"));
    }

    #[test]
    fn visual_only_low_ram_capture_without_ocr_is_not_stored() {
        let route = VlmRouteDecision::FallbackOcrOnly {
            reason: "vlm_blocked_low_ram".to_string(),
        };

        assert!(should_skip_ungrounded_low_ram_visual_capture(
            &route, 0, 0.0, 0, 20
        ));
        assert!(!should_skip_ungrounded_low_ram_visual_capture(
            &route, 64, 0.42, 6, 20
        ));
        assert!(should_skip_ungrounded_low_ram_visual_capture(
            &route, 9, 0.42, 6, 20
        ));
        assert!(!should_skip_ungrounded_low_ram_visual_capture(
            &route, 83, 0.48, 8, 20
        ));
        assert!(!should_skip_ungrounded_low_ram_visual_capture(
            &VlmRouteDecision::RunQwenVlm,
            0,
            0.0,
            0,
            20
        ));
    }

    #[test]
    fn fused_primary_embedding_text_excludes_raw_ocr_excerpt() {
        let quality = text_cleanup::CaptureQualityStats {
            total_lines: 8,
            kept_lines: 8,
            low_conf_lines: 0,
            dropped_noise_lines: 0,
            dropped_low_signal_lines: 0,
            avg_line_score: 0.72,
        };
        let raw = "README.md DESIGN_DIRECTION.md Output Truncation: All tool outputs are truncated to prevent context overflow";
        let fusion = build_low_ram_semantic_fusion(
            "Codex",
            "Push latest changes",
            None,
            raw,
            None,
            &quality,
            "ocr",
        )
        .expect("fusion should build");
        let text = compose_primary_embedding_text(
            Some(&fusion.extraction),
            "Codex",
            "Push latest changes",
            &fusion.extraction.memory_context,
            "Reviewing README and DESIGN_DIRECTION implementation status.",
            raw,
            "Output Truncation noisy raw OCR",
        );

        assert!(text.contains("intent: reviewing implementation status"));
        assert!(text.contains("files: README.md, DESIGN_DIRECTION.md"));
        assert!(text.contains("context: "));
        assert!(!text.contains("Output Truncation"));
        assert!(!text.contains("evidence:"));
        assert!(!text.contains("shadow:"));
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
        assert!(!text.contains("evidence:"));
        assert!(!text.contains(&"evidence ".repeat(20)));
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

    fn durable_context_config(min: u32, max: u32) -> crate::config::MemoryQualityConfig {
        let mut cfg = crate::config::MemoryQualityConfig::default();
        cfg.memory_context_min_chars = min;
        cfg.memory_context_max_chars = max;
        cfg
    }

    fn synth_prior_search_result(id: &str, context: &str) -> crate::storage::SearchResult {
        crate::storage::SearchResult {
            id: id.to_string(),
            memory_context: context.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn durable_memory_context_respects_min_max_bounds_with_empty_chain() {
        let extraction = StructuredMemoryExtraction {
            user_intent: "Refactor the synthesis pipeline".to_string(),
            project: "FNDR".to_string(),
            topic: "memory synthesis".to_string(),
            decisions: vec!["Use durable memory_context as embedding seed".to_string()],
            next_steps: vec!["Wire compress_to_salient_evidence into the tail".to_string()],
            ..Default::default()
        };
        let cfg = durable_context_config(220, 1800);
        let context = build_durable_memory_context(
            Some(&extraction),
            "GenericEditor",
            "synthesis-doc.md",
            "We are aligning the embedding text composition.",
            "Refactored OCR cleanup.",
            Some("com.example.editor"),
            None,
            &[],
            &cfg,
        );
        assert!(
            context.chars().count() >= cfg.memory_context_min_chars as usize,
            "durable context shorter than min ({}): {}",
            context.chars().count(),
            context
        );
        assert!(context.chars().count() <= cfg.memory_context_max_chars as usize);
        assert!(
            context.to_lowercase().contains("topic")
                || context.to_lowercase().contains("memory synthesis"),
            "should surface semantic center"
        );
    }

    #[test]
    fn durable_memory_context_references_prior_work_without_machine_marker() {
        let extraction = StructuredMemoryExtraction {
            user_intent: "Continue refactor".to_string(),
            topic: "alias generation".to_string(),
            decisions: vec!["Adopt noun-phrase sourcing".to_string()],
            ..Default::default()
        };
        let cfg = durable_context_config(160, 1800);
        let prior = synth_prior_search_result(
            "abcd1234efgh5678",
            "Earlier card outlining the durable memory context plan.",
        );
        let context = build_durable_memory_context(
            Some(&extraction),
            "GenericEditor",
            "doc",
            "More work",
            "",
            None,
            None,
            std::slice::from_ref(&prior),
            &cfg,
        );
        assert!(
            context.contains("This continues earlier related work"),
            "continuation footer missing: {}",
            context
        );
        assert!(
            !context.contains("Continues from "),
            "machine continuation marker leaked into memory_context: {}",
            context
        );
    }

    #[test]
    fn durable_memory_context_keeps_reopen_marker_out_of_context() {
        let cfg = durable_context_config(160, 1800);
        let context = build_durable_memory_context(
            None,
            "GenericEditor",
            "doc",
            "Worked on the design doc and reviewed the spec.",
            "Reviewed design doc.",
            None,
            Some("https://example.org/path"),
            &[],
            &cfg,
        );
        assert!(
            !context.contains("Reopen:"),
            "reopen marker leaked into memory_context: {}",
            context
        );
    }

    #[test]
    fn durable_memory_context_truncates_to_max_with_three_priors() {
        let extraction = StructuredMemoryExtraction {
            user_intent: "Long-running multi-session refactor".to_string(),
            topic: "memory synthesis".to_string(),
            decisions: vec!["a; ".repeat(60).trim_end().to_string()],
            errors: vec!["x; ".repeat(60).trim_end().to_string()],
            next_steps: vec!["n; ".repeat(60).trim_end().to_string()],
            results: vec!["r; ".repeat(60).trim_end().to_string()],
            ..Default::default()
        };
        let cfg = durable_context_config(220, 600);
        let prior_a = synth_prior_search_result("a1b2c3d4", "Prior alpha context.");
        let prior_b = synth_prior_search_result("e5f6g7h8", "Prior beta context.");
        let prior_c = synth_prior_search_result("i9j0k1l2", "Prior gamma context.");
        let priors = vec![prior_a, prior_b, prior_c];
        let context = build_durable_memory_context(
            Some(&extraction),
            "GenericEditor",
            "doc",
            "evidence body",
            "Reviewed design doc.",
            None,
            None,
            &priors,
            &cfg,
        );
        assert!(context.chars().count() <= cfg.memory_context_max_chars as usize);
    }

    // ── Narrative-first composer ──────────────────────────────────────────

    #[test]
    fn durable_memory_context_leads_with_narrative_and_drops_topic_when_covered() {
        let extraction = StructuredMemoryExtraction {
            user_intent: "researching".to_string(),
            topic: "memory synthesis".to_string(),
            activity_type: "researching".to_string(),
            memory_context:
                "Continued the memory synthesis investigation, comparing two ranking strategies."
                    .to_string(),
            ..Default::default()
        };
        let cfg = durable_context_config(80, 1800);
        let context = build_durable_memory_context(
            Some(&extraction),
            "GenericEditor",
            "research-doc.md",
            "evidence body",
            "Reviewed design doc.",
            None,
            None,
            &[],
            &cfg,
        );
        let head_line = context.lines().next().unwrap_or("");
        assert!(
            head_line.starts_with("Continued the memory synthesis"),
            "narrative should lead, got: {head_line:?}"
        );
        // Topic tokens ("memory", "synthesis") already appear in the narrative,
        // so the explicit Topic: line must be suppressed.
        assert!(
            !context.contains("Topic: memory synthesis"),
            "Topic: line must be dropped when narrative covers it; got: {context}"
        );
        // And the robotic "You were ..."/"Activity: ..." preamble must not fire
        // when a narrative is present.
        assert!(!context.contains("You were "));
        assert!(!context.contains("Activity: "));
    }

    #[test]
    fn durable_memory_context_appends_topic_when_narrative_lacks_it() {
        let extraction = StructuredMemoryExtraction {
            topic: "ranking quality".to_string(),
            memory_context: "Focused on chart styling improvements for the dashboard.".to_string(),
            ..Default::default()
        };
        let cfg = durable_context_config(80, 1800);
        let context = build_durable_memory_context(
            Some(&extraction),
            "GenericEditor",
            "doc",
            "evidence body",
            "Reviewed design doc.",
            None,
            None,
            &[],
            &cfg,
        );
        assert!(
            context.contains("Topic: ranking quality"),
            "Topic should be appended when narrative omits the topic tokens; got: {context}"
        );
    }

    #[test]
    fn validate_structured_memory_extraction_clears_pipe_delimited_activity_type() {
        let mut extraction = StructuredMemoryExtraction {
            confidence: 0.42,
            activity_type: "coding|debugging|reviewing_agent_output|researching|planning|writing"
                .to_string(),
            topic: "valid topic".to_string(),
            ..Default::default()
        };
        let (_grounding, issues) = validate_structured_memory_extraction(
            &mut extraction,
            "Chrome",
            "title",
            "valid topic appeared in evidence",
        );
        assert!(extraction.activity_type.is_empty());
        assert!(
            issues
                .iter()
                .any(|i| i == "activity_type_multi_option_dump"),
            "expected activity_type_multi_option_dump in issues: {issues:?}"
        );
    }

    #[test]
    fn validate_structured_memory_extraction_clears_pipe_delimited_topic_and_workflow() {
        let mut extraction = StructuredMemoryExtraction {
            confidence: 0.5,
            topic: "topic|alternative".to_string(),
            workflow: "wf_a|wf_b".to_string(),
            user_intent: "intent|other".to_string(),
            ..Default::default()
        };
        let (_, issues) =
            validate_structured_memory_extraction(&mut extraction, "App", "Title", "Evidence body");
        assert!(extraction.topic.is_empty());
        assert!(extraction.workflow.is_empty());
        assert!(extraction.user_intent.is_empty());
        assert!(issues.iter().any(|i| i == "topic_multi_option_dump"));
        assert!(issues.iter().any(|i| i == "workflow_multi_option_dump"));
        assert!(issues.iter().any(|i| i == "user_intent_multi_option_dump"));
    }

    #[test]
    fn narrative_mentions_detects_majority_overlap() {
        assert!(narrative_mentions(
            Some("The memory synthesis pipeline keeps cards short."),
            "memory synthesis pipeline",
        ));
        assert!(!narrative_mentions(
            Some("Browsed unrelated news articles."),
            "memory synthesis pipeline",
        ));
        assert!(!narrative_mentions(None, "any topic"));
    }

    // ── VisualNoveltyTracker ──────────────────────────────────────────────

    fn synth_vec(seed: usize, dim: usize) -> Vec<f32> {
        // Deterministic, non-zero, mostly-orthogonal-ish across seeds. Not
        // normalized — `cosine_similarity` already handles arbitrary norms.
        (0..dim)
            .map(|i| (((seed * 31 + i * 7) % 17) as f32 - 8.0) / 8.0)
            .collect()
    }

    #[test]
    fn visual_novelty_tracker_admits_first_frame_when_ring_is_empty() {
        let tracker = VisualNoveltyTracker::default();
        let v = synth_vec(1, 32);
        assert!((tracker.novelty(&v) - 1.0).abs() < 1e-6);
        let t0 = tracker.adaptive_threshold(0.30, 0.05, 0.85);
        assert!((t0 - 0.30).abs() < 1e-6, "first threshold = base");
    }

    #[test]
    fn visual_novelty_tracker_rejects_near_duplicate_after_admit() {
        let mut tracker = VisualNoveltyTracker::default();
        tracker.reset_for("session-a");
        let v = synth_vec(7, 32);
        tracker.admit(v.clone(), 16);
        // Same vector → cosine ~1.0 → novelty ~0.0, well below 0.30 base.
        let novelty = tracker.novelty(&v);
        assert!(
            novelty < 0.05,
            "near-duplicate should produce ~0 novelty, got {novelty}"
        );
    }

    #[test]
    fn visual_novelty_tracker_adaptive_threshold_rises_with_admits() {
        let mut tracker = VisualNoveltyTracker::default();
        tracker.reset_for("session-x");
        for i in 0..5 {
            tracker.admit(synth_vec(100 + i, 32), 16);
        }
        let t5 = tracker.adaptive_threshold(0.30, 0.05, 0.85);
        assert!((t5 - 0.55).abs() < 1e-6, "0.30 + 5*0.05 = 0.55, got {t5}");
        // Ceiling caps the threshold.
        for i in 0..20 {
            tracker.admit(synth_vec(500 + i, 32), 16);
        }
        let t_capped = tracker.adaptive_threshold(0.30, 0.05, 0.85);
        assert!(t_capped <= 0.85 + 1e-6);
    }

    #[test]
    fn visual_novelty_tracker_resets_on_session_change() {
        let mut tracker = VisualNoveltyTracker::default();
        tracker.reset_for("session-a");
        tracker.admit(synth_vec(1, 32), 16);
        tracker.admit(synth_vec(2, 32), 16);
        assert_eq!(tracker.admitted, 2);
        tracker.reset_for("session-b");
        assert_eq!(tracker.admitted, 0);
        assert!(tracker.recent.is_empty());
    }

    #[test]
    fn visual_novelty_tracker_respects_ring_capacity() {
        let mut tracker = VisualNoveltyTracker::default();
        tracker.reset_for("ring-test");
        for i in 0..10 {
            tracker.admit(synth_vec(i, 32), 4);
        }
        assert_eq!(tracker.recent.len(), 4, "ring should be capped at 4");
        assert_eq!(tracker.admitted, 10);
    }

    #[test]
    fn image_import_source_screen_capture_has_distinct_labels() {
        use crate::inference::ImageImportSource;
        assert_eq!(
            ImageImportSource::ScreenCapture.api_label(),
            "screen_capture_visual"
        );
        assert_eq!(
            ImageImportSource::ScreenCapture.header_label(),
            "Screen capture (visual)"
        );
    }

    #[test]
    fn compose_visual_capture_screen_capture_leads_with_screen_capture_header() {
        use crate::inference::{
            compose_import_memory_context, ImageImportSource, ImageSemanticInsight,
        };
        let insight = ImageSemanticInsight {
            summary_short: "User watching a cricket match overlay.".to_string(),
            summary_detailed:
                "Browser window playing a livestream with scoreboard and play-by-play.".to_string(),
            scene_type: "livestream".to_string(),
            topics: vec!["cricket livestream".to_string()],
            ..Default::default()
        };
        let composed = compose_import_memory_context(
            "GoogleChrome_1.png",
            &insight,
            None,
            ImageImportSource::ScreenCapture,
        );
        assert!(
            composed
                .memory_context
                .starts_with("Screen capture (visual):"),
            "expected screen-capture header, got: {}",
            composed.memory_context
        );
        // Narrative content from the VLM must appear right after the header.
        assert!(composed.memory_context.contains("livestream"));
    }

    #[test]
    fn text_heavy_override_fires_for_large_clean_ocr() {
        // 1,400 chars, 38 blocks, confidence 0.49, low noise — should override
        assert!(should_text_heavy_override(
            1400, 0.49, 38, 0.20, 0.50, false
        ));
    }

    #[test]
    fn text_heavy_override_blocked_by_stacked_issues() {
        // Even with good OCR, stacked issues prevent override
        assert!(!should_text_heavy_override(
            1400, 0.49, 38, 0.20, 0.50, true
        ));
    }

    #[test]
    fn text_heavy_override_blocked_by_high_noise() {
        // High noise_score prevents override even for large text
        assert!(!should_text_heavy_override(
            1400, 0.49, 38, 0.80, 0.50, false
        ));
    }

    #[test]
    fn text_heavy_override_blocked_for_tiny_text() {
        // 50 chars doesn't qualify
        assert!(!should_text_heavy_override(50, 0.49, 5, 0.20, 0.50, false));
    }
}
