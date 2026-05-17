pub use super::hybrid::{QueryIntent, QueryProfile};

#[derive(Debug, Clone)]
pub struct QueryContext {
    pub raw_query: String,
    pub normalized_query: String,
    pub anchor_terms: Vec<String>,
    pub expanded_terms: Vec<String>,
    pub prefix_variants: Vec<String>,
    pub fuzzy_variants: Vec<String>,
    pub ngram_variants: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct QueryExpansionDebug {
    pub original_query: String,
    pub expanded_terms: Vec<String>,
    pub entity_candidates: Vec<String>,
    pub graph_expansions: Vec<String>,
    pub fuzzy_variants: Vec<String>,
    pub prefix_variants: Vec<String>,
    pub retrieval_plan: Vec<String>,
}

impl QueryContext {
    pub fn from_query(query: &str) -> Self {
        let normalized_query = normalize_text(query);
        let anchor_terms = extract_base_anchor_terms(&normalized_query);

        let mut expanded_terms = Vec::new();
        for term in &anchor_terms {
            push_unique(&mut expanded_terms, term);
            let singular = singularize(term);
            if singular != *term {
                push_unique(&mut expanded_terms, &singular);
            }
            if term.len() > 3 && !term.ends_with('s') {
                push_unique(&mut expanded_terms, &format!("{term}s"));
            }
            if term.contains('-') {
                push_unique(&mut expanded_terms, &term.replace('-', " "));
                push_unique(&mut expanded_terms, &term.replace('-', ""));
            }
            if term.contains('_') {
                push_unique(&mut expanded_terms, &term.replace('_', " "));
                push_unique(&mut expanded_terms, &term.replace('_', ""));
            }
        }

        if expanded_terms.is_empty() && !normalized_query.is_empty() {
            expanded_terms.push(normalized_query.clone());
        }

        let mut prefix_variants = Vec::new();
        let mut fuzzy_variants = Vec::new();
        let mut ngram_variants = Vec::new();
        for term in &expanded_terms {
            if term.len() >= 3 {
                push_unique(&mut prefix_variants, &term[..3]);
            }
            if term.len() >= 4 {
                push_unique(&mut prefix_variants, &term[..4]);
                push_unique(&mut fuzzy_variants, &drop_char_variant(term));
            }
            for ng in char_ngrams(term, 3) {
                push_unique(&mut ngram_variants, &ng);
            }
        }

        Self {
            raw_query: query.to_string(),
            normalized_query,
            anchor_terms,
            expanded_terms,
            prefix_variants,
            fuzzy_variants,
            ngram_variants,
        }
    }

    pub fn debug_plan(&self) -> QueryExpansionDebug {
        QueryExpansionDebug {
            original_query: self.raw_query.clone(),
            expanded_terms: self.expanded_terms.clone(),
            entity_candidates: self.anchor_terms.clone(),
            graph_expansions: Vec::new(),
            fuzzy_variants: self.fuzzy_variants.clone(),
            prefix_variants: self.prefix_variants.clone(),
            retrieval_plan: vec![
                "semantic".to_string(),
                "keyword".to_string(),
                "prefix".to_string(),
                "fuzzy".to_string(),
                "ngram".to_string(),
                "graph".to_string(),
            ],
        }
    }
}

pub fn normalize_text(input: &str) -> String {
    input
        .to_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || ch.is_whitespace() {
                ch
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn extract_base_anchor_terms(normalized_query: &str) -> Vec<String> {
    if normalized_query.is_empty() {
        return Vec::new();
    }

    let mut anchors = Vec::new();
    anchors.push(normalized_query.to_string());

    for token in normalized_query.split_whitespace() {
        if token.len() <= 1 {
            continue;
        }
        if is_anchor_stop_word(token) && !token.chars().any(|ch| ch.is_ascii_digit()) {
            continue;
        }
        push_unique(&mut anchors, token);
    }

    anchors
}

fn push_unique(out: &mut Vec<String>, value: &str) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return;
    }
    if !out.iter().any(|existing| existing == trimmed) {
        out.push(trimmed.to_string());
    }
}

fn singularize(token: &str) -> String {
    if token.len() > 4 && token.ends_with("ies") {
        return format!("{}y", &token[..token.len() - 3]);
    }
    if token.len() > 3 && token.ends_with('s') {
        return token[..token.len() - 1].to_string();
    }
    token.to_string()
}

fn drop_char_variant(token: &str) -> String {
    let mut chars = token.chars().collect::<Vec<_>>();
    if chars.len() <= 3 {
        return token.to_string();
    }
    chars.remove(chars.len() / 2);
    chars.into_iter().collect()
}

fn char_ngrams(token: &str, n: usize) -> Vec<String> {
    let compact = token.replace(' ', "");
    if compact.len() < n {
        return Vec::new();
    }
    let chars = compact.chars().collect::<Vec<_>>();
    let mut out = Vec::new();
    for idx in 0..=(chars.len() - n) {
        out.push(chars[idx..idx + n].iter().collect::<String>());
    }
    out
}

fn is_anchor_stop_word(token: &str) -> bool {
    matches!(
        token,
        "a" | "an"
            | "and"
            | "are"
            | "as"
            | "at"
            | "be"
            | "for"
            | "from"
            | "in"
            | "is"
            | "it"
            | "of"
            | "on"
            | "or"
            | "that"
            | "the"
            | "this"
            | "to"
            | "was"
            | "what"
            | "when"
            | "where"
            | "who"
            | "why"
            | "with"
            | "open"
            | "go"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_terms_without_hardcoded_synonyms() {
        let context = QueryContext::from_query("MCP servers");
        assert!(context.anchor_terms.iter().any(|term| term == "mcp"));
        assert!(context.expanded_terms.iter().any(|term| term == "server"));
        assert!(!context.prefix_variants.is_empty());
        assert!(!context.ngram_variants.is_empty());
    }

    #[test]
    fn normalizes_query() {
        assert_eq!(normalize_text("Hello, World!"), "hello world");
    }

    #[test]
    fn de_dupes_variants() {
        let context = QueryContext::from_query("tasks task");
        let unique = context
            .expanded_terms
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len();
        assert_eq!(unique, context.expanded_terms.len());
    }
}
