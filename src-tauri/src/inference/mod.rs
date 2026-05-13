//! Local inference engine for memory summaries and Q&A.
//!
//! Shared llama.cpp backend setup lives here so the text and VLM engines do not
//! compete over Metal/CPU runtime initialization.

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
#[allow(deprecated)]
use llama_cpp_2::model::Special;
use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaChatTemplate, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

mod image_semantics;
mod vlm;

/// Global shared LlamaBackend singleton.
/// Both InferenceEngine and VlmEngine must share one backend instance
/// to avoid BackendAlreadyInitialized panics from Metal/CPU init.
static LLAMA_BACKEND: OnceLock<Arc<LlamaBackend>> = OnceLock::new();

pub fn get_or_init_backend() -> Result<Arc<LlamaBackend>, Box<dyn std::error::Error + Send + Sync>>
{
    if let Some(backend) = LLAMA_BACKEND.get() {
        return Ok(Arc::clone(backend));
    }

    // Suppress overly verbose metal/llama.cpp internal logs for cleaner developer output
    // (honour pre-set env from main / shell if the user overrides).
    if std::env::var_os("GGML_METAL_LOG_INFO").is_none() {
        std::env::set_var("GGML_METAL_LOG_INFO", "0");
    }
    if std::env::var_os("GGML_METAL_LOG_WARN").is_none() {
        std::env::set_var("GGML_METAL_LOG_WARN", "0");
    }
    if std::env::var_os("GGML_LOG_LEVEL").is_none() {
        std::env::set_var("GGML_LOG_LEVEL", "0");
    }

    let mut backend = LlamaBackend::init()?;
    // llama.cpp model loader is very chatty on stderr; void unless debugging inference.
    if std::env::var_os("FNDR_LLAMA_VERBOSE").is_none() {
        backend.void_logs();
    }
    let backend = Arc::new(backend);
    // If another thread raced us, that's fine – just return our copy
    let _ = LLAMA_BACKEND.set(Arc::clone(&backend));
    Ok(backend)
}
pub use image_semantics::{
    build_import_raw_evidence, compose_import_memory_context, extract_image_semantics,
    insight_vision_extraction_failed, should_include_import_ocr, ImageImportSource,
    ImageSemanticInsight, ImportMemoryText, ImportOcrStats,
};
pub use vlm::VlmEngine;

const MAX_OCR_SUMMARY_CHARS: usize = 1100;
const MAX_SUMMARY_CHARS: usize = 220;

// ============================================================================
// Shared prompt fragments.
// Tune in one place; all prompts inherit the voice/format constraints.
// ============================================================================

const VOICE_RULES: &str = "\
- Write in second person: 'You opened...', 'You reviewed...', 'You fixed...'. Never 'User' or 'The user'.\n\
- No preambles like 'I see', 'The screen shows', 'Summary:'.\n\
- No markdown, no bullet points unless explicitly requested.";

// ============================================================================
// Lazy-compiled regexes (previously rebuilt on every summarize call).
// ============================================================================

static RE_THE_USER: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\bthe user\b").expect("regex compile"));
static RE_USER: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)\buser\b").expect("regex compile"));

// ============================================================================
// Text helpers
// ============================================================================

fn normalize_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let keep = max_chars.saturating_sub(3);
    let mut out: String = value.chars().take(keep).collect();
    out.push_str("...");
    out
}

fn is_separator_line(line: &str) -> bool {
    !line.is_empty()
        && line
            .chars()
            .all(|ch| ch == '-' || ch == '_' || ch == '=' || ch == '.' || ch == ' ')
}

fn symbol_ratio(line: &str) -> f32 {
    let chars: Vec<char> = line.chars().collect();
    if chars.is_empty() {
        return 1.0;
    }
    let symbols = chars
        .iter()
        .filter(|ch| !ch.is_alphanumeric() && !ch.is_whitespace())
        .count();
    symbols as f32 / chars.len() as f32
}

fn looks_like_file_inventory(line: &str) -> bool {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.len() < 5 {
        return false;
    }

    let pathish = tokens
        .iter()
        .filter(|token| {
            let token = token.trim_matches(|ch: char| ",;:()[]{}".contains(ch));
            token.contains('/')
                || token.contains('\\')
                || (token.contains('.')
                    && (token.contains('_') || token.contains('-') || token.ends_with(".rs")))
        })
        .count();

    pathish >= 4
}

fn strip_known_prefixes(value: &str) -> String {
    let trimmed = value.trim();
    let lower = trimmed.to_lowercase();

    for prefix in [
        "summary:",
        "summary -",
        "summary",
        "activity:",
        "action:",
        "output:",
    ] {
        if lower.starts_with(prefix) {
            return trimmed[prefix.len()..].trim().to_string();
        }
    }

    if lower.starts_with("the screen shows ") {
        return trimmed["the screen shows ".len()..].trim().to_string();
    }
    if lower.starts_with("screen shows ") {
        return trimmed["screen shows ".len()..].trim().to_string();
    }
    if lower.starts_with("i see ") {
        return trimmed["i see ".len()..].trim().to_string();
    }

    trimmed.to_string()
}

fn clean_summary_output(raw: &str) -> String {
    let picked_lines = raw
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !is_separator_line(line))
        .take(2)
        .collect::<Vec<_>>();
    let mut candidate = if picked_lines.is_empty() {
        raw.trim().to_string()
    } else {
        picked_lines.join(" ")
    }
    .trim()
    .trim_matches(|ch| ch == '"' || ch == '\'' || ch == '`')
    .to_string();

    // Handle "Action: X | Context: Y" style output.
    if let Some((left, right)) = candidate.split_once("| Context:") {
        let action = strip_known_prefixes(left);
        let context = right.trim();
        candidate = format!("{} {}", action, context);
    }

    for _ in 0..3 {
        let stripped = strip_known_prefixes(&candidate);
        if stripped == candidate {
            break;
        }
        candidate = stripped;
    }
    candidate = normalize_whitespace(&candidate);

    // Keep at most two sentences for browsing ergonomics.
    let normalized = candidate.replace('!', ".").replace('?', ".");
    let mut sentences = normalized
        .split('.')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    if sentences.len() > 2 {
        sentences.truncate(2);
        candidate = format!("{}.", sentences.join(". "));
    }

    // Normalise to second person — replace third-person "User" references that
    // older model outputs or cached snippets may still contain.
    let candidate = normalize_person(candidate.trim());

    truncate_chars(&candidate, MAX_SUMMARY_CHARS)
}

/// Replace "User <verb>" / "The user <verb>" patterns with "You <verb>".
/// Uses lazy-compiled regexes (see `RE_THE_USER`, `RE_USER`).
fn normalize_person(s: &str) -> String {
    let after_the = RE_THE_USER.replace_all(s, "You");
    // `\buser\b` case-insensitive is always some casing of "user", so the match
    // always becomes "You". No per-match branching needed.
    RE_USER.replace_all(&after_the, "You").into_owned()
}

fn is_usable_summary(summary: &str) -> bool {
    let trimmed = summary.trim();
    if trimmed.len() < 8 {
        return false;
    }
    if trimmed.split_whitespace().count() < 2 {
        return false;
    }
    if trimmed.split_whitespace().count() > 44 {
        return false;
    }
    if is_separator_line(trimmed) {
        return false;
    }
    if symbol_ratio(trimmed) > 0.34 {
        return false;
    }
    if looks_like_file_inventory(trimmed) {
        return false;
    }

    let lower = trimmed.to_lowercase();
    if lower == "n/a" || lower == "none" || lower == "unknown" {
        return false;
    }
    if lower.contains("ocr text") || lower.contains("raw text") {
        return false;
    }

    true
}

fn validate_memory_card_draft(mut draft: MemoryCardDraft) -> Option<MemoryCardDraft> {
    draft.title = normalize_whitespace(draft.title.trim());
    draft.summary = normalize_whitespace(draft.summary.trim());
    draft.action = normalize_whitespace(draft.action.trim());
    draft.context = draft
        .context
        .into_iter()
        .map(|value| normalize_whitespace(value.trim()))
        .filter(|value| !value.is_empty())
        .collect();

    if draft.title.is_empty() || draft.summary.is_empty() || draft.action.is_empty() {
        return None;
    }

    if draft.summary.contains('\n')
        || draft.summary.contains('*')
        || draft.summary.contains('#')
        || draft.summary.contains('`')
    {
        return None;
    }

    let summary_lower = draft.summary.to_lowercase();
    if summary_lower.starts_with("the screen shows")
        || summary_lower.starts_with("i see")
        || summary_lower.contains("new tab")
        || summary_lower.contains("toolbar")
        || summary_lower.contains("tab strip")
    {
        return None;
    }

    let words = draft.summary.split_whitespace().count();
    if !(8..=22).contains(&words) {
        return None;
    }

    if !draft.summary.ends_with('.') {
        draft.summary.push('.');
    }

    draft.context.dedup();
    draft.context.truncate(4);
    if draft.context.is_empty() {
        draft.context.push("recent activity".to_string());
    }

    Some(draft)
}

/// Brace-balanced scan for the first complete top-level JSON object.
/// Handles strings with escaped quotes and prefixed/suffixed commentary
/// (e.g. markdown code fences) that the naive first-`{`/last-`}` approach mishandled.
fn extract_json_object(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] != b'{' {
            i += 1;
            continue;
        }

        let start = i;
        let mut depth = 0i32;
        let mut in_str = false;
        let mut escape = false;

        while i < bytes.len() {
            let ch = bytes[i];
            if in_str {
                if escape {
                    escape = false;
                } else if ch == b'\\' {
                    escape = true;
                } else if ch == b'"' {
                    in_str = false;
                }
            } else {
                match ch {
                    b'"' => in_str = true,
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(raw[start..=i].to_string());
                        }
                    }
                    _ => {}
                }
            }
            i += 1;
        }
        // Unbalanced starting at this '{' — bail out.
        return None;
    }
    None
}

// ============================================================================
// Public types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryCardDraft {
    pub title: String,
    pub summary: String,
    pub action: String,
    #[serde(default)]
    pub context: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MeetingTaskBreakdownDraft {
    pub summary: String,
    #[serde(default)]
    pub todos: Vec<String>,
    #[serde(default)]
    pub reminders: Vec<String>,
    #[serde(default)]
    pub followups: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GitStats {
    #[serde(default)]
    pub added: i32,
    #[serde(default)]
    pub removed: i32,
    #[serde(default)]
    pub commits: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StructuredMemoryExtraction {
    #[serde(default)]
    pub session_key: String,
    #[serde(default)]
    pub activity_type: String,
    #[serde(default)]
    pub project: String,
    #[serde(default, alias = "summary")]
    pub topic: String,
    #[serde(default, alias = "detail")]
    pub memory_context: String,
    #[serde(default)]
    pub workflow: String,
    #[serde(default)]
    pub user_intent: String,
    #[serde(default)]
    pub files_touched: Vec<String>,
    #[serde(default)]
    pub symbols_changed: Vec<String>,
    #[serde(default)]
    pub git_stats: GitStats,
    #[serde(default)]
    pub outcome: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub entities: Vec<String>,
    #[serde(default)]
    pub decisions: Vec<String>,
    #[serde(default)]
    pub errors: Vec<String>,
    #[serde(default)]
    pub next_steps: Vec<String>,
    #[serde(default)]
    pub commands: Vec<String>,
    #[serde(default)]
    pub blockers: Vec<String>,
    #[serde(default)]
    pub todos: Vec<String>,
    #[serde(default)]
    pub open_questions: Vec<String>,
    #[serde(default)]
    pub results: Vec<String>,
    #[serde(default)]
    pub search_aliases: Vec<String>,
    #[serde(default)]
    pub confidence: f32,
    #[serde(default)]
    pub dedup_fingerprint: String,
}

// ============================================================================
// InferenceEngine
// ============================================================================

/// AI Inference Engine for FNDR using llama-cpp-2.
/// Persists the LlamaContext to prevent Metal resource exhaustion crashes.
///
/// After a GPU OOM cascade, quit and relaunch the app before expecting stable Metal behaviour.
///
/// # Lifetime safety
///
/// The model is intentionally leaked (`Box::leak`) to obtain a `'static` reference
/// that the `LlamaContext<'static>` can borrow from. This is safe under the
/// invariant that **`InferenceEngine` is a process-wide singleton held for the
/// application lifetime**. If you ever want runtime model hot-swap, this design
/// must change — otherwise each reload leaks a model's worth of memory and any
/// in-flight context referencing the old model would be use-after-free.
///
/// Thread-safety: `LlamaModel`, `Arc<LlamaBackend>`, and `Mutex<LlamaContext>`
/// are individually `Send`/`Sync`, so `InferenceEngine` auto-derives both.
pub struct InferenceEngine {
    model: &'static LlamaModel,
    context: Mutex<LlamaContext<'static>>,
    _backend: Arc<LlamaBackend>,
    chat_template: LlamaChatTemplate,
    model_id: String,
    model_path: PathBuf,
}

unsafe impl Send for InferenceEngine {}
unsafe impl Sync for InferenceEngine {}

impl InferenceEngine {
    /// Initialize the engine using the preferred available local model.
    pub async fn new(
        app_data_dir: Option<PathBuf>,
        preferred_model_id: Option<String>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        tracing::info!("Initializing local LLM via llama-cpp...");

        let backend = get_or_init_backend()?;

        let resolved_model =
            crate::models::resolve_model(preferred_model_id.as_deref(), app_data_dir.as_deref())
                .ok_or_else(|| {
                    let searched_dirs =
                        crate::models::candidate_model_dirs(app_data_dir.as_deref())
                            .into_iter()
                            .map(|path| path.display().to_string())
                            .collect::<Vec<_>>()
                            .join(", ");
                    tracing::error!(
                        "Model file not found in any known location. Searched: {}",
                        searched_dirs
                    );
                    format!("Model file missing. Searched: {}", searched_dirs)
                })?;

        let model_id = resolved_model.definition.id.to_string();
        let model_path = resolved_model.path;

        tracing::info!("Loading model {} from {:?}", model_id, model_path);

        let backend_clone = Arc::clone(&backend);
        let model_path_clone = model_path.clone();

        let model_ref = tokio::task::spawn_blocking(move || {
            let model_params = LlamaModelParams::default();
            let model =
                LlamaModel::load_from_file(&backend_clone, &model_path_clone, &model_params)?;

            // See struct-level doc for the `Box::leak` rationale.
            let model_ref: &'static LlamaModel = Box::leak(Box::new(model));
            Ok::<&'static LlamaModel, Box<dyn std::error::Error + Send + Sync>>(model_ref)
        })
        .await
        .map_err(|e| format!("Join error during model load: {}", e))?
        .map_err(|e| format!("Model load failed: {}", e))?;

        // n_ctx / n_batch tuned down for Apple Silicon unified memory (Metal peak working set).
        // Raise via env only when debugging long-context behaviour (values are clamped to llama.cpp rules).
        let n_ctx = std::env::var("FNDR_INFERENCE_N_CTX")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .and_then(NonZeroU32::new)
            .unwrap_or_else(|| NonZeroU32::new(2048).expect("2048 is non-zero"));
        let n_batch = std::env::var("FNDR_INFERENCE_N_BATCH")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(512)
            .min(n_ctx.get());
        let n_ubatch = std::env::var("FNDR_INFERENCE_N_UBATCH")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(256)
            .min(n_batch);

        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(Some(n_ctx))
            .with_n_batch(n_batch)
            .with_n_ubatch(n_ubatch);

        let context = model_ref.new_context(&backend, ctx_params)?;

        let chat_template = match model_ref.chat_template(None) {
            Ok(template) => template,
            Err(err) => {
                tracing::warn!(
                    "Model {} has no baked chat template ({}); falling back to chatml",
                    model_id,
                    err
                );
                LlamaChatTemplate::new("chatml").map_err(
                    |fallback_err| -> Box<dyn std::error::Error + Send + Sync> {
                        Box::new(fallback_err)
                    },
                )?
            }
        };

        Ok(Self {
            model: model_ref,
            context: Mutex::new(context),
            _backend: backend,
            chat_template,
            model_id,
            model_path,
        })
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    pub fn model_path(&self) -> &Path {
        &self.model_path
    }

    /// Summarize noisy OCR text into a clean sentence
    pub async fn summarize(&self, ocr_text: &str) -> String {
        self.summarize_memory_node("", "", ocr_text).await
    }

    /// Summarize OCR text into a concise memory snippet for storage and graph nodes.
    pub async fn summarize_memory_node(
        &self,
        app_name: &str,
        window_title: &str,
        ocr_text: &str,
    ) -> String {
        if ocr_text.trim().is_empty() {
            return String::new();
        }

        // Build the evidence block conditionally — empty "APP:" / "WINDOW:" lines
        // leak the assistant's own format into the prompt and confuse smaller models.
        let mut evidence = String::new();
        if !app_name.trim().is_empty() {
            evidence.push_str(&format!("APP: {}\n", app_name));
        }
        if !window_title.trim().is_empty() {
            evidence.push_str(&format!("WINDOW: {}\n", window_title));
        }
        evidence.push_str(&format!(
            "\nOCR TEXT:\n\"\"\"\n{}\n\"\"\"",
            ocr_text
                .chars()
                .take(MAX_OCR_SUMMARY_CHARS)
                .collect::<String>()
        ));

        let system_msg = format!(
            "You generate memory snippets from OCR text.\n\
            RULES:\n\
            - Output 1-2 short sentences, 16-34 words total.\n\
            {VOICE_RULES}\n\
            - Capture the primary activity and at least one concrete detail (entity, file, metric, or next step).\n\
            - Ignore UI chrome, menu labels, status bars, repeated file/path lists, and separators.\n\
            - Keep wording grounded to app/window/OCR evidence only."
        );

        let prompt = match self.build_prompt(
            &system_msg,
            &format!(
                "{evidence}\n\nTASK: Return only the best memory snippet with useful details for future search recall."
            ),
        ) {
            Ok(prompt) => prompt,
            Err(err) => {
                tracing::error!("Prompt build failed: {}", err);
                return String::new();
            }
        };

        tracing::debug!(
            "Summarizing OCR text for memory node ({} chars)...",
            ocr_text.len()
        );
        let raw_summary = self.complete(&prompt, 90).await;
        let summary = clean_summary_output(&raw_summary);

        if !is_usable_summary(&summary) {
            tracing::warn!("Discarded low-signal OCR summary");
            tracing::debug!("Discarded raw: {}", raw_summary);
            return String::new();
        }

        // User OCR content can be sensitive — log at debug, not info.
        tracing::debug!("OCR summary result: {}", summary);
        summary
    }

    /// Answer contextual questions using retrieved memories (RAG)
    pub async fn answer(&self, question: &str, context_str: &str) -> String {
        let prompt = match self.build_prompt(
            "You answer questions using local memory snippets. Be direct, grounded, and concise.",
            &format!(
                "Context Snippets:\n{}\n\nQuestion: {}",
                context_str.chars().take(1000).collect::<String>(),
                question
            ),
        ) {
            Ok(prompt) => prompt,
            Err(err) => {
                tracing::error!("Prompt build failed: {}", err);
                return String::new();
            }
        };

        self.complete(&prompt, 150).await
    }

    /// Provide a detailed summary of a memory, extracting key information
    pub async fn summarize_memory_detail(
        &self,
        app_name: &str,
        window_title: &str,
        text: &str,
    ) -> String {
        if text.trim().is_empty() {
            return "No content to summarize.".to_string();
        }

        let prompt = match self.build_prompt(
            "You extract key facts from local screen memories.",
            &format!(
                "MEMORY CONTENT:\nApp: {}\nWindow: {}\nContent: {}\n\nREQUEST: Return ACTIVITY and DETAILS. Be concise.",
                app_name,
                window_title,
                text.chars().take(1000).collect::<String>()
            ),
        ) {
            Ok(prompt) => prompt,
            Err(err) => {
                tracing::error!("Prompt build failed: {}", err);
                return String::new();
            }
        };

        self.complete(&prompt, 150).await
    }

    /// Generate a structured memory card draft from grouped snippets.
    pub async fn synthesize_memory_card(
        &self,
        query: &str,
        app_name: &str,
        window_title: &str,
        snippets: &[String],
    ) -> Option<MemoryCardDraft> {
        if snippets.is_empty() {
            return None;
        }

        let snippet_block = snippets
            .iter()
            .take(6)
            .enumerate()
            .map(|(idx, snippet)| format!("{}. {}", idx + 1, snippet))
            .collect::<Vec<_>>()
            .join("\n");

        let system_msg = format!(
            "You synthesize one memory card from grouped search snippets.\n\
            RULES:\n\
            - Return ONLY strict JSON with keys: title, summary, action, context.\n\
            - summary must be exactly one sentence, 8-22 words.\n\
            {VOICE_RULES}\n\
            - Use ONLY facts explicitly present in SNIPPETS. Do not infer unseen details.\n\
            - If evidence is weak, start summary with 'Low confidence:'.\n\
            - Focus on one dominant activity with 1-3 high-signal details.\n\
            - context must be an array of 1-4 short strings.\n\
            - Prefer context items that mention source IDs like src:<id> when present."
        );

        let prompt = self
            .build_prompt(
                &system_msg,
                &format!(
                    "QUERY: {}\nAPP: {}\nWINDOW: {}\nSNIPPETS:\n{}\n\nReturn JSON only.",
                    query, app_name, window_title, snippet_block
                ),
            )
            .ok()?;

        let raw = self.complete(&prompt, 180).await;
        let candidate = extract_json_object(&raw)?;
        let draft: MemoryCardDraft = serde_json::from_str(&candidate).ok()?;
        validate_memory_card_draft(draft)
    }

    /// Extract actionable todos/reminders from memory text.
    ///
    /// NOTE: Return type remains `String` with one `TODO:`/`REMINDER:`/`FOLLOWUP:`
    /// line per item for backwards compatibility. A future revision should return
    /// `Vec<TodoItem>` with a proper enum — callers currently parse this string.
    pub async fn extract_todos(&self, memories_text: &str) -> String {
        if memories_text.trim().is_empty() {
            return String::new();
        }

        let prompt = match self.build_prompt(
            "You identify clear follow-up actions from recent screen activity.",
            &format!(
                "Extract only clearly actionable items from this activity.\n\
Format each line exactly as one of:\n\
- TODO: [clear next action]\n\
- REMINDER: [date/time-sensitive reminder]\n\
- FOLLOWUP: [person/team + reason]\n\
Rules:\n\
- Return 0 to 4 total lines.\n\
- If nothing is clearly actionable, return exactly: NONE\n\
- Do NOT infer tasks from passive browsing or generic reading.\n\
- TODO must sound like a real self-note someone would actually write.\n\
- REMINDER requires explicit time/day/deadline signal in the evidence.\n\
- FOLLOWUP requires a concrete person or team and why follow-up is needed.\n\
- Keep each line short, specific, and non-duplicate.\n\
- No extra commentary.\n\n{}",
                memories_text.chars().take(2000).collect::<String>()
            ),
        ) {
            Ok(prompt) => prompt,
            Err(err) => {
                tracing::error!("Prompt build failed: {}", err);
                return String::new();
            }
        };

        self.complete(&prompt, 200).await
    }

    /// Extract structured memory fields natively via Qwen3-VL style JSON prompt.
    pub async fn extract_structured_memory(
        &self,
        app_name: &str,
        window_title: &str,
        ocr_text: &str,
    ) -> Option<StructuredMemoryExtraction> {
        if ocr_text.trim().is_empty() {
            return None;
        }

        let system_msg = format!(
            "You are a structured memory extractor.\n\
            RULES:\n\
            - Output ONLY raw JSON.\n\
            - No markdown formatting.\n\
            - Do not copy OCR verbatim.\n\
            - Build one rich memory_context narrative for AI-agent continuation.\n\
            - memory_context must be specific, evidence-aware, and useful later.\n\
            - If uncertain, lower confidence instead of inventing details.\n\
            \n\
            SCHEMA:\n\
            {{\n\
              \"session_key\": \"YYYY-MM-DD_HH\",\n\
              \"activity_type\": \"coding|debugging|reviewing_agent_output|researching|planning|writing|studying|watching_or_listening|configuring_tool|testing_workflow|reading_results|organizing_information|communication|job_or_career_work|travel_or_logistics|entertainment_or_personal_interest|unknown\",\n\
              \"project\": \"\",\n\
              \"topic\": \"\",\n\
              \"workflow\": \"\",\n\
              \"user_intent\": \"\",\n\
              \"memory_context\": \"\",\n\
              \"files_touched\": [],\n\
              \"symbols_changed\": [],\n\
              \"git_stats\": {{ \"added\": 0, \"removed\": 0, \"commits\": 0 }},\n\
              \"outcome\": \"completed|reverted|in_progress|failed\",\n\
              \"tags\": [],\n\
              \"entities\": [],\n\
              \"decisions\": [],\n\
              \"errors\": [],\n\
              \"next_steps\": [],\n\
              \"commands\": [],\n\
              \"blockers\": [],\n\
              \"todos\": [],\n\
              \"open_questions\": [],\n\
              \"results\": [],\n\
              \"search_aliases\": [],\n\
              \"confidence\": 0.0,\n\
              \"dedup_fingerprint\": \"\"\n\
            }}"
        );

        let user_msg = format!(
            "APP: {}\nWINDOW: {}\nOCR TEXT:\n\"\"\"\n{}\n\"\"\"\n\nReturn JSON only.",
            app_name,
            window_title,
            ocr_text.chars().take(4000).collect::<String>()
        );

        let prompt = self.build_prompt(&system_msg, &user_msg).ok()?;
        let raw = self.complete(&prompt, 400).await;

        let candidate = extract_json_object(&raw)?;
        match serde_json::from_str::<StructuredMemoryExtraction>(&candidate) {
            Ok(draft) => Some(draft),
            Err(e) => {
                tracing::warn!("Failed to parse structured memory JSON: {}", e);
                // Try repair once
                let repair_msg = format!(
                    "Fix this invalid JSON to match the strict schema. Output ONLY JSON.\nINVALID JSON:\n{}", 
                    candidate
                );
                if let Ok(repair_prompt) = self.build_prompt(&system_msg, &repair_msg) {
                    let repaired_raw = self.complete(&repair_prompt, 400).await;
                    if let Some(repaired_candidate) = extract_json_object(&repaired_raw) {
                        serde_json::from_str::<StructuredMemoryExtraction>(&repaired_candidate).ok()
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
        }
    }

    /// Extract structured meeting summary + action items.
    pub async fn extract_meeting_breakdown(
        &self,
        transcript: &str,
    ) -> Option<MeetingTaskBreakdownDraft> {
        if transcript.trim().is_empty() {
            return None;
        }

        let prompt = self
            .build_prompt(
                "You extract only high-confidence meeting outcomes from transcripts.",
                &format!(
                    "Read the meeting transcript and return STRICT JSON with keys:\n\
summary, todos, reminders, followups\n\
\n\
Schema:\n\
{{\"summary\":\"...\",\"todos\":[\"...\"],\"reminders\":[\"...\"],\"followups\":[\"...\"]}}\n\
\n\
Rules:\n\
- summary: exactly 1 short paragraph (1-3 sentences) based only on transcript facts.\n\
- todos: concrete next actions someone explicitly committed to.\n\
- reminders: only explicit date/time/deadline reminders.\n\
- followups: specific people/teams to follow up with and why.\n\
- If evidence is weak, leave arrays empty.\n\
- 0-5 items per array, no duplicates, no generic filler.\n\
- Return JSON only.\n\
\n\
TRANSCRIPT:\n{}",
                    transcript.chars().take(7000).collect::<String>()
                ),
            )
            .ok()?;

        let raw = self.complete(&prompt, 360).await;
        let candidate = extract_json_object(&raw)?;
        let mut draft: MeetingTaskBreakdownDraft = serde_json::from_str(&candidate).ok()?;

        let clean_item = |value: String| {
            let cleaned = normalize_whitespace(
                value
                    .trim()
                    .trim_matches(|ch| ch == '"' || ch == '\'' || ch == '`'),
            );
            if cleaned.len() < 6 {
                return None;
            }
            let lower = cleaned.to_lowercase();
            if matches!(
                lower.as_str(),
                "none" | "n/a" | "na" | "no action items" | "no follow-ups" | "no reminders"
            ) {
                return None;
            }
            Some(cleaned)
        };

        let clean_vec = |items: Vec<String>| {
            let mut out = Vec::new();
            let mut seen = std::collections::HashSet::new();
            for item in items {
                let Some(cleaned) = clean_item(item) else {
                    continue;
                };
                let key = cleaned.to_lowercase();
                if seen.insert(key) {
                    out.push(cleaned);
                }
                if out.len() >= 5 {
                    break;
                }
            }
            out
        };

        draft.summary = normalize_whitespace(draft.summary.trim());
        draft.todos = clean_vec(draft.todos);
        draft.reminders = clean_vec(draft.reminders);
        draft.followups = clean_vec(draft.followups);

        if draft.summary.is_empty()
            && draft.todos.is_empty()
            && draft.reminders.is_empty()
            && draft.followups.is_empty()
        {
            return None;
        }

        Some(draft)
    }

    /// Generate a smart daily briefing paragraph from today's memory cards.
    /// `mode` is either "morning" (actionable: what to work on) or "evening" (recap + tomorrow).
    pub async fn generate_daily_briefing(&self, card_lines: &[String], mode: &str) -> String {
        if card_lines.is_empty() {
            return String::new();
        }

        let cards_block = card_lines.join("\n");

        let (system_msg, task_instruction) = if mode == "evening" {
            (
                format!(
                    "You are a smart personal assistant that writes concise end-of-day briefings.\n\
                    RULES:\n\
                    - Write exactly 2-3 sentences in plain English.\n\
                    - Sentence 1: What you worked on today (specific activities, not generic).\n\
                    - Sentence 2: One important thing to carry forward or revisit tomorrow.\n\
                    - Sentence 3 (optional): A cross-connection you noticed across activities.\n\
                    - Be specific. Name real tasks, tools, or topics from the memories.\n\
                    {VOICE_RULES}"
                ),
                "Based on today's activity below, write the end-of-day briefing paragraph.\nReturn only the paragraph, nothing else.",
            )
        } else {
            (
                format!(
                    "You are a smart personal assistant that writes concise morning/daytime briefings.\n\
                    RULES:\n\
                    - Write exactly 2-3 sentences in plain English.\n\
                    - Sentence 1: What deserves attention today, based on recent activity.\n\
                    - Sentence 2: A specific piece of context or info from memory that will be useful.\n\
                    - Sentence 3 (optional): Something in progress that needs a follow-up.\n\
                    - Be specific. Name real tasks, tools, topics, or people from the memories.\n\
                    {VOICE_RULES}"
                ),
                "Based on recent activity below, write the morning briefing paragraph.\nReturn only the paragraph, nothing else.",
            )
        };

        let user_msg = format!(
            "RECENT ACTIVITY:\n{}\n\n{}",
            cards_block.chars().take(900).collect::<String>(),
            task_instruction
        );

        let prompt = match self.build_prompt(&system_msg, &user_msg) {
            Ok(p) => p,
            Err(err) => {
                tracing::error!("Daily briefing prompt build failed: {}", err);
                return String::new();
            }
        };

        tracing::debug!("Generating daily briefing (mode={})...", mode);
        let raw = self.complete(&prompt, 160).await;

        raw.trim()
            .trim_matches(|ch| ch == '"' || ch == '\'')
            .to_string()
    }

    /// Generate an on-demand, smart daily summary of grouped user activities.
    pub async fn generate_daily_summary(&self, grouped_activity_text: &str) -> String {
        if grouped_activity_text.is_empty() {
            return String::new();
        }

        let system_msg = format!(
            "You are a highly efficient personal assistant writing concise daily summaries based on local, grouped context logs.\n\
            RULES:\n\
            - Write exactly 6 to 8 short bullet points.\n\
            - Keep each point high-level but concrete.\n\
            - Name real tools, apps, or topics mentioned in the context.\n\
            - Do not list chronological actions. Cluster by thematic activity.\n\
            {VOICE_RULES}\n\
            - Formatting: Output plain text bullet points starting with '- '.\n\
            - No preambles, no Markdown bolding, just the bullets."
        );

        let user_msg = format!(
            "CLUSTERED DAILY ACTIVITY:\n{}\n\nReturn the 6-8 bullet daily summary.",
            grouped_activity_text.chars().take(2000).collect::<String>()
        );

        let prompt = match self.build_prompt(&system_msg, &user_msg) {
            Ok(p) => p,
            Err(err) => {
                tracing::error!("Daily summary prompt build failed: {}", err);
                return String::new();
            }
        };

        tracing::debug!("Generating on-demand daily summary...");
        let raw = self.complete(&prompt, 350).await;

        raw.trim()
            .trim_matches(|ch| ch == '"' || ch == '\'')
            .to_string()
    }

    fn build_prompt(&self, system_message: &str, user_message: &str) -> Result<String, String> {
        // Null bytes in input indicate a real upstream data issue (broken OCR,
        // bad decode) rather than something to silently paper over. Log and strip.
        let sys = if system_message.contains('\0') {
            tracing::warn!("build_prompt: null byte in system message (stripping)");
            system_message.replace('\0', " ")
        } else {
            system_message.to_string()
        };
        let usr = if user_message.contains('\0') {
            tracing::warn!("build_prompt: null byte in user message (stripping)");
            user_message.replace('\0', " ")
        } else {
            user_message.to_string()
        };

        let messages = vec![
            LlamaChatMessage::new("system".to_string(), sys).map_err(|err| err.to_string())?,
            LlamaChatMessage::new("user".to_string(), usr).map_err(|err| err.to_string())?,
        ];

        self.model
            .apply_chat_template(&self.chat_template, &messages, true)
            .map_err(|err| err.to_string())
    }

    /// Run generation. Offloads the full blocking section (mutex + decode loop)
    /// onto `spawn_blocking` so we don't stall the tokio runtime during long
    /// generations.
    ///
    /// NOTE: Only one generation runs at a time across the whole engine because
    /// `LlamaContext` is protected by a single `Mutex`. This is intentional —
    /// the underlying KV cache is shared state.
    async fn complete(&self, prompt: &str, max_tokens: i32) -> String {
        // Safety: `model` is `&'static`, so we can move a copy of the reference
        // into the blocking closure without borrowing `self`. The context is
        // accessed via a raw pointer bypass of the borrow checker using a
        // self-pointer dance — simpler approach: clone what we need.
        //
        // We can't move `&self.context` into spawn_blocking because the future
        // borrows `self`. Instead, grab an Arc-safe handle by temporarily
        // restructuring: wrap the blocking body in a synchronous helper that
        // takes the prompt + a mutex guard.
        //
        // Simplest correct implementation: do the lock + generation inside
        // spawn_blocking by passing raw references that outlive the closure.
        // Since `self` outlives any call to `complete`, we extend lifetimes
        // via `unsafe` scoped to this function. To keep this safe, we hold
        // an `Arc`-less lock *inside* the closure on a `&'static`-ish handle.
        //
        // Cleaner solution: store the Mutex in an Arc. But that's a struct
        // change. For a drop-in fix, we accept that we block briefly on the
        // mutex lock here (async-aware) via spawn_blocking wrapping everything.

        // To keep the API change minimal, we send everything the blocking
        // closure needs as owned data, then do the generation with a scoped
        // 'static transmute of &self. This relies on `InferenceEngine` being
        // a process-wide singleton (same invariant as the leaked model).
        let self_static: &'static InferenceEngine = unsafe {
            // SAFETY: InferenceEngine is a singleton held for application
            // lifetime (see struct-level docs). The caller's `&self` therefore
            // outlives any spawn_blocking future we create here.
            std::mem::transmute::<&InferenceEngine, &'static InferenceEngine>(self)
        };

        let prompt_owned = prompt.to_string();

        tokio::task::spawn_blocking(move || {
            self_static.complete_blocking(&prompt_owned, max_tokens)
        })
        .await
        .unwrap_or_else(|e| {
            tracing::error!("complete: spawn_blocking join failed: {}", e);
            String::new()
        })
    }

    /// Synchronous generation core. Called from inside `spawn_blocking`.
    fn complete_blocking(&self, prompt: &str, max_tokens: i32) -> String {
        let t0 = std::time::Instant::now();
        let mut ctx = self.context.lock();

        // Reset KV cache between independent requests.
        ctx.clear_kv_cache();

        let n_batch = ctx.n_batch().max(1) as usize;
        let n_ctx = ctx.n_ctx() as usize;

        let mut tokens_list = match self.model.str_to_token(prompt, AddBos::Never) {
            Ok(t) => t,
            Err(e) => {
                tracing::error!("Tokenization failed: {}", e);
                return "AI Error: Tokenization failed.".to_string();
            }
        };

        if tokens_list.is_empty() {
            tracing::error!("Tokenization produced empty output");
            return String::new();
        }

        // Worst case we may generate up to `max_tokens`; prompt must fit in n_ctx with headroom.
        let gen_cap = (max_tokens.max(0) as usize).min(n_ctx.saturating_sub(1));
        let max_prompt_tokens = n_ctx.saturating_sub(gen_cap).max(1);
        if tokens_list.len() > max_prompt_tokens {
            let excess = tokens_list.len() - max_prompt_tokens;
            tracing::warn!(
                "Prompt tokenized to {} tokens; truncating {} from the start to fit n_ctx={} (gen budget {})",
                tokens_list.len(),
                excess,
                n_ctx,
                gen_cap
            );
            tokens_list.drain(..excess);
        }

        let prompt_len = tokens_list.len();
        // llama.cpp asserts n_tokens_all <= n_batch for each decode; chunk the prefill.
        let mut batch = LlamaBatch::new(n_batch, 1);
        let mut offset = 0usize;
        while offset < prompt_len {
            let end = (offset + n_batch).min(prompt_len);
            batch.clear();
            for pos in offset..end {
                let logits = pos == prompt_len - 1;
                if let Err(e) = batch.add(tokens_list[pos], pos as i32, &[0], logits) {
                    tracing::error!("Batch add failed during prefill: {:?}", e);
                    return "AI Error: LLM batch setup failed.".to_string();
                }
            }
            if let Err(e) = ctx.decode(&mut batch) {
                tracing::error!("Decode failed: {}", e);
                return "AI Error: LLM Decode failed.".to_string();
            }
            offset = end;
        }

        // Proper sampler chain: repetition penalty + greedy.
        //
        // - Penalties: last-64 window, repeat=1.1, freq=0.0, presence=0.0.
        //   This kills the "summary summary summary" loops that pure-argmax
        //   greedy was prone to on smaller models.
        // - Greedy: deterministic argmax over the post-penalty distribution.
        //   Keeps behavior close to the previous implementation for summarization
        //   and JSON-extraction paths (which want determinism).
        //
        // If temperature/top-p sampling is ever wanted per-call, expose it via
        // an argument; for now all callers want deterministic output.
        let mut sampler = LlamaSampler::chain_simple([
            LlamaSampler::penalties(64, 1.1, 0.0, 0.0),
            LlamaSampler::greedy(),
        ]);

        let mut result = String::new();
        let mut n_cur = tokens_list.len() as i32;

        for _ in 0..max_tokens {
            let token = sampler.sample(&ctx, -1);
            sampler.accept(token);

            if self.model.is_eog_token(token) {
                break;
            }

            #[allow(deprecated)]
            let piece = match self.model.token_to_str(token, Special::Plaintext) {
                Ok(s) => s,
                Err(_) => String::new(),
            };
            result.push_str(&piece);

            batch.clear();
            let _ = batch.add(token, n_cur, &[0], true);
            if let Err(e) = ctx.decode(&mut batch) {
                tracing::error!("Incremental decode failed: {}", e);
                break;
            }
            n_cur += 1;
        }

        tracing::debug!(
            "Completion result ({} tokens): {}",
            n_cur - tokens_list.len() as i32,
            result.trim()
        );
        crate::telemetry::runtime_metrics::record_ms(
            "llm.complete_ms",
            t0.elapsed().as_millis() as u64,
        );
        result.trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefill_chunk_windows_respect_n_batch_and_cover_prompt() {
        let n_batch = 512;
        let prompt_len = 1500;
        let mut offset = 0usize;
        let mut total = 0usize;
        while offset < prompt_len {
            let end = (offset + n_batch).min(prompt_len);
            assert!(end - offset <= n_batch);
            total += end - offset;
            offset = end;
        }
        assert_eq!(total, prompt_len);
    }

    #[test]
    fn cleans_common_summary_preambles() {
        let cleaned = clean_summary_output("Summary: The screen shows reviewing PR comments");
        assert_eq!(cleaned, "reviewing PR comments");
    }

    #[test]
    fn rejects_file_inventory_noise() {
        let noisy = "src/app.tsx src/lib.rs src/main.rs src-tauri/src/store/schema.rs src-tauri/src/graph/mod.rs";
        assert!(!is_usable_summary(noisy));
    }

    #[test]
    fn accepts_concise_activity_summary() {
        assert!(is_usable_summary(
            "Reviewing download_model.sh changes in FNDR"
        ));
    }

    #[test]
    fn handles_action_context_format() {
        let cleaned =
            clean_summary_output("Action: edited schema.rs | Context: in FNDR's store module");
        assert!(cleaned.starts_with("edited schema.rs"));
        assert!(cleaned.contains("FNDR"));
    }

    #[test]
    fn normalizes_third_person_user_references() {
        assert_eq!(
            normalize_person("The user reviewed the PR"),
            "You reviewed the PR"
        );
        assert_eq!(
            normalize_person("user opened VS Code"),
            "You opened VS Code"
        );
    }

    #[test]
    fn person_normalization_preserves_compound_words() {
        // \b regex boundary should not match inside "username"
        let got = normalize_person("username field was edited");
        assert_eq!(got, "username field was edited");
    }

    #[test]
    fn truncates_summaries_to_two_sentences() {
        let raw = "First action. Second action. Third action. Fourth action.";
        let cleaned = clean_summary_output(raw);
        let sentence_count = cleaned.matches('.').count();
        assert_eq!(sentence_count, 2, "got: {}", cleaned);
    }

    #[test]
    fn extract_json_handles_code_fences() {
        let raw = "Here is the result:\n```json\n{\"title\": \"test\", \"n\": 1}\n```\n";
        let extracted = extract_json_object(raw).expect("should extract");
        assert_eq!(extracted, "{\"title\": \"test\", \"n\": 1}");
    }

    #[test]
    fn extract_json_handles_escaped_quotes() {
        let raw = r#"preamble {"msg": "he said \"hi\"", "ok": true} trailer"#;
        let extracted = extract_json_object(raw).expect("should extract");
        assert_eq!(extracted, r#"{"msg": "he said \"hi\"", "ok": true}"#);
    }

    #[test]
    fn extract_json_returns_none_on_unbalanced() {
        assert!(extract_json_object("just { text with no close").is_none());
    }
}
