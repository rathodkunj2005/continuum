//! Derive persisted insight columns from structured fields + salience ranking.

use crate::capture::text_cleanup::{rank_salient_spans, salience_concentration, SalientSpan};
use crate::storage::schema::MemoryRecord;

const TOP_SPAN_DEBUG: usize = 8;
const DROP_SCORE: f32 = 0.12;

fn pollution_for_insight(record: &MemoryRecord) -> f32 {
    let noise = record.ocr_noise_score.clamp(0.0, 1.0);
    let concentration = salience_concentration(&record.clean_text, &record.app_name);
    let diffusion = (1.0 - concentration).clamp(0.0, 1.0);
    ((noise * 0.55) + (diffusion * 0.45)).clamp(0.0, 1.0)
}

/// Populate `insight_*` when layers are still empty (idempotent for pre-filled rows).
pub fn derive_insight_for_record(record: &mut MemoryRecord) {
    if !record.insight_what_happened.trim().is_empty() {
        return;
    }

    let spans = rank_salient_spans(&record.clean_text, &record.app_name);
    let top: Vec<&SalientSpan> = spans.iter().take(TOP_SPAN_DEBUG).collect();

    let mut dropped: Vec<String> = Vec::new();
    for s in spans.iter().skip(TOP_SPAN_DEBUG) {
        if s.score < DROP_SCORE {
            dropped.push(s.text.chars().take(200).collect());
        }
    }

    // --- what_happened ---
    let mut what = record.display_summary.trim().to_string();
    if what.is_empty() {
        what = record.snippet.trim().to_string();
    }
    if what.is_empty() && !record.topic.is_empty() && record.topic != "unknown" {
        what = format!("Activity related to {}.", record.topic);
    }
    if what.len() > 280 {
        what = what
            .chars()
            .take(280)
            .collect::<String>()
            .trim()
            .to_string();
    }
    record.insight_what_happened = what;

    // --- why_mattered ---
    let mut why = record
        .decisions
        .first()
        .cloned()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| record.errors.first().cloned())
        .unwrap_or_default();
    if why.is_empty() {
        if let Some(span) = spans.first() {
            why = span.text.chars().take(320).collect();
        } else if !record.memory_context.trim().is_empty() {
            why = record
                .memory_context
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(320)
                .collect();
        }
    }
    record.insight_why_mattered = why;

    // --- what_changed ---
    let mut changed: Vec<String> = Vec::new();
    changed.extend(
        record
            .next_steps
            .iter()
            .cloned()
            .filter(|s| !s.trim().is_empty()),
    );
    changed.extend(
        record
            .files_touched
            .iter()
            .cloned()
            .filter(|s| !s.trim().is_empty()),
    );
    let joined = changed.join("; ");
    record.insight_what_changed = if joined.is_empty() {
        String::new()
    } else {
        joined.chars().take(400).collect()
    };

    // --- context_thread (only when we have real links; avoid fabrication) ---
    if !record.related_memory_ids.is_empty() {
        record.insight_context_thread =
            format!("{} linked memories", record.related_memory_ids.len());
    } else if !record.session_id.trim().is_empty() {
        let short = record.session_id.chars().take(8).collect::<String>();
        record.insight_context_thread = format!("session …{short}");
    }

    let pollution = pollution_for_insight(record);
    let salience = spans
        .first()
        .map(|s| s.score)
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
    record.insight_card_confidence =
        (salience * (1.0 - pollution) * record.ocr_confidence.clamp(0.0, 1.0)).clamp(0.0, 1.0);

    record.insight_spans_json = serde_json::json!({
        "top": top.iter().map(|s| serde_json::json!({"text": s.text, "score": s.score})).collect::<Vec<_>>(),
        "dropped": dropped
    })
    .to_string();
}
