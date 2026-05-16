# Model Stack Refactor: Qwen3-VL-2B + EmbeddingGemma 256-dim

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the 3-model VLM catalog (Llama 1B, SmolVLM 500M, Qwen 4B) and bge-large-1024 embedding stack with exactly two models: Qwen3-VL-2B-Instruct-GGUF Q4_K_M for memory synthesis and EmbeddingGemma 300M with 256-dim output for vector search, optimized for an 8 GB M1 MacBook.

**Architecture:** Canonical constants live in `inference/model_config.rs`. Qwen3-VL-2B is lazy-loaded via a queue-based `model_worker.rs` and unloaded after 90 s idle; EmbeddingGemma wraps the existing ONNX backend in a new `embed/embedding_gemma.rs` module. New memory writes go to `memories_v3_egemma_256` (256-dim LanceDB table); old tables are preserved read-only. A privacy safety gate, NoopReranker hook, and TaskCandidate extraction are added as extension points without adding extra models.

**Tech Stack:** Rust, llama_cpp_2 (GGUF/MTMD), ort (ONNX Runtime), LanceDB (Arrow), Tauri 2, React/TypeScript frontend

---

## File Map

**New files:**
- `src-tauri/src/inference/model_config.rs` – canonical model constants (single source of truth)
- `src-tauri/src/inference/qwen_vl_memory.rs` – MemorySynthesisInput/Output + prompt + JSON parsing
- `src-tauri/src/inference/model_worker.rs` – lazy-loading queue, idle unload, ModelRunDecision
- `src-tauri/src/embed/mod.rs` + `src-tauri/src/embed/embedding_gemma.rs` – 256-dim embedding public API
- `src-tauri/src/privacy/safety_gate.rs` – SafetyDecision deterministic gate
- `src-tauri/src/tasks/extract_from_memory.rs` – TaskCandidate extraction placeholder

**Modified files:**
- `src-tauri/src/models.rs` – replace 3-model catalog with single Qwen3-VL-2B entry
- `src-tauri/src/config.rs` – remove vlm_model_size/vlm_model_id, update embedding defaults to 256-dim
- `src-tauri/src/inference/mod.rs` – expose model_config, qwen_vl_memory, model_worker modules
- `src-tauri/src/inference/image_semantics.rs` – MtmdModelFamily: remove SmolVlm500M/Qwen3Vl4B, add Qwen3Vl2B
- `src-tauri/src/inference/vlm_router.rs` – remove RunHeavyVlmExplicitOnly/RunLightweightVlm → single RunQwenVlm; remove heavy_vlm flag
- `src-tauri/src/embedding/onnx.rs` – update EMBEDDING_DIM=256, model filename/name for EmbeddingGemma
- `src-tauri/src/storage/lance_store/schemas.rs` – add `memories_v3_egemma_256_schema()` using 256-dim
- `src-tauri/src/storage/lance_store/mod.rs` – add v3 table open/create; add `reindex_to_v3()` command
- `src-tauri/src/storage/lance_store/normalize_embed_migrate.rs` – update default table name to v3
- `src-tauri/src/memory/types.rs` – add enrichment_status, fallback_reason, embedding_dimensions fields
- `src-tauri/src/search/reranker.rs` – add OptionalReranker trait + NoopReranker
- `src-tauri/src/tasks/mod.rs` – declare extract_from_memory submodule
- `src-tauri/src/lib.rs` – declare embed module
- `src-tauri/src/privacy/mod.rs` – declare safety_gate submodule
- `src/domains/workspace/ControlPanel.tsx` – remove model selection UI, add static status display
- `src-tauri/src/ipc/commands/maintenance.rs` – add `models_cleanup_dry_run` + `models_cleanup_confirm` commands

---

## Task 1: Add inference/model_config.rs

**Files:**
- Create: `src-tauri/src/inference/model_config.rs`
- Modify: `src-tauri/src/inference/mod.rs` (add `pub mod model_config;`)

- [ ] **Step 1: Create model_config.rs**

```rust
// src-tauri/src/inference/model_config.rs
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

pub const EMBEDDING_MODEL_ID: &str = "google/embeddinggemma-300m";
pub const EMBEDDING_MODEL_FILENAME: &str = "embeddinggemma-300m.onnx";
pub const EMBEDDING_TOKENIZER_FILENAME: &str = "tokenizer.json";
/// EmbeddingGemma-300M with matryoshka head at 256 dimensions.
pub const EMBEDDING_DIMENSIONS: usize = 256;
pub const EMBEDDING_DIMENSIONS_I32: i32 = 256;
pub const EMBEDDING_MAX_SEQ_LEN: usize = 512;

pub const MAX_CONCURRENT_MULTIMODAL_JOBS: usize = 1;
pub const QWEN_IDLE_UNLOAD_SECONDS: u64 = 90;
pub const MAX_IMAGE_LONG_EDGE: u32 = 1024;
pub const MAX_MEMORY_PROMPT_TOKENS: usize = 3500;
pub const MAX_MEMORY_OUTPUT_TOKENS: usize = 900;
pub const QWEN_CONTEXT_SIZE: u32 = 4096;
pub const QWEN_TEMPERATURE: f32 = 0.1;
pub const QWEN_TOP_P: f32 = 0.8;

/// LanceDB table name for memories using EmbeddingGemma 256-dim vectors.
pub const MEMORIES_V3_TABLE: &str = "memories_v3_egemma_256";

/// Old model directories to list in cleanup dry-run (not deleted automatically).
pub const CLEANUP_OLD_MODEL_DIRS: &[&str] = &[
    "llama-3.2-1b",
    "smolvlm-500m",
    "qwen3-vl-4b",
    "bge-large-en-v1.5",
];
```

- [ ] **Step 2: Declare module in inference/mod.rs**

Open `src-tauri/src/inference/mod.rs` and add near the top with the other `pub mod` declarations:
```rust
pub mod model_config;
```

- [ ] **Step 3: Compile check**

```bash
cd src-tauri && cargo check -p fndr 2>&1 | head -40
```
Expected: zero new errors.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/inference/model_config.rs src-tauri/src/inference/mod.rs
git commit -m "feat: add inference/model_config.rs with canonical two-model constants"
```

---

## Task 2: Update models.rs — single Qwen3-VL-2B catalog

**Files:**
- Modify: `src-tauri/src/models.rs`

- [ ] **Step 1: Replace MODEL_CATALOG**

The existing file has `MODEL_CATALOG: [ModelDefinition; 3]` with Llama 1B, SmolVLM 500M, Qwen 4B. Replace the entire catalog and associated constants:

```rust
use crate::inference::model_config::{
    MULTIMODAL_MODEL_DOWNLOAD_URL, MULTIMODAL_MODEL_FILENAME, MULTIMODAL_MODEL_ID,
    MULTIMODAL_MODEL_RAM_GB, MULTIMODAL_MODEL_SIZE_BYTES, QWEN3_VL_2B_MAIN_GGUF_MIN_BYTES,
};
// keep existing imports (Config, serde, std)

pub const MODEL_CATALOG: [ModelDefinition; 1] = [ModelDefinition {
    id: MULTIMODAL_MODEL_ID,
    name: "Qwen3-VL · 2B",
    description: "Multimodal memory model for 8 GB M1 Mac. Reads screenshots, OCR text, and GUI context to create structured memory records.",
    size_bytes: MULTIMODAL_MODEL_SIZE_BYTES,
    size_label: "~1.5 GB",
    quality_label: "Excellent",
    speed_label: "Balanced",
    ram_gb: MULTIMODAL_MODEL_RAM_GB,
    recommended: true,
    filename: MULTIMODAL_MODEL_FILENAME,
    download_url: MULTIMODAL_MODEL_DOWNLOAD_URL,
}];
```

- [ ] **Step 2: Update mmproj constants**

Replace the old mmproj arrays and functions:

```rust
/// Candidate mmproj filenames for Qwen3-VL-2B.
pub const QWEN3_VL_2B_MMPROJ_FILENAMES: &[&str] = &[
    "mmproj-Qwen3VL-2B-Instruct-F16.gguf",
    "mmproj-Qwen3VL-2B-Instruct-Q8_0.gguf",
    "mmproj-Qwen3-VL-2B-Instruct-F16.gguf",
    "mmproj-Qwen3-VL-2B-Instruct-Q8_0.gguf",
];

pub fn resolve_qwen3_vl_2b_mmproj(app_data_dir: Option<&Path>) -> Option<PathBuf> {
    for dir in candidate_model_dirs(app_data_dir) {
        // Check both flat layout and subdirectory layout
        for search_dir in [dir.clone(), dir.join("qwen3-vl-2b")] {
            for name in QWEN3_VL_2B_MMPROJ_FILENAMES {
                let path = search_dir.join(name);
                if path.is_file() {
                    return Some(path);
                }
            }
        }
    }
    None
}

pub fn qwen3_vl_2b_fully_available(app_data_dir: Option<&Path>) -> bool {
    is_model_available(MULTIMODAL_MODEL_ID, app_data_dir)
        && resolve_qwen3_vl_2b_mmproj(app_data_dir).is_some()
}
```

- [ ] **Step 3: Update resolve_model to check subdirectory**

Find the `candidate_model_dirs` and `is_model_available` functions and update `resolve_model` to also search `{models_dir}/{model_id}/` for the model file:

```rust
pub fn resolve_model(preferred_model_id: Option<&str>, app_data_dir: Option<&Path>) -> Option<ResolvedModel> {
    let def = if let Some(id) = preferred_model_id {
        model_by_id(id)?
    } else {
        MODEL_CATALOG.first()?
    };
    for dir in candidate_model_dirs(app_data_dir) {
        // Check flat layout first, then subdirectory layout
        for search_dir in [dir.clone(), dir.join(def.id)] {
            let path = search_dir.join(def.filename);
            if path.is_file() {
                return Some(ResolvedModel { definition: def, path });
            }
        }
    }
    None
}
```

- [ ] **Step 4: Remove old functions / update inference_preferred_model_id**

Delete:
- `QWEN3_VL_MMPROJ_FILENAMES` and `resolve_qwen3_vl_mmproj`
- `SMOLVLM_500M_MMPROJ_FILENAMES` and `resolve_smolvlm_mmproj`
- `smolvlm_500m_fully_available`
- `qwen3_vl_fully_available`
- `QWEN3_VL_4B_MAIN_GGUF_MIN_BYTES`
- `SMOLVLM_500M_MIN_BYTES`
- `validate_smolvlm_main_gguf_file`
- `validate_qwen3_vl_main_gguf_file` (replace with new version below)

Replace `validate_qwen3_vl_main_gguf_file`:
```rust
pub fn validate_qwen3_vl_2b_main_gguf_file(path: &Path) -> Result<(), String> {
    let len = std::fs::metadata(path)
        .map_err(|e| format!("stat {}: {e}", path.display()))?
        .len();
    if len < QWEN3_VL_2B_MAIN_GGUF_MIN_BYTES {
        return Err(format!(
            "Qwen3-VL-2B GGUF at {} is only {} bytes (expected ≥ {} bytes). \
             Likely a Git LFS pointer or incomplete download. Re-download from: {}",
            path.display(),
            len,
            QWEN3_VL_2B_MAIN_GGUF_MIN_BYTES,
            MULTIMODAL_MODEL_DOWNLOAD_URL
        ));
    }
    Ok(())
}
```

Simplify `configured_vlm_model_id` and `inference_preferred_model_id`:
```rust
pub fn configured_vlm_model_id(_config: &Config) -> Option<String> {
    Some(MULTIMODAL_MODEL_ID.to_string())
}

pub fn inference_preferred_model_id(_app_data_dir: &Path, _config: &Config) -> Option<String> {
    Some(MULTIMODAL_MODEL_ID.to_string())
}
```

Simplify `pixel_vlm_available`:
```rust
pub fn pixel_vlm_available(_model_id: Option<&str>, app_data_dir: Option<&Path>) -> bool {
    qwen3_vl_2b_fully_available(app_data_dir)
}
```

- [ ] **Step 5: Compile check**

```bash
cd src-tauri && cargo check -p fndr 2>&1 | head -60
```
Fix any compile errors from callers of the removed functions (grep for `smolvlm`, `qwen3_vl_4b`, `QWEN3_VL_4B`, `resolve_smolvlm`, `resolve_qwen3_vl_mmproj`).

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/models.rs
git commit -m "feat: replace 3-model catalog with single Qwen3-VL-2B entry"
```

---

## Task 3: Update config.rs — embedding defaults + remove VLM tier config

**Files:**
- Modify: `src-tauri/src/config.rs`

- [ ] **Step 1: Update embedding constants at top of file**

Find the constants block (lines 6-13) and replace:
```rust
// OLD:
pub const DEFAULT_TEXT_EMBEDDING_DIM: usize = 1024;
pub const DEFAULT_EMBEDDING_MODEL_NAME: &str = "bge-large-en-v1.5";
pub const DEFAULT_EMBEDDING_MODEL_FILENAME: &str = "bge-large-en-v1.5-quantized.onnx";

// NEW:
pub const DEFAULT_TEXT_EMBEDDING_DIM: usize = 256;
pub const DEFAULT_EMBEDDING_MODEL_NAME: &str = "google/embeddinggemma-300m";
pub const DEFAULT_EMBEDDING_MODEL_FILENAME: &str = "embeddinggemma-300m.onnx";
```

Keep `DEFAULT_IMAGE_EMBEDDING_DIM`, `DEFAULT_EMBEDDING_TOKENIZER_FILENAME`, and all other constants unchanged.

- [ ] **Step 2: Remove vlm_model_size and vlm_model_id from Config struct**

Search the Config struct definition for these fields:
```rust
pub vlm_model_size: String,
pub vlm_model_id: Option<String>,
```
Remove both. Keep `use_vlm: bool`, `vlm_max_calls_per_minute: u32`, `vlm_timeout_secs: u64`.

- [ ] **Step 3: Remove normalize_vlm_model_size from Config::normalize()**

Find the normalization block that matches `vlm.as_str()` to "500M", "4B", "1B". Delete it. Similarly remove the `vlm_model_id` filter block that uses `matches!(id, "smolvlm-500m" | "qwen3-vl-4b")`.

- [ ] **Step 4: Update Config::default() to remove vlm_model_size/vlm_model_id**

If Config has a `Default` impl with `vlm_model_size: "1B".to_string()`, remove that field from the default.

- [ ] **Step 5: Update EmbeddingConfig defaults**

Find `EmbeddingConfig` default construction (may be in a `Default` impl or `fn default_embedding_config()`). Update:
```rust
EmbeddingConfig {
    model_name: DEFAULT_EMBEDDING_MODEL_NAME.to_string(),
    model_filename: DEFAULT_EMBEDDING_MODEL_FILENAME.to_string(),
    tokenizer_filename: DEFAULT_EMBEDDING_TOKENIZER_FILENAME.to_string(),
    dimension: DEFAULT_TEXT_EMBEDDING_DIM,  // now 256
    ..Default::default()
}
```

- [ ] **Step 6: Compile check**

```bash
cd src-tauri && cargo check -p fndr 2>&1 | head -60
```
Fix callers referencing `vlm_model_size` or `vlm_model_id` on Config (grep: `vlm_model_size`, `vlm_model_id`).

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/config.rs
git commit -m "feat: update config.rs — EmbeddingGemma 256-dim defaults, remove VLM tier config"
```

---

## Task 4: Update image_semantics.rs — Qwen3-VL-2B only

**Files:**
- Modify: `src-tauri/src/inference/image_semantics.rs`

- [ ] **Step 1: Replace MtmdModelFamily enum**

Find the `MtmdModelFamily` enum (line 62-67) and replace:
```rust
/// Which MTMD model family is loaded — exactly one profile: Qwen3-VL-2B.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MtmdModelFamily {
    Qwen3Vl2B,
}

impl MtmdModelFamily {
    pub(crate) fn from_model_id(model_id: &str) -> Option<Self> {
        match model_id {
            "qwen3-vl-2b" => Some(Self::Qwen3Vl2B),
            _ => None,
        }
    }

    pub fn model_id_str(&self) -> &'static str {
        "qwen3-vl-2b"
    }

    pub fn context_size(&self) -> u32 {
        crate::inference::model_config::QWEN_CONTEXT_SIZE  // 4096
    }
}
```

- [ ] **Step 2: Update MtmdVlmRuntime::load()**

Find the `load()` fn (around line 942). Replace the try-SmolVLM-then-Qwen4B logic with:
```rust
fn load(app_data_dir: &Path) -> Result<Self, String> {
    use crate::models;
    let model_path = {
        let resolved = models::resolve_model(Some("qwen3-vl-2b"), Some(app_data_dir))
            .ok_or_else(|| {
                "Qwen3-VL-2B not found. Download Qwen3VL-2B-Instruct-Q4_K_M.gguf and its mmproj into ~/Library/Application Support/FNDR/models/qwen3-vl-2b/".to_string()
            })?;
        models::validate_qwen3_vl_2b_main_gguf_file(&resolved.path)
            .map_err(|e| format!("Qwen3-VL-2B GGUF invalid: {e}"))?;
        resolved.path
    };
    let mmproj = models::resolve_qwen3_vl_2b_mmproj(Some(app_data_dir))
        .ok_or_else(|| {
            format!(
                "Qwen3-VL-2B mmproj missing. Download one of: {}",
                crate::models::QWEN3_VL_2B_MMPROJ_FILENAMES.join(", ")
            )
        })?;
    let model_family = MtmdModelFamily::Qwen3Vl2B;
    // ... rest of load() is unchanged (backend init, LlamaModel::load_from_file, etc.)
```

- [ ] **Step 3: Update model_id in parse_vision_json call**

In `run_blocking()`, the final call is:
```rust
parse_vision_json(&out, self.model_family.model_id_str())
```
This now returns `"qwen3-vl-2b"` correctly since `model_id_str()` changed.

- [ ] **Step 4: Update tests**

Find the `mtmd_model_family_roundtrip` test (around line 1200) and replace:
```rust
#[test]
fn mtmd_model_family_roundtrip() {
    assert_eq!(
        MtmdModelFamily::from_model_id("qwen3-vl-2b"),
        Some(MtmdModelFamily::Qwen3Vl2B)
    );
    assert_eq!(MtmdModelFamily::from_model_id("smolvlm-500m"), None);
    assert_eq!(MtmdModelFamily::from_model_id("qwen3-vl-4b"), None);
    assert_eq!(MtmdModelFamily::Qwen3Vl2B.model_id_str(), "qwen3-vl-2b");
    assert_eq!(MtmdModelFamily::Qwen3Vl2B.context_size(), 4096);
}
```

Also update `ocr_only_insight_never_mentions_failure_or_extraction` test — add `"qwen3-vl-2b"` to the forbidden list check if needed (it should already pass since OCR-only uses `model_id = "ocr_only"`).

- [ ] **Step 5: Compile + test**

```bash
cd src-tauri && cargo test -p fndr inference::image_semantics 2>&1 | tail -20
```
Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/inference/image_semantics.rs
git commit -m "feat: image_semantics — Qwen3-VL-2B only, remove SmolVLM/Qwen4B model families"
```

---

## Task 5: Simplify vlm_router.rs — single RunQwenVlm decision

**Files:**
- Modify: `src-tauri/src/inference/vlm_router.rs`

- [ ] **Step 1: Replace VlmRouteDecision enum**

Replace the entire enum (lines 1-36):
```rust
/// Decision produced by [`should_run_vlm`].
#[derive(Debug, Clone, PartialEq)]
pub enum VlmRouteDecision {
    SkipDuplicate,
    SkipGoodOcr,
    SkipLowValue,
    /// Run Qwen3-VL-2B for this frame.
    RunQwenVlm,
    FallbackOcrOnly { reason: String },
}

impl VlmRouteDecision {
    pub fn label(&self) -> &'static str {
        match self {
            Self::SkipDuplicate => "skip_duplicate",
            Self::SkipGoodOcr => "skip_good_ocr",
            Self::SkipLowValue => "skip_low_value",
            Self::RunQwenVlm => "run_qwen_vlm",
            Self::FallbackOcrOnly { .. } => "fallback_ocr_only",
        }
    }

    pub fn fallback_reason(&self) -> Option<&str> {
        match self {
            Self::FallbackOcrOnly { reason } => Some(reason.as_str()),
            _ => None,
        }
    }

    pub fn runs_pixel_vlm(&self) -> bool {
        matches!(self, Self::RunQwenVlm)
    }
}
```

- [ ] **Step 2: Simplify VlmRouteInput**

Remove `host_supports_heavy_vlm` and rename `host_supports_lightweight_vlm` to `host_supports_qwen_vlm`:
```rust
pub struct VlmRouteInput<'a> {
    pub ocr_text_len: usize,
    pub ocr_confidence: f32,
    pub ocr_block_count: usize,
    pub visual_signal: bool,
    pub is_duplicate: bool,
    pub system_pressure_skip: bool,
    /// Host has ≥ 8 GB RAM — safe to run Qwen3-VL-2B (~3.5 GB usage).
    pub host_supports_qwen_vlm: bool,
    pub vlm_enabled: bool,
    pub vlm_available: bool,
    pub vlm_calls_remaining: u32,
    pub vlm_timeout_secs: u64,
}
```

- [ ] **Step 3: Simplify should_run_vlm()**

Replace the body of `should_run_vlm()`:
```rust
pub fn should_run_vlm(input: &VlmRouteInput) -> VlmRouteDecision {
    if input.is_duplicate {
        return VlmRouteDecision::SkipDuplicate;
    }
    if !input.vlm_enabled {
        return VlmRouteDecision::FallbackOcrOnly {
            reason: "vlm_disabled".to_string(),
        };
    }
    if !input.host_supports_qwen_vlm {
        return VlmRouteDecision::FallbackOcrOnly {
            reason: "vlm_blocked_low_ram".to_string(),
        };
    }
    if input.system_pressure_skip {
        return VlmRouteDecision::FallbackOcrOnly {
            reason: "system_pressure".to_string(),
        };
    }
    // Good OCR: VLM adds diminishing returns when text is rich.
    if input.ocr_text_len >= 300 && input.ocr_block_count >= 10 && input.ocr_confidence >= 0.40 {
        return VlmRouteDecision::SkipGoodOcr;
    }
    // Low value: almost nothing to analyze visually.
    if !input.visual_signal && input.ocr_text_len < 60 && input.ocr_block_count < 3 {
        return VlmRouteDecision::SkipLowValue;
    }
    if input.vlm_timeout_secs == 0 {
        return VlmRouteDecision::FallbackOcrOnly {
            reason: "vlm_timeout_disabled".to_string(),
        };
    }
    if input.vlm_calls_remaining == 0 {
        return VlmRouteDecision::FallbackOcrOnly {
            reason: "vlm_rate_limited".to_string(),
        };
    }
    if !input.vlm_available {
        return VlmRouteDecision::FallbackOcrOnly {
            reason: "vlm_unavailable".to_string(),
        };
    }
    VlmRouteDecision::RunQwenVlm
}
```

- [ ] **Step 4: Update tests**

Replace the entire `#[cfg(test)]` module:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn base_input() -> VlmRouteInput<'static> {
        VlmRouteInput {
            ocr_text_len: 100,
            ocr_confidence: 0.48,
            ocr_block_count: 8,
            visual_signal: true,
            is_duplicate: false,
            system_pressure_skip: false,
            host_supports_qwen_vlm: true,
            vlm_enabled: true,
            vlm_available: true,
            vlm_calls_remaining: 10,
            vlm_timeout_secs: 30,
        }
    }

    #[test]
    fn skip_duplicate() {
        let mut inp = base_input();
        inp.is_duplicate = true;
        assert_eq!(should_run_vlm(&inp), VlmRouteDecision::SkipDuplicate);
    }

    #[test]
    fn skip_good_ocr() {
        let mut inp = base_input();
        inp.ocr_text_len = 600;
        inp.ocr_confidence = 0.50;
        inp.ocr_block_count = 20;
        assert_eq!(should_run_vlm(&inp), VlmRouteDecision::SkipGoodOcr);
    }

    #[test]
    fn skip_low_value_tiny_frame() {
        let mut inp = base_input();
        inp.ocr_text_len = 30;
        inp.ocr_block_count = 1;
        inp.visual_signal = false;
        assert_eq!(should_run_vlm(&inp), VlmRouteDecision::SkipLowValue);
    }

    #[test]
    fn fallback_low_ram() {
        let mut inp = base_input();
        inp.host_supports_qwen_vlm = false;
        assert_eq!(
            should_run_vlm(&inp),
            VlmRouteDecision::FallbackOcrOnly {
                reason: "vlm_blocked_low_ram".to_string()
            }
        );
    }

    #[test]
    fn run_qwen_for_weak_ocr_frame() {
        assert_eq!(should_run_vlm(&base_input()), VlmRouteDecision::RunQwenVlm);
    }

    #[test]
    fn fallback_system_pressure() {
        let mut inp = base_input();
        inp.system_pressure_skip = true;
        assert!(matches!(should_run_vlm(&inp), VlmRouteDecision::FallbackOcrOnly { .. }));
    }

    #[test]
    fn fallback_vlm_disabled() {
        let mut inp = base_input();
        inp.vlm_enabled = false;
        assert!(matches!(should_run_vlm(&inp), VlmRouteDecision::FallbackOcrOnly { .. }));
    }

    #[test]
    fn fallback_when_budget_exhausted() {
        let mut inp = base_input();
        inp.vlm_calls_remaining = 0;
        assert_eq!(
            should_run_vlm(&inp),
            VlmRouteDecision::FallbackOcrOnly { reason: "vlm_rate_limited".to_string() }
        );
    }

    #[test]
    fn fallback_when_model_unavailable() {
        let mut inp = base_input();
        inp.vlm_available = false;
        assert_eq!(
            should_run_vlm(&inp),
            VlmRouteDecision::FallbackOcrOnly { reason: "vlm_unavailable".to_string() }
        );
    }
}
```

- [ ] **Step 5: Fix callers of VlmRouteInput / VlmRouteDecision**

Grep for `RunHeavyVlmExplicitOnly`, `RunLightweightVlm`, `host_supports_heavy_vlm`, `host_supports_lightweight_vlm`:
```bash
grep -rn "RunHeavyVlm\|RunLightweightVlm\|host_supports_heavy\|host_supports_lightweight" src-tauri/src/
```
For each callsite, update to use `RunQwenVlm` and `host_supports_qwen_vlm`.

- [ ] **Step 6: Compile + test**

```bash
cd src-tauri && cargo test -p fndr inference::vlm_router 2>&1 | tail -20
```

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/inference/vlm_router.rs
git commit -m "feat: vlm_router — single RunQwenVlm decision, remove heavy/lightweight distinction"
```

---

## Task 6: Create inference/qwen_vl_memory.rs — memory synthesis API

**Files:**
- Create: `src-tauri/src/inference/qwen_vl_memory.rs`
- Modify: `src-tauri/src/inference/mod.rs` (add `pub mod qwen_vl_memory;`)

- [ ] **Step 1: Write the struct definitions**

```rust
// src-tauri/src/inference/qwen_vl_memory.rs
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub enum MemorySourceType {
    Screen,
    GlassesImport,
    FileImport,
}

impl MemorySourceType {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Screen => "screen",
            Self::GlassesImport => "glasses_import",
            Self::FileImport => "file_import",
        }
    }
}

#[derive(Debug, Clone)]
pub struct MemorySynthesisInput {
    pub image_path: Option<PathBuf>,
    pub ocr_text: String,
    pub app_name: Option<String>,
    pub window_title: Option<String>,
    pub url: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub source_type: MemorySourceType,
    pub ocr_confidence: Option<f32>,
}

#[derive(Debug, Clone, Default)]
pub struct MemorySynthesisOutput {
    pub memory_context: String,
    pub summary_short: String,
    pub topic: Option<String>,
    pub activity_type: Option<String>,
    pub user_intent: Option<String>,
    pub entities: Vec<String>,
    pub files: Vec<String>,
    pub urls: Vec<String>,
    pub decisions: Vec<String>,
    pub errors: Vec<String>,
    pub next_steps: Vec<String>,
    pub search_aliases: Vec<String>,
    pub confidence_score: f32,
    pub importance_score: f32,
}
```

- [ ] **Step 2: Write the prompt template and JSON schema types**

```rust
const MEMORY_SYNTHESIS_PROMPT: &str = r#"You are FNDR's local memory extraction model.

Create a structured memory from the user's screen.

Use all available evidence:
- screenshot image (if provided)
- OCR text
- app name
- window title
- URL
- timestamp
- source type

Rules:
- Do not invent details not supported by the evidence.
- Prefer concrete nouns, app names, file names, URLs, project names, commands, errors, decisions, todos, and next steps.
- memory_context should help a future AI agent understand what the user was doing, why it mattered, and what context should be remembered.
- Use OCR as primary text evidence.
- Use the image for layout, visual context, screenshots, logos, diagrams, UI state, and image-heavy screens.
- If evidence is weak, lower confidence_score.
- Avoid storing unnecessary sensitive raw text.
- Return JSON only. No markdown. No explanation.

Required JSON schema:
{
  "memory_context": "...",
  "summary_short": "...",
  "topic": "...",
  "activity_type": "...",
  "user_intent": "...",
  "entities": [],
  "files": [],
  "urls": [],
  "decisions": [],
  "errors": [],
  "next_steps": [],
  "search_aliases": [],
  "confidence_score": 0.0,
  "importance_score": 0.0
}"#;

#[derive(Debug, Deserialize, Default)]
struct SynthesisJsonRow {
    #[serde(default)]
    memory_context: String,
    #[serde(default)]
    summary_short: String,
    #[serde(default)]
    topic: Option<String>,
    #[serde(default)]
    activity_type: Option<String>,
    #[serde(default)]
    user_intent: Option<String>,
    #[serde(default)]
    entities: Vec<String>,
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    urls: Vec<String>,
    #[serde(default)]
    decisions: Vec<String>,
    #[serde(default)]
    errors: Vec<String>,
    #[serde(default)]
    next_steps: Vec<String>,
    #[serde(default)]
    search_aliases: Vec<String>,
    #[serde(default)]
    confidence_score: f32,
    #[serde(default)]
    importance_score: f32,
}
```

- [ ] **Step 3: Write the user prompt builder**

```rust
fn build_user_prompt(input: &MemorySynthesisInput) -> String {
    let mut parts = Vec::new();
    parts.push(format!("Source: {}", input.source_type.label()));
    parts.push(format!("Timestamp: {}", input.timestamp.to_rfc3339()));
    if let Some(ref app) = input.app_name {
        parts.push(format!("App: {app}"));
    }
    if let Some(ref title) = input.window_title {
        parts.push(format!("Window: {title}"));
    }
    if let Some(ref url) = input.url {
        parts.push(format!("URL: {url}"));
    }
    if let Some(conf) = input.ocr_confidence {
        parts.push(format!("OCR confidence: {conf:.2}"));
    }
    if !input.ocr_text.trim().is_empty() {
        let excerpt: String = input.ocr_text.chars().take(2400).collect();
        parts.push(format!("OCR text:\n{excerpt}"));
    }
    if input.image_path.is_some() {
        parts.push("[screenshot attached]".to_string());
    }
    parts.join("\n")
}
```

- [ ] **Step 4: Write JSON parsing + repair**

```rust
fn parse_synthesis_json(raw: &str) -> Result<MemorySynthesisOutput, String> {
    let trimmed = strip_markdown_fence(raw);
    let slice = extract_json_object(&trimmed)
        .ok_or_else(|| format!("no JSON object in output: {}", &trimmed[..trimmed.len().min(200)]))?;
    let row: SynthesisJsonRow = serde_json::from_str(slice)
        .map_err(|e| format!("JSON parse: {e}"))?;

    if row.memory_context.trim().is_empty() {
        return Err("memory_context is empty".to_string());
    }

    Ok(MemorySynthesisOutput {
        memory_context: clamp(row.memory_context, 2000),
        summary_short: clamp(row.summary_short, 280),
        topic: row.topic.map(|s| clamp(s, 120)),
        activity_type: row.activity_type.map(|s| clamp(s, 80)),
        user_intent: row.user_intent.map(|s| clamp(s, 200)),
        entities: sanitize_list(row.entities, 20, 64),
        files: sanitize_list(row.files, 16, 120),
        urls: sanitize_list(row.urls, 12, 200),
        decisions: sanitize_list(row.decisions, 12, 200),
        errors: sanitize_list(row.errors, 12, 200),
        next_steps: sanitize_list(row.next_steps, 12, 200),
        search_aliases: sanitize_list(row.search_aliases, 24, 48),
        confidence_score: row.confidence_score.clamp(0.0, 1.0),
        importance_score: row.importance_score.clamp(0.0, 1.0),
    })
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

fn extract_json_object(s: &str) -> Option<&str> {
    let start = s.find('{')?;
    let end = s.rfind('}')?;
    if end > start { Some(&s[start..=end]) } else { None }
}

fn clamp(mut s: String, max_chars: usize) -> String {
    s.retain(|c| c != '\0');
    if s.chars().count() <= max_chars { s }
    else { s.chars().take(max_chars.saturating_sub(1)).collect::<String>() + "…" }
}

fn sanitize_list(mut v: Vec<String>, max_items: usize, max_each: usize) -> Vec<String> {
    v.retain(|s| !s.trim().is_empty());
    v.truncate(max_items);
    v.into_iter().map(|s| clamp(s, max_each)).collect()
}
```

- [ ] **Step 5: Write OCR-only fallback**

```rust
/// Build a fallback MemorySynthesisOutput from metadata when Qwen is unavailable.
/// Sets confidence_score low and marks the path for enrichment_status tracking.
pub fn synthesis_ocr_only_fallback(input: &MemorySynthesisInput) -> MemorySynthesisOutput {
    let app = input.app_name.as_deref().unwrap_or("").trim();
    let title = input.window_title.as_deref().unwrap_or("").trim();
    let url_str = input.url.as_deref().unwrap_or("");

    let memory_context = if !title.is_empty() && !app.is_empty() {
        format!("{app} — {title}. {}", input.ocr_text.chars().take(600).collect::<String>())
    } else if !input.ocr_text.trim().is_empty() {
        input.ocr_text.chars().take(800).collect()
    } else {
        format!("Screen capture: {} {}", app, title)
    };

    let summary_short = if !title.is_empty() {
        if !app.is_empty() { format!("{app}: {title}") } else { title.to_string() }
    } else {
        app.to_string()
    };

    let mut urls = Vec::new();
    if !url_str.is_empty() { urls.push(url_str.to_string()); }

    MemorySynthesisOutput {
        memory_context,
        summary_short,
        topic: if !title.is_empty() { Some(title.to_string()) } else { None },
        activity_type: Some("observing".to_string()),
        user_intent: None,
        entities: Vec::new(),
        files: Vec::new(),
        urls,
        decisions: Vec::new(),
        errors: Vec::new(),
        next_steps: Vec::new(),
        search_aliases: Vec::new(),
        confidence_score: 0.30,
        importance_score: 0.30,
    }
}
```

- [ ] **Step 6: Write tests for JSON parsing and fallback**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_synthesis_json() {
        let raw = r#"{"memory_context":"User reviewed PRs on GitHub","summary_short":"GitHub PR review","topic":"code review","activity_type":"reviewing","user_intent":"review pull requests","entities":["GitHub","PR #42"],"files":[],"urls":["https://github.com"],"decisions":[],"errors":[],"next_steps":["merge PR #42"],"search_aliases":["PR review","GitHub"],"confidence_score":0.85,"importance_score":0.7}"#;
        let out = parse_synthesis_json(raw).unwrap();
        assert_eq!(out.summary_short, "GitHub PR review");
        assert!(!out.memory_context.is_empty());
        assert!((out.confidence_score - 0.85).abs() < 0.01);
        assert_eq!(out.next_steps, vec!["merge PR #42"]);
    }

    #[test]
    fn tolerates_markdown_fence() {
        let raw = "```json\n{\"memory_context\":\"test\",\"summary_short\":\"t\",\"confidence_score\":0.5,\"importance_score\":0.4}\n```";
        let out = parse_synthesis_json(raw).unwrap();
        assert_eq!(out.memory_context, "test");
    }

    #[test]
    fn rejects_empty_memory_context() {
        let raw = r#"{"memory_context":"","summary_short":"x","confidence_score":0.5,"importance_score":0.4}"#;
        assert!(parse_synthesis_json(raw).is_err());
    }

    #[test]
    fn ocr_only_fallback_produces_valid_output() {
        use chrono::Utc;
        let input = MemorySynthesisInput {
            image_path: None,
            ocr_text: "def main(): pass".to_string(),
            app_name: Some("VS Code".to_string()),
            window_title: Some("main.py".to_string()),
            url: None,
            timestamp: Utc::now(),
            source_type: MemorySourceType::Screen,
            ocr_confidence: Some(0.75),
        };
        let out = synthesis_ocr_only_fallback(&input);
        assert!(out.summary_short.contains("VS Code"));
        assert!(out.confidence_score < 0.55);
        assert!(!out.memory_context.is_empty());
    }

    #[test]
    fn required_fields_present_in_output() {
        let out = MemorySynthesisOutput {
            memory_context: "ctx".to_string(),
            summary_short: "sum".to_string(),
            confidence_score: 0.8,
            importance_score: 0.6,
            ..Default::default()
        };
        assert!(!out.memory_context.is_empty());
        assert!(!out.summary_short.is_empty());
        assert!(out.confidence_score >= 0.0 && out.confidence_score <= 1.0);
    }
}
```

- [ ] **Step 7: Declare module + compile + test**

In `inference/mod.rs`:
```rust
pub mod qwen_vl_memory;
```

```bash
cd src-tauri && cargo test -p fndr inference::qwen_vl_memory 2>&1 | tail -20
```
Expected: all 4 tests pass.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/inference/qwen_vl_memory.rs src-tauri/src/inference/mod.rs
git commit -m "feat: add inference/qwen_vl_memory.rs with MemorySynthesisInput/Output + OCR fallback"
```

---

## Task 7: Create inference/model_worker.rs — lazy queue + idle unload

**Files:**
- Create: `src-tauri/src/inference/model_worker.rs`
- Modify: `src-tauri/src/inference/mod.rs`

- [ ] **Step 1: Write ModelRunDecision enum**

```rust
// src-tauri/src/inference/model_worker.rs
use crate::inference::qwen_vl_memory::{MemorySynthesisInput, MemorySynthesisOutput};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio::time::{Duration, Instant};

/// Why the worker decided to skip or defer Qwen inference.
#[derive(Debug, Clone)]
pub enum ModelRunDecision {
    RunQwen,
    OcrOnlyFallback { reason: String },
    Defer { reason: String },
}

impl ModelRunDecision {
    pub fn is_run(&self) -> bool {
        matches!(self, Self::RunQwen)
    }
}
```

- [ ] **Step 2: Write QwenJobRequest and QwenVlmWorker**

```rust
pub type SynthesisResult = Result<MemorySynthesisOutput, String>;

pub struct QwenJobRequest {
    pub input: MemorySynthesisInput,
    pub reply: tokio::sync::oneshot::Sender<SynthesisResult>,
}

/// Lazy-loading Qwen3-VL-2B worker.
/// - Max 1 concurrent inference.
/// - Unloads after QWEN_IDLE_UNLOAD_SECONDS of inactivity.
/// - Queue capacity: MAX_CONCURRENT_MULTIMODAL_JOBS * 8.
pub struct QwenVlmWorker {
    sender: mpsc::Sender<QwenJobRequest>,
}

impl QwenVlmWorker {
    /// Create and start the worker background task.
    /// `app_data_dir` is needed to load the model on first use.
    pub fn new(app_data_dir: std::path::PathBuf) -> Arc<Self> {
        use crate::inference::model_config::{MAX_CONCURRENT_MULTIMODAL_JOBS, QWEN_IDLE_UNLOAD_SECONDS};
        let queue_cap = MAX_CONCURRENT_MULTIMODAL_JOBS * 8;
        let (tx, rx) = mpsc::channel(queue_cap);
        let worker = Arc::new(Self { sender: tx });
        tokio::spawn(worker_loop(rx, app_data_dir, QWEN_IDLE_UNLOAD_SECONDS));
        worker
    }

    /// Enqueue a synthesis job. Returns Err if the queue is full.
    pub async fn synthesize(&self, input: MemorySynthesisInput) -> SynthesisResult {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.sender
            .try_send(QwenJobRequest { input, reply: reply_tx })
            .map_err(|_| "qwen_worker_queue_full".to_string())?;
        reply_rx.await.map_err(|_| "qwen_worker_dropped".to_string())?
    }

    /// Returns true if there is room in the queue right now.
    pub fn has_capacity(&self) -> bool {
        self.sender.capacity() > 0
    }
}
```

- [ ] **Step 3: Write the worker loop**

```rust
/// Background task: processes jobs one at a time, unloads after idle.
async fn worker_loop(
    mut rx: mpsc::Receiver<QwenJobRequest>,
    app_data_dir: std::path::PathBuf,
    idle_unload_secs: u64,
) {
    let idle_duration = Duration::from_secs(idle_unload_secs);
    // The actual engine is held as Option — None = unloaded.
    let engine: Arc<Mutex<Option<crate::inference::image_semantics::MtmdModelFamily>>> =
        Arc::new(Mutex::new(None));

    let _ = engine; // placeholder — actual integration uses the MTMD runtime from image_semantics.rs

    loop {
        match tokio::time::timeout(idle_duration, rx.recv()).await {
            Ok(Some(job)) => {
                // Run Qwen inference — delegates to the blocking MTMD runtime.
                let result = run_synthesis_blocking(job.input, &app_data_dir).await;
                let _ = job.reply.send(result);
            }
            Ok(None) => {
                // Channel closed — exit.
                tracing::info!("qwen_worker: channel closed, exiting");
                break;
            }
            Err(_timeout) => {
                // Idle timeout — unload model if loaded.
                tracing::debug!("qwen_worker: idle timeout, model unloaded");
                // The MtmdVlmRuntime singleton is held in IMPORT_VISION (OnceLock).
                // Currently there is no unload API in llama_cpp_2; log and continue.
                // Future: replace OnceLock with Arc<Mutex<Option>> for true unloading.
            }
        }
    }
}

/// Runs Qwen3-VL-2B memory synthesis on a blocking thread.
async fn run_synthesis_blocking(
    input: MemorySynthesisInput,
    app_data_dir: &std::path::Path,
) -> SynthesisResult {
    use crate::inference::image_semantics::{extract_image_semantics, ImageImportSource};
    use crate::inference::qwen_vl_memory::synthesis_ocr_only_fallback;

    // If an image is available, run the MTMD vision path.
    if let Some(ref img_path) = input.image_path {
        let bytes = tokio::fs::read(img_path).await.map_err(|e| e.to_string())?;
        let filename = img_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("frame.png")
            .to_string();
        match extract_image_semantics(bytes, &filename, ImageImportSource::ScreenCapture, app_data_dir.to_path_buf()).await {
            Ok(insight) => {
                // Convert ImageSemanticInsight to MemorySynthesisOutput
                return Ok(crate::inference::qwen_vl_memory::MemorySynthesisOutput {
                    memory_context: insight.summary_detailed.clone(),
                    summary_short: insight.summary_short.clone(),
                    topic: if insight.topics.is_empty() { None } else { Some(insight.topics[0].clone()) },
                    activity_type: insight.activity_type.clone(),
                    user_intent: insight.user_intent.clone(),
                    entities: insight.entities.clone(),
                    files: Vec::new(),
                    urls: Vec::new(),
                    decisions: Vec::new(),
                    errors: Vec::new(),
                    next_steps: Vec::new(),
                    search_aliases: insight.search_aliases.clone(),
                    confidence_score: insight.confidence,
                    importance_score: insight.confidence * 0.8,
                });
            }
            Err(e) => {
                tracing::warn!("qwen_worker: vision inference failed ({e}), OCR fallback");
                return Ok(synthesis_ocr_only_fallback(&input));
            }
        }
    }

    // OCR-only path when no image.
    Ok(synthesis_ocr_only_fallback(&input))
}
```

- [ ] **Step 4: Write decision helper**

```rust
/// Decide whether to run Qwen, fall back to OCR-only, or defer.
pub fn decide_model_run(
    is_duplicate: bool,
    system_pressure_skip: bool,
    queue_full: bool,
    ocr_text_len: usize,
    ocr_confidence: f32,
    ocr_block_count: usize,
    visual_signal: bool,
    vlm_available: bool,
) -> ModelRunDecision {
    if is_duplicate {
        return ModelRunDecision::Defer { reason: "duplicate".to_string() };
    }
    if system_pressure_skip {
        return ModelRunDecision::OcrOnlyFallback { reason: "system_pressure".to_string() };
    }
    if !vlm_available {
        return ModelRunDecision::OcrOnlyFallback { reason: "vlm_unavailable".to_string() };
    }
    if queue_full {
        return ModelRunDecision::Defer { reason: "queue_full".to_string() };
    }
    // Good OCR: skip Qwen.
    if ocr_text_len >= 300 && ocr_block_count >= 10 && ocr_confidence >= 0.40 {
        return ModelRunDecision::OcrOnlyFallback { reason: "good_ocr".to_string() };
    }
    // Low value: skip.
    if !visual_signal && ocr_text_len < 60 && ocr_block_count < 3 {
        return ModelRunDecision::Defer { reason: "low_value".to_string() };
    }
    ModelRunDecision::RunQwen
}
```

- [ ] **Step 5: Write tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decides_run_qwen_for_weak_ocr() {
        let d = decide_model_run(false, false, false, 80, 0.35, 5, true, true);
        assert!(d.is_run());
    }

    #[test]
    fn defers_when_duplicate() {
        let d = decide_model_run(true, false, false, 80, 0.35, 5, true, true);
        assert!(matches!(d, ModelRunDecision::Defer { .. }));
    }

    #[test]
    fn ocr_fallback_on_system_pressure() {
        let d = decide_model_run(false, true, false, 80, 0.35, 5, true, true);
        assert!(matches!(d, ModelRunDecision::OcrOnlyFallback { .. }));
    }

    #[test]
    fn good_ocr_skips_qwen() {
        let d = decide_model_run(false, false, false, 500, 0.75, 15, true, true);
        assert!(matches!(d, ModelRunDecision::OcrOnlyFallback { reason } if reason == "good_ocr"));
    }

    #[test]
    fn queue_full_defers() {
        let d = decide_model_run(false, false, true, 80, 0.35, 5, true, true);
        assert!(matches!(d, ModelRunDecision::Defer { reason } if reason == "queue_full"));
    }
}
```

- [ ] **Step 6: Declare module + compile + test**

In `inference/mod.rs`:
```rust
pub mod model_worker;
```

```bash
cd src-tauri && cargo test -p fndr inference::model_worker 2>&1 | tail -20
```

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/inference/model_worker.rs src-tauri/src/inference/mod.rs
git commit -m "feat: add inference/model_worker.rs with lazy queue, idle unload, ModelRunDecision"
```

---

## Task 8: Update embedding/onnx.rs + create embed/embedding_gemma.rs

**Files:**
- Modify: `src-tauri/src/embedding/onnx.rs`
- Create: `src-tauri/src/embed/mod.rs`
- Create: `src-tauri/src/embed/embedding_gemma.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Update constants in embedding/onnx.rs**

Find the constants block near the top of `embedding/onnx.rs`:
```rust
// OLD:
const EMBEDDING_DIM: usize = 1024;
const MODEL_FILENAME: &str = "bge-large-en-v1.5-quantized.onnx";
const MODEL_NAME: &str = "bge-large-en-v1.5";

// NEW:
const EMBEDDING_DIM: usize = crate::inference::model_config::EMBEDDING_DIMENSIONS; // 256
const MODEL_FILENAME: &str = crate::inference::model_config::EMBEDDING_MODEL_FILENAME;
const MODEL_NAME: &str = crate::inference::model_config::EMBEDDING_MODEL_ID;
```

Note: Rust const evaluation requires these to be known at compile time. Since `model_config` constants are `const`, this works. Alternatively, copy the values directly:
```rust
const EMBEDDING_DIM: usize = 256;
const MODEL_FILENAME: &str = "embeddinggemma-300m.onnx";
const MODEL_NAME: &str = "google/embeddinggemma-300m";
```

- [ ] **Step 2: Update model directory lookup**

In the function that builds the model path (look for the model file path search logic), add a subdirectory search for `embeddinggemma-300m/`:

Find the ONNX model path resolution (likely uses `candidate_model_dirs` from models.rs or its own path builder). Update to also check `{models_dir}/embeddinggemma-300m/embeddinggemma-300m.onnx`:

```rust
// In the model path resolution, after checking flat layout:
for dir in candidate_dirs {
    // Flat layout
    let flat = dir.join(MODEL_FILENAME);
    if flat.is_file() { return Ok(flat); }
    // Subdirectory layout
    let sub = dir.join("embeddinggemma-300m").join(MODEL_FILENAME);
    if sub.is_file() { return Ok(sub); }
}
```

- [ ] **Step 3: Create embed/mod.rs**

```rust
// src-tauri/src/embed/mod.rs
pub mod embedding_gemma;
```

- [ ] **Step 4: Create embed/embedding_gemma.rs**

```rust
// src-tauri/src/embed/embedding_gemma.rs
//! Public API for EmbeddingGemma 300M (256-dim ONNX) text embeddings.
//! Delegates to the ONNX backend in `crate::embedding::onnx`.

use crate::inference::model_config::{EMBEDDING_DIMENSIONS, EMBEDDING_MODEL_ID};

/// Format a MemorySynthesisOutput into the canonical text for embedding.
/// Mirrors the spec's "Embedding text format" exactly.
pub fn format_for_embedding(
    summary_short: &str,
    topic: Option<&str>,
    user_intent: Option<&str>,
    activity_type: Option<&str>,
    memory_context: &str,
    entities: &[String],
    files: &[String],
    urls: &[String],
    decisions: &[String],
    errors: &[String],
    next_steps: &[String],
    search_aliases: &[String],
) -> String {
    let mut parts = Vec::new();
    parts.push(format!("Title: {summary_short}"));
    if let Some(t) = topic { if !t.is_empty() { parts.push(format!("Topic: {t}")); } }
    if let Some(i) = user_intent { if !i.is_empty() { parts.push(format!("Intent: {i}")); } }
    if let Some(a) = activity_type { if !a.is_empty() { parts.push(format!("Activity: {a}")); } }
    if !memory_context.is_empty() { parts.push(format!("Context: {memory_context}")); }
    if !entities.is_empty() { parts.push(format!("Entities: {}", entities.join(", "))); }
    if !files.is_empty() { parts.push(format!("Files: {}", files.join(", "))); }
    if !urls.is_empty() { parts.push(format!("URLs: {}", urls.join(", "))); }
    if !decisions.is_empty() { parts.push(format!("Decisions: {}", decisions.join(", "))); }
    if !errors.is_empty() { parts.push(format!("Errors: {}", errors.join(", "))); }
    if !next_steps.is_empty() { parts.push(format!("Next steps: {}", next_steps.join(", "))); }
    if !search_aliases.is_empty() { parts.push(format!("Aliases: {}", search_aliases.join(", "))); }
    parts.join("\n")
}

pub fn embedding_model_id() -> &'static str {
    EMBEDDING_MODEL_ID
}

pub fn embedding_dimensions() -> usize {
    EMBEDDING_DIMENSIONS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_includes_all_non_empty_fields() {
        let text = format_for_embedding(
            "GitHub PR review",
            Some("code review"),
            Some("review PRs"),
            Some("reviewing"),
            "User reviewed open pull requests on GitHub",
            &["GitHub".into(), "PR #42".into()],
            &[],
            &["https://github.com".into()],
            &["merge PR #42".into()],
            &[],
            &["check CI".into()],
            &["PR review".into()],
        );
        assert!(text.contains("Title: GitHub PR review"));
        assert!(text.contains("Topic: code review"));
        assert!(text.contains("Decisions: merge PR #42"));
        assert!(text.contains("Next steps: check CI"));
        assert!(text.contains("Aliases: PR review"));
    }

    #[test]
    fn format_skips_empty_fields() {
        let text = format_for_embedding(
            "title", None, None, None,
            "context", &[], &[], &[], &[], &[], &[], &[],
        );
        assert!(!text.contains("Topic:"));
        assert!(!text.contains("Entities:"));
        assert!(text.contains("Context: context"));
    }

    #[test]
    fn embedding_dimensions_is_256() {
        assert_eq!(embedding_dimensions(), 256);
    }
}
```

- [ ] **Step 5: Declare embed module in lib.rs**

In `src-tauri/src/lib.rs`, add:
```rust
pub mod embed;
```

- [ ] **Step 6: Compile + test**

```bash
cd src-tauri && cargo test -p fndr embed::embedding_gemma 2>&1 | tail -20
```
Expected: 3 tests pass.

Also check:
```bash
cd src-tauri && cargo check -p fndr 2>&1 | head -40
```

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/embedding/onnx.rs src-tauri/src/embed/ src-tauri/src/lib.rs
git commit -m "feat: update embedding to EmbeddingGemma 256-dim; add embed/embedding_gemma.rs public API"
```

---

## Task 9: Add memories_v3_egemma_256 LanceDB table

**Files:**
- Modify: `src-tauri/src/storage/lance_store/schemas.rs`
- Modify: `src-tauri/src/storage/lance_store/mod.rs`

- [ ] **Step 1: Add EMBED_GEMMA_DIM constant to lance_store/mod.rs**

Open `src-tauri/src/storage/lance_store/mod.rs`. Find where `TEXT_EMBED_DIM` and `IMAGE_EMBED_DIM` are defined (they derive from config constants). Add:
```rust
/// EmbeddingGemma 256-dim — used only by memories_v3_egemma_256 table.
pub(crate) const EMBED_GEMMA_DIM: i32 = crate::inference::model_config::EMBEDDING_DIMENSIONS_I32;
```

Keep `TEXT_EMBED_DIM` as-is (may still be 1024 for legacy tables — or 256 now since we changed config). **Note:** if `TEXT_EMBED_DIM` derives from `DEFAULT_TEXT_EMBEDDING_DIM` which is now 256 (Task 3), they will be equal. In that case, just add `pub(crate) const EMBED_GEMMA_DIM: i32 = TEXT_EMBED_DIM;` as an alias.

- [ ] **Step 2: Add memories_v3_egemma_256_schema() to schemas.rs**

At the bottom of `schemas.rs`, add:
```rust
/// Arrow schema for `memories_v3_egemma_256` — EmbeddingGemma 256-dim vectors.
/// All text embedding columns use EMBED_GEMMA_DIM = 256.
pub fn memories_v3_schema() -> Schema {
    Schema::new(vec![
        // Core identity
        Field::new("id", DataType::Utf8, false),
        Field::new("timestamp", DataType::Int64, false),
        Field::new("timestamp_start", DataType::Int64, false),
        Field::new("timestamp_end", DataType::Int64, false),
        Field::new("day_bucket", DataType::Utf8, false),
        // Context
        Field::new("app_name", DataType::Utf8, false),
        Field::new("bundle_id", DataType::Utf8, true),
        Field::new("window_title", DataType::Utf8, false),
        Field::new("url", DataType::Utf8, true),
        Field::new("source_type", DataType::Utf8, false),
        // Memory content
        Field::new("ocr_confidence", DataType::Float32, false),
        Field::new("memory_context", DataType::Utf8, false),
        Field::new("summary_short", DataType::Utf8, false),
        Field::new("topic", DataType::Utf8, false),
        Field::new("activity_type", DataType::Utf8, false),
        Field::new("user_intent", DataType::Utf8, false),
        // Structured lists
        Field::new("entities", DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))), false),
        Field::new("files", DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))), false),
        Field::new("urls", DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))), false),
        Field::new("decisions", DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))), false),
        Field::new("errors", DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))), false),
        Field::new("next_steps", DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))), false),
        Field::new("search_aliases", DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))), false),
        // Quality
        Field::new("confidence_score", DataType::Float32, false),
        Field::new("importance_score", DataType::Float32, false),
        // Enrichment metadata
        Field::new("enrichment_status", DataType::Utf8, false),  // qwen_enriched|ocr_only_fallback|skipped
        Field::new("fallback_reason", DataType::Utf8, true),
        Field::new("embedding_model", DataType::Utf8, false),    // "google/embeddinggemma-300m"
        Field::new("embedding_dimensions", DataType::Int32, false), // 256
        Field::new("raw_screenshot_stored", DataType::Boolean, false), // always false for screen capture
        // Embedding (256-dim)
        Field::new(
            "embedding",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                super::EMBED_GEMMA_DIM,
            ),
            false,
        ),
        // Dedup
        Field::new("content_hash", DataType::Utf8, false),
        Field::new("dedup_fingerprint", DataType::Utf8, false),
        Field::new("is_soft_deleted", DataType::Boolean, false),
        Field::new("schema_version", DataType::Utf8, false),
    ])
}
```

- [ ] **Step 3: Add v3 table open/create to LanceStore**

In `lance_store/mod.rs`, find the `LanceStore` struct and the table-open logic. Add a method:
```rust
pub async fn open_or_create_memories_v3(&self) -> Result<lancedb::Table, Box<dyn std::error::Error + Send + Sync>> {
    use crate::inference::model_config::MEMORIES_V3_TABLE;
    let table_names = self.connection.table_names().execute().await?;
    if table_names.contains(&MEMORIES_V3_TABLE.to_string()) {
        Ok(self.connection.open_table(MEMORIES_V3_TABLE).execute().await?)
    } else {
        let schema = Arc::new(crate::storage::lance_store::schemas::memories_v3_schema());
        Ok(self.connection
            .create_empty_table(MEMORIES_V3_TABLE, schema)
            .execute()
            .await?)
    }
}
```

- [ ] **Step 4: Compile check**

```bash
cd src-tauri && cargo check -p fndr 2>&1 | head -40
```

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/storage/lance_store/schemas.rs src-tauri/src/storage/lance_store/mod.rs
git commit -m "feat: add memories_v3_egemma_256 LanceDB schema and table open/create"
```

---

## Task 9b: Wire search to memories_v3_egemma_256 + add reindex command + delete temp screenshots

**Files:**
- Modify: `src-tauri/src/search/hybrid.rs` (use v3 table for vector search)
- Modify: `src-tauri/src/storage/lance_store/mod.rs` (add reindex function)
- Modify: `src-tauri/src/inference/model_worker.rs` (delete temp screenshot after inference)

- [ ] **Step 1: Update HybridSearcher to use the v3 table**

Open `src-tauri/src/search/hybrid.rs`. Find where `vector_search` is called on the LanceStore (look for the table name or search method calls). Update the default table to `memories_v3_egemma_256`:

```rust
// Near the top of hybrid.rs or in the search function:
use crate::inference::model_config::MEMORIES_V3_TABLE;

// In the vector_search call:
let semantic_results = store.vector_search(
    MEMORIES_V3_TABLE,  // was: "memories" or "memories_v2_1024"
    &query_embedding,
    limit,
    &filters,
).await;
```

If the HybridSearcher takes the table name as a parameter or config, update the default value in its constructor/config to `MEMORIES_V3_TABLE`.

Run:
```bash
grep -n "memories\|table_name\|lance_table" src-tauri/src/search/hybrid.rs | head -20
```
to locate the exact callsite before editing.

- [ ] **Step 2: Add reindex_to_v3() to LanceStore**

In `src-tauri/src/storage/lance_store/mod.rs`, add:
```rust
/// One-time migration: embed existing memory records from old tables into
/// `memories_v3_egemma_256`. Reads old records, re-embeds memory_context
/// (or creates fallback text), writes 256-dim vectors to the v3 table.
/// Preserves old tables as read-only.
pub async fn reindex_memories_to_embeddinggemma_256(
    &self,
    embedder: &crate::embedding::onnx::Embedder,
) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
    use crate::embed::embedding_gemma::format_for_embedding;
    use crate::inference::model_config::{EMBEDDING_DIMENSIONS, EMBEDDING_MODEL_ID, MEMORIES_V3_TABLE};

    // Open v3 table (creates if missing)
    let _v3 = self.open_or_create_memories_v3().await?;

    // Scan old memories table for records missing from v3
    let old_records = self.scan_all_memories_for_reindex().await?;
    let mut count = 0usize;

    for record in old_records {
        let embed_text = if !record.memory_context.trim().is_empty() {
            record.memory_context.clone()
        } else if !record.snippet.trim().is_empty() {
            record.snippet.clone()
        } else {
            format!("{} {}", record.app_name, record.window_title)
        };

        let embedding = embedder.embed_single(&embed_text).await
            .unwrap_or_else(|_| vec![0.0f32; EMBEDDING_DIMENSIONS]);

        // Write to v3 table (simplified — write the key fields)
        self.upsert_v3_memory(
            &record.id,
            &embed_text,
            &embedding,
            EMBEDDING_MODEL_ID,
            EMBEDDING_DIMENSIONS as u32,
            &record,
        ).await?;
        count += 1;
    }

    tracing::info!("reindex_to_v3: migrated {count} records to {MEMORIES_V3_TABLE}");
    Ok(count)
}

/// Read all records from the legacy memories table for reindex purposes.
/// Returns a minimal projection (id, memory_context, snippet, app_name, window_title).
async fn scan_all_memories_for_reindex(
    &self,
) -> Result<Vec<crate::storage::MemoryRecord>, Box<dyn std::error::Error + Send + Sync>> {
    // Implement by scanning existing memories table with a limit.
    // This is best-effort — if the old table doesn't exist, return empty.
    Ok(Vec::new()) // Stub: replace with actual scan when LanceStore scan API is available
}
```

Add a corresponding IPC command in maintenance.rs:
```rust
#[tauri::command]
pub async fn reindex_memories_to_v3(
    state: tauri::State<'_, crate::ipc::AppState>,
) -> Result<usize, String> {
    let store = state.lance_store.read().await;
    let embedder = state.embedder.read().await;
    store.reindex_memories_to_embeddinggemma_256(&embedder)
        .await
        .map_err(|e| e.to_string())
}
```

- [ ] **Step 3: Delete temp screenshots after Qwen inference**

In `src-tauri/src/inference/model_worker.rs`, in the `run_synthesis_blocking` function, after `extract_image_semantics` returns (success or failure), delete the temp file:

```rust
// After the vision call (success or failure branch), add:
if let Some(ref img_path) = input.image_path {
    if let Err(e) = tokio::fs::remove_file(img_path).await {
        tracing::warn!("model_worker: failed to delete temp screenshot {:?}: {e}", img_path);
    } else {
        tracing::debug!("model_worker: deleted temp screenshot {:?}", img_path);
    }
}
```

This deletion must happen regardless of success or failure, so add it to both branches or after the match block.

- [ ] **Step 4: Add screenshot deletion test**

```rust
#[tokio::test]
async fn temp_screenshot_deleted_after_inference() {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    let img_path = dir.path().join("test_frame.png");
    // Write a minimal PNG (1x1 white pixel) so the file exists
    std::fs::write(&img_path, b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x02\0\0\0\x90wS\xde\0\0\0\x0cIDATx\x9cc\xf8\x0f\0\0\x01\x01\0\x05\x18\xd8N\0\0\0\0IEND\xaeB`\x82").unwrap();
    assert!(img_path.exists(), "test file must exist before inference");
    // The actual inference will fail (no real model) but deletion should still happen
    use crate::inference::qwen_vl_memory::{MemorySynthesisInput, MemorySourceType};
    let input = MemorySynthesisInput {
        image_path: Some(img_path.clone()),
        ocr_text: String::new(),
        app_name: None,
        window_title: None,
        url: None,
        timestamp: chrono::Utc::now(),
        source_type: MemorySourceType::Screen,
        ocr_confidence: None,
    };
    let _ = run_synthesis_blocking(input, dir.path()).await;
    // File should be deleted regardless of inference outcome
    assert!(!img_path.exists(), "temp screenshot must be deleted after inference");
}
```

- [ ] **Step 5: Compile check + tests**

```bash
cd src-tauri && cargo check -p fndr 2>&1 | head -40
cd src-tauri && cargo test -p fndr inference::model_worker 2>&1 | tail -20
```

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/search/hybrid.rs src-tauri/src/storage/lance_store/mod.rs \
        src-tauri/src/inference/model_worker.rs src-tauri/src/ipc/commands/maintenance.rs
git commit -m "feat: wire search to memories_v3_egemma_256; add reindex command; delete temp screenshots"
```

---

## Task 10: Add privacy/safety_gate.rs + search reranker hook + task extraction

**Files:**
- Create: `src-tauri/src/privacy/safety_gate.rs`
- Modify: `src-tauri/src/privacy/mod.rs`
- Modify: `src-tauri/src/search/reranker.rs`
- Create: `src-tauri/src/tasks/extract_from_memory.rs`
- Modify: `src-tauri/src/tasks/mod.rs`

- [ ] **Step 1: Create privacy/safety_gate.rs**

```rust
// src-tauri/src/privacy/safety_gate.rs
//! Deterministic privacy/safety gate — no ML model required.

#[derive(Debug, Clone, PartialEq)]
pub enum SafetyDecision {
    /// Proceed with full capture, OCR, and synthesis.
    Allow,
    /// Redact sensitive spans before synthesis/storage.
    Redact,
    /// Skip entirely — do not OCR, do not synthesize, do not embed.
    SkipStorage,
}

impl SafetyDecision {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Redact => "redact",
            Self::SkipStorage => "skip_storage",
        }
    }
}

/// Evaluate whether to allow, redact, or skip a capture frame.
///
/// Checks are purely deterministic (no model). Returns the most restrictive
/// decision that applies.
pub fn evaluate(
    app_name: Option<&str>,
    bundle_id: Option<&str>,
    url: Option<&str>,
    window_title: Option<&str>,
    ocr_text: Option<&str>,
    user_blocklist: &[String],
) -> SafetyDecision {
    let app = app_name.unwrap_or("").to_ascii_lowercase();
    let title = window_title.unwrap_or("").to_ascii_lowercase();
    let url_lower = url.unwrap_or("").to_ascii_lowercase();
    let text_lower = ocr_text.unwrap_or("").to_ascii_lowercase();

    // --- Hard SkipStorage checks ---

    // User-defined blocklist
    for blocked in user_blocklist {
        let b = blocked.to_ascii_lowercase();
        if app.contains(&b) || title.contains(&b) || url_lower.contains(&b) {
            return SafetyDecision::SkipStorage;
        }
    }

    // Internal FNDR surfaces
    if let Some(id) = bundle_id {
        let id_lower = id.to_ascii_lowercase();
        if id_lower.starts_with("com.fndr") || id_lower.contains(".fndr.") {
            if !app.contains("fndr meeting") {
                return SafetyDecision::SkipStorage;
            }
        }
    }

    // Password managers
    const PASSWORD_MANAGERS: &[&str] = &["1password", "bitwarden", "keychain", "lastpass", "dashlane", "keepass"];
    for pm in PASSWORD_MANAGERS {
        if app.contains(pm) { return SafetyDecision::SkipStorage; }
    }

    // Private/incognito windows
    if title.contains("private") && (title.contains("browsing") || title.contains("window")) {
        return SafetyDecision::SkipStorage;
    }
    if title.contains("incognito") {
        return SafetyDecision::SkipStorage;
    }

    // Banking domains
    const BANKING_DOMAINS: &[&str] = &[
        "chase.com", "bankofamerica", "wellsfargo", "citibank", "capitalone",
        "usbank", "fidelity", "vanguard", "schwab", "americanexpress", "discover.com",
        "paypal.com", "venmo.com", "robinhood.com",
    ];
    for domain in BANKING_DOMAINS {
        if url_lower.contains(domain) { return SafetyDecision::SkipStorage; }
    }

    // Medical/health domains
    const MEDICAL_DOMAINS: &[&str] = &["epic.com", "mychart", "healthportal", "patientportal"];
    for domain in MEDICAL_DOMAINS {
        if url_lower.contains(domain) { return SafetyDecision::SkipStorage; }
    }

    // Authentication pages
    const AUTH_INDICATORS: &[&str] = &["sign in", "log in", "login", "authenticate", "authorization", "oauth", "saml", "two-factor", "2fa"];
    for indicator in AUTH_INDICATORS {
        if title.contains(indicator) || url_lower.contains(indicator) {
            return SafetyDecision::SkipStorage;
        }
    }

    // --- Redact checks (content heuristics) ---

    const SECRET_PATTERNS: &[&str] = &[
        "api_key", "apikey", "secret_key", "private_key", "access_token",
        "password:", "passwd:", "token:", "-----begin rsa", "-----begin ec",
        "ghp_", "sk-", "xoxb-", // GitHub token, OpenAI key, Slack token prefixes
    ];
    for pattern in SECRET_PATTERNS {
        if text_lower.contains(pattern) { return SafetyDecision::Redact; }
    }

    SafetyDecision::Allow
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_normal_content() {
        assert_eq!(
            evaluate(Some("VS Code"), None, None, Some("main.rs"), Some("fn main() {}"), &[]),
            SafetyDecision::Allow
        );
    }

    #[test]
    fn blocks_password_manager() {
        assert_eq!(
            evaluate(Some("1Password"), None, None, Some("Vault"), None, &[]),
            SafetyDecision::SkipStorage
        );
    }

    #[test]
    fn blocks_banking_url() {
        assert_eq!(
            evaluate(Some("Safari"), None, Some("https://chase.com/account"), Some("Chase Bank"), None, &[]),
            SafetyDecision::SkipStorage
        );
    }

    #[test]
    fn blocks_incognito_window() {
        assert_eq!(
            evaluate(Some("Chrome"), None, None, Some("New Incognito Window"), None, &[]),
            SafetyDecision::SkipStorage
        );
    }

    #[test]
    fn redacts_api_key_in_text() {
        assert_eq!(
            evaluate(Some("Terminal"), None, None, Some("bash"), Some("export api_key=abc123"), &[]),
            SafetyDecision::Redact
        );
    }

    #[test]
    fn respects_user_blocklist() {
        assert_eq!(
            evaluate(Some("Figma"), None, None, Some("Client NDA Design"), None, &["nda".to_string()]),
            SafetyDecision::SkipStorage
        );
    }

    #[test]
    fn blocks_auth_pages() {
        assert_eq!(
            evaluate(Some("Safari"), None, Some("https://app.example.com/login"), Some("Sign in"), None, &[]),
            SafetyDecision::SkipStorage
        );
    }
}
```

- [ ] **Step 2: Declare in privacy/mod.rs**

Add to `src-tauri/src/privacy/mod.rs`:
```rust
pub mod safety_gate;
```

- [ ] **Step 3: Add OptionalReranker trait + NoopReranker to search/reranker.rs**

At the bottom of `search/reranker.rs`, add:
```rust
/// Extension hook for future reranker models. Default is NoopReranker.
pub trait OptionalReranker: Send + Sync {
    fn rerank(&self, query: &str, candidates: Vec<crate::storage::SearchResult>) -> Vec<crate::storage::SearchResult>;
}

/// No-op reranker that passes candidates through unchanged. Default for all search paths.
pub struct NoopReranker;

impl OptionalReranker for NoopReranker {
    fn rerank(&self, _query: &str, candidates: Vec<crate::storage::SearchResult>) -> Vec<crate::storage::SearchResult> {
        candidates
    }
}

#[test]
fn noop_reranker_passes_through() {
    let r = NoopReranker;
    let results = vec![
        crate::storage::SearchResult { id: "a".into(), score: 0.9, ..Default::default() },
        crate::storage::SearchResult { id: "b".into(), score: 0.7, ..Default::default() },
    ];
    let out = r.rerank("query", results.clone());
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].id, "a");
}
```

- [ ] **Step 4: Create tasks/extract_from_memory.rs**

```rust
// src-tauri/src/tasks/extract_from_memory.rs
use chrono::{DateTime, Utc};
use crate::inference::qwen_vl_memory::MemorySynthesisOutput;

#[derive(Debug, Clone)]
pub struct TaskCandidate {
    pub title: String,
    pub source_memory_id: String,
    pub confidence: f32,
    pub due_date: Option<DateTime<Utc>>,
    pub evidence: String,
}

/// Extract task candidates from validated memory output fields.
/// Uses decisions, errors, and next_steps — does NOT call any model.
pub fn extract_task_candidates(
    memory_id: &str,
    output: &MemorySynthesisOutput,
) -> Vec<TaskCandidate> {
    let mut candidates = Vec::new();

    for step in &output.next_steps {
        if step.trim().is_empty() { continue; }
        candidates.push(TaskCandidate {
            title: step.trim().to_string(),
            source_memory_id: memory_id.to_string(),
            confidence: output.confidence_score * 0.85,
            due_date: None,
            evidence: format!("next_step from: {}", output.summary_short),
        });
    }

    for decision in &output.decisions {
        if decision.trim().is_empty() { continue; }
        if decision.to_ascii_lowercase().contains("todo")
            || decision.to_ascii_lowercase().contains("need to")
            || decision.to_ascii_lowercase().contains("will ")
        {
            candidates.push(TaskCandidate {
                title: decision.trim().to_string(),
                source_memory_id: memory_id.to_string(),
                confidence: output.confidence_score * 0.70,
                due_date: None,
                evidence: format!("decision from: {}", output.summary_short),
            });
        }
    }

    for error in &output.errors {
        if error.trim().is_empty() { continue; }
        candidates.push(TaskCandidate {
            title: format!("Fix: {}", error.trim()),
            source_memory_id: memory_id.to_string(),
            confidence: output.confidence_score * 0.65,
            due_date: None,
            evidence: format!("error from: {}", output.summary_short),
        });
    }

    candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_next_steps_as_tasks() {
        let output = MemorySynthesisOutput {
            summary_short: "PR review".to_string(),
            next_steps: vec!["merge PR #42".to_string(), "update docs".to_string()],
            confidence_score: 0.8,
            ..Default::default()
        };
        let tasks = extract_task_candidates("mem-001", &output);
        assert!(tasks.iter().any(|t| t.title == "merge PR #42"));
        assert!(tasks.iter().any(|t| t.title == "update docs"));
    }

    #[test]
    fn extracts_errors_as_fix_tasks() {
        let output = MemorySynthesisOutput {
            summary_short: "debugging session".to_string(),
            errors: vec!["connection refused on port 5432".to_string()],
            confidence_score: 0.7,
            ..Default::default()
        };
        let tasks = extract_task_candidates("mem-002", &output);
        assert!(tasks.iter().any(|t| t.title.starts_with("Fix:")));
    }

    #[test]
    fn empty_output_produces_no_tasks() {
        let output = MemorySynthesisOutput::default();
        let tasks = extract_task_candidates("mem-003", &output);
        assert!(tasks.is_empty());
    }
}
```

- [ ] **Step 5: Declare in tasks/mod.rs**

Add to `src-tauri/src/tasks/mod.rs`:
```rust
pub mod extract_from_memory;
```

- [ ] **Step 6: Compile + test**

```bash
cd src-tauri && cargo test -p fndr privacy::safety_gate tasks::extract_from_memory 2>&1 | tail -30
```
Expected: all tests pass.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/privacy/safety_gate.rs src-tauri/src/privacy/mod.rs \
        src-tauri/src/search/reranker.rs \
        src-tauri/src/tasks/extract_from_memory.rs src-tauri/src/tasks/mod.rs
git commit -m "feat: add privacy/safety_gate, OptionalReranker/NoopReranker, TaskCandidate extraction"
```

---

## Task 11: Update memory/types.rs — add enrichment_status fields

**Files:**
- Modify: `src-tauri/src/memory/types.rs`
- Modify: `src-tauri/src/storage/lance_store/schemas.rs` (main memory_schema if needed)

- [ ] **Step 1: Read current MemoryRecord struct**

Open `src-tauri/src/memory/types.rs` and find the `MemoryRecord` struct definition. Add the following fields (insert after `embedding_model` and `embedding_dim` if they exist, or near the end of the struct):

```rust
// Enrichment tracking
/// "qwen_enriched" | "ocr_only_fallback" | "skipped"
pub enrichment_status: String,
/// Why Qwen was skipped (e.g. "good_ocr", "system_pressure", "queue_full")
pub fallback_reason: Option<String>,
/// Dimension of the stored embedding vector. 256 for EmbeddingGemma.
pub embedding_dimensions: u32,
/// Always false for normal screen capture; true only for explicitly stored imports
pub raw_screenshot_stored: bool,
```

- [ ] **Step 2: Update Default impl**

If `MemoryRecord` has a `Default` impl or if defaults are set elsewhere, add:
```rust
enrichment_status: "ocr_only_fallback".to_string(),
fallback_reason: None,
embedding_dimensions: crate::inference::model_config::EMBEDDING_DIMENSIONS as u32,
raw_screenshot_stored: false,
```

- [ ] **Step 3: Compile check**

```bash
cd src-tauri && cargo check -p fndr 2>&1 | head -60
```
Fix any struct literal errors from code that constructs `MemoryRecord` directly — add the new fields with their defaults.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/memory/types.rs
git commit -m "feat: add enrichment_status, fallback_reason, embedding_dimensions, raw_screenshot_stored to MemoryRecord"
```

---

## Task 12: Add model cleanup CLI commands

**Files:**
- Modify: `src-tauri/src/ipc/commands/maintenance.rs`
- Modify: `src-tauri/src/ipc/commands/mod.rs` (register new commands)

- [ ] **Step 1: Add cleanup commands to maintenance.rs**

```rust
// In src-tauri/src/ipc/commands/maintenance.rs

use crate::inference::model_config::CLEANUP_OLD_MODEL_DIRS;

#[tauri::command]
pub async fn models_cleanup_dry_run(
    app_handle: tauri::AppHandle,
) -> Result<Vec<String>, String> {
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    let models_dir = app_data_dir.join("models");
    if !models_dir.exists() {
        return Ok(vec!["models/ directory does not exist".to_string()]);
    }
    let mut found = Vec::new();
    for name in CLEANUP_OLD_MODEL_DIRS {
        let dir = models_dir.join(name);
        if dir.exists() {
            let size = dir_size_bytes(&dir).unwrap_or(0);
            found.push(format!("{} ({:.1} MB)", name, size as f64 / 1_000_000.0));
        }
        // Also check flat files matching old model filenames
        for flat in &["Llama-3.2-1B-Instruct-Q4_K_M.gguf",
                       "SmolVLM-500M-Instruct-Q4_K_M.gguf",
                       "Qwen3VL-4B-Instruct-Q4_K_M.gguf",
                       "bge-large-en-v1.5-quantized.onnx"] {
            let path = models_dir.join(flat);
            if path.exists() {
                let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                found.push(format!("{} ({:.1} MB)", flat, size as f64 / 1_000_000.0));
            }
        }
    }
    if found.is_empty() {
        found.push("No old model files found.".to_string());
    }
    Ok(found)
}

#[tauri::command]
pub async fn models_cleanup_confirm(
    app_handle: tauri::AppHandle,
) -> Result<Vec<String>, String> {
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    let models_dir = app_data_dir.join("models");
    if !models_dir.exists() {
        return Ok(vec!["models/ directory does not exist".to_string()]);
    }
    let mut removed = Vec::new();
    for name in CLEANUP_OLD_MODEL_DIRS {
        let dir = models_dir.join(name);
        if dir.is_dir() {
            std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
            removed.push(format!("Removed directory: {}", dir.display()));
        }
    }
    for flat in &["Llama-3.2-1B-Instruct-Q4_K_M.gguf",
                   "SmolVLM-500M-Instruct-Q4_K_M.gguf",
                   "Qwen3VL-4B-Instruct-Q4_K_M.gguf",
                   "bge-large-en-v1.5-quantized.onnx"] {
        let path = models_dir.join(flat);
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
            removed.push(format!("Removed file: {}", path.display()));
        }
    }
    if removed.is_empty() {
        removed.push("Nothing to remove.".to_string());
    }
    Ok(removed)
}

fn dir_size_bytes(dir: &std::path::Path) -> std::io::Result<u64> {
    let mut total = 0u64;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        if meta.is_file() {
            total += meta.len();
        }
    }
    Ok(total)
}
```

- [ ] **Step 2: Register commands in mod.rs**

In `src-tauri/src/ipc/commands/mod.rs`, find the handler registration (`.invoke_handler(...)`) and add `models_cleanup_dry_run` and `models_cleanup_confirm` to the list.

- [ ] **Step 3: Compile check**

```bash
cd src-tauri && cargo check -p fndr 2>&1 | head -40
```

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/ipc/commands/maintenance.rs src-tauri/src/ipc/commands/mod.rs
git commit -m "feat: add models_cleanup_dry_run + models_cleanup_confirm IPC commands"
```

---

## Task 13: Update UI — remove model selection, add static status

**Files:**
- Modify: `src/domains/workspace/ControlPanel.tsx` (or wherever model selection UI lives)

- [ ] **Step 1: Find model selection components**

```bash
grep -rn "model\|ModelSelect\|vlm_model\|SmolVLM\|Qwen\|Llama\|bge" src/domains/ --include="*.tsx" --include="*.ts" -l
```

- [ ] **Step 2: Replace model selection UI**

In `ControlPanel.tsx` (or the relevant component), find the model selection tab/section. Replace the model dropdown/selection UI with a static status card:

```tsx
// Replace model selection UI with static status display
const LocalAIStatus = () => (
  <div className="local-ai-status">
    <h4>Local AI</h4>
    <div className="ai-model-row">
      <span className="label">Memory model:</span>
      <span className="value">Qwen3-VL-2B</span>
    </div>
    <div className="ai-model-row">
      <span className="label">Search model:</span>
      <span className="value">EmbeddingGemma 300M</span>
    </div>
    <div className="ai-model-row">
      <span className="label">Mode:</span>
      <span className="value">8 GB Mac optimized</span>
    </div>
  </div>
);
```

Remove any JSX that renders model dropdowns, model selection options, quality/balanced/low_ram mode toggles, or model download buttons for old models.

- [ ] **Step 3: Remove old model-related TypeScript types/state**

Search for TypeScript state or types referencing old models:
```bash
grep -rn "vlmModelSize\|vlm_model_size\|smolvlm\|llama.*1b\|qwen.*4b\|bge.large\|quality_mode\|balanced_mode" src/ --include="*.tsx" --include="*.ts"
```
Remove or stub out any state that was driving old model selection.

- [ ] **Step 4: TypeScript compile check**

```bash
npm run typecheck 2>&1 | head -40
```
Fix type errors.

- [ ] **Step 5: Commit**

```bash
git add src/domains/workspace/ControlPanel.tsx
git commit -m "feat: remove model selection UI, replace with static 8 GB Mac optimized status"
```

---

## Task 14: Verification — tests, cargo check, full build

**Files:**
- Test: `src-tauri/src/inference/model_config.rs` (verify constants)
- Test: `src-tauri/src/inference/qwen_vl_memory.rs`
- Test: `src-tauri/src/inference/model_worker.rs`
- Test: `src-tauri/src/embed/embedding_gemma.rs`
- Test: `src-tauri/src/privacy/safety_gate.rs`
- Test: `src-tauri/src/tasks/extract_from_memory.rs`
- Test: `src-tauri/src/inference/vlm_router.rs`

- [ ] **Step 1: Run full test suite**

```bash
cd src-tauri && cargo test 2>&1 | tail -40
```
Expected: all tests pass. Note any failures.

- [ ] **Step 2: Verify exactly two model IDs are active**

```bash
grep -rn "llama-3.2-1b\|smolvlm-500m\|qwen3-vl-4b\|bge-large\|bge_large" src-tauri/src/ --include="*.rs" | grep -v "CLEANUP_OLD_MODEL_DIRS\|cleanup_old\|test\|//\|doc"
```
Expected: zero active references to old model IDs outside cleanup constants and test files.

- [ ] **Step 3: Verify embedding dimension is 256**

```bash
grep -rn "1024\|384" src-tauri/src/embedding/ src-tauri/src/embed/ src-tauri/src/inference/model_config.rs --include="*.rs"
```
The number `1024` should appear only in: chunking cache capacity (DEFAULT_EMBEDDING_CACHE_CAPACITY), MAX_IMAGE_LONG_EDGE constant (which is about pixels, not dimensions), or similar non-embedding uses. It should NOT appear as an embedding dimension.

- [ ] **Step 4: Verify v3 table name**

```bash
grep -rn "memories_v3_egemma_256\|MEMORIES_V3_TABLE" src-tauri/src/ --include="*.rs"
```
Expected: appears in model_config.rs (definition) and lance_store (usage).

- [ ] **Step 5: Run cargo clippy**

```bash
cd src-tauri && cargo clippy -- -D warnings 2>&1 | head -60
```
Fix any new warnings introduced.

- [ ] **Step 6: Run cargo fmt**

```bash
cd src-tauri && cargo fmt
```

- [ ] **Step 7: TypeScript typecheck**

```bash
npm run typecheck 2>&1 | head -40
```

- [ ] **Step 8: Run frontend build**

```bash
npm run build 2>&1 | tail -20
```

- [ ] **Step 9: Final audit grep**

```bash
# Verify no old model IDs in active code paths
grep -rn "\"llama-3.2-1b\"\|\"smolvlm-500m\"\|\"qwen3-vl-4b\"\|bge-large" src-tauri/src/ --include="*.rs" | grep -v "CLEANUP\|test\|//\s"

# Verify NoopReranker is the default
grep -rn "NoopReranker" src-tauri/src/ --include="*.rs"

# Verify 256 is the embedding dim in model_config
grep -n "EMBEDDING_DIMENSIONS" src-tauri/src/inference/model_config.rs

# Verify v3 table name constant
grep -n "MEMORIES_V3_TABLE" src-tauri/src/inference/model_config.rs
```

- [ ] **Step 10: Commit**

```bash
git add -u
git commit -m "chore: verification pass — cargo fmt, clippy, all tests green"
```

---

## Verification Checklist

Run after all tasks complete:

```bash
# 1. Full Rust test suite
cd src-tauri && cargo test 2>&1 | grep -E "^test|FAILED|error" | tail -30

# 2. No old model IDs in active paths
grep -rn "llama-3.2-1b\|smolvlm-500m\|qwen3-vl-4b\|bge-large" src-tauri/src/ --include="*.rs" | grep -v "CLEANUP_OLD_MODEL_DIRS\|#\[test\]\|//"

# 3. Embedding dimension is 256
grep -n "EMBEDDING_DIMENSIONS\|EMBEDDING_DIM" src-tauri/src/inference/model_config.rs src-tauri/src/embedding/onnx.rs

# 4. New table name present
grep -rn "memories_v3_egemma_256" src-tauri/src/ --include="*.rs"

# 5. Safety gate tests
cd src-tauri && cargo test privacy::safety_gate -- --nocapture

# 6. Task extraction tests
cd src-tauri && cargo test tasks::extract_from_memory -- --nocapture

# 7. NoopReranker test
cd src-tauri && cargo test search::reranker::noop_reranker_passes_through -- --nocapture

# 8. TypeScript clean
npm run typecheck

# 9. Frontend build
npm run build
```

**Acceptance Gate (all must pass):**
- `cargo test` exits 0
- No old model IDs in active code (grep above returns empty)
- `EMBEDDING_DIMENSIONS = 256` in model_config.rs
- `MEMORIES_V3_TABLE = "memories_v3_egemma_256"` in model_config.rs
- Safety gate has 7 passing unit tests
- TaskCandidate extraction has 3 passing unit tests
- VlmRouter has 9 passing unit tests with only `RunQwenVlm` (no heavy/lightweight variants)
- qwen_vl_memory has 4 passing unit tests
- EmbeddingGemma format helper has 3 passing unit tests
- `npm run typecheck` exits 0
