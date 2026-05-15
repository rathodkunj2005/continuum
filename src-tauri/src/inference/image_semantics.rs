//! Pixel-based visual semantic extraction for imported photos (Meta glasses, file picker).
//!
//! Uses llama.cpp **MTMD** with **SmolVLM 500M** or **Qwen3-VL 4B** GGUF + matching **mmproj** weights. This path
//! passes image bytes through `MtmdBitmap::from_buffer` — it does **not** claim vision when
//! only OCR or text-only models are available.

use super::get_or_init_backend;
use crate::models;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
#[allow(deprecated)]
use llama_cpp_2::model::Special;
use llama_cpp_2::model::{LlamaChatMessage, LlamaChatTemplate, LlamaModel};
use llama_cpp_2::mtmd::{
    mtmd_default_marker, MtmdBitmap, MtmdContext, MtmdContextParams, MtmdInputText,
};
use llama_cpp_2::sampling::LlamaSampler;
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashSet;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

/// Where the visual-semantics path was invoked from (product / analytics).
/// `ScreenCapture` is the screen-capture pipeline using the same VLM as
/// imports but only on frames the adaptive visual-novelty gate admitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageImportSource {
    MetaGlasses,
    FilePicker,
    ScreenCapture,
}

impl ImageImportSource {
    pub fn api_label(&self) -> &'static str {
        match self {
            ImageImportSource::MetaGlasses => "meta_glasses_import",
            ImageImportSource::FilePicker => "file_picker_import",
            ImageImportSource::ScreenCapture => "screen_capture_visual",
        }
    }

    /// Human-readable header used as the first sentence of the durable
    /// `memory_context`. Kept short and content-agnostic — no per-app
    /// hardcoding, just the kind of pipeline that produced the row.
    pub fn header_label(&self) -> &'static str {
        match self {
            ImageImportSource::MetaGlasses => "Meta glasses import",
            ImageImportSource::FilePicker => "Imported photo",
            ImageImportSource::ScreenCapture => "Screen capture (visual)",
        }
    }
}

/// Which MTMD model family is loaded in the singleton runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MtmdModelFamily {
    /// Qwen3-VL 4B — high quality, ~6 GB RAM
    Qwen3Vl4B,
    /// SmolVLM 500M — lightweight, ~1.2 GB RAM
    SmolVlm500M,
}

impl MtmdModelFamily {
    pub(crate) fn from_model_id(model_id: &str) -> Option<Self> {
        match model_id {
            "qwen3-vl-4b" => Some(Self::Qwen3Vl4B),
            "smolvlm-500m" => Some(Self::SmolVlm500M),
            _ => None,
        }
    }

    pub fn model_id_str(&self) -> &'static str {
        match self {
            Self::Qwen3Vl4B => "qwen3-vl-4b",
            Self::SmolVlm500M => "smolvlm-500m",
        }
    }

    /// Context size to use for this model family.
    pub fn context_size(&self) -> u32 {
        match self {
            Self::Qwen3Vl4B => 1536,
            Self::SmolVlm500M => 1024,
        }
    }
}

/// Structured output from the local vision model (JSON contract).
#[derive(Debug, Clone, Default)]
pub struct ImageSemanticInsight {
    pub summary_short: String,
    pub summary_detailed: String,
    pub scene_type: String,
    pub setting: Option<String>,
    pub activity_type: Option<String>,
    pub user_intent: Option<String>,
    pub visible_objects: Vec<String>,
    pub people_roles: Vec<String>,
    pub entities: Vec<String>,
    pub actions: Vec<String>,
    pub topics: Vec<String>,
    pub search_aliases: Vec<String>,
    pub confidence: f32,
    pub model_id: String,
}

/// Composed durable text fields for [`crate::storage::MemoryRecord`].
#[derive(Debug, Clone)]
pub struct ImportMemoryText {
    pub memory_context: String,
    pub embedding_text: String,
    pub search_aliases: Vec<String>,
    pub topic: String,
    pub activity_type: String,
    pub user_intent: String,
    pub insight_what_happened: String,
    pub insight_why_mattered: String,
}

/// Minimal OCR quality inputs for import gating (decoupled from IPC types).
#[derive(Debug, Clone, Copy)]
pub struct ImportOcrStats {
    pub confidence: f32,
    pub blocks: u32,
}

/// Run MTMD model (SmolVLM 500M or Qwen3-VL 4B) on **pixels** (lazy singleton; first call loads weights).
///
/// Returns [`Err`] when the multimodal stack is unavailable (missing model/mmproj, init
/// failure, or inference error). Callers must **not** treat OCR as a substitute for success.
pub async fn extract_image_semantics(
    image_bytes: Vec<u8>,
    filename: &str,
    source: ImageImportSource,
    app_data_dir: PathBuf,
) -> Result<ImageSemanticInsight, String> {
    if !crate::telemetry::system_metrics::host_supports_vlm() {
        return Err("vlm_blocked_low_ram".to_string());
    }
    let filename = filename.to_string();
    tracing::debug!(?source, %filename, "extract_image_semantics: scheduling blocking vision run");
    tokio::task::spawn_blocking(move || {
        let runtime = MtmdVlmRuntime::instance(&app_data_dir)?;
        runtime.run_blocking(&image_bytes, &filename)
    })
    .await
    .map_err(|e| format!("vision task join: {e}"))?
}

/// Reject low-value OCR for **imported photos** (distinct from screenshot OCR policy).
pub fn should_include_import_ocr(text: &str, stats: &ImportOcrStats) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return false;
    }
    if t.len() < 12 {
        return false;
    }
    if stats.confidence < 0.35 {
        return false;
    }
    if stats.blocks < 2 && stats.confidence < 0.55 {
        return false;
    }
    // Mostly punctuation / noise
    let alnum: usize = t.chars().filter(|c| c.is_alphanumeric()).count();
    if alnum * 3 < t.chars().count() {
        return false;
    }
    // Repeated-character spam (e.g. ". Ja JJjJ'JJJJJ.")
    if repeated_char_noise(t) {
        return false;
    }
    if lexical_diversity(t) < 0.18 {
        return false;
    }
    if !has_meaningful_word(t) {
        return false;
    }
    true
}

fn repeated_char_noise(text: &str) -> bool {
    let chars: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
    if chars.len() < 8 {
        return false;
    }
    let mut max_run = 1usize;
    let mut run = 1usize;
    for w in chars.windows(2) {
        if w[0] == w[1] {
            run += 1;
            max_run = max_run.max(run);
        } else {
            run = 1;
        }
    }
    max_run >= chars.len() / 3 || max_run >= 7
}

fn lexical_diversity(text: &str) -> f32 {
    let words: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_ascii_lowercase())
        .collect();
    if words.is_empty() {
        return 0.0;
    }
    let unique: HashSet<_> = words.iter().cloned().collect();
    unique.len() as f32 / words.len() as f32
}

fn has_meaningful_word(text: &str) -> bool {
    const STOP: &[&str] = &[
        "ja", "jj", "jpg", "jpeg", "png", "meta", "glasses", "import",
    ];
    text.split(|c: char| !c.is_alphanumeric())
        .filter_map(|w| {
            let w = w.trim();
            if w.len() < 3 {
                return None;
            }
            let lower = w.to_ascii_lowercase();
            if STOP.iter().any(|s| *s == lower) {
                return None;
            }
            Some(())
        })
        .next()
        .is_some()
}

/// Build durable memory strings from vision insight; optional OCR appendix when allowed.
///
/// `source` controls the leading sentence ("Meta glasses import:", "Screen
/// capture (visual):" …) so the same composer works for both the glasses
/// import flow and the screen-capture visual-narrative path. The rest of
/// the composition is content-agnostic.
pub fn compose_import_memory_context(
    filename: &str,
    insight: &ImageSemanticInsight,
    ocr_text: Option<&str>,
    source: ImageImportSource,
) -> ImportMemoryText {
    let header = format!("{}: {filename}", source.header_label());
    let mut body = if !insight.summary_detailed.trim().is_empty() {
        insight.summary_detailed.trim().to_string()
    } else {
        insight.summary_short.trim().to_string()
    };
    if let Some(ref s) = insight.setting {
        let needle = s.trim().to_ascii_lowercase();
        if !needle.is_empty() && !body.to_ascii_lowercase().contains(&needle) {
            body.push_str(&format!(" Setting: {}.", s.trim()));
        }
    }
    let mut memory_context = format!("{header}. {body}");
    // Only append the raw OCR appendix when the insight came from an actual
    // vision model. In the OCR-only and LLM-on-OCR fallback paths the OCR
    // *is* the source of the summary above, so repeating it here would
    // just pollute the memory card with the same low-signal text we
    // already summarized.
    let summary_already_grounded_in_ocr =
        matches!(insight.model_id.as_str(), "ocr_only" | "llm_ocr_grounded");
    if !summary_already_grounded_in_ocr {
        if let Some(ocr) = ocr_text {
            let excerpt: String = ocr.chars().take(400).collect();
            memory_context.push_str("\n\nSupporting OCR (low weight):\n");
            memory_context.push_str(&excerpt);
        }
    }

    let topic = if !insight.topics.is_empty() {
        insight.topics[0].clone()
    } else if !insight.scene_type.is_empty() {
        insight.scene_type.clone()
    } else {
        "imported photo".to_string()
    };

    let activity = insight
        .activity_type
        .clone()
        .unwrap_or_else(|| "photo_capture".to_string());
    let intent = insight
        .user_intent
        .clone()
        .unwrap_or_else(|| "documenting an imported photo".to_string());

    let mut alias_set: HashSet<String> = HashSet::new();
    for a in &insight.search_aliases {
        let k = normalize_alias_key(a);
        if !k.is_empty() && k.len() > 2 {
            alias_set.insert(a.trim().to_string());
        }
    }
    for t in &insight.topics {
        let k = normalize_alias_key(t);
        if k.len() > 3 {
            alias_set.insert(t.trim().to_string());
        }
    }
    for e in &insight.entities {
        let k = normalize_alias_key(e);
        if k.len() > 3 {
            alias_set.insert(e.trim().to_string());
        }
    }
    let search_aliases: Vec<String> = alias_set.into_iter().take(24).collect();

    let embedding_text = format!(
        "source: Meta glasses import\nfile: {filename}\nscene: {}\nactivity: {}\npeople_roles: {}\nobjects: {}\ntopics: {}\nsummary: {}\naliases: {}",
        insight.scene_type,
        activity,
        insight.people_roles.join(", "),
        insight.visible_objects.join(", "),
        insight.topics.join(", "),
        insight.summary_short,
        search_aliases.join(", ")
    );

    let insight_what_happened = if !insight.summary_short.is_empty() {
        insight.summary_short.clone()
    } else {
        body.chars().take(220).collect()
    };
    let insight_why_mattered = if !insight.summary_detailed.is_empty() {
        insight.summary_detailed.clone()
    } else {
        intent.clone()
    };

    ImportMemoryText {
        memory_context,
        embedding_text,
        search_aliases,
        topic,
        activity_type: activity,
        user_intent: intent,
        insight_what_happened,
        insight_why_mattered,
    }
}

fn normalize_alias_key(s: &str) -> String {
    s.to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

#[derive(Deserialize)]
struct VisionJsonRow {
    #[serde(default)]
    summary_short: String,
    #[serde(default)]
    summary_detailed: String,
    #[serde(default)]
    scene_type: String,
    #[serde(default)]
    setting: Option<String>,
    #[serde(default)]
    activity_type: Option<String>,
    #[serde(default)]
    user_intent: Option<String>,
    #[serde(default)]
    people_roles: Vec<String>,
    #[serde(default)]
    visible_objects: Vec<String>,
    #[serde(default)]
    actions: Vec<String>,
    #[serde(default)]
    topics: Vec<String>,
    #[serde(default)]
    search_aliases: Vec<String>,
    #[serde(default)]
    confidence: f32,
}

fn parse_vision_json(raw: &str, model_id: &str) -> Result<ImageSemanticInsight, String> {
    let trimmed = strip_markdown_fence(raw);
    let slice = extract_json_object_slice(&trimmed).ok_or_else(|| {
        format!(
            "no JSON object in model output: {}",
            short_preview(&trimmed, 200)
        )
    })?;
    let row: VisionJsonRow = serde_json::from_str(slice).map_err(|e| {
        format!(
            "vision JSON parse: {e}; preview {}",
            short_preview(slice, 240)
        )
    })?;
    let mut entities: Vec<String> = Vec::new();
    entities.extend(row.topics.iter().cloned());
    entities.extend(row.visible_objects.iter().take(8).cloned());
    entities.dedup_by(|a, b| a.eq_ignore_ascii_case(b));

    Ok(ImageSemanticInsight {
        summary_short: clamp_field(row.summary_short, 280),
        summary_detailed: clamp_field(row.summary_detailed, 1200),
        scene_type: clamp_field(row.scene_type, 120),
        setting: row.setting.map(|s| clamp_field(s, 160)),
        activity_type: row.activity_type.map(|s| clamp_field(s, 80)),
        user_intent: row.user_intent.map(|s| clamp_field(s, 200)),
        visible_objects: sanitize_list(row.visible_objects, 24, 64),
        people_roles: sanitize_list(row.people_roles, 12, 48),
        entities: sanitize_list(entities, 20, 64),
        actions: sanitize_list(row.actions, 16, 80),
        topics: sanitize_list(row.topics, 16, 64),
        search_aliases: sanitize_list(row.search_aliases, 32, 48),
        confidence: row.confidence.clamp(0.0, 1.0),
        model_id: model_id.to_string(),
    })
}

fn short_preview(s: &str, max: usize) -> String {
    let t = s.trim().replace('\n', " ");
    if t.chars().count() <= max {
        t
    } else {
        t.chars().take(max.saturating_sub(3)).collect::<String>() + "..."
    }
}

fn clamp_field(s: String, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.replace('\0', " ")
    } else {
        s.chars()
            .take(max_chars.saturating_sub(1))
            .collect::<String>()
            + "…"
    }
}

fn sanitize_list(mut v: Vec<String>, max_items: usize, max_each: usize) -> Vec<String> {
    v.retain(|s| !s.trim().is_empty());
    v.truncate(max_items);
    v.into_iter().map(|s| clamp_field(s, max_each)).collect()
}

fn strip_markdown_fence(s: &str) -> String {
    let t = s.trim();
    if let Some(rest) = t.strip_prefix("```json") {
        return rest.trim_end_matches('`').trim().to_string();
    }
    if let Some(rest) = t.strip_prefix("```") {
        return rest.trim_end_matches('`').trim().to_string();
    }
    t.to_string()
}

fn extract_json_object_slice(s: &str) -> Option<&str> {
    let start = s.find('{')?;
    let end = s.rfind('}')?;
    if end > start {
        Some(&s[start..=end])
    } else {
        None
    }
}

/// Convert a [`crate::inference::StructuredMemoryExtraction`] (the LLM JSON
/// schema we already use elsewhere) into an [`ImageSemanticInsight`] so the
/// import composer can consume it without caring whether the underlying
/// content came from a VLM or from an OCR-grounded LLM call.
///
/// The output uses `model_id = "llm_ocr_grounded"` so downstream gates
/// (e.g. the "Supporting OCR" appendix in [`compose_import_memory_context`]
/// and the storage threshold) can tell apart the three insight origins:
/// `"qwen3-vl-..."` (VLM success), `"llm_ocr_grounded"` (LLM-on-OCR
/// fallback), and `"ocr_only"` (heuristic fallback when even Llama is
/// unavailable).
pub fn insight_from_structured(
    structured: &crate::inference::StructuredMemoryExtraction,
) -> ImageSemanticInsight {
    let summary_short = if !structured.topic.trim().is_empty() {
        structured.topic.trim().to_string()
    } else if !structured.memory_context.trim().is_empty() {
        structured
            .memory_context
            .trim()
            .chars()
            .take(180)
            .collect::<String>()
    } else {
        String::new()
    };
    let summary_detailed = structured
        .memory_context
        .trim()
        .chars()
        .take(420)
        .collect::<String>();

    let topics = if structured.topic.trim().is_empty() {
        Vec::new()
    } else {
        vec![structured.topic.trim().to_string()]
    };

    let mut search_aliases: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for a in structured
        .search_aliases
        .iter()
        .chain(structured.tags.iter())
        .chain(structured.entities.iter())
    {
        let k = normalize_alias_key(a);
        if k.is_empty() || k.len() < 3 {
            continue;
        }
        // Apply the same anti-pollution guards as `insight_from_ocr_only`.
        if a.contains('[') || a.contains(']') || a.contains('|') {
            continue;
        }
        if seen.insert(k) {
            search_aliases.push(a.trim().to_string());
        }
        if search_aliases.len() >= 12 {
            break;
        }
    }

    ImageSemanticInsight {
        summary_short,
        summary_detailed,
        scene_type: if structured.activity_type.trim().is_empty() {
            "screen content".to_string()
        } else {
            structured.activity_type.trim().to_string()
        },
        setting: None,
        activity_type: if structured.activity_type.trim().is_empty() {
            Some("observing".to_string())
        } else {
            Some(structured.activity_type.trim().to_string())
        },
        user_intent: if structured.user_intent.trim().is_empty() {
            None
        } else {
            Some(structured.user_intent.trim().to_string())
        },
        visible_objects: Vec::new(),
        people_roles: Vec::new(),
        entities: structured.entities.clone(),
        actions: Vec::new(),
        topics,
        search_aliases,
        // Below the VLM-success threshold (0.55) so retrieval treats this
        // as evidence rather than a primary vision insight, but above the
        // OCR-only path (0.30) so we prefer it when it's available.
        confidence: structured.confidence.clamp(0.0, 1.0).max(0.40),
        model_id: "llm_ocr_grounded".to_string(),
    }
}

/// Build a semantic insight grounded entirely in the supporting OCR text +
/// app/window context — no fabricated scene content, no error messages in
/// user-visible fields. Use this whenever the VLM is unavailable (failure,
/// memory pressure, model not installed) and no LLM is available either.
/// The actual error reason belongs in [`build_import_raw_evidence`]'s
/// `extraction_failure_reason` field, not in `summary_short`/`summary_detailed`.
///
/// The output is intentionally honest: it summarizes *what FNDR could read*
/// without pretending to have analyzed pixels. Confidence is set below the
/// success threshold so retrieval ranking treats it as supporting evidence,
/// not as a primary vision insight.
pub fn insight_from_ocr_only(
    filename: &str,
    app_name: Option<&str>,
    window_title: Option<&str>,
    clean_text: &str,
) -> ImageSemanticInsight {
    let app = app_name.unwrap_or("").trim();
    let window = window_title.unwrap_or("").trim();
    // Pre-scrub before everything else so the headline / detail body /
    // ranking / entity extractor all see the same content without
    // `[LOW_CONF]` and similar OCR-quality annotations.
    let scrubbed_text = scrub_ocr_annotations(clean_text);
    let cleaned = scrubbed_text.trim();

    let salient = rank_salient_spans(cleaned, 6);
    let entities = lightweight_entities_from_text(cleaned, 6);

    // Build a short headline from the strongest available signals. Prefer
    // an explicit window title since it's almost always meaningful; fall
    // back to the first salient span, then the filename.
    let headline = if !window.is_empty() && window.len() <= 90 {
        window.to_string()
    } else if let Some(first) = salient.first() {
        first.clone()
    } else if !app.is_empty() {
        app.to_string()
    } else {
        filename.to_string()
    };

    let summary_short = if app.is_empty() {
        headline.clone()
    } else {
        format!("{app}: {headline}")
    };

    // Detailed summary: stitch up to three salient spans into one sentence,
    // capped to MAX_SUMMARY_CHARS so embeddings stay focused.
    let detail_body = if salient.is_empty() {
        if cleaned.is_empty() {
            "No supporting text was visible on screen.".to_string()
        } else {
            cleaned.chars().take(180).collect::<String>()
        }
    } else {
        salient
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(" · ")
    };
    let prefix = if !app.is_empty() && !window.is_empty() {
        format!("{app} — {window}. ")
    } else if !app.is_empty() {
        format!("{app}. ")
    } else if !window.is_empty() {
        format!("{window}. ")
    } else {
        String::new()
    };
    let summary_detailed: String = format!("{prefix}{detail_body}").chars().take(420).collect();

    // Topics: dedupe-merge salient spans + window title fragments.
    let mut topics: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut candidates: Vec<&str> = Vec::with_capacity(1 + salient.len());
    candidates.push(headline.as_str());
    for s in &salient {
        candidates.push(s.as_str());
    }
    for candidate in candidates {
        let key = normalize_alias_key(candidate);
        if key.is_empty() || key.len() < 3 {
            continue;
        }
        if seen.insert(key) {
            topics.push(candidate.trim().to_string());
        }
        if topics.len() >= 4 {
            break;
        }
    }

    // Aliases: topics + entities, capped.
    let mut alias_set: HashSet<String> = HashSet::new();
    for t in &topics {
        alias_set.insert(t.clone());
    }
    for e in &entities {
        alias_set.insert(e.clone());
    }
    let search_aliases: Vec<String> = alias_set.into_iter().take(12).collect();

    let scene_type = if app.is_empty() {
        "screen content".to_string()
    } else {
        format!("{app} screen")
    };

    ImageSemanticInsight {
        summary_short,
        summary_detailed,
        scene_type,
        setting: None,
        activity_type: Some("observing".to_string()),
        user_intent: None,
        visible_objects: Vec::new(),
        people_roles: Vec::new(),
        entities,
        actions: Vec::new(),
        topics,
        search_aliases,
        // Below the typical success threshold (>= 0.55) so downstream
        // ranking treats this as supporting evidence, not as a primary
        // vision insight.
        confidence: 0.30,
        model_id: "ocr_only".to_string(),
    }
}

/// Strip FNDR's own OCR/quality annotations and structural junk before we
/// hand the text to span ranking or entity extraction. These markers
/// (`[LOW_CONF]`, `[OCR]`, etc.) are produced by upstream OCR cleanup —
/// they are metadata about the OCR, not content. Echoing them as topics
/// or aliases is exactly the failure mode that produced the
/// `"LOW_CONF] Explore companies..."` topics in the StartupCompass card.
fn scrub_ocr_annotations(text: &str) -> String {
    if text.trim().is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(text.len());
    for raw_line in text.lines() {
        let mut line = raw_line.to_string();
        // Remove `[ANYTHING]` and `]ANYTHING]` style annotations the
        // upstream OCR pass may inject when confidence is low.
        for marker in [
            "[LOW_CONF]",
            "LOW_CONF]",
            "[LOW_CONF",
            "[OCR]",
            "[NOISE]",
            "[REDACTED]",
            "[…]",
            "[...]",
        ] {
            line = line.replace(marker, " ");
        }
        // Collapse repeated whitespace introduced by replacement.
        let collapsed: String = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if !collapsed.is_empty() {
            out.push_str(&collapsed);
            out.push('\n');
        }
    }
    out
}

/// Rank short noun-ish spans from OCR/clean_text for use in OCR-only
/// insights. Heuristic only — never calls a model. Order: longest
/// alphanumeric-rich runs first, ties broken by earlier occurrence.
fn rank_salient_spans(text: &str, max_spans: usize) -> Vec<String> {
    let scrubbed = scrub_ocr_annotations(text);
    if scrubbed.trim().is_empty() {
        return Vec::new();
    }
    let mut spans: Vec<(usize, usize, String)> = Vec::new();
    for (idx, raw_line) in scrubbed.lines().enumerate() {
        for chunk in raw_line
            .split(|c: char| matches!(c, '|' | '\t' | '·' | '•' | '◦' | '↑' | '↓' | '→' | '←'))
        {
            let trimmed = chunk.trim();
            if trimmed.len() < 6 || trimmed.len() > 90 {
                continue;
            }
            let alnum: usize = trimmed.chars().filter(|c| c.is_alphanumeric()).count();
            if alnum * 2 < trimmed.chars().count() {
                continue;
            }
            // Reject obvious "all-uppercase noise" or "repeating-char noise".
            let alpha: usize = trimmed.chars().filter(|c| c.is_alphabetic()).count();
            if alpha < 3 {
                continue;
            }
            // Penalize trailing punctuation/garbage on both ends.
            let cleaned = trimmed
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_string();
            if cleaned.is_empty() || cleaned.len() < 6 {
                continue;
            }
            // Reject residual annotation fragments (e.g. anything that
            // still starts/ends with brackets after scrubbing).
            if cleaned.contains('[') || cleaned.contains(']') {
                continue;
            }
            // Require multi-word content. Single-word "topics" are almost
            // always OCR noise or punctuation-trimmed garbage.
            let word_count = cleaned.split_whitespace().count();
            if word_count < 2 {
                continue;
            }
            spans.push((cleaned.len(), idx, cleaned));
        }
    }
    spans.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for (_len, _idx, span) in spans {
        let key = span.to_lowercase();
        if seen.insert(key) {
            out.push(span);
            if out.len() >= max_spans {
                break;
            }
        }
    }
    out
}

/// Pull obvious named entities (URLs, file names, app-ish tokens) out of
/// raw text without invoking a model. Best-effort; safe to call on empty
/// input.
fn lightweight_entities_from_text(text: &str, max_entities: usize) -> Vec<String> {
    let scrubbed = scrub_ocr_annotations(text);
    if scrubbed.trim().is_empty() {
        return Vec::new();
    }
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for tok in scrubbed.split(|c: char| c.is_whitespace() || c == ',' || c == ';') {
        let tok =
            tok.trim_matches(|c: char| !c.is_alphanumeric() && c != '.' && c != '/' && c != ':');
        // Tighter floor: 4-char tokens like "matter." pass too easily;
        // require enough alphabetic characters to look like a real entity.
        let alpha_count: usize = tok.chars().filter(|c| c.is_alphabetic()).count();
        if alpha_count < 5 {
            continue;
        }
        // Reject tokens that end on punctuation (e.g. "matter.") — true
        // entities don't carry trailing periods after the trim above
        // unless they were sentence-terminated nouns, which we don't
        // want as entities anyway.
        if tok.ends_with('.') && tok.matches('.').count() == 1 {
            continue;
        }
        // Reject residual OCR-annotation fragments.
        if tok.contains('[') || tok.contains(']') {
            continue;
        }
        let looks_url =
            tok.starts_with("http://") || tok.starts_with("https://") || tok.contains("://");
        let looks_domain = tok.matches('.').count() >= 1
            && tok
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | '/'))
            && tok.chars().any(|c| c.is_alphabetic())
            // Domains have a TLD-shaped tail (>=2 chars after last dot).
            && tok.rsplit_once('.').map(|(_, t)| t.len() >= 2).unwrap_or(false);
        // CamelCase / PascalCase product names need at least one internal
        // uppercase *after* the first char to avoid catching plain words
        // that happen to be sentence-initial.
        let mut chars = tok.chars();
        let first = chars.next();
        let internal_upper = chars.filter(|c| c.is_uppercase()).count() >= 1
            && first.map(|c| c.is_uppercase()).unwrap_or(false);
        let looks_camel = internal_upper && tok.chars().any(|c| c.is_lowercase());
        if !(looks_url || looks_domain || looks_camel) {
            continue;
        }
        let key = tok.to_lowercase();
        if seen.insert(key) {
            out.push(tok.to_string());
            if out.len() >= max_entities {
                break;
            }
        }
    }
    out
}

// --- Runtime (singleton) ----------------------------------------------------

struct LlamaMtmdPair {
    llama: LlamaContext<'static>,
    mtmd: MtmdContext,
}

struct MtmdVlmRuntime {
    model: &'static LlamaModel,
    chat_template: LlamaChatTemplate,
    _backend: Arc<LlamaBackend>,
    inner: Mutex<LlamaMtmdPair>,
    model_family: MtmdModelFamily,
}

/// [`LlamaContext`] is not `Send` in the Rust bindings, but this runtime is only used behind
/// `Arc` + [`Mutex`] from blocking import tasks (same pattern as [`super::VlmEngine`]).
unsafe impl Send for MtmdVlmRuntime {}
unsafe impl Sync for MtmdVlmRuntime {}

static IMPORT_VISION: OnceLock<Mutex<Option<Arc<MtmdVlmRuntime>>>> = OnceLock::new();

impl MtmdVlmRuntime {
    fn instance(app_data_dir: &Path) -> Result<Arc<MtmdVlmRuntime>, String> {
        let cell = IMPORT_VISION.get_or_init(|| Mutex::new(None));
        let mut slot = cell.lock();
        if let Some(ref arc) = *slot {
            return Ok(Arc::clone(arc));
        }
        let arc = Arc::new(Self::load(app_data_dir)?);
        *slot = Some(arc.clone());
        Ok(arc)
    }

    fn load(app_data_dir: &Path) -> Result<Self, String> {
        // Try SmolVLM 500M first (lightweight); fall back to Qwen3-VL 4B.
        let (model_path, mmproj, model_family) = {
            let smolvlm_model = models::resolve_model(Some("smolvlm-500m"), Some(app_data_dir));
            let smolvlm_mmproj = models::resolve_smolvlm_mmproj(Some(app_data_dir));

            if let (Some(main), Some(mmproj)) = (smolvlm_model, smolvlm_mmproj) {
                models::validate_smolvlm_main_gguf_file(&main.path)
                    .map_err(|e| format!("SmolVLM GGUF invalid: {e}"))?;
                (main.path, mmproj, MtmdModelFamily::SmolVlm500M)
            } else {
                tracing::debug!("SmolVLM 500M not fully available; trying Qwen3-VL 4B fallback");
                // Fall back to Qwen3-VL 4B
                let qwen_model = models::resolve_model(Some("qwen3-vl-4b"), Some(app_data_dir))
                    .ok_or_else(|| {
                        "No MTMD model found: install SmolVLM 500M or Qwen3-VL 4B".to_string()
                    })?;
                if let Err(e) = models::validate_qwen3_vl_main_gguf_file(&qwen_model.path) {
                    return Err(format!("Qwen3-VL main GGUF invalid: {e}"));
                }
                let mmproj =
                    models::resolve_qwen3_vl_mmproj(Some(app_data_dir)).ok_or_else(|| {
                        format!(
                            "Qwen3-VL 4B model found but mmproj missing. Download one of: {}",
                            models::QWEN3_VL_MMPROJ_FILENAMES.join(", ")
                        )
                    })?;
                (qwen_model.path, mmproj, MtmdModelFamily::Qwen3Vl4B)
            }
        };

        let backend = get_or_init_backend().map_err(|e| e.to_string())?;
        let load_path = model_path.clone();

        let backend_clone = Arc::clone(&backend);
        let model_ref = std::thread::spawn(move || -> Result<&'static LlamaModel, String> {
            let params = LlamaModelParams::default();
            let model = LlamaModel::load_from_file(&backend_clone, &load_path, &params)
                .map_err(|e| format!("load MTMD model: {e}"))?;
            let model_ref: &'static LlamaModel = Box::leak(Box::new(model));
            Ok(model_ref)
        })
        .join()
        .map_err(|_| "model load thread panicked".to_string())??;

        let chat_template = match model_ref.chat_template(None) {
            Ok(t) => t,
            Err(err) => {
                tracing::warn!(
                    model = ?model_family,
                    "MTMD model chat template missing ({err}); using chatml fallback"
                );
                LlamaChatTemplate::new("chatml").map_err(|e| e.to_string())?
            }
        };

        let n_ctx = NonZeroU32::new(model_family.context_size() * 8)
            .expect("context_size * 8 is always nonzero");
        let n_batch: u32 = 512;
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(Some(n_ctx))
            .with_n_batch(n_batch)
            .with_n_ubatch(256.min(n_batch));

        let llama = model_ref
            .new_context(&backend, ctx_params)
            .map_err(|e| format!("llama context: {e}"))?;

        let mmproj_str = mmproj.to_str().ok_or("mmproj path is not UTF-8")?;
        let mut mparams = MtmdContextParams::default();
        mparams.use_gpu = true;
        mparams.print_timings = false;
        let model_path_str = model_path.display().to_string();
        let mtmd = MtmdContext::init_from_file(mmproj_str, model_ref, &mparams).map_err(|e| {
            let detail = e.to_string();
            let mut msg = format!("mtmd init: {detail}");
            if detail.contains("returned null") {
                msg.push_str(&format!(
                    " Check the terminal for `mtmd_init_from_file: error:` (often `n_embd` mismatch). \
                     That means `{model_path_str}` is not a matching GGUF for this mmproj — \
                     replace both from the same Hugging Face repo (see README)."
                ));
            }
            msg
        })?;
        if !mtmd.support_vision() {
            return Err("Loaded mmproj does not report vision support.".to_string());
        }

        tracing::info!(
            model = ?model_family,
            mmproj = %mmproj.display(),
            "MTMD import vision runtime ready"
        );

        Ok(Self {
            model: model_ref,
            chat_template,
            _backend: backend,
            inner: Mutex::new(LlamaMtmdPair { llama, mtmd }),
            model_family,
        })
    }

    fn run_blocking(
        &self,
        image_bytes: &[u8],
        filename: &str,
    ) -> Result<ImageSemanticInsight, String> {
        let mut guard = self.inner.lock();
        let LlamaMtmdPair { llama, mtmd } = &mut *guard;
        llama.clear_kv_cache();

        let marker = mtmd_default_marker();
        let system = VISION_SYSTEM_PROMPT;
        let user_body = format!(
            "Imported file name (metadata only, not part of the image): {filename}\n\n\
             Analyze the **image pixels** and respond with **JSON only** per your instructions."
        );
        let user_with_media = format!("{marker}\n{user_body}");

        let messages = vec![
            LlamaChatMessage::new("system".to_string(), system.to_string())
                .map_err(|e| e.to_string())?,
            LlamaChatMessage::new("user".to_string(), user_with_media)
                .map_err(|e| e.to_string())?,
        ];
        let formatted = self
            .model
            .apply_chat_template(&self.chat_template, &messages, true)
            .map_err(|e| format!("chat template: {e}"))?;

        let bitmap = MtmdBitmap::from_buffer(mtmd, image_bytes)
            .map_err(|e| format!("image bitmap (pixels): {e}"))?;

        let chunks = mtmd
            .tokenize(
                MtmdInputText {
                    text: formatted,
                    add_special: true,
                    parse_special: true,
                },
                &[&bitmap],
            )
            .map_err(|e| format!("mtmd tokenize: {e}"))?;

        let n_batch = llama.n_batch().max(1) as i32;
        let mut n_past = 0i32;
        n_past = chunks
            .eval_chunks(mtmd, llama, n_past, 0, n_batch, true)
            .map_err(|e| format!("mtmd eval chunks: {e}"))?;

        let mut sampler = LlamaSampler::chain_simple([
            LlamaSampler::penalties(64, 1.05, 0.0, 0.0),
            LlamaSampler::greedy(),
        ]);

        let max_new = 768i32;
        let mut out = String::new();
        let mut batch = LlamaBatch::new(llama.n_batch().max(512) as usize, 1);

        for _ in 0..max_new {
            let tok = sampler.sample(llama, -1);
            sampler.accept(tok);
            if self.model.is_eog_token(tok) {
                break;
            }
            #[allow(deprecated)]
            let piece = self
                .model
                .token_to_str(tok, Special::Plaintext)
                .unwrap_or_default();
            out.push_str(&piece);

            batch.clear();
            batch
                .add(tok, n_past, &[0], true)
                .map_err(|e| format!("gen batch: {e:?}"))?;
            llama
                .decode(&mut batch)
                .map_err(|e| format!("gen decode: {e}"))?;
            n_past += 1;
        }

        drop(chunks);
        drop(bitmap);

        parse_vision_json(&out, self.model_family.model_id_str())
    }
}

const VISION_SYSTEM_PROMPT: &str = r#"You are FNDR's local visual memory extractor. Analyze the imported photo **from pixels** and return **compact JSON only** (no markdown fences, no commentary).

Rules:
- Do **not** identify people by name or guess private identities.
- Describe **roles** only (presenter, audience member, teammate, reviewer, student, mentor, participant, etc.).
- Prefer **searchable retrieval terms** over artistic prose.
- If uncertain, lower `confidence` and list a few possible `topics` / `scene_type` values rather than inventing specifics.

Required JSON schema (all string arrays may be empty):
{
  "summary_short": "one sentence",
  "summary_detailed": "2-5 sentences",
  "scene_type": "short label",
  "setting": "optional",
  "activity_type": "optional short machine-friendly label e.g. presentation, meeting, project_demo, feedback_session",
  "user_intent": "optional short phrase",
  "people_roles": [],
  "visible_objects": [],
  "actions": [],
  "topics": [],
  "search_aliases": [],
  "confidence": 0.0
}"#;

/// JSON blob stored in `MemoryRecord::raw_evidence` for imports (extended fields).
///
/// `extraction_failure_reason` should be populated whenever the VLM failed
/// (or was skipped) so the user-visible `memory_context` stays semantic but
/// debugging information remains accessible via the raw evidence pane.
pub fn build_import_raw_evidence(
    source_type: &str,
    filename: &str,
    insight: &ImageSemanticInsight,
    ocr_included: bool,
    ocr_rejected_reason: Option<&str>,
    ocr_excerpt: Option<&str>,
    extraction_failure_reason: Option<&str>,
) -> String {
    json!({
        "source_type": source_type,
        "filename": filename,
        "vision_model_id": insight.model_id,
        "vision_summary_short": insight.summary_short,
        "vision_summary_detailed": insight.summary_detailed,
        "scene_type": insight.scene_type,
        "activity_type": insight.activity_type,
        "user_intent": insight.user_intent,
        "setting": insight.setting,
        "people_roles": insight.people_roles,
        "visible_objects": insight.visible_objects,
        "semantic_topics": insight.topics,
        "semantic_entities": insight.entities,
        "semantic_actions": insight.actions,
        "semantic_search_aliases": insight.search_aliases,
        "semantic_confidence": insight.confidence,
        "ocr_included": ocr_included,
        "ocr_rejected_reason": ocr_rejected_reason,
        "ocr_excerpt_if_rejected": ocr_excerpt,
        "extraction_failure_reason": extraction_failure_reason,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mtmd_model_family_roundtrip() {
        assert_eq!(
            MtmdModelFamily::from_model_id("smolvlm-500m"),
            Some(MtmdModelFamily::SmolVlm500M)
        );
        assert_eq!(
            MtmdModelFamily::from_model_id("qwen3-vl-4b"),
            Some(MtmdModelFamily::Qwen3Vl4B)
        );
        assert_eq!(MtmdModelFamily::from_model_id("unknown"), None);
        assert_eq!(MtmdModelFamily::SmolVlm500M.model_id_str(), "smolvlm-500m");
        assert_eq!(MtmdModelFamily::SmolVlm500M.context_size(), 1024);
        assert_eq!(MtmdModelFamily::Qwen3Vl4B.context_size(), 1536);
    }

    #[test]
    fn rejects_garbage_ocr_line() {
        let stats = ImportOcrStats {
            confidence: 0.5,
            blocks: 1,
        };
        assert!(!should_include_import_ocr(
            ". Ja JJjJ'JJJJJ. Meta glasses import: Pitch.jpeg . Ja JJjJ'JJJJJ.",
            &stats
        ));
    }

    #[test]
    fn accepts_reasonable_ocr() {
        let stats = ImportOcrStats {
            confidence: 0.72,
            blocks: 4,
        };
        let t = "Quarterly revenue summary table shows 12% growth in enterprise segment.";
        assert!(should_include_import_ocr(t, &stats));
    }

    #[test]
    fn composition_includes_scene_and_roles() {
        let insight = ImageSemanticInsight {
            summary_short: "Team project pitch in a conference room.".to_string(),
            summary_detailed: "Two presenters stand at the front while several participants sit around a table with laptops, watching a discussion. Whiteboard and red cups are visible.".to_string(),
            scene_type: "project pitch".to_string(),
            setting: Some("conference room".to_string()),
            activity_type: Some("presentation".to_string()),
            user_intent: Some("capturing a project demo or feedback session".to_string()),
            visible_objects: vec![
                "laptops".to_string(),
                "whiteboard".to_string(),
                "conference table".to_string(),
            ],
            people_roles: vec!["presenters".to_string(), "audience".to_string()],
            entities: vec!["project pitch".to_string(), "team meeting".to_string()],
            actions: vec!["presenting".to_string()],
            topics: vec![
                "project pitch meeting".to_string(),
                "team presentation".to_string(),
            ],
            search_aliases: vec![
                "pitch".to_string(),
                "project demo".to_string(),
                "feedback session".to_string(),
            ],
            confidence: 0.82,
            model_id: "qwen3-vl-4b".to_string(),
        };
        let composed = compose_import_memory_context(
            "Pitch.jpeg",
            &insight,
            None,
            ImageImportSource::MetaGlasses,
        );
        assert!(composed
            .memory_context
            .to_ascii_lowercase()
            .contains("conference"));
        assert!(
            composed.memory_context.contains("presenters")
                || composed.embedding_text.contains("presenters")
        );
        assert!(composed.topic.to_ascii_lowercase().contains("pitch"));
        assert_eq!(composed.activity_type, "presentation");
        assert!(
            composed.user_intent.to_ascii_lowercase().contains("demo")
                || composed.user_intent.contains("feedback")
        );
        let joined = composed.search_aliases.join(" ").to_ascii_lowercase();
        assert!(joined.contains("pitch"));
        assert!(joined.contains("demo") || joined.contains("feedback"));
    }

    #[test]
    fn import_raw_evidence_json_shape() {
        let insight = ImageSemanticInsight {
            summary_short: "s".into(),
            summary_detailed: "d".into(),
            scene_type: "scene".into(),
            setting: None,
            activity_type: Some("presentation".into()),
            user_intent: Some("intent".into()),
            visible_objects: vec!["laptop".into()],
            people_roles: vec![],
            entities: vec![],
            actions: vec![],
            topics: vec!["pitch".into()],
            search_aliases: vec!["alias".into()],
            confidence: 0.7,
            model_id: "qwen-test".into(),
        };
        let raw = build_import_raw_evidence(
            "meta_glasses_import",
            "Pitch.jpeg",
            &insight,
            false,
            Some("noisy_low_signal_text"),
            None,
            None,
        );
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["ocr_included"], false);
        assert_eq!(v["vision_model_id"], "qwen-test");
        assert_eq!(v["semantic_topics"].as_array().unwrap()[0], "pitch");
    }

    #[test]
    fn parse_json_tolerates_fences() {
        let raw = "```json\n{\"summary_short\":\"x\",\"summary_detailed\":\"y\",\"scene_type\":\"s\",\"setting\":null,\"activity_type\":\"presentation\",\"user_intent\":\"u\",\"people_roles\":[\"presenter\"],\"visible_objects\":[\"laptop\"],\"actions\":[],\"topics\":[\"pitch\"],\"search_aliases\":[\"a\"],\"confidence\":0.5}\n```";
        let out = parse_vision_json(raw, "test").unwrap();
        assert_eq!(out.summary_short, "x");
        assert_eq!(out.activity_type.as_deref(), Some("presentation"));
    }

    #[test]
    fn ocr_only_insight_never_mentions_failure_or_extraction() {
        let i = insight_from_ocr_only(
            "ElCamino.jpeg",
            Some("Camera"),
            Some("Lunch line"),
            "Order Food OPEN READY BY Track Order We're ready to serve you 11 min wait",
        );
        let blob = format!(
            "{} {} {} {}",
            i.summary_short,
            i.summary_detailed,
            i.scene_type,
            i.search_aliases.join(" ")
        )
        .to_ascii_lowercase();
        for forbidden in [
            "fail",
            "failed",
            "eval failed",
            "error",
            "could not",
            "mtmd",
            "qwen",
            "multimodal extraction",
            "extraction failed",
            "code:",
        ] {
            assert!(
                !blob.contains(forbidden),
                "OCR-only insight leaked debug language `{forbidden}`: {blob}"
            );
        }
        assert!(i.confidence < 0.55);
        assert_eq!(i.model_id, "ocr_only");
        assert!(!i.topics.is_empty(), "expected at least one topic");
    }

    #[test]
    fn ocr_only_insight_grounds_in_window_title_when_text_is_empty() {
        let i = insight_from_ocr_only("Untitled.png", Some("Cursor"), Some("memory.rs"), "");
        assert!(i.summary_short.contains("Cursor"));
        assert!(i
            .summary_detailed
            .to_ascii_lowercase()
            .contains("memory.rs"));
    }

    #[test]
    fn ocr_only_insight_picks_rich_spans_over_short_garbage() {
        let i = insight_from_ocr_only(
            "Notes.png",
            Some("Notes"),
            Some("Project plan"),
            "| | | ok\nQuarterly revenue summary table shows 12% growth in enterprise segment.\n. .",
        );
        let joined = i.summary_detailed.to_ascii_lowercase();
        assert!(joined.contains("quarterly"));
        assert!(!joined.contains("| |"));
    }

    #[test]
    fn ocr_only_strips_low_conf_markers_from_topics_and_aliases() {
        let polluted = "[LOW_CONF] Explore companies that are actively hiring, growing fast\n\
             [LOW_CONF] Search startups, investors, sectors, or locations\n\
             LOW_CONF] A Personalized for Job Seekers Change\n\
             Startup Compass UTAH . STARTUP STATE Home Map Find Resources Pulse Demo Founder\n\
             Explore companies that are actively hiring, growing fast";
        let i = insight_from_ocr_only(
            "StartupCompass.jpeg",
            Some("Camera"),
            Some("Startup Compass"),
            polluted,
        );
        let joined = format!(
            "{} {} {} {}",
            i.summary_short,
            i.summary_detailed,
            i.topics.join(" "),
            i.search_aliases.join(" ")
        );
        assert!(
            !joined.contains("[LOW_CONF]")
                && !joined.contains("LOW_CONF]")
                && !joined.contains("[LOW_CONF"),
            "OCR annotations must be stripped: {joined}"
        );
        for topic in &i.topics {
            assert!(
                topic.split_whitespace().count() >= 2,
                "topics must be multi-word, got: {topic}"
            );
            assert!(
                !topic.contains('['),
                "topic must not have brackets: {topic}"
            );
            assert!(
                !topic.contains(']'),
                "topic must not have brackets: {topic}"
            );
        }
        for entity in &i.entities {
            assert!(
                !entity.ends_with('.'),
                "entities must not end with period: {entity}"
            );
            let alpha: usize = entity.chars().filter(|c| c.is_alphabetic()).count();
            assert!(alpha >= 5, "entities must have >=5 letters: {entity}");
        }
    }

    #[test]
    fn compose_import_memory_context_skips_supporting_ocr_for_ocr_only() {
        let mut insight = ImageSemanticInsight::default();
        insight.summary_short = "Cursor — main.rs".to_string();
        insight.summary_detailed = "Editing main.rs in Cursor.".to_string();
        insight.model_id = "ocr_only".to_string();
        let composed = compose_import_memory_context(
            "frame.png",
            &insight,
            Some("noise [LOW_CONF] noisy text that should not be appended"),
            ImageImportSource::ScreenCapture,
        );
        assert!(
            !composed.memory_context.contains("Supporting OCR"),
            "ocr_only insight must not get an OCR appendix: {}",
            composed.memory_context
        );
    }

    #[test]
    fn compose_import_memory_context_skips_supporting_ocr_for_llm_ocr_grounded() {
        let mut insight = ImageSemanticInsight::default();
        insight.summary_short = "Reviewing Q4 sales table".to_string();
        insight.summary_detailed = "User examined a quarterly revenue summary.".to_string();
        insight.model_id = "llm_ocr_grounded".to_string();
        let composed = compose_import_memory_context(
            "frame.png",
            &insight,
            Some("raw ocr that the llm already summarized"),
            ImageImportSource::MetaGlasses,
        );
        assert!(
            !composed.memory_context.contains("Supporting OCR"),
            "llm_ocr_grounded insight must not duplicate the OCR: {}",
            composed.memory_context
        );
    }

    #[test]
    fn scrub_ocr_annotations_collapses_markers_and_whitespace() {
        let raw = "[LOW_CONF] Hello   world\n  LOW_CONF] Foo Bar  \n[OCR] baz";
        let scrubbed = scrub_ocr_annotations(raw);
        assert!(!scrubbed.contains("LOW_CONF"));
        assert!(!scrubbed.contains("[OCR]"));
        assert!(scrubbed.contains("Hello world"));
        assert!(scrubbed.contains("Foo Bar"));
        assert!(scrubbed.contains("baz"));
    }

    #[test]
    fn insight_from_structured_maps_topic_and_entities() {
        let structured = crate::inference::StructuredMemoryExtraction {
            session_key: String::new(),
            activity_type: "coding".to_string(),
            project: String::new(),
            topic: "refactoring telemetry sampler".to_string(),
            memory_context: "User refactored the telemetry sampler to fix phys_footprint."
                .to_string(),
            workflow: String::new(),
            user_intent: "stabilize the engine inspector".to_string(),
            files_touched: vec![],
            symbols_changed: vec![],
            git_stats: Default::default(),
            outcome: String::new(),
            tags: vec!["telemetry".to_string()],
            entities: vec!["telemetry".to_string()],
            decisions: vec![],
            errors: vec![],
            next_steps: vec![],
            commands: vec![],
            blockers: vec![],
            todos: vec![],
            open_questions: vec![],
            results: vec![],
            search_aliases: vec!["fix phys_footprint".to_string()],
            confidence: 0.7,
            dedup_fingerprint: String::new(),
        };
        let i = insight_from_structured(&structured);
        assert_eq!(i.model_id, "llm_ocr_grounded");
        assert!(i.confidence >= 0.40 && i.confidence <= 1.0);
        assert!(i.summary_short.contains("refactoring telemetry sampler"));
        assert!(i
            .topics
            .iter()
            .any(|t| t == "refactoring telemetry sampler"));
        // Aliases should include the explicit alias and the entity tag.
        assert!(i.search_aliases.iter().any(|a| a == "fix phys_footprint"));
    }

    #[test]
    fn raw_evidence_carries_extraction_failure_reason() {
        let insight = ImageSemanticInsight::default();
        let raw = build_import_raw_evidence(
            "meta_glasses_import",
            "ElCamino.jpeg",
            &insight,
            false,
            None,
            None,
            Some("mtmd eval chunks: Eval failed with code: -3"),
        );
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert!(v["extraction_failure_reason"]
            .as_str()
            .map(|s| s.contains("mtmd eval chunks"))
            .unwrap_or(false));
        // And the failure reason must not leak into any user-visible insight field.
        assert!(
            !insight.summary_detailed.contains("Eval failed"),
            "user-visible summary must not contain raw extraction errors"
        );
    }
}
