use crate::memory::types::{EmbeddingDocument, ValidatedMemory};

fn looks_like_bad_alias(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return true;
    }
    if trimmed.len() > 120 {
        return true;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("www.") {
        return true;
    }
    let has_many_symbols = trimmed
        .chars()
        .filter(|ch| !ch.is_ascii_alphanumeric() && !ch.is_whitespace())
        .count()
        > (trimmed.len() / 3);
    has_many_symbols || lower.contains("reopen") || lower.contains("find similar")
}

pub fn build_embedding_document(memory: &ValidatedMemory) -> EmbeddingDocument {
    let mut lines = Vec::new();
    if !memory.title.trim().is_empty() {
        lines.push(format!("title: {}", memory.title.trim()));
    }
    if !memory.topic.trim().is_empty() {
        lines.push(format!("topic: {}", memory.topic.trim()));
    }
    if !memory.summary_short.trim().is_empty() {
        lines.push(format!("summary: {}", memory.summary_short.trim()));
    }
    if !memory.memory_context.trim().is_empty() {
        lines.push(format!("context: {}", memory.memory_context.trim()));
    }
    if !memory.workflow.trim().is_empty() {
        lines.push(format!("workflow: {}", memory.workflow.trim()));
    }
    if !memory.project.trim().is_empty() {
        lines.push(format!("project: {}", memory.project.trim()));
    }
    if !memory.user_intent.trim().is_empty() {
        lines.push(format!("intent: {}", memory.user_intent.trim()));
    }
    if !memory.entities.is_empty() {
        lines.push(format!("entities: {}", memory.entities.join(", ")));
    }
    if !memory.actions.is_empty() {
        lines.push(format!("actions: {}", memory.actions.join("; ")));
    }
    // Broader semantic categories — enables cross-domain concept search.
    // E.g., a cricket capture's categories = ["sport", "entertainment"], so
    // a query for "sport" can semantically reach it.
    if !memory.topic_categories.is_empty() {
        let cats: Vec<String> = memory
            .topic_categories
            .iter()
            .map(|c| c.trim().to_lowercase())
            .filter(|c| !c.is_empty() && c.len() <= 40)
            .collect();
        if !cats.is_empty() {
            lines.push(format!("categories: {}", cats.join(", ")));
        }
    }
    // Search aliases: synonyms, abbreviations, alternate phrasings supplied
    // by synthesis. Both included in the embedding text AND surfaced in the
    // returned `aliases` list (for the keyword index).
    let clean_aliases: Vec<String> = memory
        .search_aliases
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !looks_like_bad_alias(s))
        .collect();
    if !clean_aliases.is_empty() {
        lines.push(format!("aliases: {}", clean_aliases.join(", ")));
    }

    // Aliases for keyword retrieval combine entities + search_aliases, all
    // lowercased and deduped, filtered through the bad-alias guard.
    // Length >= 2 prevents single-char tokens from polluting the keyword index.
    let mut seen = std::collections::HashSet::new();
    let aliases: Vec<String> = memory
        .entities
        .iter()
        .chain(memory.search_aliases.iter())
        .map(|v| v.trim().to_ascii_lowercase())
        .filter(|v| !looks_like_bad_alias(v) && v.len() >= 2)
        .filter(|v| seen.insert(v.clone()))
        .collect();

    EmbeddingDocument {
        text: lines.join("\n"),
        aliases,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> ValidatedMemory {
        ValidatedMemory {
            title: "T".to_string(),
            topic: "x".to_string(),
            ..ValidatedMemory::default()
        }
    }

    #[test]
    fn embedding_doc_includes_topic_categories() {
        let mut m = base();
        m.topic_categories = vec!["sport".into(), "entertainment".into()];
        let doc = build_embedding_document(&m);
        assert!(doc.text.contains("categories: sport, entertainment"));
    }

    #[test]
    fn embedding_doc_includes_search_aliases_in_text_and_alias_list() {
        let mut m = base();
        m.search_aliases = vec!["KKR".into(), "IPL".into()];
        let doc = build_embedding_document(&m);
        assert!(doc.text.contains("aliases: KKR, IPL"));
        assert!(doc.aliases.contains(&"kkr".to_string()));
        assert!(doc.aliases.contains(&"ipl".to_string()));
    }

    #[test]
    fn embedding_doc_dedupes_overlap_between_entities_and_aliases() {
        let mut m = base();
        m.entities = vec!["GitHub".into()];
        m.search_aliases = vec!["github".into()];
        let doc = build_embedding_document(&m);
        let count = doc.aliases.iter().filter(|a| *a == "github").count();
        assert_eq!(count, 1, "expected dedup, got {:?}", doc.aliases);
    }

    #[test]
    fn embedding_doc_filters_bad_aliases() {
        let mut m = base();
        m.search_aliases = vec![
            "https://example.com/reopen?find similar".into(),
            "valid".into(),
        ];
        let doc = build_embedding_document(&m);
        assert!(!doc.text.contains("https://"));
        assert!(doc.text.contains("valid"));
    }
}
