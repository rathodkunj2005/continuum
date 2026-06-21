use crate::config::Config;
use crate::inference::model_config::{
    MULTIMODAL_MODEL_DOWNLOAD_URL, MULTIMODAL_MODEL_FILENAME, MULTIMODAL_MODEL_ID,
    MULTIMODAL_MODEL_RAM_GB, MULTIMODAL_MODEL_SIZE_BYTES, QWEN3_VL_2B_MAIN_GGUF_MIN_BYTES,
};
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
        dirs.push(data_dir.join("continuum/models"));
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
            if path.is_file() {
                return Some(ResolvedModel { definition, path });
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_temp_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!("continuum-model-test-{}", uuid::Uuid::new_v4()));
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
}
