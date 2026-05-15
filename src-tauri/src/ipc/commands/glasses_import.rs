//! Manual import of photos (e.g. from Meta AI glasses via phone → Mac).
//!
//! Pipeline: **visual semantic extraction (Qwen3-VL + mmproj on pixels)** → optional gated OCR
//! → BGE text embeddings from semantic copy → CLIP image embedding → durable memory + context sync.

use super::common::shared_embedder;
use crate::config::DEFAULT_IMAGE_EMBEDDING_DIM;
use crate::embedding::{embed_imported_image, EMBEDDING_DIM};
use crate::inference::vlm_router::{should_run_vlm, VlmRouteDecision, VlmRouteInput};
use crate::inference::{
    build_import_raw_evidence, compose_import_memory_context, extract_image_semantics,
    insight_from_ocr_only, insight_from_structured, should_include_import_ocr, ImageImportSource,
    ImageSemanticInsight, ImportOcrStats,
};
use crate::memory_compaction::{
    build_lexical_shadow, compact_summary_embedding_text, mean_pool_embeddings,
    support_embedding_texts,
};
use crate::models;
use crate::ocr::OcrEngine;
use crate::storage::MemoryRecord;
use crate::AppState;
use chrono::Local;
use image::ImageFormat;
use serde_json::{json, Value};
use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;
use tauri::State;

const APP_LABEL: &str = "Meta glasses import";

fn route_label(decision: &VlmRouteDecision) -> &'static str {
    match decision {
        VlmRouteDecision::SkipDuplicate => "skip_duplicate",
        VlmRouteDecision::SkipGoodOcr => "skip_good_ocr",
        VlmRouteDecision::SkipLowValue => "skip_low_value",
        VlmRouteDecision::RunLightweightVlm => "run_lightweight_vlm",
        VlmRouteDecision::RunHeavyVlmExplicitOnly => "run_heavy_vlm_explicit",
        VlmRouteDecision::FallbackOcrOnly { .. } => "fallback_ocr_only",
    }
}

fn fallback_reason(decision: &VlmRouteDecision) -> Option<&str> {
    match decision {
        VlmRouteDecision::FallbackOcrOnly { reason } => Some(reason.as_str()),
        _ => None,
    }
}

fn route_import_pixel_vlm(
    config: &crate::config::Config,
    model_id: Option<&str>,
    host_supports_vlm: bool,
    pressure_skip: bool,
    vlm_available: bool,
    calls_remaining: u32,
) -> VlmRouteDecision {
    should_run_vlm(&VlmRouteInput {
        ocr_text_len: 120,
        ocr_confidence: 0.50,
        ocr_block_count: 8,
        visual_signal: true,
        is_duplicate: false,
        system_pressure_skip: pressure_skip,
        host_supports_vlm,
        vlm_enabled: config.use_vlm,
        vlm_model_id: model_id,
        vlm_available,
        vlm_calls_remaining: calls_remaining,
        vlm_timeout_secs: config.vlm_timeout_secs,
    })
}

fn allowed_image_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            matches!(
                e.to_ascii_lowercase().as_str(),
                "jpg" | "jpeg" | "png" | "heic" | "heif"
            )
        })
        .unwrap_or(false)
}

fn dynamic_image_to_png_bytes(img: &image::DynamicImage) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
        .map_err(|e| format!("encode png for OCR: {e}"))?;
    Ok(buf)
}

/// Import a photo: **visual semantics (local VLM on pixels)** + gated OCR + embeddings.
#[tauri::command]
pub async fn import_meta_glasses_photo(
    state: State<'_, Arc<AppState>>,
    path: Option<String>,
) -> Result<String, String> {
    let resolved_path = match path.filter(|p| !p.trim().is_empty()) {
        Some(p) => std::path::PathBuf::from(p),
        None => tokio::task::spawn_blocking(|| {
            rfd::FileDialog::new()
                .add_filter("Images", &["png", "jpg", "jpeg", "heic", "HEIC"])
                .pick_file()
        })
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "No file selected".to_string())?,
    };

    if !resolved_path.is_file() {
        return Err(format!("Not a file: {}", resolved_path.display()));
    }
    if !allowed_image_extension(&resolved_path) {
        return Err("Unsupported image type (use JPEG, PNG, or HEIC).".to_string());
    }

    let file_bytes = tokio::fs::read(&resolved_path)
        .await
        .map_err(|e| format!("read file: {e}"))?;

    let filename = resolved_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("photo")
        .to_string();

    let models_dir = models::models_dir(state.app_data_dir.as_path());
    let app_data_dir = state.app_data_dir.clone();

    // Serialize all heavy model work (OCR is light, but CLIP + VLM + BGE
    // contend on Metal). Holding the global pipeline lock here pauses any
    // concurrent capture-loop model phase until this import finishes,
    // mirroring the user-requested "pause then process then restart"
    // invariant and fixing the `mtmd eval chunks: -3` Metal contention
    // failure when capture and import collide.
    let _pipeline_guard = state.model_pipeline_lock.lock().await;

    let (ocr_text, ocr_confidence, ocr_blocks, image_embedding) =
        tokio::task::spawn_blocking({
            let file_bytes = file_bytes.clone();
            let models_dir = models_dir.clone();
            move || -> Result<(String, f32, usize, Vec<f32>), String> {
                let dynamic =
                    image::load_from_memory(&file_bytes).map_err(|e| format!("decode image: {e}"))?;
                let png_bytes = dynamic_image_to_png_bytes(&dynamic)?;
                let engine = OcrEngine::new().map_err(|e| format!("OCR init: {e}"))?;
                let recognized = engine
                    .recognize_with_metadata(&png_bytes)
                    .map_err(|e| format!("OCR: {e}"))?;
                let ocr_text = recognized.0.text.clone();
                let ocr_confidence = recognized.0.confidence;
                let ocr_blocks = recognized.0.block_count;
                let image_embedding = embed_imported_image(&dynamic, &models_dir)
                    .unwrap_or_else(|e| {
                        tracing::warn!(
                            "CLIP image embedding skipped for import: {e}; storing a zero vector (text embeddings still apply)."
                        );
                        vec![0.0f32; DEFAULT_IMAGE_EMBEDDING_DIM]
                    });
                Ok((ocr_text, ocr_confidence, ocr_blocks, image_embedding))
            }
        })
        .await
        .map_err(|e| e.to_string())??;

    let ocr_stats = ImportOcrStats {
        confidence: ocr_confidence,
        blocks: ocr_blocks.min(u32::MAX as usize) as u32,
    };
    let include_ocr = should_include_import_ocr(&ocr_text, &ocr_stats);
    let ocr_rejected_reason: Option<&'static str> = if include_ocr {
        None
    } else if ocr_text.trim().is_empty() {
        Some("empty_ocr")
    } else {
        Some("noisy_low_signal_text")
    };

    let config = state.config.read().clone();
    let model_id = models::configured_vlm_model_id(&config);
    let host_supports_vlm = crate::telemetry::system_metrics::host_supports_vlm();
    let vlm_available =
        models::pixel_vlm_available(model_id.as_deref(), Some(app_data_dir.as_path()));

    // System-pressure and host-size throttle: when the host cannot safely hold
    // a pixel VLM, or the current process is hot, skip MTMD entirely and use
    // OCR-grounded fallback semantics.
    let (skip_vlm, skip_reason) =
        crate::telemetry::system_metrics::pressure_recommends_skipping_heavy_models();
    let vlm_route = route_import_pixel_vlm(
        &config,
        model_id.as_deref(),
        host_supports_vlm,
        skip_vlm,
        vlm_available,
        config.vlm_max_calls_per_minute,
    );

    let vision = if matches!(
        vlm_route,
        VlmRouteDecision::RunLightweightVlm | VlmRouteDecision::RunHeavyVlmExplicitOnly
    ) {
        extract_image_semantics(
            file_bytes.clone(),
            filename.as_str(),
            ImageImportSource::MetaGlasses,
            app_data_dir.clone(),
        )
        .await
    } else {
        let reason = fallback_reason(&vlm_route)
            .map(str::to_string)
            .unwrap_or_else(|| route_label(&vlm_route).to_string());
        tracing::info!(
            "glasses_import: skipping VLM ({reason}); pressure_reason={skip_reason}; using OCR-grounded insight"
        );
        Err(reason)
    };

    let mut extraction_issues: Vec<String> = Vec::new();
    // VLM failure no longer leaks into user-visible fields; the reason is
    // recorded once in `extraction_failure_reason` (raw_evidence). When
    // the Llama engine is loaded we use it to author a real narrative
    // from the OCR; only when that's also unavailable do we drop to the
    // heuristic OCR-only insight.
    let mut extraction_failure_reason: Option<String> = None;
    let insight: ImageSemanticInsight = match vision {
        Ok(i) => i,
        Err(e) => {
            tracing::warn!("import vision extraction failed: {e}; attempting LLM-on-OCR fallback");
            extraction_issues.push("vision_semantic_extraction_failed".to_string());
            extraction_failure_reason = Some(e);
            let ocr_grounding = if include_ocr { ocr_text.trim() } else { "" };

            let llm_insight: Option<ImageSemanticInsight> = if ocr_grounding.len() >= 20 {
                if let Some(engine) = state.inference_engine() {
                    engine
                        .extract_structured_memory("Meta glasses import", &filename, ocr_grounding)
                        .await
                        .map(|s| insight_from_structured(&s))
                } else {
                    None
                }
            } else {
                None
            };

            llm_insight
                .unwrap_or_else(|| insight_from_ocr_only(&filename, None, None, ocr_grounding))
        }
    };

    let ocr_append = if include_ocr {
        Some(ocr_text.trim())
    } else {
        None
    };

    let composed = compose_import_memory_context(
        &filename,
        &insight,
        ocr_append,
        ImageImportSource::MetaGlasses,
    );

    let clean_text = composed.memory_context.clone();
    let snippet = if !insight.summary_short.trim().is_empty() {
        insight
            .summary_short
            .trim()
            .chars()
            .take(200)
            .collect::<String>()
    } else {
        format!("Imported photo: {filename}")
    };

    let lexical_shadow = build_lexical_shadow(APP_LABEL, &snippet, &clean_text, None);
    let compact_summary_text =
        compact_summary_embedding_text("import", &snippet, &clean_text, &lexical_shadow);
    let support_texts = support_embedding_texts(APP_LABEL, &filename, &clean_text, &lexical_shadow);

    let embedder = shared_embedder()?;
    let mut contexts = vec![
        (APP_LABEL.to_string(), filename.clone(), clean_text.clone()),
        (
            APP_LABEL.to_string(),
            filename.clone(),
            compact_summary_text,
        ),
    ];
    contexts.extend(
        support_texts
            .iter()
            .cloned()
            .map(|value| (APP_LABEL.to_string(), filename.clone(), value)),
    );
    let vectors = embedder
        .embed_batch_with_context(&contexts)
        .map_err(|e| format!("embed: {e}"))?;

    // All heavy model work is done; release the pipeline lock so the
    // capture loop can resume immediately.
    drop(_pipeline_guard);

    let embedding = vectors
        .first()
        .cloned()
        .unwrap_or_else(|| vec![0.0; EMBEDDING_DIM]);
    let snippet_embedding = vectors
        .get(1)
        .cloned()
        .unwrap_or_else(|| vec![0.0; EMBEDDING_DIM]);
    let support_embedding = if vectors.len() > 2 {
        mean_pool_embeddings(&vectors[2..])
    } else {
        vec![0.0; EMBEDDING_DIM]
    };

    let ocr_excerpt_owned = if include_ocr {
        None
    } else {
        Some(ocr_text.chars().take(400).collect::<String>())
    };
    let raw_evidence_base = build_import_raw_evidence(
        ImageImportSource::MetaGlasses.api_label(),
        &filename,
        &insight,
        include_ocr,
        ocr_rejected_reason,
        ocr_excerpt_owned.as_deref(),
        extraction_failure_reason.as_deref(),
    );
    let mut raw_evidence_json: Value =
        serde_json::from_str(&raw_evidence_base).unwrap_or_else(|_| json!({}));
    raw_evidence_json["vlm_route"] = json!(route_label(&vlm_route));
    raw_evidence_json["vlm_block_reason"] = json!(fallback_reason(&vlm_route));
    raw_evidence_json["host_supports_vlm"] = json!(host_supports_vlm);
    raw_evidence_json["pressure_reason"] = json!(skip_reason);
    raw_evidence_json["clip_embedding_status"] =
        json!(if image_embedding.iter().all(|v| *v == 0.0) {
            "zero_vector"
        } else {
            "ok"
        });
    let raw_evidence = raw_evidence_json.to_string();

    let internal_context = json!({
        "import_pipeline": "visual_semantics_mtmd",
        "vision_model_id": insight.model_id,
        "semantic_confidence": insight.confidence,
        "ocr_included": include_ocr,
        "ocr_rejected_reason": ocr_rejected_reason,
        "vlm_route": route_label(&vlm_route),
        "vlm_block_reason": fallback_reason(&vlm_route),
        "host_supports_vlm": host_supports_vlm,
        "pressure_reason": skip_reason,
    })
    .to_string();

    let now = Local::now();
    let mut record = MemoryRecord {
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: now.timestamp_millis(),
        day_bucket: now.format("%Y-%m-%d").to_string(),
        app_name: APP_LABEL.to_string(),
        bundle_id: None,
        window_title: filename.clone(),
        session_id: format!("{}-glasses-import", now.format("%Y%m%d")),
        text: if include_ocr {
            ocr_text.clone()
        } else {
            String::new()
        },
        clean_text: clean_text.clone(),
        ocr_confidence,
        ocr_block_count: ocr_blocks.min(u32::MAX as usize) as u32,
        snippet: snippet.clone(),
        display_summary: snippet.clone(),
        internal_context,
        summary_source: if extraction_issues.is_empty() {
            "vision_mtmd".to_string()
        } else {
            "vision_fallback".to_string()
        },
        noise_score: if include_ocr {
            0.0
        } else {
            ocr_confidence * 0.25
        },
        session_key: "import:meta_glasses".to_string(),
        lexical_shadow,
        embedding,
        image_embedding,
        screenshot_path: None,
        url: None,
        snippet_embedding,
        support_embedding,
        decay_score: 1.0,
        last_accessed_at: now.timestamp_millis(),
        source_type: "import".to_string(),
        topic: composed.topic.clone(),
        workflow: "import".to_string(),
        user_intent: composed.user_intent.clone(),
        memory_context: composed.memory_context.clone(),
        raw_evidence,
        search_aliases: composed.search_aliases.clone(),
        activity_type: composed.activity_type.clone(),
        entities: insight.entities.clone(),
        tags: insight.topics.clone(),
        embedding_text: composed.embedding_text.clone(),
        extraction_confidence: insight.confidence,
        insight_what_happened: composed.insight_what_happened.clone(),
        insight_why_mattered: composed.insight_why_mattered.clone(),
        insight_card_confidence: insight.confidence,
        errors: extraction_issues.clone(),
        ..Default::default()
    };

    if !extraction_issues.is_empty() {
        record.outcome = "vision_semantic_extraction_failed".to_string();
    }

    state
        .store
        .add_batch(&[record.clone()])
        .await
        .map_err(|e| e.to_string())?;

    if let Err(err) =
        crate::context_runtime::sync_memory_record(state.as_ref(), &record, Some("import")).await
    {
        tracing::warn!("glasses import: context_runtime sync failed: {err}");
    }

    state.invalidate_memory_derived_caches();

    Ok(record.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_pixel_vlm_route_blocks_low_ram_before_model_availability() {
        let mut config = crate::config::Config::default().normalized();
        config.use_vlm = true;
        config.vlm_model_size = "500M".to_string();
        config.vlm_max_calls_per_minute = 10;
        config.vlm_timeout_secs = 30;

        let decision = route_import_pixel_vlm(
            &config,
            Some("smolvlm-500m"),
            false,
            false,
            true,
            config.vlm_max_calls_per_minute,
        );

        assert_eq!(
            decision,
            VlmRouteDecision::FallbackOcrOnly {
                reason: "vlm_blocked_low_ram".to_string()
            }
        );
    }

    #[test]
    fn import_pixel_vlm_route_honors_disabled_config() {
        let mut config = crate::config::Config::default().normalized();
        config.use_vlm = false;

        let decision = route_import_pixel_vlm(
            &config,
            Some("smolvlm-500m"),
            true,
            false,
            true,
            config.vlm_max_calls_per_minute,
        );

        assert_eq!(
            decision,
            VlmRouteDecision::FallbackOcrOnly {
                reason: "vlm_disabled".to_string()
            }
        );
    }
}
