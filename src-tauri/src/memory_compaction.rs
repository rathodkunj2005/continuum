//! Helpers for compacting persisted memory payloads.

use crate::embedding::{TextChunker, EMBEDDING_DIM};
use crate::storage::MemoryRecord;

const SUMMARY_CLEAN_TEXT_CHARS: usize = 360;
const FALLBACK_CLEAN_TEXT_CHARS: usize = 560;
const GENERIC_CLEAN_TEXT_CHARS: usize = 420;
const LEXICAL_SHADOW_CHARS: usize = 320;
const SUPPORT_CHUNK_CHARS: usize = 720;
const SUPPORT_CHUNK_COUNT: usize = 4;
const COMPACT_EMBED_CHARS: usize = 720;
const EMBEDDING_MIN_NORM: f32 = 1e-6;

pub fn compact_clean_text(summary_source: &str, snippet: &str, clean_text: &str) -> String {
    let summary_src = summary_source.trim().to_ascii_lowercase();
    let is_high_signal_summary = summary_src == "llm"
        || summary_src == "vlm"
        || summary_src == "vision_mtmd"
        || summary_src == "vision_fallback";

    if is_high_signal_summary {
        let normalized_snippet = normalize_memory_text(snippet);
        if !normalized_snippet.is_empty() {
            return trim_chars(&normalized_snippet, SUMMARY_CLEAN_TEXT_CHARS);
        }
    }

    let normalized_clean = normalize_memory_text(clean_text);
    if !normalized_clean.is_empty() {
        let limit = if summary_src == "fallback" {
            FALLBACK_CLEAN_TEXT_CHARS
        } else {
            GENERIC_CLEAN_TEXT_CHARS
        };
        return trim_chars(&normalized_clean, limit);
    }

    let normalized_snippet = normalize_memory_text(snippet);
    if !normalized_snippet.is_empty() {
        let limit = match summary_src.as_str() {
            "llm" | "vlm" | "vision_mtmd" | "vision_fallback" => SUMMARY_CLEAN_TEXT_CHARS,
            "fallback" => FALLBACK_CLEAN_TEXT_CHARS,
            _ => GENERIC_CLEAN_TEXT_CHARS,
        };
        return trim_chars(&normalized_snippet, limit);
    }

    String::new()
}

pub fn compact_memory_record_payload(record: &MemoryRecord) -> MemoryRecord {
    let mut compacted = record.clone();
    let lexical_shadow_source = if record.lexical_shadow.trim().is_empty() {
        build_lexical_shadow(
            &record.window_title,
            &record.snippet,
            &record.clean_text,
            record.url.as_deref(),
        )
    } else {
        record.lexical_shadow.clone()
    };
    compacted.lexical_shadow = compact_lexical_shadow(&lexical_shadow_source);
    compacted.text = String::new();
    compacted.clean_text =
        compact_clean_text(&record.summary_source, &record.snippet, &record.clean_text);
    compacted.screenshot_path = None;
    compacted
}

pub fn best_embedding_text(record: &MemoryRecord) -> String {
    let precomputed = normalize_memory_text(&record.embedding_text);
    if !precomputed.is_empty() {
        return trim_chars(&precomputed, COMPACT_EMBED_CHARS * 2);
    }
    let context = normalize_memory_text(&record.memory_context);
    if !context.is_empty() {
        let mut parts = vec![context];
        if !record.user_intent.trim().is_empty() {
            parts.insert(
                0,
                format!("intent {}", normalize_memory_text(&record.user_intent)),
            );
        }
        if !record.project.trim().is_empty() {
            parts.insert(
                1.min(parts.len()),
                format!("project {}", normalize_memory_text(&record.project)),
            );
        }
        return trim_chars(&parts.join(" "), COMPACT_EMBED_CHARS * 2);
    }
    let clean = normalize_memory_text(&record.clean_text);
    let shadow = compact_lexical_shadow(&record.lexical_shadow);
    if !clean.is_empty() {
        if shadow.is_empty() {
            return clean;
        }
        return trim_chars(&format!("{clean} {shadow}"), COMPACT_EMBED_CHARS);
    }
    let snippet = normalize_memory_text(&record.snippet);
    if !snippet.is_empty() {
        if shadow.is_empty() {
            return snippet;
        }
        return trim_chars(&format!("{snippet} {shadow}"), COMPACT_EMBED_CHARS);
    }
    if shadow.is_empty() {
        normalize_memory_text(&record.window_title)
    } else {
        trim_chars(
            &format!("{} {}", normalize_memory_text(&record.window_title), shadow),
            COMPACT_EMBED_CHARS,
        )
    }
}

pub fn best_snippet_embedding_text(record: &MemoryRecord) -> String {
    compact_summary_embedding_text(
        &record.summary_source,
        &record.snippet,
        &record.clean_text,
        &record.lexical_shadow,
    )
}

pub fn best_support_embedding_texts(record: &MemoryRecord) -> Vec<String> {
    support_embedding_texts(
        &record.app_name,
        &record.window_title,
        &record.clean_text,
        &record.lexical_shadow,
    )
}

pub fn compact_summary_embedding_text(
    summary_source: &str,
    snippet: &str,
    clean_text: &str,
    lexical_shadow: &str,
) -> String {
    let base = compact_clean_text(summary_source, snippet, clean_text);
    let shadow = compact_lexical_shadow(lexical_shadow);
    if shadow.is_empty() {
        return trim_chars(&base, COMPACT_EMBED_CHARS);
    }
    if base.is_empty() {
        return trim_chars(&shadow, COMPACT_EMBED_CHARS);
    }

    let base_norm = normalize_memory_text(&base).to_lowercase();
    let shadow_terms = shadow
        .split_whitespace()
        .filter(|term| term.len() >= 3)
        .filter(|term| !base_norm.contains(&term.to_ascii_lowercase()))
        .collect::<Vec<_>>();

    if shadow_terms.is_empty() {
        trim_chars(&base, COMPACT_EMBED_CHARS)
    } else {
        trim_chars(
            &format!("{base} support {}", shadow_terms.join(" ")),
            COMPACT_EMBED_CHARS,
        )
    }
}

pub fn support_embedding_texts(
    app_name: &str,
    window_title: &str,
    clean_text: &str,
    lexical_shadow: &str,
) -> Vec<String> {
    let chunker = TextChunker::new();
    let chunks = chunker.chunk_ocr_text(app_name, window_title, clean_text);
    let mut selected = Vec::new();

    if let Some(first) = chunks.first() {
        selected.push(trim_chars(
            &normalize_memory_text(first),
            SUPPORT_CHUNK_CHARS,
        ));
    }
    if let Some(longest) = chunks.iter().max_by_key(|chunk| chunk.len()) {
        selected.push(trim_chars(
            &normalize_memory_text(longest),
            SUPPORT_CHUNK_CHARS,
        ));
    }
    if let Some(last) = chunks.last() {
        selected.push(trim_chars(
            &normalize_memory_text(last),
            SUPPORT_CHUNK_CHARS,
        ));
    }

    if selected.is_empty() {
        let clean = normalize_memory_text(clean_text);
        if !clean.is_empty() {
            selected.push(trim_chars(&clean, SUPPORT_CHUNK_CHARS));
        }
    }

    let shadow = compact_lexical_shadow(lexical_shadow);
    if !shadow.is_empty() {
        selected.push(shadow);
    }

    let mut deduped = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for text in selected {
        let normalized = normalize_memory_text(&text);
        if normalized.is_empty() {
            continue;
        }
        let key = normalized.to_lowercase();
        if seen.insert(key) {
            deduped.push(normalized);
        }
        if deduped.len() >= SUPPORT_CHUNK_COUNT {
            break;
        }
    }
    deduped
}

pub fn mean_pool_embeddings(vectors: &[Vec<f32>]) -> Vec<f32> {
    if vectors.is_empty() {
        return vec![0.0; EMBEDDING_DIM];
    }

    let dim = vectors
        .iter()
        .find(|vector| !vector.is_empty())
        .map(|vector| vector.len())
        .unwrap_or(EMBEDDING_DIM);
    let mut pooled = vec![0.0; dim];
    let mut count = 0usize;

    for vector in vectors {
        if vector.len() != dim {
            continue;
        }
        for (index, value) in vector.iter().enumerate() {
            pooled[index] += *value;
        }
        count += 1;
    }

    if count == 0 {
        return vec![0.0; dim];
    }

    for value in &mut pooled {
        *value /= count as f32;
    }
    normalize_vector(&mut pooled);
    pooled
}

fn normalize_vector(vector: &mut [f32]) {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm <= EMBEDDING_MIN_NORM {
        return;
    }
    for value in vector {
        *value /= norm;
    }
}

pub fn build_lexical_shadow(
    window_title: &str,
    snippet: &str,
    clean_text: &str,
    url: Option<&str>,
) -> String {
    let mut parts = Vec::new();
    let mut seen = std::collections::HashSet::new();

    if let Some(url) = url {
        let normalized = normalize_memory_text(url);
        for token in normalized.split_whitespace() {
            push_shadow_token(&mut parts, &mut seen, token);
            if parts.join(" ").chars().count() >= LEXICAL_SHADOW_CHARS {
                return compact_lexical_shadow(&parts.join(" "));
            }
        }
    }

    for source in [window_title, snippet, clean_text] {
        for token in shadow_candidates(source) {
            push_shadow_token(&mut parts, &mut seen, &token);
            if parts.join(" ").chars().count() >= LEXICAL_SHADOW_CHARS {
                return compact_lexical_shadow(&parts.join(" "));
            }
        }
    }

    compact_lexical_shadow(&parts.join(" "))
}

fn compact_lexical_shadow(raw: &str) -> String {
    trim_chars(&normalize_memory_text(raw), LEXICAL_SHADOW_CHARS)
}

pub fn is_low_signal_embedding(vector: &[f32]) -> bool {
    if vector.is_empty() {
        return true;
    }
    let mut norm = 0.0f32;
    for value in vector {
        if !value.is_finite() {
            return true;
        }
        norm += value * value;
    }
    norm.sqrt() <= EMBEDDING_MIN_NORM
}

fn normalize_memory_text(raw: &str) -> String {
    raw.chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

fn trim_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect::<String>()
}

fn shadow_candidates(source: &str) -> Vec<String> {
    let mut out = Vec::new();

    for segment in source.split('`').skip(1).step_by(2) {
        let normalized = normalize_memory_text(segment);
        if normalized.len() >= 3 {
            out.push(normalized);
        }
    }

    for token in source.split_whitespace() {
        let trimmed = token.trim_matches(|ch: char| {
            ch.is_ascii_punctuation()
                && ch != '.'
                && ch != '/'
                && ch != '_'
                && ch != '-'
                && ch != ':'
        });
        if !looks_like_shadow_token(trimmed) {
            continue;
        }
        out.push(trimmed.to_string());
    }

    out
}

fn looks_like_shadow_token(token: &str) -> bool {
    let trimmed = token.trim();
    if trimmed.len() < 3 || trimmed.len() > 96 {
        return false;
    }
    if trimmed.chars().all(|ch| ch.is_ascii_digit()) {
        return false;
    }
    if trimmed.contains("://") || trimmed.starts_with("www.") {
        return true;
    }
    if trimmed.contains("::") || trimmed.contains("->") || trimmed.ends_with("()") {
        return true;
    }
    if trimmed.contains('/') && trimmed.chars().any(|ch| ch.is_ascii_alphabetic()) {
        return true;
    }
    if trimmed.contains('_') || trimmed.contains('-') {
        return true;
    }
    if trimmed.contains('.') {
        let lower = trimmed.to_ascii_lowercase();
        if lower.ends_with(".rs")
            || lower.ends_with(".ts")
            || lower.ends_with(".tsx")
            || lower.ends_with(".js")
            || lower.ends_with(".jsx")
            || lower.ends_with(".json")
            || lower.ends_with(".toml")
            || lower.ends_with(".md")
            || lower.ends_with(".py")
            || lower.ends_with(".yml")
            || lower.ends_with(".yaml")
            || lower.ends_with(".csv")
            || lower.ends_with(".pdf")
        {
            return true;
        }
        let parts = lower.split('.').collect::<Vec<_>>();
        if parts.len() >= 2 && parts.iter().all(|part| !part.is_empty()) {
            return true;
        }
    }
    let has_upper = trimmed.chars().any(|ch| ch.is_ascii_uppercase());
    let has_lower = trimmed.chars().any(|ch| ch.is_ascii_lowercase());
    let has_digit = trimmed.chars().any(|ch| ch.is_ascii_digit());
    if (has_upper && has_lower) || (has_digit && (has_upper || has_lower)) {
        return true;
    }
    if has_upper
        && trimmed
            .chars()
            .next()
            .unwrap_or_default()
            .is_ascii_uppercase()
    {
        return true;
    }
    false
}

fn push_shadow_token(
    parts: &mut Vec<String>,
    seen: &mut std::collections::HashSet<String>,
    raw: &str,
) {
    let normalized = normalize_memory_text(raw);
    if normalized.is_empty() {
        return;
    }
    let key = normalized.to_ascii_lowercase();
    if seen.insert(key) {
        parts.push(normalized);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(snippet: &str, clean_text: &str) -> MemoryRecord {
        MemoryRecord {
            id: "memory-1".to_string(),
            timestamp: 1,
            day_bucket: "2026-04-21".to_string(),
            app_name: "Chrome".to_string(),
            bundle_id: None,
            window_title: "Title".to_string(),
            session_id: "session-1".to_string(),
            text: "raw ocr payload".to_string(),
            clean_text: clean_text.to_string(),
            ocr_confidence: 0.9,
            ocr_block_count: 4,
            snippet: snippet.to_string(),
            summary_source: "llm".to_string(),
            noise_score: 0.1,
            session_key: "session-key".to_string(),
            lexical_shadow: String::new(),
            embedding: vec![0.1; EMBEDDING_DIM],
            image_embedding: vec![0.0; crate::config::DEFAULT_IMAGE_EMBEDDING_DIM],
            screenshot_path: Some("/tmp/screenshot.png".to_string()),
            url: None,
            snippet_embedding: vec![0.2; EMBEDDING_DIM],
            support_embedding: vec![0.0; EMBEDDING_DIM],
            decay_score: 1.0,
            last_accessed_at: 0,
            ..Default::default()
        }
    }

    #[test]
    fn compaction_preserves_clean_text_and_clears_payload_fields() {
        let source = record(
            "Discussed fixing memory reclaim and preserving embeddings.",
            "very long raw ocr text should not remain",
        );
        let compacted = compact_memory_record_payload(&source);

        assert!(compacted.text.is_empty());
        // LLM/VLM summaries prefer the normalized snippet as durable clean text.
        assert_eq!(compacted.clean_text, normalize_memory_text(&source.snippet));
        assert!(compacted.screenshot_path.is_none());
    }

    #[test]
    fn compaction_uses_snippet_when_clean_text_is_empty() {
        let source = record(
            "Discussed fixing memory reclaim and preserving embeddings.",
            "",
        );
        let compacted = compact_memory_record_payload(&source);

        assert_eq!(compacted.clean_text, normalize_memory_text(&source.snippet));
    }

    #[test]
    fn low_signal_embedding_detects_zero_vectors() {
        assert!(is_low_signal_embedding(&vec![0.0; EMBEDDING_DIM]));
        assert!(!is_low_signal_embedding(&vec![0.01; EMBEDDING_DIM]));
    }
}
