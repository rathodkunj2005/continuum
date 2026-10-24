use crate::config::Config;
use crate::inference::model_config::{
    EMBEDDING_GEMMA2_MODEL_DOWNLOAD_URL, EMBEDDING_GEMMA2_MODEL_FILENAME,
    EMBEDDING_GEMMA2_MODEL_SHA256, EMBEDDING_GEMMA2_MODEL_SIZE_BYTES,
    MULTIMODAL_MODEL_DOWNLOAD_URL, MULTIMODAL_MODEL_FILENAME, MULTIMODAL_MODEL_ID,
    MULTIMODAL_MODEL_RAM_GB, MULTIMODAL_MODEL_SIZE_BYTES, QWEN3_VL_2B_MAIN_GGUF_MIN_BYTES,
};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Additional file a model needs beyond its main weights (e.g. a tokenizer).
#[derive(Debug, Clone, Copy)]
pub struct ExtraModelFile {
    pub filename: &'static str,
    pub download_url: &'static str,
    pub sha256: Option<&'static str>,
}

#[derive(Debug, Clone, Copy)]
pub struct ModelDefinition {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub size_bytes: u64,
    pub size_label: &'static str,
    pub quality_label: &'static str,
    pub speed_label: &'static str,
    pub ram_gb: f32,
    pub recommended: bool,
    /// Required models are auto-installed by onboarding rather than offered
    /// as a choice; core features stay gated until they are on disk.
    pub required: bool,
    pub filename: &'static str,
    pub download_url: &'static str,
    /// Pinned hex digest for `filename`; `None` skips verification.
    pub sha256: Option<&'static str>,
    /// Every extra file must be present for the model to count as available.
    pub extra_files: &'static [ExtraModelFile],
}

pub const EMBEDDER_MODEL_ID: &str = "embeddinggemma-2-q8";

pub const MODEL_CATALOG: [ModelDefinition; 2] = [
    ModelDefinition {
        id: MULTIMODAL_MODEL_ID,
        name: "Qwen3-VL · 2B",
        description: "Multimodal memory model for 8 GB M1 Mac. Reads screenshots, OCR text, and GUI context to create structured memory records.",
        size_bytes: MULTIMODAL_MODEL_SIZE_BYTES,
        size_label: "~1.5 GB",
        quality_label: "Excellent",
        speed_label: "Balanced",
        ram_gb: MULTIMODAL_MODEL_RAM_GB,
        recommended: true,
        required: false,
        filename: MULTIMODAL_MODEL_FILENAME,
        download_url: MULTIMODAL_MODEL_DOWNLOAD_URL,
        sha256: None,
        extra_files: &[],
    },
    ModelDefinition {
        id: EMBEDDER_MODEL_ID,
        name: "EmbeddingGemma 2 · Search Embedder",
        description: "Required search embedding model (768-d). Capture stays paused until this model is installed.",
        size_bytes: EMBEDDING_GEMMA2_MODEL_SIZE_BYTES,
        size_label: "~310 MB",
        quality_label: "Required",
        speed_label: "Fast",
        ram_gb: 0.6,
        recommended: false,
        required: true,
        filename: EMBEDDING_GEMMA2_MODEL_FILENAME,
        download_url: EMBEDDING_GEMMA2_MODEL_DOWNLOAD_URL,
        sha256: Some(EMBEDDING_GEMMA2_MODEL_SHA256),
        extra_files: &[],
    },
];

#[derive(Debug, Clone)]
pub struct ResolvedModel {
    pub definition: &'static ModelDefinition,
    pub path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct StoredOnboardingState {
    model_id: Option<String>,
}

pub fn catalog() -> &'static [ModelDefinition] {
    &MODEL_CATALOG
}

pub fn model_by_id(model_id: &str) -> Option<&'static ModelDefinition> {
    MODEL_CATALOG.iter().find(|model| model.id == model_id)
}

/// Candidate mmproj filenames for Qwen3-VL-2B.
pub const QWEN3_VL_2B_MMPROJ_FILENAMES: &[&str] = &[
    "mmproj-Qwen3VL-2B-Instruct-F16.gguf",
    "mmproj-Qwen3VL-2B-Instruct-Q8_0.gguf",
    "mmproj-Qwen3-VL-2B-Instruct-F16.gguf",
    "mmproj-Qwen3-VL-2B-Instruct-Q8_0.gguf",
];

pub fn resolve_qwen3_vl_2b_mmproj(app_data_dir: Option<&Path>) -> Option<PathBuf> {
    for dir in candidate_model_dirs(app_data_dir) {
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
    is_model_available(
        crate::inference::model_config::MULTIMODAL_MODEL_ID,
        app_data_dir,
    ) && resolve_qwen3_vl_2b_mmproj(app_data_dir).is_some()
}

/// Whether a pixel MTMD runtime can load for the selected model tier.
pub fn pixel_vlm_available(_model_id: Option<&str>, app_data_dir: Option<&Path>) -> bool {
    qwen3_vl_2b_fully_available(app_data_dir)
}

/// Effective pixel-VLM model id from runtime config.
///
/// Always returns the single canonical multimodal model ID.
pub fn configured_vlm_model_id(_config: &Config) -> Option<String> {
    Some(crate::inference::model_config::MULTIMODAL_MODEL_ID.to_string())
}

/// Returns `Err` if `path` exists but is too small to be a real Qwen3-VL-2B weights file.
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

pub fn models_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("models")
}

pub fn partial_model_path(app_data_dir: &Path, filename: &str) -> PathBuf {
    models_dir(app_data_dir).join(format!("{filename}.partial"))
}

pub fn preferred_model_id_from_onboarding(app_data_dir: &Path) -> Option<String> {
    let onboarding_path = app_data_dir.join("onboarding.json");
    let raw = std::fs::read_to_string(onboarding_path).ok()?;
    serde_json::from_str::<StoredOnboardingState>(&raw)
        .ok()?
        .model_id
}

/// GGUF id passed to [`crate::inference::InferenceEngine`] resolution.
///
/// Always returns the single canonical multimodal model ID.
pub fn inference_preferred_model_id(_app_data_dir: &Path, _config: &Config) -> Option<String> {
    Some(crate::inference::model_config::MULTIMODAL_MODEL_ID.to_string())
}

pub fn candidate_model_dirs(app_data_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Some(data_dir) = app_data_dir {
        dirs.push(models_dir(data_dir));
    }

    dirs.push(PathBuf::from("models"));
    dirs.push(PathBuf::from("src-tauri/models"));

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(parent) = exe_path.parent() {
            dirs.push(parent.join("models"));
            dirs.push(parent.join("../Resources/models"));
        }
    }

    if let Some(data_dir) = dirs::data_dir() {
        dirs.push(data_dir.join("fndr/models"));
    }

    let mut seen = HashSet::new();
    dirs.into_iter()
        .filter(|dir| seen.insert(dir.clone()))
        .collect()
}

pub fn is_model_available(model_id: &str, app_data_dir: Option<&Path>) -> bool {
    resolve_specific_model(model_id, app_data_dir).is_some()
}

pub fn resolve_model(
    preferred_model_id: Option<&str>,
    app_data_dir: Option<&Path>,
) -> Option<ResolvedModel> {
    let def = if let Some(id) = preferred_model_id {
        model_by_id(id)?
    } else {
        MODEL_CATALOG.first()?
    };
    for dir in candidate_model_dirs(app_data_dir) {
        for search_dir in [dir.clone(), dir.join(def.id)] {
            let path = search_dir.join(def.filename);
            if path.is_file() {
                return Some(ResolvedModel {
                    definition: def,
                    path,
                });
            }
        }
    }
    None
}

fn resolve_specific_model(model_id: &str, app_data_dir: Option<&Path>) -> Option<ResolvedModel> {
    let definition = model_by_id(model_id)?;

    for dir in candidate_model_dirs(app_data_dir) {
        for search_dir in [dir.clone(), dir.join(model_id)] {
            let path = search_dir.join(definition.filename);
            let extras_present = definition
                .extra_files
                .iter()
                .all(|extra| search_dir.join(extra.filename).is_file());
            if path.is_file() && extras_present {
                return Some(ResolvedModel { definition, path });
            }
        }
    }
    None
}

/// Pinned digest for one of a model's files, whether main weights or extra.
pub fn expected_sha256_for(model_id: &str, filename: &str) -> Option<&'static str> {
    let definition = model_by_id(model_id)?;
    if definition.filename == filename {
        return definition.sha256;
    }
    definition
        .extra_files
        .iter()
        .find(|extra| extra.filename == filename)
        .and_then(|extra| extra.sha256)
}

/// Streams `path` through SHA-256 and compares against `expected_hex`.
pub fn verify_file_sha256(path: &Path, expected_hex: &str) -> Result<(), String> {
    use sha2::{Digest, Sha256};

    let mut file =
        std::fs::File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).map_err(|e| format!("read {}: {e}", path.display()))?;
    let actual = format!("{:x}", hasher.finalize());
    if actual.eq_ignore_ascii_case(expected_hex) {
        Ok(())
    } else {
        Err(format!(
            "sha256 mismatch for {}: expected {expected_hex}, got {actual}",
            path.display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_temp_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!("fndr-model-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn resolve_model_prefers_app_data_dir() {
        let temp_dir = make_temp_dir();
        let model_dir = models_dir(&temp_dir);
        std::fs::create_dir_all(&model_dir).unwrap();
        let expected_path = model_dir.join("Qwen3VL-2B-Instruct-Q4_K_M.gguf");
        std::fs::write(&expected_path, b"test").unwrap();

        let resolved = resolve_model(Some("qwen3-vl-2b"), Some(temp_dir.as_path())).unwrap();

        assert_eq!(resolved.definition.id, "qwen3-vl-2b");
        assert_eq!(resolved.path, expected_path);

        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn partial_file_does_not_count_as_downloaded() {
        let temp_dir = make_temp_dir();
        let model_dir = models_dir(&temp_dir);
        std::fs::create_dir_all(&model_dir).unwrap();
        let partial_path = partial_model_path(&temp_dir, "Qwen3VL-2B-Instruct-Q4_K_M.gguf");
        std::fs::write(&partial_path, b"partial").unwrap();

        let resolved = resolve_model(Some("qwen3-vl-2b"), Some(temp_dir.as_path()));
        assert_ne!(
            resolved.as_ref().map(|model| model.path.clone()),
            Some(partial_path)
        );

        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn resolve_qwen3_vl_2b_mmproj_finds_known_filename() {
        let temp_dir = make_temp_dir();
        let model_dir = models_dir(&temp_dir);
        std::fs::create_dir_all(&model_dir).unwrap();
        let mm = model_dir.join("mmproj-Qwen3-VL-2B-Instruct-F16.gguf");
        std::fs::write(&mm, b"x").unwrap();
        let found = resolve_qwen3_vl_2b_mmproj(Some(temp_dir.as_path()));
        assert_eq!(found, Some(mm));
        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn resolve_qwen3_vl_2b_mmproj_finds_huggingface_qwen3vl_filename() {
        let temp_dir = make_temp_dir();
        let model_dir = models_dir(&temp_dir);
        std::fs::create_dir_all(&model_dir).unwrap();
        let mm = model_dir.join("mmproj-Qwen3VL-2B-Instruct-Q8_0.gguf");
        std::fs::write(&mm, b"x").unwrap();
        let found = resolve_qwen3_vl_2b_mmproj(Some(temp_dir.as_path()));
        assert_eq!(found, Some(mm));
        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn resolve_qwen3_vl_2b_mmproj_finds_subdirectory_layout() {
        let temp_dir = make_temp_dir();
        let model_dir = models_dir(&temp_dir).join("qwen3-vl-2b");
        std::fs::create_dir_all(&model_dir).unwrap();
        let mm = model_dir.join("mmproj-Qwen3VL-2B-Instruct-F16.gguf");
        std::fs::write(&mm, b"x").unwrap();
        let found = resolve_qwen3_vl_2b_mmproj(Some(temp_dir.as_path()));
        assert_eq!(found, Some(mm));
        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn validate_qwen3_vl_2b_main_gguf_file_rejects_small_file() {
        let temp_dir = make_temp_dir();
        let p = models_dir(&temp_dir).join("Qwen3VL-2B-Instruct-Q4_K_M.gguf");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"x").unwrap();
        let err = validate_qwen3_vl_2b_main_gguf_file(&p).expect_err("tiny file");
        assert!(err.contains("bytes"), "{err}");
        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn validate_qwen3_vl_2b_main_gguf_file_accepts_sparse_min_size() {
        let temp_dir = make_temp_dir();
        let p = models_dir(&temp_dir).join("Qwen3VL-2B-Instruct-Q4_K_M.gguf");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        let f = std::fs::File::create(&p).unwrap();
        f.set_len(QWEN3_VL_2B_MAIN_GGUF_MIN_BYTES).unwrap();
        drop(f);
        validate_qwen3_vl_2b_main_gguf_file(&p).expect("size gate should pass");
        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn resolve_model_without_preference_returns_first_catalog_entry() {
        let temp_dir = make_temp_dir();
        let model_dir = models_dir(&temp_dir);
        std::fs::create_dir_all(&model_dir).unwrap();
        let qwen_path = model_dir.join("Qwen3VL-2B-Instruct-Q4_K_M.gguf");
        std::fs::write(&qwen_path, b"a").unwrap();

        let resolved = resolve_model(None, Some(temp_dir.as_path())).unwrap();
        assert_eq!(resolved.definition.id, "qwen3-vl-2b");

        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn qwen3_vl_2b_is_in_catalog() {
        let model = model_by_id("qwen3-vl-2b");
        assert!(model.is_some(), "qwen3-vl-2b not in MODEL_CATALOG");
        let m = model.unwrap();
        assert!(
            m.ram_gb <= 4.0,
            "Qwen3-VL-2B should be <= 4 GB RAM, got {}",
            m.ram_gb
        );
        assert!(m.recommended, "Qwen3-VL-2B should be recommended");
        assert_eq!(m.filename, "Qwen3VL-2B-Instruct-Q4_K_M.gguf");
    }

    #[test]
    fn inference_preferred_model_id_always_returns_qwen3_vl_2b() {
        let temp_dir = make_temp_dir();
        let cfg = crate::config::Config::default();
        assert_eq!(
            super::inference_preferred_model_id(temp_dir.as_path(), &cfg).as_deref(),
            Some("qwen3-vl-2b")
        );
        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn configured_vlm_model_id_always_returns_qwen3_vl_2b() {
        let cfg = crate::config::Config::default();
        assert_eq!(
            configured_vlm_model_id(&cfg).as_deref(),
            Some("qwen3-vl-2b")
        );
    }

    fn assert_sha256_hex(digest: &str) {
        assert_eq!(digest.len(), 64, "sha256 hex digest must be 64 chars");
        assert!(
            digest.chars().all(|c| c.is_ascii_hexdigit()),
            "sha256 digest must be hex: {digest}"
        );
    }

    #[test]
    fn embedder_is_required_and_qwen_is_optional() {
        // The onboarding flow auto-installs required models and only offers
        // optional ones as user choices.
        assert!(model_by_id(EMBEDDER_MODEL_ID).unwrap().required);
        assert!(!model_by_id("qwen3-vl-2b").unwrap().required);
    }

    #[test]
    fn embedder_is_in_catalog_with_pinned_checksum() {
        let model = model_by_id(EMBEDDER_MODEL_ID).expect("embedder must be in MODEL_CATALOG");
        assert_eq!(model.filename, "embeddinggemma-2-Q8_0.gguf");
        assert_sha256_hex(model.sha256.expect("embedder weights must pin a sha256"));
        assert!(
            model.extra_files.is_empty(),
            "the GGUF carries its own tokenizer"
        );
    }

    #[test]
    fn expected_sha256_lookup_covers_main_and_extra_files() {
        assert!(expected_sha256_for(EMBEDDER_MODEL_ID, "embeddinggemma-2-Q8_0.gguf").is_some());
        // Qwen has no pinned digest yet; downloads must keep working without one.
        assert!(expected_sha256_for("qwen3-vl-2b", "Qwen3VL-2B-Instruct-Q4_K_M.gguf").is_none());
        assert!(expected_sha256_for("unknown-model", "whatever.bin").is_none());
    }

    #[test]
    fn verify_file_sha256_accepts_matching_content() {
        let temp_dir = make_temp_dir();
        let path = temp_dir.join("blob.bin");
        std::fs::write(&path, b"hello").unwrap();
        verify_file_sha256(
            &path,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
        )
        .expect("matching digest must verify");
        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn verify_file_sha256_rejects_corrupted_content() {
        let temp_dir = make_temp_dir();
        let path = temp_dir.join("blob.bin");
        std::fs::write(&path, b"hell0").unwrap();
        let err = verify_file_sha256(
            &path,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
        )
        .expect_err("corrupted content must fail verification");
        assert!(err.contains("sha256"), "{err}");
        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn embedder_availability_requires_the_model_file() {
        let temp_dir = make_temp_dir();
        let model_dir = models_dir(&temp_dir);
        std::fs::create_dir_all(&model_dir).unwrap();

        assert!(!is_model_available(
            EMBEDDER_MODEL_ID,
            Some(temp_dir.as_path())
        ));

        std::fs::write(model_dir.join("embeddinggemma-2-Q8_0.gguf"), b"weights").unwrap();
        assert!(is_model_available(
            EMBEDDER_MODEL_ID,
            Some(temp_dir.as_path())
        ));

        std::fs::remove_dir_all(temp_dir).unwrap();
    }
}
