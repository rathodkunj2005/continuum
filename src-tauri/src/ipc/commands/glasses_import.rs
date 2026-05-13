//! Manual import of photos (e.g. from Meta AI glasses via phone → Mac).
//!
//! Pipeline: **visual semantic extraction (Qwen3-VL + mmproj on pixels)** → optional gated OCR
//! → BGE text embeddings from semantic copy → CLIP image embedding → durable memory + context sync.

use super::common::shared_embedder;
use crate::config::DEFAULT_IMAGE_EMBEDDING_DIM;
use crate::embedding::{embed_imported_image, EMBEDDING_DIM};
use crate::inference::{
    build_import_raw_evidence, compose_import_memory_context, extract_image_semantics,
    insight_vision_extraction_failed, should_include_import_ocr, ImageImportSource,
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
use serde_json::json;
use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;
use tauri::State;

const APP_LABEL: &str = "Meta glasses import";

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

    let vision = extract_image_semantics(
        file_bytes.clone(),
        filename.as_str(),
        ImageImportSource::MetaGlasses,
        app_data_dir.clone(),
    )
    .await;

    let mut extraction_issues: Vec<String> = Vec::new();
    let insight: ImageSemanticInsight = match vision {
        Ok(i) => i,
        Err(e) => {
            tracing::warn!("import vision extraction failed: {e}");
            extraction_issues.push("vision_semantic_extraction_failed".to_string());
            insight_vision_extraction_failed(&filename, &e)
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
    let raw_evidence = build_import_raw_evidence(
        ImageImportSource::MetaGlasses.api_label(),
        &filename,
        &insight,
        include_ocr,
        ocr_rejected_reason,
        ocr_excerpt_owned.as_deref(),
    );

    let internal_context = json!({
        "import_pipeline": "visual_semantics_mtmd",
        "vision_model_id": insight.model_id,
        "semantic_confidence": insight.confidence,
        "ocr_included": include_ocr,
        "ocr_rejected_reason": ocr_rejected_reason,
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
