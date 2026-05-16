//! Debug/inspection IPC commands.
//!
//! Returns the full pipeline state of a stored memory so the UI can render
//! a diagnostic panel without needing access to the raw LanceDB row.

use crate::memory::distill::distill_memory_from_record;
use crate::memory::embed_doc::build_embedding_document;
use crate::memory::types::{CleanedEvidence, MemoryDecision, QualityDecision, QualityScores};
use crate::memory::validate::decide_memory;
use crate::AppState;

use serde::Serialize;
use std::sync::Arc;
use tauri::State;

#[derive(Debug, Serialize)]
pub struct MemoryPipelineInspection {
    pub memory_id: String,
    pub synthesis_branch: String,
    pub app_name: String,
    pub window_title: String,
    pub topic: String,
    pub topic_categories: Vec<String>,
    pub entities: Vec<String>,
    pub search_aliases: Vec<String>,
    pub display_summary: String,
    pub memory_context: String,
    pub insight_what_happened: String,
    pub insight_why_mattered: String,
    pub insight_what_changed: String,
    pub insight_card_confidence: f32,
    pub ocr_confidence: f32,
    pub ocr_noise_score: f32,
    pub embedding_document_text: String,
    pub embedding_aliases: Vec<String>,
    pub embedding_dim: usize,
    pub has_image_embedding: bool,
    pub insight_spans_json: String,
    pub lexical_shadow: String,
}

#[tauri::command]
pub async fn inspect_memory_pipeline(
    state: State<'_, Arc<AppState>>,
    memory_id: String,
) -> Result<MemoryPipelineInspection, String> {
    let record = state
        .store
        .get_memory_by_id(&memory_id)
        .await
        .map_err(|e| format!("store lookup failed: {e}"))?
        .ok_or_else(|| format!("memory not found: {memory_id}"))?;

    // Reproduce the indexing path so the user can see exactly what the
    // embedding model saw at write-time.
    let evidence = CleanedEvidence {
        clean_text: record.clean_text.clone(),
        ..CleanedEvidence::default()
    };
    let distilled = distill_memory_from_record(&record, &evidence);
    let quality_decision = QualityDecision {
        decision: "store".to_string(),
        passed: true,
        reasons: Vec::new(),
        scores: QualityScores {
            grounding_confidence: record.evidence_confidence,
            evidence_quality: record.confidence_score,
            contamination_score: record.ocr_noise_score,
            ..QualityScores::default()
        },
    };
    let validated = match decide_memory(distilled, &quality_decision) {
        MemoryDecision::Store(v) => v,
        _ => Default::default(),
    };
    let embed_doc = build_embedding_document(&validated);

    Ok(MemoryPipelineInspection {
        memory_id: record.id.clone(),
        synthesis_branch: record.synthesis_branch.clone(),
        app_name: record.app_name.clone(),
        window_title: record.window_title.clone(),
        topic: record.topic.clone(),
        topic_categories: record.topic_categories.clone(),
        entities: record.entities.clone(),
        search_aliases: record.search_aliases.clone(),
        display_summary: record.display_summary.clone(),
        memory_context: record.memory_context.clone(),
        insight_what_happened: record.insight_what_happened.clone(),
        insight_why_mattered: record.insight_why_mattered.clone(),
        insight_what_changed: record.insight_what_changed.clone(),
        insight_card_confidence: record.insight_card_confidence,
        ocr_confidence: record.ocr_confidence,
        ocr_noise_score: record.ocr_noise_score,
        embedding_document_text: embed_doc.text,
        embedding_aliases: embed_doc.aliases,
        embedding_dim: record.embedding.len(),
        has_image_embedding: !record.image_embedding.is_empty(),
        insight_spans_json: record.insight_spans_json.clone(),
        lexical_shadow: record.lexical_shadow.clone(),
    })
}
