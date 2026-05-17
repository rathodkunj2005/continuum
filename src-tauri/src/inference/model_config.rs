pub const FNDR_MODEL_PROFILE: &str = "m1_8gb_default";

pub const MULTIMODAL_MODEL_REPO: &str = "Qwen/Qwen3-VL-2B-Instruct-GGUF";
pub const MULTIMODAL_MODEL_QUANT: &str = "Q4_K_M";
pub const MULTIMODAL_MODEL_ID: &str = "qwen3-vl-2b";
pub const MULTIMODAL_MODEL_FILENAME: &str = "Qwen3VL-2B-Instruct-Q4_K_M.gguf";
pub const MULTIMODAL_MODEL_DOWNLOAD_URL: &str =
    "https://huggingface.co/Qwen/Qwen3-VL-2B-Instruct-GGUF/resolve/main/Qwen3VL-2B-Instruct-Q4_K_M.gguf";
pub const MULTIMODAL_MODEL_SIZE_BYTES: u64 = 1_500_000_000;
pub const MULTIMODAL_MODEL_RAM_GB: f32 = 3.5;

/// Minimum on-disk size to accept as a real Qwen3-VL-2B GGUF (rejects LFS pointers).
pub const QWEN3_VL_2B_MAIN_GGUF_MIN_BYTES: u64 = 900_000_000;

pub const EMBEDDING_MODEL_ID: &str = "sentence-transformers/all-MiniLM-L6-v2";
pub const EMBEDDING_MODEL_FILENAME: &str = "all-MiniLM-L6-v2.onnx";
pub const EMBEDDING_TOKENIZER_FILENAME: &str = "tokenizer.json";
/// all-MiniLM-L6-v2 produces 384-dimensional sentence embeddings.
pub const EMBEDDING_DIMENSIONS: usize = 384;
pub const EMBEDDING_DIMENSIONS_I32: i32 = EMBEDDING_DIMENSIONS as i32;
pub const EMBEDDING_MAX_SEQ_LEN: usize = 512;

pub const MAX_CONCURRENT_MULTIMODAL_JOBS: usize = 1;
pub const QWEN_IDLE_UNLOAD_SECONDS: u64 = 90;
pub const MAX_IMAGE_LONG_EDGE: u32 = 1024;
pub const MAX_MEMORY_PROMPT_TOKENS: usize = 3500;
pub const MAX_MEMORY_OUTPUT_TOKENS: usize = 900;
pub const QWEN_CONTEXT_SIZE: u32 = 4096;
pub const QWEN_TEMPERATURE: f32 = 0.1;
pub const QWEN_TOP_P: f32 = 0.8;

/// LanceDB table name for memories using EmbeddingGemma 256-dim vectors (read-only sidecar).
pub const MEMORIES_V3_TABLE: &str = "memories_v3_egemma_256";

/// LanceDB table name for memories using all-MiniLM-L6-v2 384-dim vectors (primary table).
pub const MEMORIES_V4_TABLE: &str = "memories_v4_minilm_384";

/// Old model directories to list in cleanup dry-run (not deleted automatically).
pub const CLEANUP_OLD_MODEL_DIRS: &[&str] = &[
    "llama-3.2-1b",
    "smolvlm-500m",
    "qwen3-vl-4b",
    "bge-large-en-v1.5",
];
