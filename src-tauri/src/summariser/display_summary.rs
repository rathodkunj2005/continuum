use chrono::{Local, TimeZone};

const MAX_DISPLAY_WORDS: usize = 30;
const MAX_DISPLAY_CHARS: usize = 220;

pub fn build_display_summary(
    page_title: &str,
    url: Option<&str>,
    candidate: &str,
    timestamp_ms: i64,
) -> String {
    let cleaned = clean_sentence(candidate);
    if cleaned.is_empty() {
        return fallback_display_summary(page_title, url, timestamp_ms);
    }

    let trimmed = truncate_words(&cleaned, MAX_DISPLAY_WORDS);
    let trimmed = trim_chars(&trimmed, MAX_DISPLAY_CHARS);
    ensure_period(&trimmed)
}

pub fn fallback_display_summary(page_title: &str, url: Option<&str>, timestamp_ms: i64) -> String {
    let title = clean_sentence(page_title);
    let domain = domain_from_url(url);
    let time_label = Local
        .timestamp_millis_opt(timestamp_ms)
        .single()
        .map(|dt| dt.format("%I:%M %p").to_string())
        .unwrap_or_else(|| "recently".to_string());

    let mut summary = if !title.is_empty() {
        if let Some(domain) = domain {
            format!("Viewed {} on {} at {}", title, domain, time_label)
        } else {
            format!("Viewed {} at {}", title, time_label)
        }
    } else if let Some(domain) = domain {
        format!("Viewed content on {} at {}", domain, time_label)
    } else {
        format!("Captured recent activity at {}", time_label)
    };

    summary = truncate_words(&summary, MAX_DISPLAY_WORDS);
    summary = trim_chars(&summary, MAX_DISPLAY_CHARS);
    ensure_period(&summary)
}

pub fn clean_sentence(input: &str) -> String {
    let mut text = input.replace('\n', " ");
    text = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();
    if text.is_empty() {
        return String::new();
    }

    // Keep only the first sentence to enforce concise, one-sentence summaries.
    let sentence_end = text.char_indices().find_map(|(idx, ch)| {
        if matches!(ch, '.' | '!' | '?') {
            Some(idx)
        } else {
            None
        }
    });
    if let Some(end_idx) = sentence_end {
        text = text[..=end_idx].to_string();
    }

    text = text
        .trim_matches(|ch: char| ch == '"' || ch == '\'' || ch == '`')
        .trim()
        .to_string();

    if text.is_empty() {
        return String::new();
    }

    // Remove parenthetical asides and em-dash style side comments.
    text = text.replace(['(', ')'], " ");
    text = text.replace('—', "-");
    text = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();

    text
}

fn ensure_period(sentence: &str) -> String {
    let mut out = sentence
        .trim()
        .trim_end_matches(['.', '!', '?'])
        .to_string();
    if !out.is_empty() {
        out.push('.');
    }
    out
}

fn truncate_words(text: &str, max_words: usize) -> String {
    text.split_whitespace()
        .take(max_words)
        .collect::<Vec<_>>()
        .join(" ")
}

fn trim_chars(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect::<String>()
}

fn domain_from_url(url: Option<&str>) -> Option<String> {
    let raw = url?.trim();
    if raw.is_empty() {
        return None;
    }

    let host = raw
        .split("://")
        .nth(1)
        .unwrap_or(raw)
        .split('/')
        .next()
        .unwrap_or_default()
        .trim();

    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_display_summary_keeps_single_sentence_and_word_budget() {
        let summary = build_display_summary(
            "IPL 2026 highlights - YouTube",
            Some("https://www.youtube.com/watch?v=abc"),
            "Watched IPL highlights and compared match stats. Then reviewed unrelated code changes.",
            1_700_000_000_000,
        );

        assert!(summary.ends_with('.'));
        assert!(summary.split_whitespace().count() <= MAX_DISPLAY_WORDS);
        assert!(!summary.to_lowercase().contains("then reviewed unrelated"));
    }

    #[test]
    fn fallback_display_summary_uses_title_or_domain() {
        let with_title = fallback_display_summary(
            "Cricket Schedule",
            Some("https://www.espncricinfo.com/story"),
            1_700_000_000_000,
        );
        assert!(with_title.to_lowercase().contains("cricket schedule"));

        let no_title =
            fallback_display_summary("", Some("https://example.com/path"), 1_700_000_000_000);
        assert!(no_title.to_lowercase().contains("example.com"));
    }
}
