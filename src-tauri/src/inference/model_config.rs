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
    /// EmbeddingGemma (VS-47). Defined so the embedder can be checked against
    /// the reference implementation; not the active contract (VS-49 migrates).
    V6EmbeddingGemma,
    /// EmbeddingGemma 2 (Q8_0 GGUF) through `fndr-eg2-helper`, the same
    /// pipeline DigUp runs. The active contract.
    V7EmbeddingGemma2,
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

/// Pinned download source for the v4 MiniLM contract, revision-locked so the
/// artifact cannot drift under the sha256 pins below.
pub const EMBEDDING_MODEL_DOWNLOAD_URL: &str = "https://huggingface.co/Xenova/all-MiniLM-L6-v2/resolve/751bff37182d3f1213fa05d7196b954e230abad9/onnx/model.onnx";
pub const EMBEDDING_MODEL_SHA256: &str =
    "759c3cd2b7fe7e93933ad23c4c9181b7396442a2ed746ec7c1d46192c469c46e";
pub const EMBEDDING_MODEL_SIZE_BYTES: u64 = 90_387_606;
pub const EMBEDDING_TOKENIZER_DOWNLOAD_URL: &str = "https://huggingface.co/Xenova/all-MiniLM-L6-v2/resolve/751bff37182d3f1213fa05d7196b954e230abad9/tokenizer.json";
pub const EMBEDDING_TOKENIZER_SHA256: &str =
    "da0e79933b9ed51798a3ae27893d3c5fa4a201126cef75586296df9b4d2c62a0";
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

pub const EMBEDDING_GEMMA_MODEL_ID: &str = "google/embeddinggemma-300m";
/// The onnx-community export (revision 5090578d9565bb06545b4552f76e6bc2c93e4a66).
/// Its `sentence_embedding` output applies the model's mean pooling, its two
/// dense layers, and normalization; `model.onnx_data` must sit beside it.
pub const EMBEDDING_GEMMA_MODEL_FILENAME: &str = "model.onnx";
pub const EMBEDDING_GEMMA_TOKENIZER_FILENAME: &str = "tokenizer.json";
/// The model's full size; 512, 256, and 128 are Matryoshka truncations.
pub const EMBEDDING_GEMMA_FULL_DIMENSIONS: usize = 768;
pub const EMBEDDING_GEMMA_MAX_SEQ_LEN: usize = 2048;
pub const EMBEDDING_GEMMA_MAX_BATCH_SIZE: usize = 4;

/// ggml-org's Q8_0 GGUF of google/embeddinggemma-2, the file DigUp runs, pinned
/// to one repository revision. Never the F16 files: the model's activations
/// overflow float16.
pub const EMBEDDING_GEMMA2_MODEL_ID: &str = "google/embeddinggemma-2";
pub const EMBEDDING_GEMMA2_MODEL_FILENAME: &str = "embeddinggemma-2-Q8_0.gguf";
pub const EMBEDDING_GEMMA2_MODEL_DOWNLOAD_URL: &str = "https://huggingface.co/ggml-org/embeddinggemma-2-GGUF/resolve/bfcd298762cc34d0357ece5ebdd31791a3a374d8/embeddinggemma-2-Q8_0.gguf";
pub const EMBEDDING_GEMMA2_MODEL_SHA256: &str =
    "2188ac1deca4b77dffefd603c2776a9d76d9d74ec01841392982ebb840b09135";
pub const EMBEDDING_GEMMA2_MODEL_SIZE_BYTES: u64 = 309_855_456;
pub const EMBEDDING_GEMMA2_DIMENSIONS: usize = 768;
/// The helper's context: one input holds at most this many tokens.
pub const EMBEDDING_GEMMA2_MAX_SEQ_LEN: usize = 2048;
/// The helper embeds up to 16 inputs per call.
pub const EMBEDDING_GEMMA2_MAX_BATCH_SIZE: usize = 16;

pub const QWEN_CONTEXT_SIZE: u32 = 4096;

/// Default context window (prompt plus output) of the text engine; `FNDR_INFERENCE_N_CTX` overrides it.
/// 4,096 costs 448 MiB of KV cache for Qwen3-VL-2B (224 MiB at 2,048); see docs/evidence/W01/LRN-01.md (F3, F10).
pub const TEXT_ENGINE_DEFAULT_N_CTX: u32 = 4096;
/// Generation cap for `extract_structured_memory` and its repair pass. Complete answers measured 316 to 573
/// tokens; a cut-off answer has no closing brace and is discarded.
pub const EXTRACTION_MAX_OUTPUT_TOKENS: i32 = 640;

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

/// LanceDB table for memories embedded by EmbeddingGemma 2 (768-d). The
/// **current durable write path**; the v4 table is left untouched beside it.
pub const MEMORIES_V7_TABLE: &str = "memories_v7_embeddinggemma2_768";

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

/// EmbeddingGemma at `dimensions`: 768 (full) or a Matryoshka truncation
/// (512, 256, 128), which the embedder cuts and renormalizes (VS-47).
/// Unsupported dimensions are rejected so different vector sizes cannot claim
/// the same table identity.
pub const fn embedding_v6_contract(
    dimensions: usize,
) -> Result<TextEmbeddingContract, &'static str> {
    Ok(TextEmbeddingContract {
        version: EmbeddingContractVersion::V6EmbeddingGemma,
        model_id: EMBEDDING_GEMMA_MODEL_ID,
        model_filename: EMBEDDING_GEMMA_MODEL_FILENAME,
        tokenizer_filename: EMBEDDING_GEMMA_TOKENIZER_FILENAME,
        dimensions,
        max_sequence_length: EMBEDDING_GEMMA_MAX_SEQ_LEN,
        max_batch_size: EMBEDDING_GEMMA_MAX_BATCH_SIZE,
        table_name: match dimensions {
            768 => "memories_v6_embeddinggemma_768",
            512 => "memories_v6_embeddinggemma_512",
            256 => "memories_v6_embeddinggemma_256",
            128 => "memories_v6_embeddinggemma_128",
            _ => return Err("EmbeddingGemma dimensions must be one of 128, 256, 512, or 768"),
        },
    })
}

pub const fn embedding_v7_contract() -> TextEmbeddingContract {
    TextEmbeddingContract {
        version: EmbeddingContractVersion::V7EmbeddingGemma2,
        model_id: EMBEDDING_GEMMA2_MODEL_ID,
        model_filename: EMBEDDING_GEMMA2_MODEL_FILENAME,
        // The GGUF carries its own tokenizer.
        tokenizer_filename: EMBEDDING_GEMMA2_MODEL_FILENAME,
        dimensions: EMBEDDING_GEMMA2_DIMENSIONS,
        max_sequence_length: EMBEDDING_GEMMA2_MAX_SEQ_LEN,
        max_batch_size: EMBEDDING_GEMMA2_MAX_BATCH_SIZE,
        table_name: MEMORIES_V7_TABLE,
    }
}

impl TextEmbeddingContract {
    /// Whether the model was trained so that its vectors can be cut to fewer
    /// dimensions and renormalized (Matryoshka Representation Learning).
    pub const fn supports_truncation(&self) -> bool {
        matches!(self.version, EmbeddingContractVersion::V6EmbeddingGemma)
    }
}

pub const fn active_embedding_contract() -> TextEmbeddingContract {
    embedding_v7_contract()
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
        let active = active_embedding_contract();

        // config defaults are the active contract
        assert_eq!(active.model_id, DEFAULT_EMBEDDING_MODEL_NAME);
        assert_eq!(active.model_filename, DEFAULT_EMBEDDING_MODEL_FILENAME);
        assert_eq!(
            active.tokenizer_filename,
            DEFAULT_EMBEDDING_TOKENIZER_FILENAME
        );
        assert_eq!(active.dimensions, DEFAULT_TEXT_EMBEDDING_DIM);
        assert_eq!(active.max_sequence_length, DEFAULT_EMBEDDING_MAX_SEQ_LEN);
        assert_eq!(active.dimensions, 768, "EmbeddingGemma 2 is 768-d");

        // the v4 constants still describe the MiniLM contract
        assert_eq!(EMBEDDING_DIMENSIONS_I32, EMBEDDING_DIMENSIONS as i32);
        assert_eq!(EMBEDDING_DIMENSIONS, 384, "v4 MiniLM contract is 384-d");
        assert!(
            EMBEDDING_MODEL_FILENAME.contains("MiniLM"),
            "filename {EMBEDDING_MODEL_FILENAME} does not match MiniLM model id {EMBEDDING_MODEL_ID}"
        );
        assert_eq!(MEMORIES_V4_TABLE, "memories_v4_minilm_384");

        // every table name is its own
        assert_ne!(MEMORIES_V5_TABLE, MEMORIES_V4_TABLE);
        assert_ne!(MEMORIES_V7_TABLE, MEMORIES_V4_TABLE);
        assert!(
            MEMORIES_V7_TABLE.contains("embeddinggemma2")
                && MEMORIES_V7_TABLE.contains(&EMBEDDING_GEMMA2_DIMENSIONS.to_string()),
            "v7 table name {MEMORIES_V7_TABLE} must mention model + dim"
        );

        // storage::MEMORIES_TABLE re-exports the active write target
        assert_eq!(crate::storage::MEMORIES_TABLE, active.table_name);
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
    fn embeddinggemma_contract_rejects_unsupported_dimensions() {
        for dimensions in [0, 1, 127, 129, 384, 769, usize::MAX] {
            assert!(
                embedding_v6_contract(dimensions).is_err(),
                "unsupported dimension {dimensions} must not claim an EmbeddingGemma table"
            );
        }
    }

    #[test]
    fn embeddinggemma_contract_preserves_supported_table_identities() {
        for (dimensions, table_name) in [
            (128, "memories_v6_embeddinggemma_128"),
            (256, "memories_v6_embeddinggemma_256"),
            (512, "memories_v6_embeddinggemma_512"),
            (768, "memories_v6_embeddinggemma_768"),
        ] {
            let contract = embedding_v6_contract(dimensions).expect("supported dimension");
            assert_eq!(contract.dimensions, dimensions);
            assert_eq!(contract.table_name, table_name);
            assert_eq!(contract.model_id, EMBEDDING_GEMMA_MODEL_ID);
        }
        assert_eq!(active_embedding_contract(), embedding_v7_contract());
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

    #[test]
    fn extraction_output_cap_exceeds_measured_answer_lengths() {
        // Eight synthetic captures of different kinds produced complete answers of 316 to 573 tokens
        // (median about 426). A cut-off answer has no closing brace and is discarded, so the cap must
        // sit above the longest natural answer with some headroom.
        const LONGEST_MEASURED_ANSWER_TOKENS: i32 = 573;
        assert!(
            EXTRACTION_MAX_OUTPUT_TOKENS >= LONGEST_MEASURED_ANSWER_TOKENS + 50,
            "cap {EXTRACTION_MAX_OUTPUT_TOKENS} is too close to or below the longest measured answer"
        );
    }
}
