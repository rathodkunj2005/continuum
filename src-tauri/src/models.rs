use crate::config::Config;
use serde::Deserialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

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
    pub filename: &'static str,
    pub download_url: &'static str,
}

pub const MODEL_CATALOG: [ModelDefinition; 3] = [
    ModelDefinition {
        id: "llama-3.2-1b",
        name: "Llama 3.2 · 1B",
        description: "Recommended default: fast text summaries and OCR-grounded prompts. Best for ~8 GB RAM.",
        size_bytes: 770_000_000,
        size_label: "770 MB",
        quality_label: "Good",
        speed_label: "Fastest",
        ram_gb: 2.0,
        recommended: true,
        filename: "Llama-3.2-1B-Instruct-Q4_K_M.gguf",
        download_url:
            "https://huggingface.co/bartowski/Llama-3.2-1B-Instruct-GGUF/resolve/main/Llama-3.2-1B-Instruct-Q4_K_M.gguf",
    },
    ModelDefinition {
        id: "smolvlm-500m",
        name: "SmolVLM 500M (lightweight VLM)",
        description: "Lightweight vision-language model for on-demand visual understanding. Safe for 8 GB RAM alongside dev tools. Requires matching mmproj weights.",
        size_bytes: 320_000_000,
        size_label: "320 MB",
        quality_label: "Good",
        speed_label: "Fast",
        ram_gb: 1.2,
        recommended: false,
        filename: "SmolVLM-500M-Instruct-Q4_K_M.gguf",
        download_url:
            "https://huggingface.co/HuggingFaceTB/SmolVLM-500M-Instruct-GGUF/resolve/main/SmolVLM-500M-Instruct-Q4_K_M.gguf",
    },
    ModelDefinition {
        id: "qwen3-vl-4b",
        name: "Qwen3-VL · 4B (advanced)",
        description: "Heavier optional GGUF for richer screen understanding. High RAM use alongside dev tools.",
        size_bytes: 2_500_000_000,
        size_label: "2.5 GB",
        quality_label: "Best",
        speed_label: "Balanced",
        ram_gb: 6.0,
        recommended: false,
        filename: "Qwen3VL-4B-Instruct-Q4_K_M.gguf",
        download_url:
            "https://huggingface.co/Qwen/Qwen3-VL-4B-Instruct-GGUF/resolve/main/Qwen3VL-4B-Instruct-Q4_K_M.gguf",
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

/// Filenames for Qwen3-VL multimodal projection weights (`mmproj`), searched under
/// [`candidate_model_dirs`]. At least one must be present for pixel-based import vision.
///
/// Hugging Face ships `Qwen3VL` (no hyphen) in the filename; some mirrors use `Qwen3-VL`.
pub const QWEN3_VL_MMPROJ_FILENAMES: &[&str] = &[
    "mmproj-Qwen3VL-4B-Instruct-F16.gguf",
    "mmproj-Qwen3VL-4B-Instruct-Q8_0.gguf",
    "mmproj-Qwen3-VL-4B-Instruct-F16.gguf",
    "mmproj-Qwen3-VL-4B-Instruct-Q8_0.gguf",
    "mmproj-Qwen3-VL-4B-Instruct-Q4_0.gguf",
    "mmproj-Qwen3-VL-4B-Instruct-Q4_K_M.gguf",
];

/// Resolve a Qwen3-VL `mmproj` GGUF on disk (any known catalog name).
pub fn resolve_qwen3_vl_mmproj(app_data_dir: Option<&Path>) -> Option<PathBuf> {
    for dir in candidate_model_dirs(app_data_dir) {
        for name in QWEN3_VL_MMPROJ_FILENAMES {
            let path = dir.join(name);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

/// Filenames for SmolVLM-500M multimodal projection weights.
pub const SMOLVLM_500M_MMPROJ_FILENAMES: &[&str] = &[
    "mmproj-SmolVLM-500M-Instruct-f16.gguf",
    "mmproj-SmolVLM-500M-Instruct-Q8_0.gguf",
];

/// Resolve a SmolVLM-500M mmproj GGUF on disk.
pub fn resolve_smolvlm_mmproj(app_data_dir: Option<&Path>) -> Option<PathBuf> {
    for dir in candidate_model_dirs(app_data_dir) {
        for name in SMOLVLM_500M_MMPROJ_FILENAMES {
            let path = dir.join(name);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

/// True if smolvlm-500m GGUF and its mmproj are both present on disk.
pub fn smolvlm_500m_fully_available(app_data_dir: Option<&Path>) -> bool {
    is_model_available("smolvlm-500m", app_data_dir)
        && resolve_smolvlm_mmproj(app_data_dir).is_some()
}

/// Minimum on-disk size for the catalog `Qwen3VL-4B-Instruct-*.gguf` to be treated as plausible.
///
/// Rejects Git LFS pointer files, empty placeholders, or unrelated GGUFs renamed to the
/// expected filename (a common cause of `n_embd` mismatch vs the official mmproj).
pub const QWEN3_VL_4B_MAIN_GGUF_MIN_BYTES: u64 = 1_800_000_000;

/// Returns `Err` if `path` exists but is too small to be a real Qwen3-VL 4B weights file.
pub fn validate_qwen3_vl_main_gguf_file(path: &Path) -> Result<(), String> {
    let len = std::fs::metadata(path)
        .map_err(|e| format!("stat {}: {e}", path.display()))?
        .len();
    if len < QWEN3_VL_4B_MAIN_GGUF_MIN_BYTES {
        let def = model_by_id("qwen3-vl-4b").expect("qwen3-vl-4b in catalog");
        return Err(format!(
            "Qwen3-VL main GGUF at {} is only {} bytes (expected at least ~{} bytes for {}). \
             This is often a Git LFS pointer, incomplete download, or the wrong model renamed to the catalog filename — \
             which then breaks mmproj pairing (n_embd mismatch). Re-download: {}",
            path.display(),
            len,
            QWEN3_VL_4B_MAIN_GGUF_MIN_BYTES,
            def.filename,
            def.download_url
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
/// Uses `config.vlm_model_size` as product intent: default **`1B`** tier prefers
/// Llama 3.2 1B even when onboarding still references a past **`qwen3-vl-4b`**
/// download. **`4B`** tier prefers Qwen3-VL (with catalog fallback if only Llama is on disk).
pub fn inference_preferred_model_id(app_data_dir: &Path, config: &Config) -> Option<String> {
    let from_onboarding = preferred_model_id_from_onboarding(app_data_dir);
    match config.vlm_model_size.as_str() {
        "500M" => Some("smolvlm-500m".to_string()),
        "4B" => Some("qwen3-vl-4b".to_string()),
        _ => {
            let id = match from_onboarding.as_deref() {
                Some("qwen3-vl-4b") => "llama-3.2-1b".to_string(),
                Some(other) => other.to_string(),
                None => "llama-3.2-1b".to_string(),
            };
            Some(id)
        }
    }
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
    let mut ordered_models: Vec<&'static ModelDefinition> = Vec::new();

    if let Some(model_id) = preferred_model_id {
        if let Some(model) = model_by_id(model_id) {
            ordered_models.push(model);
        }
    }

    for model in MODEL_CATALOG.iter() {
        if ordered_models
            .iter()
            .all(|candidate| candidate.id != model.id)
        {
            ordered_models.push(model);
        }
    }

    let candidate_dirs = candidate_model_dirs(app_data_dir);
    for model in ordered_models {
        for dir in &candidate_dirs {
            let path = dir.join(model.filename);
            if path.exists() {
                return Some(ResolvedModel {
                    definition: model,
                    path,
                });
            }
        }
    }

    None
}

fn resolve_specific_model(model_id: &str, app_data_dir: Option<&Path>) -> Option<ResolvedModel> {
    let definition = model_by_id(model_id)?;

    candidate_model_dirs(app_data_dir)
        .into_iter()
        .map(|dir| dir.join(definition.filename))
        .find(|path| path.exists())
        .map(|path| ResolvedModel { definition, path })
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
        let expected_path = model_dir.join("Qwen3VL-4B-Instruct-Q4_K_M.gguf");
        std::fs::write(&expected_path, b"test").unwrap();

        let resolved = resolve_model(Some("qwen3-vl-4b"), Some(temp_dir.as_path())).unwrap();

        assert_eq!(resolved.definition.id, "qwen3-vl-4b");
        assert_eq!(resolved.path, expected_path);

        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn partial_file_does_not_count_as_downloaded() {
        let temp_dir = make_temp_dir();
        let model_dir = models_dir(&temp_dir);
        std::fs::create_dir_all(&model_dir).unwrap();
        let partial_path = partial_model_path(&temp_dir, "Qwen3VL-4B-Instruct-Q4_K_M.gguf");
        std::fs::write(&partial_path, b"partial").unwrap();

        let resolved = resolve_model(Some("qwen3-vl-4b"), Some(temp_dir.as_path()));
        assert_ne!(
            resolved.as_ref().map(|model| model.path.clone()),
            Some(partial_path)
        );

        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn resolve_qwen3_vl_mmproj_finds_known_filename() {
        let temp_dir = make_temp_dir();
        let model_dir = models_dir(&temp_dir);
        std::fs::create_dir_all(&model_dir).unwrap();
        let mm = model_dir.join("mmproj-Qwen3-VL-4B-Instruct-F16.gguf");
        std::fs::write(&mm, b"x").unwrap();
        let found = resolve_qwen3_vl_mmproj(Some(temp_dir.as_path()));
        assert_eq!(found, Some(mm));
        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn resolve_qwen3_vl_mmproj_finds_huggingface_qwen3vl_filename() {
        let temp_dir = make_temp_dir();
        let model_dir = models_dir(&temp_dir);
        std::fs::create_dir_all(&model_dir).unwrap();
        let mm = model_dir.join("mmproj-Qwen3VL-4B-Instruct-Q8_0.gguf");
        std::fs::write(&mm, b"x").unwrap();
        let found = resolve_qwen3_vl_mmproj(Some(temp_dir.as_path()));
        assert_eq!(found, Some(mm));
        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn validate_qwen3_vl_main_gguf_file_rejects_small_file() {
        let temp_dir = make_temp_dir();
        let p = models_dir(&temp_dir).join("Qwen3VL-4B-Instruct-Q4_K_M.gguf");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"x").unwrap();
        let err = validate_qwen3_vl_main_gguf_file(&p).expect_err("tiny file");
        assert!(err.contains("bytes"), "{err}");
        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn validate_qwen3_vl_main_gguf_file_accepts_sparse_min_size() {
        let temp_dir = make_temp_dir();
        let p = models_dir(&temp_dir).join("Qwen3VL-4B-Instruct-Q4_K_M.gguf");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        let f = std::fs::File::create(&p).unwrap();
        f.set_len(QWEN3_VL_4B_MAIN_GGUF_MIN_BYTES).unwrap();
        drop(f);
        validate_qwen3_vl_main_gguf_file(&p).expect("size gate should pass");
        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn resolve_model_without_preference_prefers_first_catalog_file_found() {
        let temp_dir = make_temp_dir();
        let model_dir = models_dir(&temp_dir);
        std::fs::create_dir_all(&model_dir).unwrap();
        let llama_path = model_dir.join("Llama-3.2-1B-Instruct-Q4_K_M.gguf");
        let qwen_path = model_dir.join("Qwen3VL-4B-Instruct-Q4_K_M.gguf");
        std::fs::write(&llama_path, b"a").unwrap();
        std::fs::write(&qwen_path, b"b").unwrap();

        let resolved = resolve_model(None, Some(temp_dir.as_path())).unwrap();
        assert_eq!(resolved.definition.id, "llama-3.2-1b");

        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn inference_preferred_model_id_1b_tier_ignores_stale_qwen_onboarding() {
        let temp_dir = make_temp_dir();
        let onboarding_path = temp_dir.join("onboarding.json");
        std::fs::write(&onboarding_path, r#"{"model_id":"qwen3-vl-4b"}"#).unwrap();
        let mut cfg = crate::config::Config::default();
        cfg.vlm_model_size = "1B".to_string();
        assert_eq!(
            super::inference_preferred_model_id(temp_dir.as_path(), &cfg).as_deref(),
            Some("llama-3.2-1b")
        );
        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn inference_preferred_model_id_4b_tier_prefers_qwen() {
        let temp_dir = make_temp_dir();
        let mut cfg = crate::config::Config::default();
        cfg.vlm_model_size = "4B".to_string();
        assert_eq!(
            super::inference_preferred_model_id(temp_dir.as_path(), &cfg).as_deref(),
            Some("qwen3-vl-4b")
        );
        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn smolvlm_500m_is_in_catalog() {
        let model = model_by_id("smolvlm-500m");
        assert!(model.is_some(), "smolvlm-500m not in MODEL_CATALOG");
        let m = model.unwrap();
        assert!(m.ram_gb <= 2.0, "SmolVLM 500M should be <= 2 GB RAM, got {}", m.ram_gb);
        assert!(!m.recommended, "SmolVLM should not be recommended by default");
        assert_eq!(m.filename, "SmolVLM-500M-Instruct-Q4_K_M.gguf");
    }

    #[test]
    fn resolve_smolvlm_mmproj_finds_known_filename() {
        let temp_dir = make_temp_dir();
        let model_dir = models_dir(&temp_dir);
        std::fs::create_dir_all(&model_dir).unwrap();
        let mm = model_dir.join("mmproj-SmolVLM-500M-Instruct-f16.gguf");
        std::fs::write(&mm, b"x").unwrap();
        let found = resolve_smolvlm_mmproj(Some(temp_dir.as_path()));
        assert_eq!(found, Some(mm));
        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn smolvlm_500m_fully_available_requires_both_files() {
        let temp_dir = make_temp_dir();
        let model_dir = models_dir(&temp_dir);
        std::fs::create_dir_all(&model_dir).unwrap();

        // Neither file present
        assert!(!smolvlm_500m_fully_available(Some(temp_dir.as_path())));

        // Only main GGUF
        let main = model_dir.join("SmolVLM-500M-Instruct-Q4_K_M.gguf");
        std::fs::write(&main, b"x").unwrap();
        assert!(!smolvlm_500m_fully_available(Some(temp_dir.as_path())));

        // Both files present
        let mm = model_dir.join("mmproj-SmolVLM-500M-Instruct-f16.gguf");
        std::fs::write(&mm, b"x").unwrap();
        assert!(smolvlm_500m_fully_available(Some(temp_dir.as_path())));

        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn inference_preferred_model_id_500m_tier_returns_smolvlm() {
        let temp_dir = make_temp_dir();
        let mut cfg = crate::config::Config::default();
        cfg.vlm_model_size = "500M".to_string();
        assert_eq!(
            inference_preferred_model_id(temp_dir.as_path(), &cfg).as_deref(),
            Some("smolvlm-500m")
        );
        std::fs::remove_dir_all(temp_dir).unwrap();
    }
}
