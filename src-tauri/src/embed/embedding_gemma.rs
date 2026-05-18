use crate::inference::model_config::{EMBEDDING_DIMENSIONS, EMBEDDING_MODEL_ID};

/// Format memory synthesis output fields into the canonical embedding text.
pub fn format_for_embedding(
    summary_short: &str,
    topic: Option<&str>,
    user_intent: Option<&str>,
    activity_type: Option<&str>,
    memory_context: &str,
    entities: &[String],
    files: &[String],
    urls: &[String],
    decisions: &[String],
    errors: &[String],
    next_steps: &[String],
    search_aliases: &[String],
) -> String {
    let mut parts = Vec::new();
    parts.push(format!("Title: {summary_short}"));
    if let Some(t) = topic {
        if !t.is_empty() {
            parts.push(format!("Topic: {t}"));
        }
    }
    if let Some(i) = user_intent {
        if !i.is_empty() {
            parts.push(format!("Intent: {i}"));
        }
    }
    if let Some(a) = activity_type {
        if !a.is_empty() {
            parts.push(format!("Activity: {a}"));
        }
    }
    if !memory_context.is_empty() {
        parts.push(format!("Context: {memory_context}"));
    }
    if !entities.is_empty() {
        parts.push(format!("Entities: {}", entities.join(", ")));
    }
    if !files.is_empty() {
        parts.push(format!("Files: {}", files.join(", ")));
    }
    if !urls.is_empty() {
        parts.push(format!("URLs: {}", urls.join(", ")));
    }
    if !decisions.is_empty() {
        parts.push(format!("Decisions: {}", decisions.join(", ")));
    }
    if !errors.is_empty() {
        parts.push(format!("Errors: {}", errors.join(", ")));
    }
    if !next_steps.is_empty() {
        parts.push(format!("Next steps: {}", next_steps.join(", ")));
    }
    if !search_aliases.is_empty() {
        parts.push(format!("Aliases: {}", search_aliases.join(", ")));
    }
    parts.join("\n")
}

pub fn embedding_model_id() -> &'static str {
    EMBEDDING_MODEL_ID
}

pub fn embedding_dimensions() -> usize {
    EMBEDDING_DIMENSIONS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_includes_all_non_empty_fields() {
        let text = format_for_embedding(
            "GitHub PR review",
            Some("code review"),
            Some("review PRs"),
            Some("reviewing"),
            "User reviewed open pull requests on GitHub",
            &["GitHub".into(), "PR #42".into()],
            &[],
            &["https://github.com".into()],
            &["merge PR #42".into()],
            &[],
            &["check CI".into()],
            &["PR review".into()],
        );
        assert!(text.contains("Title: GitHub PR review"));
        assert!(text.contains("Topic: code review"));
        assert!(text.contains("Decisions: merge PR #42"));
        assert!(text.contains("Next steps: check CI"));
        assert!(text.contains("Aliases: PR review"));
    }

    #[test]
    fn format_skips_empty_fields() {
        let text = format_for_embedding(
            "title",
            None,
            None,
            None,
            "context",
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
        );
        assert!(!text.contains("Topic:"));
        assert!(!text.contains("Entities:"));
        assert!(text.contains("Context: context"));
    }

    #[test]
    fn embedding_dimensions_is_384() {
        assert_eq!(embedding_dimensions(), 384);
    }
}
