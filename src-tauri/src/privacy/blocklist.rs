//! Application and web-context blocklist management

/// Blocklist for applications and websites that should not be captured.
pub struct Blocklist;

impl Blocklist {
    /// Check if an application is blocked
    pub fn is_blocked(app_name: &str, blocklist: &[String]) -> bool {
        let app_lower = app_name.to_lowercase();
        blocklist.iter().any(|blocked| {
            let blocked_lower = blocked.to_lowercase();
            app_lower.contains(&blocked_lower) || blocked_lower.contains(&app_lower)
        })
    }

    /// Check if the current URL/title matches a blocked site or title pattern.
    pub fn is_context_blocked(
        url: Option<&str>,
        window_title: Option<&str>,
        blocklist: &[String],
    ) -> bool {
        let host = url.and_then(normalize_host);
        let url_lower = url.unwrap_or("").to_lowercase();
        let title_lower = window_title.unwrap_or("").to_lowercase();

        blocklist.iter().any(|blocked| {
            let blocked = blocked.trim();
            if blocked.is_empty() {
                return false;
            }

            let blocked_lower = blocked.to_lowercase();
            if !title_lower.is_empty() && title_lower.contains(&blocked_lower) {
                return true;
            }
            if !url_lower.is_empty() && url_lower.contains(&blocked_lower) {
                return true;
            }

            let Some(host) = host.as_deref() else {
                return false;
            };
            let Some(blocked_host) = normalize_host(blocked) else {
                return false;
            };

            host == blocked_host
                || host.ends_with(&format!(".{blocked_host}"))
                || blocked_host.ends_with(&format!(".{host}"))
        })
    }

    /// Canonical alert/blocklist key for a URL or title.
    pub fn context_key(url: Option<&str>, window_title: Option<&str>) -> Option<String> {
        if let Some(host) = url.and_then(normalize_host) {
            return Some(host);
        }
        window_title
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .map(str::to_string)
    }

    /// Check if the frontmost app belongs to FNDR itself and should never be captured.
    ///
    /// This is **not** the user blocklist — it is a hard privacy rule so the
    /// app does not OCR/embed its own UI. Pipeline stats use
    /// [`crate::SkipReason::SelfApp`] (not `Blocklist`) so the Capture Status
    /// UI does not read this as "Finder" or a mistaken privacy block.
    pub fn is_internal_app(app_name: &str, bundle_id: Option<&str>) -> bool {
        let normalized_name = app_name.trim().to_lowercase();
        if normalized_name.starts_with("fndr") && !normalized_name.contains("meeting") {
            return true;
        }

        bundle_id.is_some_and(|bundle| {
            let normalized_bundle = bundle.trim().to_lowercase();
            normalized_bundle == "com.fndr"
                || normalized_bundle.starts_with("com.fndr.")
                || normalized_bundle.ends_with(".fndr")
                || normalized_bundle.contains(".fndr.")
        })
    }

    /// Check if the current context (URL or Title) suggests a highly sensitive site (like banks)
    /// that isn't already explicitly blocked, so we can prompt the user to block it.
    pub fn is_sensitive_context(url: Option<&str>, window_title: Option<&str>) -> bool {
        let keywords = [
            "chase.com",
            "bankofamerica",
            "wellsfargo",
            "citibank",
            "capitalone",
            "usbank",
            "fidelity",
            "vanguard",
            "schwab",
            "americanexpress",
            "discover.com",
            "online banking",
            "sign in - bank",
            "login - bank",
        ];

        let url_lower = url.unwrap_or("").to_lowercase();
        let title_lower = window_title.unwrap_or("").to_lowercase();

        keywords
            .iter()
            .any(|&k| url_lower.contains(k) || title_lower.contains(k))
    }
}

fn normalize_host(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    let without_scheme = trimmed.split("://").nth(1).unwrap_or(trimmed);
    let authority = without_scheme
        .split('/')
        .next()
        .unwrap_or(without_scheme)
        .split('?')
        .next()
        .unwrap_or(without_scheme)
        .split('#')
        .next()
        .unwrap_or(without_scheme)
        .trim()
        .trim_end_matches('.');

    if authority.is_empty() {
        return None;
    }

    let without_userinfo = authority.rsplit('@').next().unwrap_or(authority);
    let host = without_userinfo
        .split(':')
        .next()
        .unwrap_or(without_userinfo)
        .trim()
        .trim_start_matches("www.")
        .to_lowercase();

    if host.contains('.') {
        Some(host)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match() {
        let blocklist = vec!["1Password".to_string()];
        assert!(Blocklist::is_blocked("1Password", &blocklist));
    }

    #[test]
    fn test_case_insensitive() {
        let blocklist = vec!["1Password".to_string()];
        assert!(Blocklist::is_blocked("1password", &blocklist));
    }

    #[test]
    fn test_partial_match() {
        let blocklist = vec!["Keychain".to_string()];
        assert!(Blocklist::is_blocked("Keychain Access", &blocklist));
    }

    #[test]
    fn test_not_blocked() {
        let blocklist = vec!["1Password".to_string()];
        assert!(!Blocklist::is_blocked("Safari", &blocklist));
    }

    #[test]
    fn test_url_context_blocked_by_domain() {
        let blocklist = vec!["chase.com".to_string()];
        assert!(Blocklist::is_context_blocked(
            Some("https://secure.chase.com/web/auth"),
            Some("Chase Sign In"),
            &blocklist
        ));
    }

    #[test]
    fn test_url_context_not_blocked_by_different_domain() {
        let blocklist = vec!["chase.com".to_string()];
        assert!(!Blocklist::is_context_blocked(
            Some("https://example.com"),
            Some("Example"),
            &blocklist
        ));
    }

    #[test]
    fn test_context_key_normalizes_url_to_host() {
        assert_eq!(
            Blocklist::context_key(Some("https://www.bankofamerica.com/login"), None),
            Some("bankofamerica.com".to_string())
        );
    }

    #[test]
    fn test_detects_internal_app_by_name() {
        assert!(Blocklist::is_internal_app("FNDR", None));
        assert!(!Blocklist::is_internal_app("FNDR Meetings", None));
    }

    #[test]
    fn test_detects_internal_app_by_bundle() {
        assert!(Blocklist::is_internal_app(
            "Anything",
            Some("com.fndr.desktop")
        ));
        assert!(!Blocklist::is_internal_app(
            "Finder",
            Some("com.apple.finder")
        ));
    }
}
