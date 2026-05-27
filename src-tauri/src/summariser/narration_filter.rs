use once_cell::sync::Lazy;
use regex::Regex;

use super::display_summary::{build_display_summary, clean_sentence, fallback_display_summary};

static BANNED_PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
    [
        r"(?i)^you reviewed",
        r"(?i)^user viewed",
        r"(?i)^the user is viewing",
        r"(?i)^the ocr text indicates",
        r"(?i)\bvisual-only frame\b",
        r"(?i)\bno visible content\b",
        r"(?i)^screen capture shows",
        r"(?i)noting\s+[A-Z]",
        r"(?i)then\s+you",
        r"(?i)\buser\.$",
        r"(?i)memory_compaction",
        r"(?i)src-tauri",
    ]
    .iter()
    .map(|pattern| Regex::new(pattern).expect("valid narration filter regex"))
    .collect()
});

static SCRUB_PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
    [
        r"(?i)^you reviewed\s+",
        r"(?i)^user viewed\s+",
        r"(?i)^the user is viewing\s+",
        r"(?i)^the ocr text indicates\s+",
        r"(?i)\bvisual-only frame\b",
        r"(?i)\bno visible content\b",
        r"(?i)^screen capture shows\s+",
        r"(?i)^then\s+you\s+",
        r"(?i)\bnoting\s+[A-Z][^,.!?]*",
        r"(?i)\bmemory_compaction\b",
        r"(?i)\bsrc-tauri\b",
    ]
    .iter()
    .map(|pattern| Regex::new(pattern).expect("valid narration scrub regex"))
    .collect()
});

pub fn narration_filter_hits(summary: &str) -> bool {
    let value = summary.trim();
    if value.is_empty() {
        return false;
    }
    BANNED_PATTERNS
        .iter()
        .any(|pattern| pattern.is_match(value))
}

pub fn clean_or_fallback_display_summary(
    candidate: &str,
    page_title: &str,
    url: Option<&str>,
    timestamp_ms: i64,
) -> (String, bool) {
    let generated = build_display_summary(page_title, url, candidate, timestamp_ms);
    if !narration_filter_hits(&generated) {
        return (generated, false);
    }

    // Regeneration pass: scrub known narration markers and re-normalize once.
    let mut scrubbed = generated.clone();
    for pattern in SCRUB_PATTERNS.iter() {
        scrubbed = pattern.replace_all(&scrubbed, " ").to_string();
    }
    let scrubbed = clean_sentence(&scrubbed);
    if !scrubbed.is_empty() {
        let regenerated = build_display_summary(page_title, url, &scrubbed, timestamp_ms);
        if !narration_filter_hits(&regenerated) {
            return (regenerated, true);
        }
    }

    (
        fallback_display_summary(page_title, url, timestamp_ms),
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_narration_leaks() {
        assert!(narration_filter_hits(
            "You reviewed memory_compaction.rs and tests"
        ));
        assert!(narration_filter_hits("User viewed dashboard"));
        assert!(narration_filter_hits(
            "The OCR text indicates a visual-only frame with no visible content"
        ));
        assert!(narration_filter_hits("Screen capture shows a browser page"));
        assert!(!narration_filter_hits("Watched IPL highlights on YouTube."));
    }

    #[test]
    fn scrub_or_fallback_removes_internal_voice() {
        let (summary, filtered) = clean_or_fallback_display_summary(
            "You reviewed FNDR src-tauri memory_compaction while noting Refactor ideas",
            "FNDR Refactor",
            Some("https://github.com/org/repo"),
            1_700_000_000_000,
        );

        assert!(filtered);
        assert!(!narration_filter_hits(&summary));
        assert!(summary.ends_with('.'));
    }
}
