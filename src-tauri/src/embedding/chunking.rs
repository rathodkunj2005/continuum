//! OCR-aware text chunking for embedding.

use crate::capture::text_cleanup;
use crate::config::{
    ChunkingConfig, DEFAULT_CHARS_PER_TOKEN, DEFAULT_CHUNK_MAX_TOKENS, DEFAULT_CHUNK_MIN_TOKENS,
    DEFAULT_CHUNK_OCR_TARGET_MAX_CHARS, DEFAULT_CHUNK_OCR_TARGET_MIN_CHARS,
    DEFAULT_CHUNK_OVERLAP_TOKENS,
};

const MAX_CHUNK_TOKENS: usize = DEFAULT_CHUNK_MAX_TOKENS;
const CHUNK_OVERLAP: usize = DEFAULT_CHUNK_OVERLAP_TOKENS;
const MIN_CHUNK_TOKENS: usize = DEFAULT_CHUNK_MIN_TOKENS;
const CHARS_PER_TOKEN: usize = DEFAULT_CHARS_PER_TOKEN;

const OCR_TARGET_MIN: usize = DEFAULT_CHUNK_OCR_TARGET_MIN_CHARS;
const OCR_TARGET_MAX: usize = DEFAULT_CHUNK_OCR_TARGET_MAX_CHARS;

/// Text chunker for splitting long texts.
pub struct TextChunker {
    max_chars: usize,
    overlap_chars: usize,
    min_chunk_tokens: usize,
    chars_per_token: usize,
    ocr_target_min: usize,
    ocr_target_max: usize,
}

#[derive(Debug, Clone)]
pub struct TextChunk {
    pub text: String,
    pub approx_tokens: usize,
    pub chunk_index: usize,
    pub line_kind: &'static str,
    /// Dominant `LineKind` as an enum so callers can pattern-match without
    /// parsing the string label.
    pub dominant_line_kind: LineKind,
    /// Byte span in the cleaned source text when the chunker can preserve it
    /// without guessing. OCR line regrouping leaves this unset.
    pub source_start_byte: Option<usize>,
    pub source_end_byte: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Title,
    Url,
    Search,
    Email,
    Code,
    Plain,
}

impl TextChunker {
    pub fn new() -> Self {
        Self {
            max_chars: MAX_CHUNK_TOKENS * CHARS_PER_TOKEN,
            overlap_chars: CHUNK_OVERLAP * CHARS_PER_TOKEN,
            min_chunk_tokens: MIN_CHUNK_TOKENS,
            chars_per_token: CHARS_PER_TOKEN,
            ocr_target_min: OCR_TARGET_MIN,
            ocr_target_max: OCR_TARGET_MAX,
        }
    }

    /// Build a `TextChunker` from runtime `ChunkingConfig`.
    ///
    /// Use this instead of `TextChunker::new()` wherever a loaded `Config` is
    /// available so that the operator can tune chunking without recompiling.
    pub fn from_config(cfg: &ChunkingConfig) -> Self {
        let cpt = cfg.chars_per_token.max(1);
        let max_tokens = cfg.max_tokens.max(1);
        let min_tokens = cfg.min_tokens.clamp(1, max_tokens);
        let overlap_tokens = cfg.overlap_tokens.min(max_tokens.saturating_sub(1));
        Self {
            max_chars: max_tokens * cpt,
            overlap_chars: overlap_tokens * cpt,
            min_chunk_tokens: min_tokens,
            chars_per_token: cpt,
            ocr_target_min: cfg.ocr_target_min_chars,
            ocr_target_max: cfg.ocr_target_max_chars,
        }
    }

    /// Split plain text into embedding chunks.
    pub fn chunk(&self, text: &str) -> Vec<String> {
        self.chunk_ocr_text("", "", text)
    }

    /// OCR-aware chunking that preserves semantic boundaries and drops low-signal lines.
    pub fn chunk_ocr_text(&self, app_name: &str, window_title: &str, text: &str) -> Vec<String> {
        chunk_screen_text(self, app_name, window_title, text)
    }

    /// Product-named wrapper for the capture -> OCR -> embedding pipeline.
    pub fn chunk_screen_text(&self, app_name: &str, window_title: &str, text: &str) -> Vec<String> {
        self.chunk_ocr_text_with_metadata(app_name, window_title, text)
            .into_iter()
            .map(|chunk| chunk.text)
            .collect()
    }

    /// OCR-aware chunking with lightweight metadata used for diagnostics/ranking.
    pub fn chunk_ocr_text_with_metadata(
        &self,
        app_name: &str,
        window_title: &str,
        text: &str,
    ) -> Vec<TextChunk> {
        let cleaned_text = text_cleanup::reduce_chrome_noise_for_app(app_name, text);
        let mut lines = Vec::new();
        let title = normalize_line(window_title);
        if !title.is_empty() && !self.is_low_signal_line(&title) {
            lines.push((title, LineKind::Title));
        }

        let mut seen_lines = std::collections::HashSet::new();
        for raw_line in cleaned_text.lines() {
            let line = normalize_line(raw_line);
            if line.is_empty() || self.is_low_signal_line(&line) {
                continue;
            }
            let dedup_key = line.to_lowercase();
            if !seen_lines.insert(dedup_key) {
                continue;
            }
            lines.push((line.clone(), classify_line(&line)));
        }

        // Fallback: use CLEANED text, not raw OCR input, so noise already
        // stripped by `reduce_chrome_noise_for_app` stays gone.
        if lines.is_empty() {
            return self
                .chunk_by_chars_with_spans(&cleaned_text)
                .into_iter()
                .enumerate()
                .map(|(index, chunk)| TextChunk {
                    approx_tokens: approx_tokens_for(&chunk.text, self.chars_per_token),
                    text: chunk.text,
                    chunk_index: index,
                    line_kind: line_kind_label(LineKind::Plain),
                    dominant_line_kind: LineKind::Plain,
                    source_start_byte: Some(chunk.start_byte),
                    source_end_byte: Some(chunk.end_byte),
                })
                .collect();
        }

        let mut chunks: Vec<(String, LineKind)> = Vec::new();
        let mut current = String::new();
        let mut current_kind = LineKind::Plain;

        for (line, kind) in lines {
            if line.len() > self.ocr_target_max {
                if !current.trim().is_empty() {
                    chunks.push((current.trim().to_string(), current_kind));
                    current.clear();
                }
                for chunk in self.chunk_by_chars(&line) {
                    chunks.push((chunk, kind));
                }
                current_kind = LineKind::Plain;
                continue;
            }

            // Force a chunk break on URL lines once the current chunk is at
            // or beyond `min_chunk_size`. This prevents URL noise from being
            // merged into semantic prose chunks.
            let url_break = kind == LineKind::Url
                && current.len() >= self.min_chunk_tokens * self.chars_per_token;

            let should_boundary_break = url_break
                || matches!(kind, LineKind::Code | LineKind::Email | LineKind::Search)
                || matches!(
                    current_kind,
                    LineKind::Code | LineKind::Email | LineKind::Search
                )
                || (kind != current_kind && !current.is_empty());

            if should_boundary_break && !current.is_empty() && current.len() >= self.ocr_target_min
            {
                chunks.push((current.trim().to_string(), current_kind));
                let overlap = self.overlap_chars_for_text(&current);
                let prev_tail = overlap_tail(&current, overlap);
                current.clear();
                if !prev_tail.is_empty() {
                    current.push_str(&prev_tail);
                    current.push('\n');
                }
            }

            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(&line);
            current_kind = kind;

            if current.len() >= self.ocr_target_max {
                chunks.push((current.trim().to_string(), current_kind));
                let overlap = self.overlap_chars_for_text(&current);
                let prev_tail = overlap_tail(&current, overlap);
                current.clear();
                if !prev_tail.is_empty() {
                    current.push_str(&prev_tail);
                }
            }
        }

        if !current.trim().is_empty() {
            chunks.push((current.trim().to_string(), current_kind));
        }

        // Secondary fallback also uses cleaned text.
        let mut rendered = if chunks.is_empty() {
            self.chunk_by_chars(&cleaned_text)
                .into_iter()
                .enumerate()
                .map(|(index, chunk)| TextChunk {
                    approx_tokens: approx_tokens_for(&chunk, self.chars_per_token),
                    text: chunk,
                    chunk_index: index,
                    line_kind: line_kind_label(LineKind::Plain),
                    dominant_line_kind: LineKind::Plain,
                    source_start_byte: None,
                    source_end_byte: None,
                })
                .collect::<Vec<_>>()
        } else {
            chunks
                .into_iter()
                .enumerate()
                .map(|(index, (chunk, kind))| TextChunk {
                    approx_tokens: approx_tokens_for(&chunk, self.chars_per_token),
                    text: chunk,
                    chunk_index: index,
                    line_kind: line_kind_label(kind),
                    dominant_line_kind: kind,
                    source_start_byte: None,
                    source_end_byte: None,
                })
                .collect::<Vec<_>>()
        };

        // Drop near-identical chunks from the same frame to keep index pressure low.
        let mut seen = std::collections::HashSet::new();
        rendered.retain(|chunk| {
            let key = normalize_line(&chunk.text).to_lowercase();
            seen.insert(key)
        });

        merge_short_orphans(rendered, self.min_chunk_tokens, self.chars_per_token)
    }

    /// Compute how many chars to carry over as overlap for a given text
    /// segment. Scales with text length and clamps to the configured overlap.
    /// Used by both the OCR boundary path and the char-fallback path.
    fn overlap_chars_for_text(&self, text: &str) -> usize {
        let configured_tokens = self.overlap_chars / self.chars_per_token;
        if configured_tokens == 0 {
            return 0;
        }
        let tokens = approx_tokens_for(text, self.chars_per_token);
        let target = ((tokens as f32) * 0.24).round() as usize;
        let min_tokens = self.min_chunk_tokens.min(configured_tokens);
        target.clamp(min_tokens, configured_tokens) * self.chars_per_token
    }

    pub fn is_low_signal_line(&self, line: &str) -> bool {
        let normalized = normalize_line(line);
        if normalized.len() < 6 {
            return true;
        }
        if text_cleanup::symbol_ratio(&normalized) > 0.62 {
            return true;
        }
        if text_cleanup::looks_like_file_inventory(&normalized)
            && !self.is_code_like_line(&normalized)
        {
            return true;
        }

        let lower = normalized.to_lowercase();
        if matches!(lower.as_str(), "new tab" | "home" | "trending" | "untitled") {
            return true;
        }

        false
    }

    pub fn is_code_like_line(&self, line: &str) -> bool {
        is_code_token(line)
    }

    pub fn is_search_like_line(&self, line: &str) -> bool {
        is_search_query(line)
    }

    pub fn is_email_like_line(&self, line: &str) -> bool {
        is_email_header(line)
    }

    fn chunk_by_chars(&self, text: &str) -> Vec<String> {
        self.chunk_by_chars_with_spans(text)
            .into_iter()
            .map(|chunk| chunk.text)
            .collect()
    }

    fn chunk_by_chars_with_spans(&self, text: &str) -> Vec<CharChunk> {
        if text.trim().is_empty() {
            return Vec::new();
        }

        if text.len() <= self.max_chars {
            return vec![CharChunk {
                text: text.to_string(),
                start_byte: 0,
                end_byte: text.len(),
            }];
        }

        let mut chunks = Vec::new();
        let mut start = 0;

        while start < text.len() {
            start = floor_char_boundary(text, start);
            let end = floor_char_boundary(text, (start + self.max_chars).min(text.len()));
            if end <= start {
                break;
            }

            // Try to break at word boundary
            let chunk_end = if end < text.len() {
                text[start..end]
                    .rfind(|c: char| c.is_whitespace())
                    .map(|pos| start + pos)
                    .unwrap_or(end)
            } else {
                end
            };

            let chunk = text[start..chunk_end].trim().to_string();
            if !chunk.is_empty() {
                chunks.push(CharChunk {
                    text: chunk,
                    start_byte: start,
                    end_byte: chunk_end,
                });
            }

            let overlap = self.overlap_chars_for_text(&text[start..chunk_end]);
            let next_start = if overlap > 0 && chunk_end > overlap {
                floor_char_boundary(text, chunk_end - overlap)
            } else {
                chunk_end
            };

            // Safety: ensure we're making progress
            if start >= text.len() || chunk_end == text.len() {
                break;
            }
            start = if next_start <= start {
                chunk_end
            } else {
                next_start
            };
        }

        chunks
    }
}

#[derive(Debug, Clone)]
struct CharChunk {
    text: String,
    start_byte: usize,
    end_byte: usize,
}

pub fn chunk_screen_text(
    chunker: &TextChunker,
    app_name: &str,
    window_title: &str,
    text: &str,
) -> Vec<String> {
    chunker
        .chunk_ocr_text_with_metadata(app_name, window_title, text)
        .into_iter()
        .map(|chunk| chunk.text)
        .collect()
}

/// Shared parent-child RAG selector: chunk sanitized OCR with the configured
/// chunker, then choose the highest-salience chunks for child embedding.
pub fn select_salient_memory_chunks(
    chunking: &ChunkingConfig,
    app_name: &str,
    window_title: &str,
    clean_text: &str,
    max_chunks: usize,
) -> Vec<TextChunk> {
    let cap = max_chunks.max(1);
    let chunker = TextChunker::from_config(chunking);
    let mut scored = chunker
        .chunk_ocr_text_with_metadata(app_name, window_title, clean_text)
        .into_iter()
        .filter(|chunk| !chunk.text.trim().is_empty())
        .map(|chunk| {
            let score = text_cleanup::rank_salient_spans(&chunk.text, app_name)
                .into_iter()
                .map(|span| span.score)
                .fold(0.0_f32, f32::max);
            (chunk, score)
        })
        .collect::<Vec<_>>();

    scored.sort_by(|(left, left_score), (right, right_score)| {
        right_score
            .partial_cmp(left_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.chunk_index.cmp(&right.chunk_index))
    });
    scored
        .into_iter()
        .take(cap)
        .map(|(chunk, _)| chunk)
        .collect()
}

fn line_kind_label(kind: LineKind) -> &'static str {
    match kind {
        LineKind::Title => "title",
        LineKind::Url => "url",
        LineKind::Search => "search",
        LineKind::Email => "email",
        LineKind::Code => "code",
        LineKind::Plain => "plain",
    }
}

/// Classify a text line by its dominant content type without constructing a
/// full `TextChunker`. Pure functions used here have no mutable state.
fn classify_line(line: &str) -> LineKind {
    let lower = line.to_lowercase();
    if line.starts_with("http://") || line.starts_with("https://") || lower.contains("www.") {
        return LineKind::Url;
    }
    if lower.contains(" - ") && line.len() < 120 {
        return LineKind::Title;
    }

    if is_email_header(line) {
        return LineKind::Email;
    }
    if is_search_query(line) {
        return LineKind::Search;
    }
    if is_code_token(line) {
        return LineKind::Code;
    }

    LineKind::Plain
}

// --- standalone predicate helpers (no TextChunker allocation) ---------------

fn is_email_header(line: &str) -> bool {
    let lower = line.trim().to_lowercase();
    lower.starts_with("from:")
        || lower.starts_with("to:")
        || lower.starts_with("subject:")
        || lower.starts_with("cc:")
        || lower.starts_with("bcc:")
}

fn is_search_query(line: &str) -> bool {
    let lower = line.trim().to_lowercase();
    lower.starts_with("search ")
        || lower.starts_with("search:")
        || lower.starts_with("query:")
        || lower.starts_with("find ")
        || lower.contains(" results for ")
        || lower.ends_with(" near me")
}

fn is_code_token(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.starts_with('$') || trimmed.starts_with('>') {
        return true;
    }
    let lower = trimmed.to_lowercase();
    lower.starts_with("cargo ")
        || lower.starts_with("npm ")
        || lower.starts_with("pnpm ")
        || lower.starts_with("git ")
        || lower.starts_with("fn ")
        || lower.starts_with("let ")
        || lower.contains(" => ")
        || lower.contains("::")
        || (trimmed.contains('{') && trimmed.contains('}'))
        || (trimmed.contains('(') && trimmed.contains(')') && trimmed.contains(';'))
}

// ---------------------------------------------------------------------------

fn normalize_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

/// Single shared helper for both the OCR-boundary path and char-fallback path.
/// Takes the last `target_chars` worth of lines from `text` for overlap.
fn overlap_tail(text: &str, target_chars: usize) -> String {
    if target_chars == 0 {
        return String::new();
    }

    let mut chars = 0usize;
    let mut lines: Vec<String> = Vec::new();
    for line in text.lines().rev() {
        let normalized = normalize_line(line);
        if normalized.is_empty() {
            continue;
        }
        chars += normalized.len();
        lines.push(normalized);
        if chars >= target_chars {
            break;
        }
    }
    lines.reverse();
    lines.join("\n")
}

fn approx_tokens_for(text: &str, chars_per_token: usize) -> usize {
    (text.len() / chars_per_token.max(1)).max(1)
}

fn stitch_chunks(left: &str, right: &str) -> String {
    let left = left.trim();
    let right = right.trim();

    if left.is_empty() {
        return right.to_string();
    }
    if right.is_empty() {
        return left.to_string();
    }
    if left == right || left.ends_with(right) {
        return left.to_string();
    }
    if right.starts_with(left) {
        return right.to_string();
    }

    format!("{left}\n{right}")
}

fn merge_source_span(
    left: Option<(usize, usize)>,
    right: Option<(usize, usize)>,
) -> Option<(usize, usize)> {
    match (left, right) {
        (Some((left_start, left_end)), Some((right_start, right_end))) => {
            Some((left_start.min(right_start), left_end.max(right_end)))
        }
        _ => None,
    }
}

fn merge_short_orphans(
    mut chunks: Vec<TextChunk>,
    min_tokens: usize,
    chars_per_token: usize,
) -> Vec<TextChunk> {
    if chunks.len() <= 1 {
        for (index, chunk) in chunks.iter_mut().enumerate() {
            chunk.chunk_index = index;
            chunk.approx_tokens = approx_tokens_for(&chunk.text, chars_per_token);
        }
        return chunks;
    }

    let mut merged: Vec<TextChunk> = Vec::with_capacity(chunks.len());
    for mut chunk in chunks.drain(..) {
        chunk.approx_tokens = approx_tokens_for(&chunk.text, chars_per_token);
        if chunk.approx_tokens < min_tokens && chunk.dominant_line_kind != LineKind::Url {
            if let Some(previous) = merged.last_mut() {
                let span = merge_source_span(
                    previous.source_start_byte.zip(previous.source_end_byte),
                    chunk.source_start_byte.zip(chunk.source_end_byte),
                );
                previous.text = stitch_chunks(&previous.text, &chunk.text);
                previous.approx_tokens = approx_tokens_for(&previous.text, chars_per_token);
                previous.source_start_byte = span.map(|(start, _)| start);
                previous.source_end_byte = span.map(|(_, end)| end);
                continue;
            }
        }
        merged.push(chunk);
    }

    if merged.len() >= 2
        && merged[0].approx_tokens < min_tokens
        && merged[0].dominant_line_kind != LineKind::Url
    {
        let first = merged.remove(0);
        if let Some(next) = merged.first_mut() {
            let span = merge_source_span(
                first.source_start_byte.zip(first.source_end_byte),
                next.source_start_byte.zip(next.source_end_byte),
            );
            next.text = stitch_chunks(&first.text, &next.text);
            next.approx_tokens = approx_tokens_for(&next.text, chars_per_token);
            next.source_start_byte = span.map(|(start, _)| start);
            next.source_end_byte = span.map(|(_, end)| end);
        } else {
            merged.push(first);
        }
    }

    for (index, chunk) in merged.iter_mut().enumerate() {
        chunk.chunk_index = index;
        chunk.approx_tokens = approx_tokens_for(&chunk.text, chars_per_token);
    }

    merged
}

impl Default for TextChunker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_short_text_no_chunking() {
        let chunker = TextChunker::new();
        let text = "Hello world";
        let chunks = chunker.chunk(text);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "Hello world");
    }

    #[test]
    fn test_empty_ocr_text_produces_no_chunks() {
        let chunker = TextChunker::new();
        assert!(chunker
            .chunk_ocr_text("Chrome", "New Tab", "   \n\t")
            .is_empty());
    }

    #[test]
    fn test_ocr_chunking_removes_repeated_garbage_lines() {
        let chunker = TextChunker::new();
        let repeated = "syncing status syncing status\n".repeat(12);
        let text = format!("{repeated}\nPlanning launch checklist for FNDR search pipeline");
        let chunks = chunker.chunk_ocr_text("Chrome", "Launch Plan", &text);
        let merged = chunks.join("\n").to_lowercase();
        assert_eq!(merged.matches("syncing status syncing status").count(), 1);
        assert!(merged.contains("planning launch checklist"));
    }

    #[test]
    fn test_long_text_chunking() {
        let chunker = TextChunker::new();
        let text = "word ".repeat(500); // >2000 chars
        let chunks = chunker.chunk(&text);
        assert!(chunks.len() > 1);
        // Each chunk should be within limit
        for chunk in &chunks {
            assert!(chunk.len() <= chunker.max_chars + 50); // Allow some flexibility
        }
    }

    #[test]
    fn test_ocr_chunking_drops_chrome_lines() {
        let chunker = TextChunker::new();
        let text = "New Tab\nHome\nTrending\nPlanning launch checklist for FNDR search pipeline";
        let chunks = chunker.chunk_ocr_text("Chrome", "New Tab", text);
        let merged = chunks.join("\n").to_lowercase();
        assert!(merged.contains("planning launch checklist"));
        assert!(!merged.contains("new tab"));
        assert!(!merged.contains("trending"));
    }

    #[test]
    fn test_line_helpers() {
        let chunker = TextChunker::new();
        assert!(chunker.is_code_like_line("let x = foo(bar);"));
        assert!(chunker.is_email_like_line("Subject: Weekly update"));
        assert!(chunker.is_search_like_line("Search: best tennis racket"));
        assert!(chunker.is_low_signal_line("new tab"));
    }

    #[test]
    fn test_chunking_merges_short_orphan_tails() {
        let chunker = TextChunker::new();
        let text = format!("{} tail words", "alpha ".repeat(410));
        let chunks = chunker.chunk(&text);
        assert!(chunks
            .iter()
            .all(|chunk| chunk.split_whitespace().count() >= 15));
    }

    // ---- new tests for the chunking correctness work -------------------------

    #[test]
    fn test_from_config_overrides_max_chunk_size() {
        use crate::config::ChunkingConfig;

        // Default config produces large chunks; a tight config produces more.
        let mut cfg = ChunkingConfig::default();
        cfg.max_tokens = 20; // ~80 chars per chunk
        cfg.overlap_tokens = 2;
        cfg.min_tokens = 1;
        cfg.ocr_target_min_chars = 20;
        cfg.ocr_target_max_chars = 80;

        let tight = TextChunker::from_config(&cfg);
        let default = TextChunker::new();

        let text = "word ".repeat(200); // 1 000 chars
        let tight_chunks = tight.chunk(&text);
        let default_chunks = default.chunk(&text);

        assert!(
            tight_chunks.len() > default_chunks.len(),
            "tight config should produce more chunks than default (got {} vs {})",
            tight_chunks.len(),
            default_chunks.len()
        );
    }

    #[test]
    fn test_from_config_chunk_count_differs_from_default() {
        use crate::config::ChunkingConfig;

        // Verify that a config with a larger max_tokens produces fewer chunks.
        let mut big_cfg = ChunkingConfig::default();
        big_cfg.max_tokens = 900;
        big_cfg.overlap_tokens = 0;
        big_cfg.ocr_target_max_chars = 3_600;

        let chunker_big = TextChunker::from_config(&big_cfg);
        let chunker_default = TextChunker::new();

        let text = "sentence content here. ".repeat(300); // ~6 600 chars
        let big_chunks = chunker_big.chunk(&text);
        let def_chunks = chunker_default.chunk(&text);

        assert!(
            big_chunks.len() < def_chunks.len(),
            "larger max_tokens should produce fewer chunks (got {} vs {})",
            big_chunks.len(),
            def_chunks.len()
        );
    }

    #[test]
    fn test_overlap_suffix_shared_between_adjacent_chunks() {
        let chunker = TextChunker::new();
        // Build text that will definitely be split into multiple char-chunks.
        let text = (0..500)
            .map(|index| format!("unique_word_{index:04}"))
            .collect::<Vec<_>>()
            .join(" ");
        let chunks = chunker.chunk(&text);
        assert!(chunks.len() >= 2, "need at least 2 chunks for overlap test");

        let c0_words = chunks[0].split_whitespace().collect::<Vec<_>>();
        let c1_words = chunks[1].split_whitespace().collect::<Vec<_>>();
        let suffix = c0_words.iter().rev().take(32).copied().collect::<Vec<_>>();
        let prefix = c1_words.iter().take(40).copied().collect::<Vec<_>>();
        let any_shared = suffix.iter().any(|word| prefix.contains(word));
        assert!(
            any_shared,
            "expected shared suffix/prefix overlap; c0 suffix: {:?}, c1 prefix: {:?}",
            suffix, prefix
        );
    }

    #[test]
    fn test_url_line_starts_new_chunk_after_min_size() {
        use crate::config::ChunkingConfig;

        let mut cfg = ChunkingConfig::default();
        cfg.min_tokens = 5; // very low so "sentence." reaches it quickly
        cfg.ocr_target_min_chars = 20;
        cfg.ocr_target_max_chars = 2_000;

        let chunker = TextChunker::from_config(&cfg);

        // Prose followed by a URL — the URL should open a new chunk.
        let prose = "This is meaningful prose content about a topic. ".repeat(3);
        let url = "https://example.com/some-long-path?query=value";
        let text = format!("{prose}\n{url}");

        let chunks = chunker.chunk_ocr_text_with_metadata("Chrome", "Test Page", &text);

        // At least one chunk should be dominated by the URL kind.
        let has_url_chunk = chunks.iter().any(|c| c.dominant_line_kind == LineKind::Url);
        assert!(
            has_url_chunk,
            "expected a chunk with dominant_line_kind=Url"
        );
    }

    #[test]
    fn salient_memory_chunk_selection_enforces_cap() {
        use crate::config::ChunkingConfig;

        let mut cfg = ChunkingConfig::default();
        cfg.max_tokens = 32;
        cfg.overlap_tokens = 4;
        cfg.min_tokens = 1;
        cfg.ocr_target_min_chars = 40;
        cfg.ocr_target_max_chars = 140;
        cfg.max_chunks_per_memory = 2;
        let text = (0..12)
            .map(|i| {
                format!(
                    "Implement memory chunk pipeline item {i}. This should preserve parent context and fix retrieval precision."
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        let selected = select_salient_memory_chunks(
            &cfg,
            "Code",
            "Parent child RAG notes",
            &text,
            cfg.max_chunks_per_memory,
        );

        assert_eq!(selected.len(), 2);
        assert!(selected.iter().all(|chunk| !chunk.text.trim().is_empty()));
    }

    #[test]
    fn test_chrome_heavy_text_chunk_count_within_range() {
        let chunker = TextChunker::new();
        // Simulate a Chrome page with lots of navigation/noise + content.
        let mut text = String::new();
        for _ in 0..8 {
            text.push_str("New Tab\nHome\nTrending\nBookmarks\n");
        }
        text.push_str("How to set up a Rust workspace with multiple crates: a deep-dive tutorial covering Cargo.toml, workspace members, shared dependencies, and publish flows.\n");
        text.push_str("The workspace root Cargo.toml should list all crate members.\n");
        text.push_str("Each crate has its own Cargo.toml with a name and version.\n");

        let chunks = chunker.chunk_ocr_text("Chrome", "Rust Workspaces Tutorial", &text);
        assert!(
            !chunks.is_empty(),
            "should produce at least one chunk from chrome-heavy text"
        );
        assert!(
            chunks.len() <= 10,
            "chrome-heavy text should not explode into too many chunks (got {})",
            chunks.len()
        );
    }

    #[test]
    fn test_char_fallback_uses_cleaned_text_not_raw_ocr() {
        let chunker = TextChunker::new();
        let browser_chrome_only = "New Tab\nHome\nTrending\nUntitled";
        let chunks = chunker.chunk_ocr_text_with_metadata("Chrome", "", browser_chrome_only);
        assert!(
            chunks.is_empty(),
            "fallback must use cleaned text; raw browser chrome should not reappear"
        );

        let real_content = "Planning the sprint backlog for upcoming release cycle";
        let text = format!("@ # $ % ^ & *\n{real_content}");
        let chunks = chunker.chunk_ocr_text("Chrome", "Sprint Planning", &text);
        let merged = chunks.join(" ");
        assert!(
            merged.contains("sprint backlog"),
            "real content should survive"
        );
    }

    #[test]
    fn test_char_chunks_expose_source_byte_spans() {
        use crate::config::ChunkingConfig;

        let mut cfg = ChunkingConfig::default();
        cfg.max_tokens = 12;
        cfg.overlap_tokens = 2;
        cfg.min_tokens = 1;
        let chunker = TextChunker::from_config(&cfg);
        let chunks = chunker.chunk_by_chars_with_spans(&"span ".repeat(60));

        assert!(chunks.len() > 1);
        for chunk in chunks {
            assert!(chunk.start_byte < chunk.end_byte);
            assert_eq!(
                chunk.text,
                "span ".repeat(60)[chunk.start_byte..chunk.end_byte]
                    .trim()
                    .to_string()
            );
        }
    }
}
