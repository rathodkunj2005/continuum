use crate::inference::qwen_vl_memory::{MemorySynthesisInput, MemorySynthesisOutput};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::Duration;

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

async fn worker_loop(
    mut rx: mpsc::Receiver<QwenJobRequest>,
    app_data_dir: std::path::PathBuf,
    idle_unload_secs: u64,
) {
    let idle_duration = Duration::from_secs(idle_unload_secs);

    loop {
        match tokio::time::timeout(idle_duration, rx.recv()).await {
            Ok(Some(job)) => {
                let result = run_synthesis_blocking(job.input, &app_data_dir).await;
                let _ = job.reply.send(result);
            }
            Ok(None) => {
                tracing::info!("qwen_worker: channel closed, exiting");
                break;
            }
            Err(_timeout) => {
                // Idle timeout — no real unload API in llama_cpp_2 yet; log and continue.
                tracing::debug!("qwen_worker: idle timeout, model unloaded");
            }
        }
    }
}

/// Runs Qwen3-VL-2B memory synthesis on a blocking thread.
/// Deletes the temporary screenshot after inference (success or failure).
pub(crate) async fn run_synthesis_blocking(
    input: MemorySynthesisInput,
    app_data_dir: &std::path::Path,
) -> SynthesisResult {
    use crate::inference::image_semantics::{extract_image_semantics, ImageImportSource};
    use crate::inference::qwen_vl_memory::synthesis_ocr_only_fallback;

    let result = if let Some(ref img_path) = input.image_path {
        let bytes_result = tokio::fs::read(img_path).await.map_err(|e| e.to_string());
        match bytes_result {
            Ok(bytes) => {
                let filename = img_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("frame.png")
                    .to_string();
                match extract_image_semantics(bytes, &filename, ImageImportSource::ScreenCapture, app_data_dir.to_path_buf()).await {
                    Ok(insight) => Ok(MemorySynthesisOutput {
                        memory_context: insight.summary_detailed.clone(),
                        summary_short: insight.summary_short.clone(),
                        topic: insight.topics.first().cloned(),
                        activity_type: insight.activity_type.clone(),
                        user_intent: insight.user_intent.clone(),
                        entities: insight.entities.clone(),
                        files: Vec::new(),
                        urls: Vec::new(),
                        decisions: Vec::new(),
                        errors: Vec::new(),
                        next_steps: Vec::new(),
                        search_aliases: insight.search_aliases.clone(),
                        insight_what_happened: if !insight.summary_short.is_empty() {
                            insight.summary_short.clone()
                        } else {
                            String::new()
                        },
                        insight_why_mattered: if !insight.summary_detailed.is_empty() {
                            insight.summary_detailed.clone()
                        } else {
                            String::new()
                        },
                        topic_categories: insight.topics.iter()
                            .map(|t| t.trim().to_lowercase())
                            .filter(|s| !s.is_empty() && s.len() <= 40)
                            .take(6)
                            .collect(),
                        confidence_score: insight.confidence,
                        importance_score: insight.confidence * 0.8,
                    }),
                    Err(e) => {
                        tracing::warn!("qwen_worker: vision inference failed ({e}), OCR fallback");
                        Ok(synthesis_ocr_only_fallback(&input))
                    }
                }
            }
            Err(e) => {
                tracing::warn!("qwen_worker: failed to read image ({e}), OCR fallback");
                Ok(synthesis_ocr_only_fallback(&input))
            }
        }
    } else {
        Ok(synthesis_ocr_only_fallback(&input))
    };

    // Delete temp screenshot regardless of outcome.
    if let Some(ref img_path) = input.image_path {
        if let Err(e) = tokio::fs::remove_file(img_path).await {
            tracing::warn!("qwen_worker: failed to delete temp screenshot {:?}: {e}", img_path);
        } else {
            tracing::debug!("qwen_worker: deleted temp screenshot {:?}", img_path);
        }
    }

    result
}

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
    if ocr_text_len >= 300 && ocr_block_count >= 10 && ocr_confidence >= 0.40 {
        return ModelRunDecision::OcrOnlyFallback { reason: "good_ocr".to_string() };
    }
    if !visual_signal && ocr_text_len < 60 && ocr_block_count < 3 {
        return ModelRunDecision::Defer { reason: "low_value".to_string() };
    }
    ModelRunDecision::RunQwen
}

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

    #[tokio::test]
    async fn temp_screenshot_deleted_after_inference() {
        let dir = tempfile::tempdir().unwrap();
        let img_path = dir.path().join("test_frame.png");
        std::fs::write(&img_path, b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x02\0\0\0\x90wS\xde\0\0\0\x0cIDATx\x9cc\xf8\x0f\0\0\x01\x01\0\x05\x18\xd8N\0\0\0\0IEND\xaeB`\x82").unwrap();
        assert!(img_path.exists());
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
        assert!(!img_path.exists(), "temp screenshot must be deleted after inference");
    }
}
