//! Shared keyword normalization and light URL canonicalization for indexing paths.

use std::collections::HashSet;

pub(super) fn normalize_keyword_text(input: &str) -> String {
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

pub(super) fn trim_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let keep = max_chars.saturating_sub(3);
    let mut out = value.chars().take(keep).collect::<String>();
    out.push_str("...");
    out
}

pub(super) fn is_keyword_stop_word(token: &str) -> bool {
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
    )
}

pub(super) fn keyword_terms(query: &str) -> Vec<String> {
    let normalized = normalize_keyword_text(query);
    if normalized.is_empty() {
        return Vec::new();
    }

    let mut terms = Vec::new();
    let mut seen = HashSet::new();
    let mut push = |value: String| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return;
        }
        if seen.insert(trimmed.to_string()) {
            terms.push(trimmed.to_string());
        }
    };

    push(normalized.clone());

    for token in normalized.split_whitespace() {
        if token.len() <= 1 {
            continue;
        }
        if is_keyword_stop_word(token) && !token.chars().any(|ch| ch.is_ascii_digit()) {
            continue;
        }
        push(token.to_string());

        if token.len() >= 3 {
            push(token[..3].to_string());
        }
        if token.len() >= 4 {
            push(token[..4].to_string());
        }
        if token.len() >= 5 {
            push(drop_middle_char(token));
        }
        if token.len() >= 6 {
            for gram in char_ngrams(token, 3).into_iter().take(3) {
                push(gram);
            }
        }
    }

    terms.truncate(24);
    terms
}

pub(super) fn drop_middle_char(token: &str) -> String {
    let mut chars = token.chars().collect::<Vec<_>>();
    if chars.len() <= 3 {
        return token.to_string();
    }
    chars.remove(chars.len() / 2);
    chars.into_iter().collect()
}

pub(super) fn char_ngrams(token: &str, n: usize) -> Vec<String> {
    let chars = token.chars().collect::<Vec<_>>();
    if chars.len() < n {
        return Vec::new();
    }
    let mut grams = Vec::new();
    for idx in 0..=(chars.len() - n) {
        grams.push(chars[idx..idx + n].iter().collect());
    }
    grams
}

pub(super) fn canonicalize_index_url(url: &str) -> String {
    let no_fragment = url.split('#').next().unwrap_or(url);
    let no_query = no_fragment.split('?').next().unwrap_or(no_fragment);
    no_query.trim_end_matches('/').to_string()
}
