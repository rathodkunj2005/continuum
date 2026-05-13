//! Centralized URL / title / activity substrings for timeline action classification.
//! Add or adjust rules here (and extend tests) instead of scattering literals in `classify.rs`.

/// Lowercased URL fragments that strongly indicate a meeting surface.
pub const MEETING_URL_FRAGMENTS: &[&str] = &["zoom", "meet.google"];

/// Lowercased title/summary fragments for meetings when URL is absent.
pub const MEETING_TITLE_FRAGMENTS: &[&str] = &["zoom meeting", "zoom", "meet.google"];

/// Mail / chat host fragments (lowercased URL).
pub const COMMUNICATION_URL_FRAGMENTS: &[&str] = &["mail.google", "slack.com", "discord.com"];

/// Lowercased title/summary fragments for communication.
pub const COMMUNICATION_TITLE_FRAGMENTS: &[&str] = &["inbox"];

/// PR / MR URL path segments or review phrases (lowercased).
pub const REVIEW_URL_PATHS: &[&str] = &["/pull/", "/merge_requests/"];
pub const REVIEW_TITLE_PHRASES: &[&str] = &["pull request", "code review"];

/// Source file extensions implying coding work (lowercased path suffix).
pub const CODING_FILE_SUFFIXES: &[&str] = &[".rs", ".ts", ".tsx", ".go", ".py"];

/// Title phrases implying debugging / compile work.
pub const CODING_TITLE_PHRASES: &[&str] = &["debug", "compiler error"];

/// Doc / writing hosts (lowercased URL).
pub const WRITING_URL_HOSTS: &[&str] = &["notion.", "docs.google"];
pub const WRITING_TITLE_PHRASES: &[&str] = &["design doc", "prd"];

/// Planning tools (lowercased URL).
pub const PLANNING_URL_HOSTS: &[&str] = &["linear.app", "jira"];
pub const PLANNING_TITLE_PHRASES: &[&str] = &["roadmap", "sprint planning"];

/// Research hosts / phrases.
pub const RESEARCH_URL_HOSTS: &[&str] = &["arxiv.org", "wikipedia.org"];
pub const RESEARCH_TITLE_PHRASES: &[&str] = &["research"];

pub fn any_substring(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| haystack.contains(n))
}

pub fn any_path_suffix(paths: &[String], suffixes: &[&str]) -> bool {
    paths.iter().any(|p| {
        let pl = p.to_lowercase();
        suffixes.iter().any(|suf| pl.ends_with(suf))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_tables_nonempty() {
        assert!(!MEETING_URL_FRAGMENTS.is_empty());
        assert!(!CODING_FILE_SUFFIXES.is_empty());
    }
}
