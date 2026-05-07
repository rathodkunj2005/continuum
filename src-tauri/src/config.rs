//! Configuration management

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const DEFAULT_TEXT_EMBEDDING_DIM: usize = 1024;
pub const DEFAULT_IMAGE_EMBEDDING_DIM: usize = 512;
pub const DEFAULT_EMBEDDING_MODEL_NAME: &str = "bge-large-en-v1.5";
pub const DEFAULT_EMBEDDING_MODEL_FILENAME: &str = "bge-large-en-v1.5-quantized.onnx";
pub const DEFAULT_EMBEDDING_TOKENIZER_FILENAME: &str = "tokenizer.json";
pub const DEFAULT_EMBEDDING_MAX_SEQ_LEN: usize = 512;
pub const DEFAULT_EMBEDDING_CACHE_CAPACITY: usize = 1024;
pub const DEFAULT_EMBEDDING_MAX_BATCH: usize = 16;

pub const DEFAULT_CHUNK_MAX_TOKENS: usize = 450;
pub const DEFAULT_CHUNK_OVERLAP_TOKENS: usize = 96;
pub const DEFAULT_CHUNK_MIN_TOKENS: usize = 15;
pub const DEFAULT_CHARS_PER_TOKEN: usize = 4;
pub const DEFAULT_CHUNK_OCR_TARGET_MIN_CHARS: usize = 300;
pub const DEFAULT_CHUNK_OCR_TARGET_MAX_CHARS: usize = 900;

pub const DEFAULT_SEARCH_CANDIDATE_MULTIPLIER: usize = 4;
pub const DEFAULT_SEARCH_MAX_RERANK_POOL: usize = 36;
pub const DEFAULT_SEARCH_MAX_KEYWORD_VARIANTS: usize = 4;
pub const DEFAULT_SEARCH_MAX_KEYWORD_FALLBACK_VARIANTS: usize = 2;
pub const DEFAULT_SEARCH_DIVERSITY_PRESERVE_TOP: usize = 3;
pub const DEFAULT_SEARCH_MAX_SEMANTIC_BRANCH_LIMIT: usize = 64;
pub const DEFAULT_SEARCH_MAX_KEYWORD_BRANCH_LIMIT: usize = 48;
pub const DEFAULT_SEARCH_MIN_SNIPPET_QUERY_TERMS: usize = 3;
pub const DEFAULT_SEARCH_VECTOR_WEIGHT: f32 = 0.44;
pub const DEFAULT_SEARCH_SNIPPET_WEIGHT: f32 = 0.18;
pub const DEFAULT_SEARCH_KEYWORD_WEIGHT: f32 = 0.38;
pub const DEFAULT_SEARCH_DECAY_FLOOR: f32 = 0.15;
pub const DEFAULT_SEARCH_ABSOLUTE_RELEVANCE_FLOOR: f32 = 0.24;
pub const DEFAULT_SEARCH_RELATIVE_RELEVANCE_FLOOR: f32 = 0.32;
pub const DEFAULT_SEARCH_STRONG_RESULT_FLOOR: f32 = 0.70;
pub const DEFAULT_SEARCH_MEDIUM_RESULT_FLOOR: f32 = 0.60;
pub const DEFAULT_SEARCH_SEMANTIC_TIMEOUT_MS: u64 = 950;
pub const DEFAULT_SEARCH_SNIPPET_TIMEOUT_MS: u64 = 760;
pub const DEFAULT_SEARCH_KEYWORD_TIMEOUT_MS: u64 = 900;
pub const DEFAULT_SEARCH_KEYWORD_VARIANT_TIMEOUT_MS: u64 = 320;

pub const DEFAULT_CAPTURE_FLUSH_INTERVAL_SECS: u64 = 30;
pub const DEFAULT_CAPTURE_MAX_BATCH_SIZE: usize = 100;
pub const DEFAULT_CAPTURE_EMBEDDING_CACHE_SIZE: usize = 256;
pub const DEFAULT_CAPTURE_SEMANTIC_DEDUP_WINDOW_MS: i64 = 90_000;
pub const DEFAULT_CAPTURE_NOISE_SKIP_THRESHOLD: f32 = 0.97;
pub const DEFAULT_CAPTURE_DEEP_IDLE_SECONDS: f64 = 300.0;
pub const DEFAULT_CAPTURE_IDLE_BLEND_SECONDS: f64 = 30.0;
pub const DEFAULT_FOCUS_DRIFT_SIMILARITY_THRESHOLD: f32 = 0.30;
pub const DEFAULT_FOCUS_DRIFT_CAPTURE_COUNT: u32 = 3;

pub const DEFAULT_MEMORY_CARD_MAX_GROUPS: usize = 6;
pub const DEFAULT_MEMORY_CARD_MAX_LLM_GROUPS: usize = 3;
pub const DEFAULT_MEMORY_CARD_MAX_GROUP_SNIPPETS: usize = 6;
pub const DEFAULT_MEMORY_CARD_GROUPING_TIMEOUT_MS: u64 = 350;
pub const DEFAULT_MEMORY_CARD_LLM_TIMEOUT_MS: u64 = 1_500;

pub const DEFAULT_STORE_VECTOR_QUERY_MULTIPLIER: usize = 3;
pub const DEFAULT_STORE_KEYWORD_QUERY_MULTIPLIER: usize = 8;
pub const DEFAULT_STORE_MAX_KEYWORD_SCAN: usize = 600;

pub const DEFAULT_PROACTIVE_INTERVAL_SECS: u64 = 30;
pub const DEFAULT_PROACTIVE_SEEN_RING_CAPACITY: usize = 20;
pub const DEFAULT_PROACTIVE_SEARCH_LIMIT: usize = 5;
pub const DEFAULT_PROACTIVE_LOOKBACK_FILTER: &str = "7d";
pub const DEFAULT_PROACTIVE_SIMILARITY_THRESHOLD: f32 = 0.82;
pub const DEFAULT_PRIMARY_MEMORY_SPECIFICITY_MIN: f32 = 0.60;
pub const DEFAULT_PRIMARY_MEMORY_INTENT_MIN: f32 = 0.55;
pub const DEFAULT_PRIMARY_MEMORY_AGENT_USEFULNESS_MIN: f32 = 0.60;
pub const DEFAULT_PRIMARY_MEMORY_OCR_NOISE_MAX: f32 = 0.50;

/// Local text embedding configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EmbeddingConfig {
    #[serde(default = "default_embedding_model_name")]
    pub model_name: String,
    #[serde(default = "default_embedding_model_filename")]
    pub model_filename: String,
    #[serde(default = "default_embedding_tokenizer_filename")]
    pub tokenizer_filename: String,
    #[serde(default = "default_text_embedding_dim")]
    pub dimension: usize,
    #[serde(default = "default_embedding_max_seq_len")]
    pub max_sequence_length: usize,
    #[serde(default = "default_embedding_cache_capacity")]
    pub cache_capacity: usize,
    #[serde(default = "default_embedding_max_batch")]
    pub max_batch_size: usize,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            model_name: default_embedding_model_name(),
            model_filename: default_embedding_model_filename(),
            tokenizer_filename: default_embedding_tokenizer_filename(),
            dimension: default_text_embedding_dim(),
            max_sequence_length: default_embedding_max_seq_len(),
            cache_capacity: default_embedding_cache_capacity(),
            max_batch_size: default_embedding_max_batch(),
        }
    }
}

impl EmbeddingConfig {
    pub fn normalized(mut self) -> Self {
        self.model_name = self.model_name.trim().to_string();
        if self.model_name.is_empty() {
            self.model_name = default_embedding_model_name();
        }
        self.model_filename = self.model_filename.trim().to_string();
        if self.model_filename.is_empty() {
            self.model_filename = default_embedding_model_filename();
        }
        self.tokenizer_filename = self.tokenizer_filename.trim().to_string();
        if self.tokenizer_filename.is_empty() {
            self.tokenizer_filename = default_embedding_tokenizer_filename();
        }
        self.dimension = self.dimension.clamp(128, 4096);
        self.max_sequence_length = self.max_sequence_length.clamp(16, 1024);
        self.cache_capacity = self.cache_capacity.clamp(64, 16_384);
        self.max_batch_size = self.max_batch_size.clamp(1, 128);
        self
    }
}

/// OCR-aware chunking configuration used before embedding.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChunkingConfig {
    #[serde(default = "default_chunk_max_tokens")]
    pub max_tokens: usize,
    #[serde(default = "default_chunk_overlap_tokens")]
    pub overlap_tokens: usize,
    #[serde(default = "default_chunk_min_tokens")]
    pub min_tokens: usize,
    #[serde(default = "default_chars_per_token")]
    pub chars_per_token: usize,
    #[serde(default = "default_chunk_ocr_target_min_chars")]
    pub ocr_target_min_chars: usize,
    #[serde(default = "default_chunk_ocr_target_max_chars")]
    pub ocr_target_max_chars: usize,
}

impl Default for ChunkingConfig {
    fn default() -> Self {
        Self {
            max_tokens: default_chunk_max_tokens(),
            overlap_tokens: default_chunk_overlap_tokens(),
            min_tokens: default_chunk_min_tokens(),
            chars_per_token: default_chars_per_token(),
            ocr_target_min_chars: default_chunk_ocr_target_min_chars(),
            ocr_target_max_chars: default_chunk_ocr_target_max_chars(),
        }
    }
}

impl ChunkingConfig {
    pub fn normalized(mut self) -> Self {
        self.chars_per_token = self.chars_per_token.clamp(2, 8);
        self.max_tokens = self.max_tokens.clamp(64, 1024);
        self.min_tokens = self.min_tokens.clamp(1, self.max_tokens / 2);
        self.overlap_tokens = self.overlap_tokens.min(self.max_tokens / 2);
        self.ocr_target_min_chars = self.ocr_target_min_chars.clamp(80, 4_000);
        self.ocr_target_max_chars = self
            .ocr_target_max_chars
            .clamp(self.ocr_target_min_chars, 8_000);
        self
    }
}

/// Search and reranking knobs. Stored as primitive values so the TOML remains readable.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchConfig {
    #[serde(default = "default_search_candidate_multiplier")]
    pub candidate_multiplier: usize,
    #[serde(default = "default_search_max_rerank_pool")]
    pub max_rerank_pool: usize,
    #[serde(default = "default_search_max_keyword_variants")]
    pub max_keyword_variants: usize,
    #[serde(default = "default_search_max_keyword_fallback_variants")]
    pub max_keyword_fallback_variants: usize,
    #[serde(default = "default_search_diversity_preserve_top")]
    pub diversity_preserve_top: usize,
    #[serde(default = "default_search_max_semantic_branch_limit")]
    pub max_semantic_branch_limit: usize,
    #[serde(default = "default_search_max_keyword_branch_limit")]
    pub max_keyword_branch_limit: usize,
    #[serde(default = "default_search_min_snippet_query_terms")]
    pub min_snippet_query_terms: usize,
    #[serde(default = "default_search_vector_weight")]
    pub vector_weight: f32,
    #[serde(default = "default_search_snippet_weight")]
    pub snippet_weight: f32,
    #[serde(default = "default_search_keyword_weight")]
    pub keyword_weight: f32,
    #[serde(default = "default_search_decay_floor")]
    pub decay_floor: f32,
    #[serde(default = "default_search_absolute_relevance_floor")]
    pub absolute_relevance_floor: f32,
    #[serde(default = "default_search_relative_relevance_floor")]
    pub relative_relevance_floor: f32,
    #[serde(default = "default_search_strong_result_floor")]
    pub strong_result_floor: f32,
    #[serde(default = "default_search_medium_result_floor")]
    pub medium_result_floor: f32,
    #[serde(default = "default_search_semantic_timeout_ms")]
    pub semantic_timeout_ms: u64,
    #[serde(default = "default_search_snippet_timeout_ms")]
    pub snippet_timeout_ms: u64,
    #[serde(default = "default_search_keyword_timeout_ms")]
    pub keyword_timeout_ms: u64,
    #[serde(default = "default_search_keyword_variant_timeout_ms")]
    pub keyword_variant_timeout_ms: u64,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            candidate_multiplier: default_search_candidate_multiplier(),
            max_rerank_pool: default_search_max_rerank_pool(),
            max_keyword_variants: default_search_max_keyword_variants(),
            max_keyword_fallback_variants: default_search_max_keyword_fallback_variants(),
            diversity_preserve_top: default_search_diversity_preserve_top(),
            max_semantic_branch_limit: default_search_max_semantic_branch_limit(),
            max_keyword_branch_limit: default_search_max_keyword_branch_limit(),
            min_snippet_query_terms: default_search_min_snippet_query_terms(),
            vector_weight: default_search_vector_weight(),
            snippet_weight: default_search_snippet_weight(),
            keyword_weight: default_search_keyword_weight(),
            decay_floor: default_search_decay_floor(),
            absolute_relevance_floor: default_search_absolute_relevance_floor(),
            relative_relevance_floor: default_search_relative_relevance_floor(),
            strong_result_floor: default_search_strong_result_floor(),
            medium_result_floor: default_search_medium_result_floor(),
            semantic_timeout_ms: default_search_semantic_timeout_ms(),
            snippet_timeout_ms: default_search_snippet_timeout_ms(),
            keyword_timeout_ms: default_search_keyword_timeout_ms(),
            keyword_variant_timeout_ms: default_search_keyword_variant_timeout_ms(),
        }
    }
}

impl SearchConfig {
    pub fn normalized(mut self) -> Self {
        self.candidate_multiplier = self.candidate_multiplier.clamp(1, 12);
        self.max_rerank_pool = self.max_rerank_pool.clamp(4, 200);
        self.max_keyword_variants = self.max_keyword_variants.clamp(1, 16);
        self.max_keyword_fallback_variants = self.max_keyword_fallback_variants.clamp(1, 8);
        self.diversity_preserve_top = self.diversity_preserve_top.clamp(0, 12);
        self.max_semantic_branch_limit = self.max_semantic_branch_limit.clamp(1, 500);
        self.max_keyword_branch_limit = self.max_keyword_branch_limit.clamp(1, 500);
        self.min_snippet_query_terms = self.min_snippet_query_terms.clamp(1, 12);
        self.vector_weight = self.vector_weight.clamp(0.0, 1.0);
        self.snippet_weight = self.snippet_weight.clamp(0.0, 1.0);
        self.keyword_weight = self.keyword_weight.clamp(0.0, 1.0);
        self.decay_floor = self.decay_floor.clamp(0.0, 1.0);
        self.absolute_relevance_floor = self.absolute_relevance_floor.clamp(0.0, 1.0);
        self.relative_relevance_floor = self.relative_relevance_floor.clamp(0.0, 1.0);
        self.strong_result_floor = self.strong_result_floor.clamp(0.0, 1.0);
        self.medium_result_floor = self.medium_result_floor.clamp(0.0, 1.0);
        let total = self.vector_weight + self.snippet_weight + self.keyword_weight;
        if total > f32::EPSILON {
            self.vector_weight /= total;
            self.snippet_weight /= total;
            self.keyword_weight /= total;
        }
        self.semantic_timeout_ms = self.semantic_timeout_ms.clamp(100, 10_000);
        self.snippet_timeout_ms = self.snippet_timeout_ms.clamp(100, 10_000);
        self.keyword_timeout_ms = self.keyword_timeout_ms.clamp(100, 10_000);
        self.keyword_variant_timeout_ms = self.keyword_variant_timeout_ms.clamp(50, 5_000);
        self
    }
}

/// Capture-loop batching, dedupe, and focus-drift knobs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CapturePipelineConfig {
    #[serde(default = "default_capture_flush_interval_secs")]
    pub flush_interval_secs: u64,
    #[serde(default = "default_capture_max_batch_size")]
    pub max_batch_size: usize,
    #[serde(default = "default_capture_embedding_cache_size")]
    pub embedding_cache_size: usize,
    #[serde(default = "default_capture_semantic_dedup_window_ms")]
    pub semantic_dedup_window_ms: i64,
    #[serde(default = "default_capture_noise_skip_threshold")]
    pub noise_skip_threshold: f32,
    #[serde(default = "default_capture_deep_idle_seconds")]
    pub deep_idle_seconds: f64,
    #[serde(default = "default_capture_idle_blend_seconds")]
    pub idle_blend_seconds: f64,
    #[serde(default = "default_focus_drift_similarity_threshold")]
    pub focus_drift_similarity_threshold: f32,
    #[serde(default = "default_focus_drift_capture_count")]
    pub focus_drift_capture_count: u32,
}

impl Default for CapturePipelineConfig {
    fn default() -> Self {
        Self {
            flush_interval_secs: default_capture_flush_interval_secs(),
            max_batch_size: default_capture_max_batch_size(),
            embedding_cache_size: default_capture_embedding_cache_size(),
            semantic_dedup_window_ms: default_capture_semantic_dedup_window_ms(),
            noise_skip_threshold: default_capture_noise_skip_threshold(),
            deep_idle_seconds: default_capture_deep_idle_seconds(),
            idle_blend_seconds: default_capture_idle_blend_seconds(),
            focus_drift_similarity_threshold: default_focus_drift_similarity_threshold(),
            focus_drift_capture_count: default_focus_drift_capture_count(),
        }
    }
}

impl CapturePipelineConfig {
    pub fn normalized(mut self) -> Self {
        self.flush_interval_secs = self.flush_interval_secs.clamp(1, 300);
        self.max_batch_size = self.max_batch_size.clamp(1, 1_000);
        self.embedding_cache_size = self.embedding_cache_size.clamp(16, 16_384);
        self.semantic_dedup_window_ms = self.semantic_dedup_window_ms.clamp(1_000, 3_600_000);
        self.noise_skip_threshold = self.noise_skip_threshold.clamp(0.0, 1.0);
        self.deep_idle_seconds = self.deep_idle_seconds.clamp(30.0, 86_400.0);
        self.idle_blend_seconds = self.idle_blend_seconds.clamp(1.0, 3_600.0);
        self.focus_drift_similarity_threshold =
            self.focus_drift_similarity_threshold.clamp(0.0, 1.0);
        self.focus_drift_capture_count = self.focus_drift_capture_count.clamp(1, 60);
        self
    }
}

/// MemoryCard grouping and synthesis knobs used by search surfaces.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryCardConfig {
    #[serde(default = "default_memory_card_max_groups")]
    pub max_groups: usize,
    #[serde(default = "default_memory_card_max_llm_groups")]
    pub max_llm_groups: usize,
    #[serde(default = "default_memory_card_max_group_snippets")]
    pub max_group_snippets: usize,
    #[serde(default = "default_memory_card_grouping_timeout_ms")]
    pub grouping_timeout_ms: u64,
    #[serde(default = "default_memory_card_llm_timeout_ms")]
    pub llm_timeout_ms: u64,
}

impl Default for MemoryCardConfig {
    fn default() -> Self {
        Self {
            max_groups: default_memory_card_max_groups(),
            max_llm_groups: default_memory_card_max_llm_groups(),
            max_group_snippets: default_memory_card_max_group_snippets(),
            grouping_timeout_ms: default_memory_card_grouping_timeout_ms(),
            llm_timeout_ms: default_memory_card_llm_timeout_ms(),
        }
    }
}

impl MemoryCardConfig {
    pub fn normalized(mut self) -> Self {
        self.max_groups = self.max_groups.clamp(1, 24);
        self.max_llm_groups = self.max_llm_groups.min(self.max_groups);
        self.max_group_snippets = self.max_group_snippets.clamp(1, 24);
        self.grouping_timeout_ms = self.grouping_timeout_ms.clamp(50, 5_000);
        self.llm_timeout_ms = self.llm_timeout_ms.clamp(100, 30_000);
        self
    }
}

/// LanceDB retrieval expansion knobs used before application-level reranking.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StoreConfig {
    #[serde(default = "default_store_vector_query_multiplier")]
    pub vector_query_multiplier: usize,
    #[serde(default = "default_store_keyword_query_multiplier")]
    pub keyword_query_multiplier: usize,
    #[serde(default = "default_store_max_keyword_scan")]
    pub max_keyword_scan: usize,
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            vector_query_multiplier: default_store_vector_query_multiplier(),
            keyword_query_multiplier: default_store_keyword_query_multiplier(),
            max_keyword_scan: default_store_max_keyword_scan(),
        }
    }
}

impl StoreConfig {
    pub fn normalized(mut self) -> Self {
        self.vector_query_multiplier = self.vector_query_multiplier.clamp(1, 12);
        self.keyword_query_multiplier = self.keyword_query_multiplier.clamp(1, 32);
        self.max_keyword_scan = self.max_keyword_scan.clamp(50, 10_000);
        self
    }
}

/// Proactive recall surface knobs for background similarity suggestions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProactiveConfig {
    #[serde(default = "default_proactive_interval_secs")]
    pub interval_secs: u64,
    #[serde(default = "default_proactive_seen_ring_capacity")]
    pub seen_ring_capacity: usize,
    #[serde(default = "default_proactive_search_limit")]
    pub search_limit: usize,
    #[serde(default = "default_proactive_lookback_filter")]
    pub lookback_filter: String,
    #[serde(default = "default_proactive_similarity_threshold")]
    pub similarity_threshold: f32,
}

impl Default for ProactiveConfig {
    fn default() -> Self {
        Self {
            interval_secs: default_proactive_interval_secs(),
            seen_ring_capacity: default_proactive_seen_ring_capacity(),
            search_limit: default_proactive_search_limit(),
            lookback_filter: default_proactive_lookback_filter(),
            similarity_threshold: default_proactive_similarity_threshold(),
        }
    }
}

impl ProactiveConfig {
    pub fn normalized(mut self) -> Self {
        self.interval_secs = self.interval_secs.clamp(5, 3_600);
        self.seen_ring_capacity = self.seen_ring_capacity.clamp(1, 200);
        self.search_limit = self.search_limit.clamp(1, 50);
        self.lookback_filter = self.lookback_filter.trim().to_string();
        if self.lookback_filter.is_empty() {
            self.lookback_filter = default_proactive_lookback_filter();
        }
        self.similarity_threshold = self.similarity_threshold.clamp(0.0, 1.0);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryQualityConfig {
    #[serde(default = "default_primary_memory_specificity_min")]
    pub primary_memory_specificity_min: f32,
    #[serde(default = "default_primary_memory_intent_min")]
    pub primary_memory_intent_min: f32,
    #[serde(default = "default_primary_memory_agent_usefulness_min")]
    pub primary_memory_agent_usefulness_min: f32,
    #[serde(default = "default_primary_memory_ocr_noise_max")]
    pub primary_memory_ocr_noise_max: f32,
}

impl Default for MemoryQualityConfig {
    fn default() -> Self {
        Self {
            primary_memory_specificity_min: default_primary_memory_specificity_min(),
            primary_memory_intent_min: default_primary_memory_intent_min(),
            primary_memory_agent_usefulness_min: default_primary_memory_agent_usefulness_min(),
            primary_memory_ocr_noise_max: default_primary_memory_ocr_noise_max(),
        }
    }
}

impl MemoryQualityConfig {
    pub fn normalized(mut self) -> Self {
        self.primary_memory_specificity_min = self.primary_memory_specificity_min.clamp(0.0, 1.0);
        self.primary_memory_intent_min = self.primary_memory_intent_min.clamp(0.0, 1.0);
        self.primary_memory_agent_usefulness_min =
            self.primary_memory_agent_usefulness_min.clamp(0.0, 1.0);
        self.primary_memory_ocr_noise_max = self.primary_memory_ocr_noise_max.clamp(0.0, 1.0);
        self
    }
}

/// Auto-fill configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AutofillConfig {
    /// Whether global screen auto-fill is enabled.
    #[serde(default = "default_autofill_enabled")]
    pub enabled: bool,
    /// Global shortcut in tauri/global-hotkey format, e.g. `Alt+F`.
    #[serde(default = "default_autofill_shortcut")]
    pub shortcut: String,
    /// How far back semantic retrieval should search.
    #[serde(default = "default_autofill_lookback_days")]
    pub lookback_days: u32,
    /// Confidence threshold above which FNDR injects without confirmation.
    #[serde(default = "default_autofill_auto_inject_threshold")]
    pub auto_inject_threshold: f32,
    /// Whether FNDR should prefer system-style typing when the target app remains frontmost.
    #[serde(default = "default_autofill_prefer_typed_injection")]
    pub prefer_typed_injection: bool,
    /// Maximum number of candidates to return for quick-pick conflict resolution.
    #[serde(default = "default_autofill_max_candidates")]
    pub max_candidates: usize,
}

impl Default for AutofillConfig {
    fn default() -> Self {
        Self {
            enabled: default_autofill_enabled(),
            shortcut: default_autofill_shortcut(),
            lookback_days: default_autofill_lookback_days(),
            auto_inject_threshold: default_autofill_auto_inject_threshold(),
            prefer_typed_injection: default_autofill_prefer_typed_injection(),
            max_candidates: default_autofill_max_candidates(),
        }
    }
}

impl AutofillConfig {
    pub fn normalized(mut self) -> Self {
        self.shortcut = self.shortcut.trim().to_string();
        if self.shortcut.is_empty() {
            self.shortcut = default_autofill_shortcut();
        }
        self.lookback_days = self.lookback_days.clamp(7, 365);
        self.auto_inject_threshold = self.auto_inject_threshold.clamp(0.55, 0.995);
        self.max_candidates = self.max_candidates.clamp(1, 6);
        self
    }
}

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Base capture FPS (0.5 - 1.0 recommended)
    pub fps_base: f64,
    /// Seconds of idle before reducing FPS
    pub idle_pause_seconds: u64,
    /// FPS when idle
    pub idle_fps: f64,
    /// Perceptual hash threshold for deduplication (0-64, lower = stricter)
    pub dedupe_threshold: u32,
    /// Force capture every N seconds even if duplicate
    pub forced_capture_interval: u64,
    /// Days to retain records
    pub retention_days: u32,
    /// Blocked application names and website/title patterns
    pub blocklist: Vec<String>,
    /// Sensitive sites/titles the user dismissed and does not want alerted about again.
    #[serde(default)]
    pub dismissed_privacy_alerts: Vec<String>,
    /// Enable pattern redaction (emails, credit cards)
    pub redact_mode: bool,
    /// Minimum text length to store
    pub min_text_length: usize,
    /// Enable VLM for intelligent image understanding
    #[serde(default = "default_use_vlm")]
    pub use_vlm: bool,
    /// VLM model size: "4B" (primary)
    #[serde(default = "default_vlm_model_size")]
    pub vlm_model_size: String,
    /// Days to retain screenshot files on disk (records kept; only pixel data deleted). 0 = keep forever.
    #[serde(default = "default_screenshot_retention_days")]
    pub screenshot_retention_days: u32,
    /// Enable proactive surface: nudges when current screen is semantically close to old unresolved context.
    #[serde(default = "default_proactive_surface_enabled")]
    pub proactive_surface_enabled: bool,
    /// Half-life for Ebbinghaus memory decay in days. Records decay toward 0.15 floor over time.
    #[serde(default = "default_decay_half_life_days")]
    pub decay_half_life_days: u32,
    /// Intelligent Screen Auto-Fill configuration.
    #[serde(default)]
    pub autofill: AutofillConfig,
    /// Authoritative local embedding model contract.
    #[serde(default)]
    pub embedding: EmbeddingConfig,
    /// OCR-aware chunking knobs.
    #[serde(default)]
    pub chunking: ChunkingConfig,
    /// Hybrid search weights, limits, and timeouts.
    #[serde(default)]
    pub search: SearchConfig,
    /// Capture-loop batching, dedupe, and focus-drift settings.
    #[serde(default)]
    pub capture_pipeline: CapturePipelineConfig,
    /// MemoryCard grouping and synthesis settings.
    #[serde(default)]
    pub memory_cards: MemoryCardConfig,
    /// LanceDB retrieval expansion settings.
    #[serde(default)]
    pub store: StoreConfig,
    /// Proactive recall surface settings.
    #[serde(default)]
    pub proactive: ProactiveConfig,
    /// Memory quality thresholds for primary cards and evidence gating.
    #[serde(default)]
    pub memory_quality: MemoryQualityConfig,
}

fn default_embedding_model_name() -> String {
    DEFAULT_EMBEDDING_MODEL_NAME.to_string()
}

fn default_embedding_model_filename() -> String {
    DEFAULT_EMBEDDING_MODEL_FILENAME.to_string()
}

fn default_embedding_tokenizer_filename() -> String {
    DEFAULT_EMBEDDING_TOKENIZER_FILENAME.to_string()
}

fn default_text_embedding_dim() -> usize {
    DEFAULT_TEXT_EMBEDDING_DIM
}

fn default_embedding_max_seq_len() -> usize {
    DEFAULT_EMBEDDING_MAX_SEQ_LEN
}

fn default_embedding_cache_capacity() -> usize {
    DEFAULT_EMBEDDING_CACHE_CAPACITY
}

fn default_embedding_max_batch() -> usize {
    DEFAULT_EMBEDDING_MAX_BATCH
}

fn default_chunk_max_tokens() -> usize {
    DEFAULT_CHUNK_MAX_TOKENS
}

fn default_chunk_overlap_tokens() -> usize {
    DEFAULT_CHUNK_OVERLAP_TOKENS
}

fn default_chunk_min_tokens() -> usize {
    DEFAULT_CHUNK_MIN_TOKENS
}

fn default_chars_per_token() -> usize {
    DEFAULT_CHARS_PER_TOKEN
}

fn default_chunk_ocr_target_min_chars() -> usize {
    DEFAULT_CHUNK_OCR_TARGET_MIN_CHARS
}

fn default_chunk_ocr_target_max_chars() -> usize {
    DEFAULT_CHUNK_OCR_TARGET_MAX_CHARS
}

fn default_search_candidate_multiplier() -> usize {
    DEFAULT_SEARCH_CANDIDATE_MULTIPLIER
}

fn default_search_max_rerank_pool() -> usize {
    DEFAULT_SEARCH_MAX_RERANK_POOL
}

fn default_search_max_keyword_variants() -> usize {
    DEFAULT_SEARCH_MAX_KEYWORD_VARIANTS
}

fn default_search_max_keyword_fallback_variants() -> usize {
    DEFAULT_SEARCH_MAX_KEYWORD_FALLBACK_VARIANTS
}

fn default_search_diversity_preserve_top() -> usize {
    DEFAULT_SEARCH_DIVERSITY_PRESERVE_TOP
}

fn default_search_max_semantic_branch_limit() -> usize {
    DEFAULT_SEARCH_MAX_SEMANTIC_BRANCH_LIMIT
}

fn default_search_max_keyword_branch_limit() -> usize {
    DEFAULT_SEARCH_MAX_KEYWORD_BRANCH_LIMIT
}

fn default_search_min_snippet_query_terms() -> usize {
    DEFAULT_SEARCH_MIN_SNIPPET_QUERY_TERMS
}

fn default_search_vector_weight() -> f32 {
    DEFAULT_SEARCH_VECTOR_WEIGHT
}

fn default_search_snippet_weight() -> f32 {
    DEFAULT_SEARCH_SNIPPET_WEIGHT
}

fn default_search_keyword_weight() -> f32 {
    DEFAULT_SEARCH_KEYWORD_WEIGHT
}

fn default_search_decay_floor() -> f32 {
    DEFAULT_SEARCH_DECAY_FLOOR
}

fn default_search_absolute_relevance_floor() -> f32 {
    DEFAULT_SEARCH_ABSOLUTE_RELEVANCE_FLOOR
}

fn default_search_relative_relevance_floor() -> f32 {
    DEFAULT_SEARCH_RELATIVE_RELEVANCE_FLOOR
}

fn default_search_strong_result_floor() -> f32 {
    DEFAULT_SEARCH_STRONG_RESULT_FLOOR
}

fn default_search_medium_result_floor() -> f32 {
    DEFAULT_SEARCH_MEDIUM_RESULT_FLOOR
}

fn default_search_semantic_timeout_ms() -> u64 {
    DEFAULT_SEARCH_SEMANTIC_TIMEOUT_MS
}

fn default_search_snippet_timeout_ms() -> u64 {
    DEFAULT_SEARCH_SNIPPET_TIMEOUT_MS
}

fn default_search_keyword_timeout_ms() -> u64 {
    DEFAULT_SEARCH_KEYWORD_TIMEOUT_MS
}

fn default_search_keyword_variant_timeout_ms() -> u64 {
    DEFAULT_SEARCH_KEYWORD_VARIANT_TIMEOUT_MS
}

fn default_capture_flush_interval_secs() -> u64 {
    DEFAULT_CAPTURE_FLUSH_INTERVAL_SECS
}

fn default_capture_max_batch_size() -> usize {
    DEFAULT_CAPTURE_MAX_BATCH_SIZE
}

fn default_capture_embedding_cache_size() -> usize {
    DEFAULT_CAPTURE_EMBEDDING_CACHE_SIZE
}

fn default_capture_semantic_dedup_window_ms() -> i64 {
    DEFAULT_CAPTURE_SEMANTIC_DEDUP_WINDOW_MS
}

fn default_capture_noise_skip_threshold() -> f32 {
    DEFAULT_CAPTURE_NOISE_SKIP_THRESHOLD
}

fn default_capture_deep_idle_seconds() -> f64 {
    DEFAULT_CAPTURE_DEEP_IDLE_SECONDS
}

fn default_capture_idle_blend_seconds() -> f64 {
    DEFAULT_CAPTURE_IDLE_BLEND_SECONDS
}

fn default_focus_drift_similarity_threshold() -> f32 {
    DEFAULT_FOCUS_DRIFT_SIMILARITY_THRESHOLD
}

fn default_focus_drift_capture_count() -> u32 {
    DEFAULT_FOCUS_DRIFT_CAPTURE_COUNT
}

fn default_memory_card_max_groups() -> usize {
    DEFAULT_MEMORY_CARD_MAX_GROUPS
}

fn default_memory_card_max_llm_groups() -> usize {
    DEFAULT_MEMORY_CARD_MAX_LLM_GROUPS
}

fn default_memory_card_max_group_snippets() -> usize {
    DEFAULT_MEMORY_CARD_MAX_GROUP_SNIPPETS
}

fn default_memory_card_grouping_timeout_ms() -> u64 {
    DEFAULT_MEMORY_CARD_GROUPING_TIMEOUT_MS
}

fn default_memory_card_llm_timeout_ms() -> u64 {
    DEFAULT_MEMORY_CARD_LLM_TIMEOUT_MS
}

fn default_store_vector_query_multiplier() -> usize {
    DEFAULT_STORE_VECTOR_QUERY_MULTIPLIER
}

fn default_store_keyword_query_multiplier() -> usize {
    DEFAULT_STORE_KEYWORD_QUERY_MULTIPLIER
}

fn default_store_max_keyword_scan() -> usize {
    DEFAULT_STORE_MAX_KEYWORD_SCAN
}

fn default_proactive_interval_secs() -> u64 {
    DEFAULT_PROACTIVE_INTERVAL_SECS
}

fn default_proactive_seen_ring_capacity() -> usize {
    DEFAULT_PROACTIVE_SEEN_RING_CAPACITY
}

fn default_proactive_search_limit() -> usize {
    DEFAULT_PROACTIVE_SEARCH_LIMIT
}

fn default_proactive_lookback_filter() -> String {
    DEFAULT_PROACTIVE_LOOKBACK_FILTER.to_string()
}

fn default_proactive_similarity_threshold() -> f32 {
    DEFAULT_PROACTIVE_SIMILARITY_THRESHOLD
}

fn default_primary_memory_specificity_min() -> f32 {
    DEFAULT_PRIMARY_MEMORY_SPECIFICITY_MIN
}

fn default_primary_memory_intent_min() -> f32 {
    DEFAULT_PRIMARY_MEMORY_INTENT_MIN
}

fn default_primary_memory_agent_usefulness_min() -> f32 {
    DEFAULT_PRIMARY_MEMORY_AGENT_USEFULNESS_MIN
}

fn default_primary_memory_ocr_noise_max() -> f32 {
    DEFAULT_PRIMARY_MEMORY_OCR_NOISE_MAX
}

fn default_use_vlm() -> bool {
    true
}

fn default_vlm_model_size() -> String {
    "4B".to_string()
}

fn default_screenshot_retention_days() -> u32 {
    30
}

fn default_proactive_surface_enabled() -> bool {
    true
}

fn default_decay_half_life_days() -> u32 {
    21
}

fn default_autofill_enabled() -> bool {
    true
}

fn default_autofill_shortcut() -> String {
    "Alt+F".to_string()
}

fn default_autofill_lookback_days() -> u32 {
    90
}

fn default_autofill_auto_inject_threshold() -> f32 {
    0.90
}

fn default_autofill_prefer_typed_injection() -> bool {
    true
}

fn default_autofill_max_candidates() -> usize {
    4
}

impl Default for Config {
    fn default() -> Self {
        Self {
            fps_base: 0.5,
            idle_pause_seconds: 5,
            idle_fps: 0.2,
            dedupe_threshold: 5,
            forced_capture_interval: 60,
            retention_days: 7,
            blocklist: vec![
                "1Password".to_string(),
                "Keychain Access".to_string(),
                "System Preferences".to_string(),
                "System Settings".to_string(),
            ],
            dismissed_privacy_alerts: Vec::new(),
            redact_mode: false,
            min_text_length: 20,
            use_vlm: true,
            vlm_model_size: "4B".to_string(),
            screenshot_retention_days: 30,
            proactive_surface_enabled: true,
            decay_half_life_days: 21,
            autofill: AutofillConfig::default(),
            embedding: EmbeddingConfig::default(),
            chunking: ChunkingConfig::default(),
            search: SearchConfig::default(),
            capture_pipeline: CapturePipelineConfig::default(),
            memory_cards: MemoryCardConfig::default(),
            store: StoreConfig::default(),
            proactive: ProactiveConfig::default(),
            memory_quality: MemoryQualityConfig::default(),
        }
    }
}

impl Config {
    pub fn normalized(mut self) -> Self {
        self.blocklist = dedupe_trimmed(self.blocklist);
        self.dismissed_privacy_alerts = dedupe_trimmed(self.dismissed_privacy_alerts);
        self.autofill = self.autofill.normalized();
        self.embedding = self.embedding.normalized();
        self.chunking = self.chunking.normalized();
        self.search = self.search.normalized();
        self.capture_pipeline = self.capture_pipeline.normalized();
        self.memory_cards = self.memory_cards.normalized();
        self.store = self.store.normalized();
        self.proactive = self.proactive.normalized();
        self.memory_quality = self.memory_quality.normalized();
        self.fps_base = self.fps_base.clamp(0.05, 4.0);
        self.idle_fps = self.idle_fps.clamp(0.02, self.fps_base.max(0.02));
        self.idle_pause_seconds = self.idle_pause_seconds.clamp(1, 3600);
        self.dedupe_threshold = self.dedupe_threshold.min(64);
        self.forced_capture_interval = self.forced_capture_interval.clamp(5, 3600);
        self.retention_days = self.retention_days.min(3650);
        self.min_text_length = self.min_text_length.clamp(1, 2000);
        self.screenshot_retention_days = self.screenshot_retention_days.min(3650);
        self.decay_half_life_days = self.decay_half_life_days.clamp(1, 3650);
        self
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.embedding.dimension == 0 {
            return Err("Embedding dimension must be greater than zero".to_string());
        }
        if self.embedding.dimension != DEFAULT_TEXT_EMBEDDING_DIM {
            return Err(format!(
                "This FNDR build expects {}-dimensional text embeddings, but config.toml sets {}. Change the embedding model, schema, and config together before using a non-default dimension.",
                DEFAULT_TEXT_EMBEDDING_DIM,
                self.embedding.dimension
            ));
        }
        if self.embedding.model_filename.trim().is_empty()
            || self.embedding.tokenizer_filename.trim().is_empty()
        {
            return Err("Embedding model and tokenizer filenames must be configured".to_string());
        }
        if self.search.vector_weight + self.search.snippet_weight + self.search.keyword_weight
            <= f32::EPSILON
        {
            return Err("At least one hybrid search weight must be non-zero".to_string());
        }
        Ok(())
    }

    /// Load configuration from file or create default
    pub fn load_or_create() -> Result<Self, Box<dyn std::error::Error>> {
        let config_path = Self::config_path()?;

        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            let config: Config = toml::from_str(&content)?;
            let config = config.normalized();
            config.validate().map_err(|err| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Invalid FNDR config: {err}"),
                )
            })?;
            Ok(config)
        } else {
            let config = Config::default().normalized();
            config.validate().map_err(|err| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Invalid FNDR config: {err}"),
                )
            })?;
            config.save()?;
            Ok(config)
        }
    }

    /// Save configuration to file
    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let config_path = Self::config_path()?;
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(&config_path, content)?;
        Ok(())
    }

    fn config_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
        let dirs = directories::ProjectDirs::from("com", "fndr", "FNDR")
            .ok_or("Could not determine config directory")?;
        Ok(dirs.config_dir().join("config.toml"))
    }
}

fn dedupe_trimmed(values: Vec<String>) -> Vec<String> {
    let mut deduped = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        if deduped
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(trimmed))
        {
            continue;
        }
        deduped.push(trimmed.to_string());
    }
    deduped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_validates() {
        Config::default()
            .normalized()
            .validate()
            .expect("default config should stay internally consistent");
    }

    #[test]
    fn rejects_embedding_dimension_mismatch() {
        let mut config = Config::default();
        config.embedding.dimension = 384;

        let err = config
            .normalized()
            .validate()
            .expect_err("wrong embedding dimension must fail at startup");

        assert!(err.contains("1024-dimensional text embeddings"));
    }

    #[test]
    fn rejects_zero_search_weights() {
        let mut config = Config::default();
        config.search.vector_weight = 0.0;
        config.search.snippet_weight = 0.0;
        config.search.keyword_weight = 0.0;

        let err = config
            .normalized()
            .validate()
            .expect_err("zeroed hybrid weights should not pass validation");

        assert!(err.contains("hybrid search weight"));
    }

    #[test]
    fn memory_card_defaults_enable_llm_group_synthesis() {
        let config = Config::default().normalized();
        assert!(config.memory_cards.max_llm_groups > 0);
    }
}
