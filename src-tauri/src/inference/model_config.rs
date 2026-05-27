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

// ── Embedding contracts (single source of truth) ────────────────────────────
//
// v4 is the current durable runtime contract. v5 is the explicit BGE target
// used by the migration/reindex path. Keeping both contracts named here avoids
// silent model/schema dimension drift while v4 remains readable.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingContractVersion {
    V4MiniLm384,
    V5Bge1024,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextEmbeddingContract {
    pub version: EmbeddingContractVersion,
    pub model_id: &'static str,
    pub model_filename: &'static str,
    pub tokenizer_filename: &'static str,
    pub dimensions: usize,
    pub max_sequence_length: usize,
    pub max_batch_size: usize,
    pub table_name: &'static str,
}

pub const EMBEDDING_MODEL_ID: &str = "sentence-transformers/all-MiniLM-L6-v2";
pub const EMBEDDING_MODEL_FILENAME: &str = "all-MiniLM-L6-v2.onnx";
pub const EMBEDDING_TOKENIZER_FILENAME: &str = "tokenizer.json";
/// all-MiniLM-L6-v2 produces 384-dimensional sentence embeddings.
pub const EMBEDDING_DIMENSIONS: usize = 384;
pub const EMBEDDING_DIMENSIONS_I32: i32 = EMBEDDING_DIMENSIONS as i32;
pub const EMBEDDING_MAX_SEQ_LEN: usize = 512;
pub const EMBEDDING_MAX_BATCH_SIZE: usize = 16;

pub const BGE_V5_MODEL_ID: &str = "BAAI/bge-large-en-v1.5";
pub const BGE_V5_MODEL_FILENAME: &str = "bge-large-en-v1.5-quantized.onnx";
pub const BGE_V5_TOKENIZER_FILENAME: &str = "tokenizer.json";
pub const BGE_V5_DIMENSIONS: usize = 1024;
pub const BGE_V5_DIMENSIONS_I32: i32 = BGE_V5_DIMENSIONS as i32;
pub const BGE_V5_MAX_SEQ_LEN: usize = 512;
/// Keep explicit BGE reindex batches small on the default 8GB-safe profile.
pub const BGE_V5_MAX_BATCH_SIZE: usize = 4;

pub const MAX_CONCURRENT_MULTIMODAL_JOBS: usize = 1;
pub const QWEN_IDLE_UNLOAD_SECONDS: u64 = 90;
pub const MAX_IMAGE_LONG_EDGE: u32 = 1024;
pub const MAX_MEMORY_PROMPT_TOKENS: usize = 3500;
pub const MAX_MEMORY_OUTPUT_TOKENS: usize = 900;
pub const QWEN_CONTEXT_SIZE: u32 = 4096;
pub const QWEN_TEMPERATURE: f32 = 0.1;
pub const QWEN_TOP_P: f32 = 0.8;

/// LanceDB table name for memories using all-MiniLM-L6-v2 384-dim vectors.
/// This is the **current durable write path** for memories. Search, capture,
/// and ingestion all target this table. Re-exported from
/// `lance_store::MEMORIES_TABLE` for legacy callers.
pub const MEMORIES_V4_TABLE: &str = "memories_v4_minilm_384";

/// Forward-intent placeholder for the planned BGE 1024-d migration.
/// **Not wired anywhere yet** — Subagent 6 will add the schema, validation,
/// and write path together. The name is reserved here so doc references stay
/// consistent and no other slice accidentally claims the table name.
pub const MEMORIES_V5_TABLE: &str = "memories_v5_bge_1024";

pub const fn embedding_v4_contract() -> TextEmbeddingContract {
    TextEmbeddingContract {
        version: EmbeddingContractVersion::V4MiniLm384,
        model_id: EMBEDDING_MODEL_ID,
        model_filename: EMBEDDING_MODEL_FILENAME,
        tokenizer_filename: EMBEDDING_TOKENIZER_FILENAME,
        dimensions: EMBEDDING_DIMENSIONS,
        max_sequence_length: EMBEDDING_MAX_SEQ_LEN,
        max_batch_size: EMBEDDING_MAX_BATCH_SIZE,
        table_name: MEMORIES_V4_TABLE,
    }
}

pub const fn embedding_v5_contract() -> TextEmbeddingContract {
    TextEmbeddingContract {
        version: EmbeddingContractVersion::V5Bge1024,
        model_id: BGE_V5_MODEL_ID,
        model_filename: BGE_V5_MODEL_FILENAME,
        tokenizer_filename: BGE_V5_TOKENIZER_FILENAME,
        dimensions: BGE_V5_DIMENSIONS,
        max_sequence_length: BGE_V5_MAX_SEQ_LEN,
        max_batch_size: BGE_V5_MAX_BATCH_SIZE,
        table_name: MEMORIES_V5_TABLE,
    }
}

pub const fn active_embedding_contract() -> TextEmbeddingContract {
    embedding_v4_contract()
}

pub fn validate_embedding_config_against_contract(
    config: &crate::config::EmbeddingConfig,
    contract: TextEmbeddingContract,
) -> Result<(), String> {
    if config.dimension != contract.dimensions {
        return Err(format!(
            "Embedding contract drift for {}: config dimension is {}, but contract expects {}.",
            contract.table_name, config.dimension, contract.dimensions
        ));
    }
    if config.model_filename != contract.model_filename {
        return Err(format!(
            "Embedding contract drift for {}: config model_filename is '{}', but contract expects '{}'.",
            contract.table_name, config.model_filename, contract.model_filename
        ));
    }
    if config.tokenizer_filename != contract.tokenizer_filename {
        return Err(format!(
            "Embedding contract drift for {}: config tokenizer_filename is '{}', but contract expects '{}'.",
            contract.table_name, config.tokenizer_filename, contract.tokenizer_filename
        ));
    }
    if config.model_name != contract.model_id {
        return Err(format!(
            "Embedding contract drift for {}: config model_name is '{}', but contract expects '{}'.",
            contract.table_name, config.model_name, contract.model_id
        ));
    }
    Ok(())
}

/// Old model directories to list in cleanup dry-run (not deleted automatically).
pub const CLEANUP_OLD_MODEL_DIRS: &[&str] = &[
    "llama-3.2-1b",
    "smolvlm-500m",
    "qwen3-vl-4b",
    "bge-large-en-v1.5",
    "embeddinggemma-300m",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        DEFAULT_EMBEDDING_MAX_SEQ_LEN, DEFAULT_EMBEDDING_MODEL_FILENAME,
        DEFAULT_EMBEDDING_MODEL_NAME, DEFAULT_EMBEDDING_TOKENIZER_FILENAME,
        DEFAULT_TEXT_EMBEDDING_DIM,
    };

    #[test]
    fn embedding_contract_constants_are_internally_consistent() {
        // model identity
        assert_eq!(EMBEDDING_MODEL_ID, DEFAULT_EMBEDDING_MODEL_NAME);
        assert_eq!(EMBEDDING_MODEL_FILENAME, DEFAULT_EMBEDDING_MODEL_FILENAME);
        assert_eq!(
            EMBEDDING_TOKENIZER_FILENAME,
            DEFAULT_EMBEDDING_TOKENIZER_FILENAME
        );

        // vector dimension
        assert_eq!(EMBEDDING_DIMENSIONS, DEFAULT_TEXT_EMBEDDING_DIM);
        assert_eq!(EMBEDDING_DIMENSIONS_I32, EMBEDDING_DIMENSIONS as i32);
        assert_eq!(EMBEDDING_DIMENSIONS, 384, "v4 MiniLM contract is 384-d");

        // model file matches the model name (MiniLM stem, not BGE / EmbeddingGemma)
        assert!(
            EMBEDDING_MODEL_FILENAME.contains("MiniLM"),
            "filename {EMBEDDING_MODEL_FILENAME} does not match MiniLM model id {EMBEDDING_MODEL_ID}"
        );

        // sequence length
        assert_eq!(EMBEDDING_MAX_SEQ_LEN, DEFAULT_EMBEDDING_MAX_SEQ_LEN);

        // current durable Lance table reflects the model+dim contract
        assert_eq!(MEMORIES_V4_TABLE, "memories_v4_minilm_384");
        assert!(
            MEMORIES_V4_TABLE.contains("minilm")
                && MEMORIES_V4_TABLE.contains(&EMBEDDING_DIMENSIONS.to_string()),
            "v4 table name {MEMORIES_V4_TABLE} must mention model + dim"
        );

        // v5 is a forward placeholder only — it must NOT collide with v4
        assert_ne!(MEMORIES_V5_TABLE, MEMORIES_V4_TABLE);
        assert!(
            MEMORIES_V5_TABLE.contains("v5"),
            "v5 placeholder {MEMORIES_V5_TABLE} should be tagged v5"
        );

        // storage::MEMORIES_TABLE re-exports the same write target
        assert_eq!(crate::storage::MEMORIES_TABLE, MEMORIES_V4_TABLE);
    }

    #[test]
    fn bge_v5_contract_is_explicit_and_separate_from_v4() {
        let v4 = embedding_v4_contract();
        let v5 = embedding_v5_contract();

        assert_eq!(v4.table_name, MEMORIES_V4_TABLE);
        assert_eq!(v4.dimensions, 384);
        assert_eq!(v5.table_name, MEMORIES_V5_TABLE);
        assert_eq!(v5.dimensions, 1024);
        assert_eq!(v5.model_filename, "bge-large-en-v1.5-quantized.onnx");
        assert_eq!(v5.tokenizer_filename, "tokenizer.json");
        assert_ne!(v4.model_id, v5.model_id);
        assert_ne!(v4.table_name, v5.table_name);
    }

    #[test]
    fn contract_validation_rejects_dimension_and_asset_drift() {
        let v5 = embedding_v5_contract();
        let bad_dimension = crate::config::EmbeddingConfig {
            model_name: v5.model_id.to_string(),
            model_filename: v5.model_filename.to_string(),
            tokenizer_filename: v5.tokenizer_filename.to_string(),
            dimension: 384,
            max_sequence_length: v5.max_sequence_length,
            cache_capacity: 128,
            max_batch_size: 1,
        };

        let err = validate_embedding_config_against_contract(&bad_dimension, v5)
            .expect_err("384-d config must not validate against v5");
        assert!(err.contains("1024"));

        let bad_model = crate::config::EmbeddingConfig {
            dimension: v5.dimensions,
            model_filename: "all-MiniLM-L6-v2.onnx".to_string(),
            ..bad_dimension
        };
        let err = validate_embedding_config_against_contract(&bad_model, v5)
            .expect_err("MiniLM file must not validate against BGE v5");
        assert!(err.contains("model_filename"));
    }
}
