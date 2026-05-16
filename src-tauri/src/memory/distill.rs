use crate::memory::types::{ActivityType, CleanedEvidence, DistilledMemory};
use crate::storage::MemoryRecord;

pub(crate) fn is_prompt_scaffold(value: &str) -> bool {
    let lower = value.trim().to_ascii_lowercase();
    lower.starts_with("here is")
        || lower.contains("best memory snippet")
        || lower.contains("```json")
        || lower.contains("topic:")
}

pub(crate) fn sanitize_field(value: &str) -> String {
    let trimmed = value.trim();
    if is_prompt_scaffold(trimmed) {
        String::new()
    } else {
        trimmed.to_string()
    }
}

pub fn distill_memory_from_record(
    record: &MemoryRecord,
    evidence: &CleanedEvidence,
) -> DistilledMemory {
    let summary_short = if !record.display_summary.trim().is_empty() {
        sanitize_field(&record.display_summary)
    } else {
        evidence
            .salient_spans
            .first()
            .map(|span| span.text.clone())
            .unwrap_or_default()
    };

    let title = if !record.window_title.trim().is_empty() {
        sanitize_field(&record.window_title)
    } else {
        summary_short.clone()
    };

    let topic = sanitize_field(&record.topic);
    let memory_context = sanitize_field(&record.memory_context);

    let mut quality_flags = Vec::new();
    if title.is_empty() {
        quality_flags.push("missing_title".to_string());
    }
    if topic.is_empty() || topic.eq_ignore_ascii_case("unknown") {
        quality_flags.push("missing_topic".to_string());
    }
    if memory_context.is_empty() {
        quality_flags.push("missing_memory_context".to_string());
    }

    DistilledMemory {
        title,
        topic,
        summary_short,
        memory_context,
        activity_type: ActivityType::from_label(&record.activity_type),
        workflow: sanitize_field(&record.workflow),
        project: sanitize_field(&record.project),
        entities: record.entities.clone(),
        actions: record.next_steps.clone(),
        user_intent: sanitize_field(&record.user_intent),
        confidence: record
            .extraction_confidence
            .max(record.evidence_confidence)
            .clamp(0.0, 1.0),
        quality_flags,
    }
}
