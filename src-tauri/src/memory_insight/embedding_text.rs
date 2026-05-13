//! Compose `embedding_text` from structured / insight fields only (no OCR blob).

use crate::storage::schema::MemoryRecord;

fn trim_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max).collect()
}

fn push_segment(out: &mut Vec<String>, value: &str, label: &str) {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("unknown") {
        return;
    }
    out.push(format!("{label}: {trimmed}"));
}

/// Build retrieval text for embedding from insight and structured fields only.
/// Does **not** append `clean_text`, compressed OCR, or `raw_evidence`.
pub fn compose_insight_embedding_text(record: &MemoryRecord) -> String {
    let mut segments = Vec::new();

    push_segment(&mut segments, &record.user_intent, "intent");
    push_segment(&mut segments, &record.project, "project");
    push_segment(&mut segments, &record.topic, "topic");
    push_segment(&mut segments, &record.workflow, "workflow");
    push_segment(&mut segments, &record.memory_context, "context");

    // Insight layers (when populated) anchor semantics for cards + retrieval.
    push_segment(
        &mut segments,
        &record.insight_what_happened,
        "what_happened",
    );
    push_segment(&mut segments, &record.insight_why_mattered, "why_mattered");
    push_segment(&mut segments, &record.insight_what_changed, "what_changed");
    push_segment(
        &mut segments,
        &record.insight_context_thread,
        "context_thread",
    );

    let entity_blob = record.entities.join(", ");
    push_segment(&mut segments, &entity_blob, "entities");
    let alias_blob = record.search_aliases.join(", ");
    push_segment(&mut segments, &alias_blob, "aliases");
    let decisions = record.decisions.join("; ");
    push_segment(&mut segments, &decisions, "decisions");
    let errors = record.errors.join("; ");
    push_segment(&mut segments, &errors, "errors");
    let blockers = record.blockers.join("; ");
    push_segment(&mut segments, &blockers, "blockers");
    let todos = if !record.todos.is_empty() {
        record.todos.join("; ")
    } else {
        record.next_steps.join("; ")
    };
    push_segment(&mut segments, &todos, "todos");
    let results = record.results.join("; ");
    push_segment(&mut segments, &results, "results");
    push_segment(&mut segments, &record.files_touched.join(", "), "files");
    if let Some(url) = record.url.as_deref() {
        push_segment(&mut segments, url, "urls");
    }
    push_segment(&mut segments, &record.commands.join("; "), "commands");

    trim_chars(&segments.join("\n"), 2_000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::schema::MemoryRecord;

    #[test]
    fn compose_never_includes_clean_text_when_structured_present() {
        let mut r = MemoryRecord::default();
        r.user_intent = "Ship the fix".to_string();
        r.project = "fndr".to_string();
        r.topic = "memory".to_string();
        r.clean_text = "SECRET_OCR_BLOB_THAT_MUST_NOT_LEAK_INTO_EMBEDDING".to_string();
        r.insight_what_happened = "Worked on memory indexing.".to_string();
        let out = compose_insight_embedding_text(&r);
        assert!(out.contains("what_happened"));
        assert!(!out.contains("SECRET_OCR"));
    }
}
