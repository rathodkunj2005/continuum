//! Vision Language Model (VLM) inference engine
//!
//! Loads a GGUF from the shared catalog for OCR-grounded screen prompts.
//! Default path uses Llama 3.2 1B (text weights; prompts are OCR-only).
//! Optional Qwen3-VL 4B for users who accept higher RAM use.

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
#[allow(deprecated)]
use llama_cpp_2::model::Special;
use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaChatTemplate, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use parking_lot::Mutex;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

/// Errors that can occur during VLM operations
#[derive(Debug, thiserror::Error)]
pub enum VlmError {
    #[error("Model initialization failed: {0}")]
    InitializationError(String),

    #[error("Model not found: {0}")]
    ModelNotFound(String),

    #[error("Tokenization failed: {0}")]
    TokenizationError(String),

    #[error("Inference failed: {0}")]
    InferenceError(String),
}

/// Configuration for VLM inference
#[derive(Debug, Clone)]
pub struct VlmConfig {
    /// Maximum context size
    pub context_size: u32,

    /// Maximum tokens to generate
    pub max_tokens: i32,

    /// Temperature for sampling (higher = more random)
    pub temperature: f32,

    /// Top-p (nucleus) sampling threshold
    pub top_p: f32,

    /// Top-k sampling limit
    pub top_k: i32,
}

impl Default for VlmConfig {
    fn default() -> Self {
        Self {
            context_size: 2048,
            max_tokens: 100,
            temperature: 0.7,
            top_p: 0.9,
            top_k: 40,
        }
    }
}

/// VLM Engine for image understanding
/// Provides intelligent screen analysis beyond raw OCR text
pub struct VlmEngine {
    model: &'static LlamaModel,
    context: Mutex<llama_cpp_2::context::LlamaContext<'static>>,
    _backend: Arc<LlamaBackend>,
    chat_template: LlamaChatTemplate,
    model_size: String,
    model_id: String,
    model_path: PathBuf,
    config: VlmConfig,
}

unsafe impl Send for VlmEngine {}
unsafe impl Sync for VlmEngine {}

impl VlmEngine {
    /// Initialize the VLM engine with specified model size and default config
    pub async fn new(model_size: &str, app_data_dir: Option<PathBuf>) -> Result<Self, VlmError> {
        Self::with_config(model_size, VlmConfig::default(), app_data_dir).await
    }

    /// Initialize the VLM engine with custom configuration
    pub async fn with_config(
        model_size: &str,
        config: VlmConfig,
        app_data_dir: Option<PathBuf>,
    ) -> Result<Self, VlmError> {
        let resolved_model = Self::resolve_model_path(model_size, app_data_dir.as_deref())?;
        let model_path = resolved_model.path.clone();
        let model_id = resolved_model.definition.id.to_string();
        let size_label = resolved_model.definition.name.to_string();

        tracing::info!(
            "Initializing VLM engine ({}) from {:?}...",
            size_label,
            model_path
        );

        let backend = crate::inference::get_or_init_backend()
            .map_err(|e| VlmError::InitializationError(format!("Backend init failed: {}", e)))?;

        let model_params = LlamaModelParams::default();
        let model = LlamaModel::load_from_file(&backend, &model_path, &model_params)
            .map_err(|e| VlmError::InitializationError(format!("Model load failed: {}", e)))?;

        // Leak the model to get a 'static reference (singleton pattern)
        let model_ref: &'static LlamaModel = Box::leak(Box::new(model));
        let chat_template = match model_ref.chat_template(None) {
            Ok(template) => template,
            Err(err) => {
                tracing::warn!(
                    "VLM model {} has no baked chat template ({}); falling back to chatml",
                    model_id,
                    err
                );
                LlamaChatTemplate::new("chatml").map_err(|fallback_err| {
                    VlmError::InitializationError(format!(
                        "Fallback chat template failed: {}",
                        fallback_err
                    ))
                })?
            }
        };

        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(Some(NonZeroU32::new(config.context_size).ok_or_else(
                || VlmError::InitializationError("Context size must be non-zero".to_string()),
            )?))
            .with_n_batch(config.context_size);

        let context = model_ref.new_context(&backend, ctx_params).map_err(|e| {
            VlmError::InitializationError(format!("Context creation failed: {}", e))
        })?;

        tracing::info!(
            "VLM engine initialized successfully ({}, ctx_size={})",
            size_label,
            config.context_size
        );

        Ok(Self {
            model: model_ref,
            context: Mutex::new(context),
            _backend: backend,
            chat_template,
            model_size: size_label,
            model_id,
            model_path,
            config,
        })
    }

    /// Resolve model path, trying primary then fallback
    fn resolve_model_path(
        preferred_size: &str,
        app_data_dir: Option<&Path>,
    ) -> Result<crate::models::ResolvedModel, VlmError> {
        let preferred_model_id = match preferred_size {
            "4B" => Some("qwen3-vl-4b"),
            "1B" => Some("llama-3.2-1b"),
            _ => None,
        };

        crate::models::resolve_model(preferred_model_id, app_data_dir).ok_or_else(|| {
            let searched_dirs = crate::models::candidate_model_dirs(app_data_dir)
                .into_iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            VlmError::ModelNotFound(format!(
                "No VLM model found. Searched: {}. Please run the download script.",
                searched_dirs
            ))
        })
    }

    /// Get the active model size
    pub fn model_size(&self) -> &str {
        &self.model_size
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    pub fn model_path(&self) -> &Path {
        &self.model_path
    }

    /// Get the current configuration
    pub fn config(&self) -> &VlmConfig {
        &self.config
    }

    /// Update the configuration
    pub fn update_config(&mut self, config: VlmConfig) {
        self.config = config;
    }

    /// Describe what's visible in a screenshot
    /// Returns a concise description of the screen content
    pub async fn describe_screen(&self, ocr_text: &str) -> Result<String, VlmError> {
        if ocr_text.trim().is_empty() {
            return Ok(String::new());
        }

        let prompt = self.build_prompt(
            "You are a screen activity analyzer. Extract the PRIMARY user action from OCR text.\n\
            \n\
            RULES:\n\
            - Output ONE action verb + object (e.g., 'Writing email', 'Reading documentation', 'Debugging code')\n\
            - Infer activity from content context, not UI chrome\n\
            - Maximum 5 words\n\
            - No articles (a/an/the), no subjects (user/I/they)\n\
            - No meta-commentary ('appears to be', 'seems like')\n\
            - If multiple activities, pick the DOMINANT one\n\
            \n\
            EXAMPLES:\n\
            OCR: 'From: john@example.com Subject: Re: Q4 Budget' → 'Reading budget email'\n\
            OCR: 'def calculate_sum(a, b): return a + b' → 'Writing Python function'\n\
            OCR: 'Google Search: best restaurants near me' → 'Searching restaurants'\n\
            OCR: 'Video 0:45 / 12:30 The Art of Code' → 'Watching programming tutorial'\n\
            OCR: 'Pull Request #234 Fix authentication bug' → 'Reviewing code PR'",
            &format!("OCR: '{}'", ocr_text.trim()),
        )?;

        self.complete(&prompt, Some(50)).await
    }

    /// Analyze screen content and combine with OCR for richer context
    pub async fn analyze_screen(&self, ocr_text: &str, app_name: &str) -> Result<String, VlmError> {
        if ocr_text.trim().is_empty() {
            return Ok(format!("Using {}", app_name));
        }

        let prompt = self.build_prompt(
            "You are a memory indexing system. Extract searchable metadata from screen activity.\n\
            \n\
            OUTPUT FORMAT (strict):\n\
            Action: [verb] | Context: [2-4 key details]\n\
            \n\
            EXTRACTION RULES:\n\
            - Action: ONE action verb (editing, browsing, debugging, writing, reading, configuring, searching)\n\
            - Context: Extract ONLY:\n\
              * Document/file names\n\
              * Code symbols/function names\n\
              * Email subjects/senders\n\
              * Search queries\n\
              * URL domains\n\
              * Key entities (people, projects, topics)\n\
            - Ignore: UI text, buttons, menus, status bars, chrome\n\
            - Maximum 12 words total\n\
            - Use abbreviations where clear (impl → implementation, config → configuration)\n\
            \n\
            EXAMPLES:\n\
            App: VSCode | OCR: 'src/auth.rs fn validate_token()' → Action: editing | Context: auth.rs validate_token function\n\
            App: Gmail | OCR: 'From: Sarah Chen Re: Sprint Planning' → Action: reading | Context: email from Sarah Chen re Sprint Planning\n\
            App: Chrome | OCR: 'Stack Overflow How to handle Rust lifetimes' → Action: browsing | Context: Stack Overflow Rust lifetimes\n\
            App: Terminal | OCR: '$ cargo test integration_tests' → Action: testing | Context: cargo integration tests\n\
            App: Figma | OCR: 'Dashboard Mockup v3 Mobile View' → Action: designing | Context: Dashboard Mockup v3 mobile",
            &format!("App: {} | OCR: '{}'", app_name, ocr_text.trim()),
        )?;

        self.complete(&prompt, Some(80)).await
    }

    /// Build a properly formatted prompt
    fn build_prompt(&self, system_message: &str, user_message: &str) -> Result<String, VlmError> {
        let messages = vec![
            LlamaChatMessage::new("system".to_string(), system_message.replace('\0', " "))
                .map_err(|err| VlmError::InitializationError(err.to_string()))?,
            LlamaChatMessage::new("user".to_string(), user_message.replace('\0', " "))
                .map_err(|err| VlmError::InitializationError(err.to_string()))?,
        ];

        self.model
            .apply_chat_template(&self.chat_template, &messages, true)
            .map_err(|err| VlmError::InitializationError(err.to_string()))
    }

    /// Internal completion method with improved sampling
    async fn complete(&self, prompt: &str, max_tokens: Option<i32>) -> Result<String, VlmError> {
        let max_tokens = max_tokens.unwrap_or(self.config.max_tokens);
        let t0 = Instant::now();
        let mut ctx = self.context.lock();

        // Clear previous context
        ctx.clear_kv_cache();

        // Tokenize input
        let mut tokens_list = self
            .model
            .str_to_token(prompt, AddBos::Never)
            .map_err(|e| VlmError::TokenizationError(e.to_string()))?;

        // Truncate to ensure the batch never exceeds context_size (cparams.n_batch)
        let max_prompt_len = self
            .config
            .context_size
            .saturating_sub(max_tokens as u32)
            .saturating_sub(1) as usize;
        if tokens_list.len() > max_prompt_len {
            tracing::warn!(
                "Prompt tokens ({}) > context limit ({}), truncating...",
                tokens_list.len(),
                max_prompt_len
            );
            // Drain excess tokens from the beginning and keep the BOS intact
            let excess = tokens_list.len() - max_prompt_len;
            tokens_list.drain(1..1 + excess);
        }

        // Create batch with appropriate size
        let batch_size = (tokens_list.len() + max_tokens as usize).max(512);
        let mut batch = LlamaBatch::new(batch_size, 1);

        // Add tokens to batch
        for (i, token) in tokens_list.iter().enumerate() {
            let last = i == tokens_list.len() - 1;
            batch
                .add(*token, i as i32, &[0], last)
                .map_err(|e| VlmError::InferenceError(format!("Batch add failed: {}", e)))?;
        }

        // Initial decode
        ctx.decode(&mut batch)
            .map_err(|e| VlmError::InferenceError(format!("Initial decode failed: {}", e)))?;

        // Create sampler with configured parameters
        let mut sampler = LlamaSampler::chain_simple(vec![
            LlamaSampler::temp(self.config.temperature),
            LlamaSampler::top_k(self.config.top_k),
            LlamaSampler::top_p(self.config.top_p, 1),
            LlamaSampler::dist(0), // Sample from distribution
        ]);

        let mut result = String::new();
        let mut n_cur = tokens_list.len() as i32;

        // The first time we sample, we want the logits from the last token of the prompt batch.
        // For subsequent generation steps, the batch only contains 1 token, so the index is 0.
        let mut batch_idx_to_sample = (tokens_list.len() - 1) as i32;

        // Generate tokens
        for _ in 0..max_tokens {
            // Sampler needs context and the batch index where logits were calculated
            let token = sampler.sample(&ctx, batch_idx_to_sample);

            // For all next iterations, the batch will be of size 1
            batch_idx_to_sample = 0;

            // Check for end-of-generation
            if self.model.is_eog_token(token) {
                break;
            }

            // Convert token to text
            #[allow(deprecated)]
            let piece = self
                .model
                .token_to_str(token, Special::Plaintext)
                .unwrap_or_default();
            result.push_str(&piece);

            // Prepare next batch
            batch.clear();
            batch
                .add(token, n_cur, &[0], true)
                .map_err(|e| VlmError::InferenceError(format!("Batch add failed: {}", e)))?;

            // Decode next token
            ctx.decode(&mut batch).map_err(|e| {
                VlmError::InferenceError(format!("Incremental decode failed: {}", e))
            })?;

            n_cur += 1;
        }

        crate::telemetry::runtime_metrics::record_ms(
            "vlm.complete_ms",
            t0.elapsed().as_millis() as u64,
        );
        Ok(result.trim().to_string())
    }

    /// Health check - verify the engine is operational
    pub async fn health_check(&self) -> Result<(), VlmError> {
        let test_prompt = self.build_prompt(
            "You are a helpful assistant.",
            "Respond with 'OK' if you are working.",
        )?;

        self.complete(&test_prompt, Some(10)).await?;
        Ok(())
    }

    /// Get model information
    pub fn info(&self) -> VlmInfo {
        VlmInfo {
            model_size: self.model_size.clone(),
            context_size: self.config.context_size,
            vocab_size: self.model.n_vocab(),
        }
    }
}

/// Information about the loaded VLM model
#[derive(Debug, Clone)]
pub struct VlmInfo {
    pub model_size: String,
    pub context_size: u32,
    pub vocab_size: i32,
}
