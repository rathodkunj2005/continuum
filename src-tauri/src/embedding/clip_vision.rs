//! CLIP ViT-B/32 vision tower (ONNX) for imported photos — 512-d `image_embedding`.
//!
//! Default weights: Xenova `onnx/vision_model_q4.onnx` (placed next to BGE assets).
//! Override with `FNDR_CLIP_VISION_ONNX` (absolute path to any compatible vision ONNX).

use crate::config::DEFAULT_IMAGE_EMBEDDING_DIM;
use image::imageops::FilterType;
use image::DynamicImage;
use ndarray::Array4;
use ort::session::Session;
use ort::value::Tensor;
use parking_lot::Mutex;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

const CLIP_VISION_ONNX_FILENAME: &str = "clip-vit-base-patch32-vision_q4.onnx";
const CLIP_INPUT: &str = "pixel_values";
const CLIP_OUTPUT: &str = "image_embeds";
const CLIP_SIZE: u32 = 224;

static CLIP_RUNTIME: OnceLock<Mutex<Option<ClipVisionSession>>> = OnceLock::new();
static CLIP_SESSION_LOADED: AtomicBool = AtomicBool::new(false);
static LAST_CLIP_INFER_MS: AtomicU64 = AtomicU64::new(0);

/// True after the CLIP ONNX session has been created in this process.
pub fn clip_session_loaded() -> bool {
    CLIP_SESSION_LOADED.load(Ordering::Relaxed)
}

/// Wall time of the last successful CLIP vision forward pass (milliseconds).
pub fn last_clip_infer_ms() -> u64 {
    LAST_CLIP_INFER_MS.load(Ordering::Relaxed)
}

fn clip_cell() -> &'static Mutex<Option<ClipVisionSession>> {
    CLIP_RUNTIME.get_or_init(|| Mutex::new(None))
}

struct ClipVisionSession {
    session: Session,
}

impl ClipVisionSession {
    fn load(models_dir: &Path) -> Result<Self, String> {
        let onnx_path = resolve_clip_onnx_path(models_dir)?;
        let session = Session::builder()
            .map_err(|e| format!("CLIP ort builder: {e}"))?
            .commit_from_file(&onnx_path)
            .map_err(|e| format!("Failed to load CLIP vision ONNX {}: {e}", onnx_path.display()))?;
        tracing::info!(
            model = %onnx_path.display(),
            "CLIP vision ONNX session ready"
        );
        Ok(Self { session })
    }

    fn embed(&mut self, dynamic: &DynamicImage) -> Result<Vec<f32>, String> {
        let t0 = Instant::now();
        let tensor = clip_preprocess(dynamic)?;
        let input = Tensor::from_array(tensor).map_err(|e| format!("CLIP input tensor: {e}"))?;
        let outputs = self
            .session
            .run(ort::inputs![CLIP_INPUT => input])
            .map_err(|e| format!("CLIP inference: {e}"))?;
        let output = if let Some(o) = outputs.get(CLIP_OUTPUT) {
            o
        } else {
            let key = outputs
                .keys()
                .next()
                .ok_or_else(|| "CLIP ONNX returned empty output map".to_string())?;
            outputs
                .get(key)
                .ok_or_else(|| format!("CLIP missing output key {key}"))?
        };
        let (shape, data) = output
            .try_extract_tensor::<f32>()
            .map_err(|e| format!("CLIP output extract: {e}"))?;
        let dims: Vec<usize> = shape.iter().map(|d| *d as usize).collect();
        let mut vec = match dims.as_slice() {
            [1, dim] if *dim == DEFAULT_IMAGE_EMBEDDING_DIM => data.to_vec(),
            [dim] if *dim == DEFAULT_IMAGE_EMBEDDING_DIM => data.to_vec(),
            _ => {
                return Err(format!(
                    "Unexpected CLIP output shape {dims:?}; expected [1, {}]",
                    DEFAULT_IMAGE_EMBEDDING_DIM
                ));
            }
        };
        if vec.len() != DEFAULT_IMAGE_EMBEDDING_DIM {
            return Err(format!(
                "CLIP vector length {} (expected {})",
                vec.len(),
                DEFAULT_IMAGE_EMBEDDING_DIM
            ));
        }
        l2_normalize(&mut vec);
        let ms = t0.elapsed().as_millis() as u64;
        LAST_CLIP_INFER_MS.store(ms, Ordering::Relaxed);
        crate::telemetry::runtime_metrics::record_ms("clip.infer_ms", ms);
        Ok(vec)
    }
}

/// Resolve ONNX file: env override, else `models_dir/clip-vit-base-patch32-vision_q4.onnx`.
pub fn resolve_clip_onnx_path(models_dir: &Path) -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("FNDR_CLIP_VISION_ONNX") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!(
            "FNDR_CLIP_VISION_ONNX is set but not a file: {}",
            path.display()
        ));
    }
    let path = models_dir.join(CLIP_VISION_ONNX_FILENAME);
    if path.is_file() {
        return Ok(path);
    }
    Err(format!(
        "CLIP vision ONNX not found at {}. Run scripts/bootstrap/download-clip-vision-onnx.sh (see README).",
        path.display()
    ))
}

/// RGB image → CLIP `pixel_values` NCHW float32.
fn clip_preprocess(dynamic: &DynamicImage) -> Result<Array4<f32>, String> {
    let rgb = dynamic.to_rgb8();
    let resized = image::imageops::resize(&rgb, CLIP_SIZE, CLIP_SIZE, FilterType::Triangle);
    let mean = [0.48145466f32, 0.4578275, 0.40821073];
    let stdv = [0.26862954, 0.26130258, 0.27577711];
    let mut tensor = Array4::<f32>::zeros((1, 3, CLIP_SIZE as usize, CLIP_SIZE as usize));
    for (x, y, pixel) in resized.enumerate_pixels() {
        let xi = x as usize;
        let yi = y as usize;
        let [r, g, b] = pixel.0;
        let rf = r as f32 / 255.0;
        let gf = g as f32 / 255.0;
        let bf = b as f32 / 255.0;
        tensor[[0, 0, yi, xi]] = (rf - mean[0]) / stdv[0];
        tensor[[0, 1, yi, xi]] = (gf - mean[1]) / stdv[1];
        tensor[[0, 2, yi, xi]] = (bf - mean[2]) / stdv[2];
    }
    Ok(tensor)
}

fn l2_normalize(v: &mut [f32]) {
    let n: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 1e-8 {
        for x in v.iter_mut() {
            *x /= n;
        }
    }
}

/// Lazy-load CLIP once per process; thread-safe.
pub fn embed_imported_image(dynamic: &DynamicImage, models_dir: &Path) -> Result<Vec<f32>, String> {
    let mut guard = clip_cell().lock();
    if guard.is_none() {
        *guard = Some(ClipVisionSession::load(models_dir)?);
        CLIP_SESSION_LOADED.store(true, Ordering::Relaxed);
    }
    guard
        .as_mut()
        .ok_or_else(|| "CLIP session unavailable".to_string())?
        .embed(dynamic)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_preprocess_shape() {
        let img = DynamicImage::ImageRgb8(image::RgbImage::new(32, 32));
        let t = clip_preprocess(&img).expect("preprocess");
        assert_eq!(t.shape(), [1, 3, CLIP_SIZE as usize, CLIP_SIZE as usize]);
    }
}
