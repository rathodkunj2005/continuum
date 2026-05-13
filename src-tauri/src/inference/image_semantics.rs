//! Pixel-based visual semantic extraction for imported photos (Meta glasses, file picker).
//!
//! Uses llama.cpp **MTMD** with **Qwen3-VL** GGUF + matching **mmproj** weights. This path
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

/// Run Qwen3-VL + mmproj on **pixels** (lazy singleton; first call loads weights).
///
/// Returns [`Err`] when the multimodal stack is unavailable (missing model/mmproj, init
/// failure, or inference error). Callers must **not** treat OCR as a substitute for success.
pub async fn extract_image_semantics(
    image_bytes: Vec<u8>,
    filename: &str,
    source: ImageImportSource,
    app_data_dir: PathBuf,
) -> Result<ImageSemanticInsight, String> {
    let filename = filename.to_string();
    tracing::debug!(?source, %filename, "extract_image_semantics: scheduling blocking vision run");
    tokio::task::spawn_blocking(move || {
        let runtime = QwenVlImportRuntime::instance(&app_data_dir)?;
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
    if let Some(ocr) = ocr_text {
        let excerpt: String = ocr.chars().take(400).collect();
        memory_context.push_str("\n\nSupporting OCR (low weight):\n");
        memory_context.push_str(&excerpt);
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

/// Insight used when MTMD vision fails — never fabricates scene content.
pub fn insight_vision_extraction_failed(filename: &str, err: &str) -> ImageSemanticInsight {
    ImageSemanticInsight {
        summary_short: format!("Imported photo {filename}: visual semantic extraction failed."),
        summary_detailed: format!(
            "FNDR could not run local Qwen3-VL multimodal extraction on this image. \
             Reason: {err}. The memory still has an image embedding for similarity search."
        ),
        scene_type: "unknown".to_string(),
        setting: None,
        activity_type: Some("import".to_string()),
        user_intent: Some(
            "importing a photo without successful on-device vision analysis".to_string(),
        ),
        visible_objects: vec![],
        people_roles: vec![],
        entities: vec!["photo import".to_string()],
        actions: vec![],
        topics: vec!["photo import".to_string()],
        search_aliases: vec!["photo import".to_string(), "imported image".to_string()],
        confidence: 0.15,
        model_id: "none".to_string(),
    }
}

// --- Runtime (singleton) ----------------------------------------------------

struct LlamaMtmdPair {
    llama: LlamaContext<'static>,
    mtmd: MtmdContext,
}

struct QwenVlImportRuntime {
    model: &'static LlamaModel,
    chat_template: LlamaChatTemplate,
    _backend: Arc<LlamaBackend>,
    inner: Mutex<LlamaMtmdPair>,
}

/// [`LlamaContext`] is not `Send` in the Rust bindings, but this runtime is only used behind
/// `Arc` + [`Mutex`] from blocking import tasks (same pattern as [`super::VlmEngine`]).
unsafe impl Send for QwenVlImportRuntime {}
unsafe impl Sync for QwenVlImportRuntime {}

static IMPORT_VISION: OnceLock<Mutex<Option<Arc<QwenVlImportRuntime>>>> = OnceLock::new();

impl QwenVlImportRuntime {
    fn instance(app_data_dir: &Path) -> Result<Arc<QwenVlImportRuntime>, String> {
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
        let resolved =
            models::resolve_model(Some("qwen3-vl-4b"), Some(app_data_dir)).ok_or_else(|| {
                "Qwen3-VL 4B GGUF not found. Install it (see README / model picker), \
                 including mmproj weights for vision."
                    .to_string()
            })?;
        models::validate_qwen3_vl_main_gguf_file(&resolved.path)?;
        let mmproj = models::resolve_qwen3_vl_mmproj(Some(app_data_dir)).ok_or_else(|| {
            format!(
                "Qwen3-VL mmproj file not found under models directories. \
                 Download one of: {}",
                models::QWEN3_VL_MMPROJ_FILENAMES.join(", ")
            )
        })?;

        let backend = get_or_init_backend().map_err(|e| e.to_string())?;
        let model_path = resolved.path.clone();
        let model_id = resolved.definition.id.to_string();

        let backend_clone = Arc::clone(&backend);
        let model_ref = std::thread::spawn(move || -> Result<&'static LlamaModel, String> {
            let params = LlamaModelParams::default();
            let model = LlamaModel::load_from_file(&backend_clone, &model_path, &params)
                .map_err(|e| format!("load Qwen3-VL: {e}"))?;
            let model_ref: &'static LlamaModel = Box::leak(Box::new(model));
            Ok(model_ref)
        })
        .join()
        .map_err(|_| "model load thread panicked".to_string())??;

        let chat_template = match model_ref.chat_template(None) {
            Ok(t) => t,
            Err(err) => {
                tracing::warn!("Qwen3-VL chat template missing ({err}); using chatml fallback");
                LlamaChatTemplate::new("chatml").map_err(|e| e.to_string())?
            }
        };

        let n_ctx = NonZeroU32::new(8192).expect("nonzero");
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
        let qwen_path = resolved.path.display().to_string();
        let mtmd = MtmdContext::init_from_file(mmproj_str, model_ref, &mparams).map_err(|e| {
            let detail = e.to_string();
            let mut msg = format!("mtmd init: {detail}");
            if detail.contains("returned null") {
                msg.push_str(&format!(
                    " Check the terminal for `mtmd_init_from_file: error:` (often `n_embd` mismatch). \
                     That means `{qwen_path}` is not a matching Qwen3-VL-4B-Instruct GGUF for this mmproj — \
                     replace both from the same Hugging Face repo (see README)."
                ));
            }
            msg
        })?;
        if !mtmd.support_vision() {
            return Err("Loaded mmproj does not report vision support.".to_string());
        }

        tracing::info!(
            "Qwen3-VL import vision runtime ready (model_id={}, mmproj={})",
            model_id,
            mmproj.display()
        );

        Ok(Self {
            model: model_ref,
            chat_template,
            _backend: backend,
            inner: Mutex::new(LlamaMtmdPair { llama, mtmd }),
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

        parse_vision_json(&out, "qwen3-vl-4b-mtmd")
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
pub fn build_import_raw_evidence(
    source_type: &str,
    filename: &str,
    insight: &ImageSemanticInsight,
    ocr_included: bool,
    ocr_rejected_reason: Option<&str>,
    ocr_excerpt: Option<&str>,
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
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

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
            model_id: "qwen3-vl-4b-mtmd".to_string(),
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
}
