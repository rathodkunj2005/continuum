//! Share-policy classifier and the cluster/manager sharing gate.
//!
//! Two independent decisions govern whether an observation reaches the team
//! graph:
//!
//! 1. **Per-observation safety** — [`classify`] labels each descriptor
//!    `BLOCKED` / `LOCAL_ONLY` / `SHARED_ANON`, porting the reference desktop's
//!    `privacy.classify` and reusing Continuum's existing [`Blocklist`] for app
//!    and context blocking. This is a privacy *floor*: it can only ever
//!    withhold data, never force-share it.
//! 2. **Cluster/manager policy** — [`ClusterSharePolicy`] is set by the cluster
//!    manager (not a local per-user toggle). It decides whether `SHARED_ANON`
//!    observations are actually pushed. The safe default is [`Disabled`], so a
//!    freshly-joined member shares nothing until the manager opts the cluster
//!    in.
//!
//! [`Disabled`]: ClusterSharePolicy::Disabled

use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::cloud::descriptor::Descriptor;
use crate::privacy::Blocklist;

/// Sensitive content that must never leave the device, regardless of policy.
const BLOCK_KEYWORDS: &[&str] = &[
    "password",
    "bank",
    "chase",
    "wellsfargo",
    "paypal",
    "venmo",
    "whatsapp",
    "imessage",
    "messenger",
    "signal",
    "credit card",
    "creditcard",
    "ssn",
    "social security",
];

/// Personal-but-not-secret content that stays on-device (kept out of the team
/// graph but still locally useful).
const LOCAL_ONLY_KEYWORDS: &[&str] = &[
    "salary",
    "compensation",
    "offer letter",
    "personal",
    "private",
];

/// Apps blocked by macOS bundle id no matter what is on screen. Matched exactly
/// or as a prefix (`entry.`) to also catch helper processes. Ported from the
/// reference desktop's `BUNDLE_BLOCKLIST`.
const BUNDLE_BLOCKLIST: &[&str] = &[
    // Password managers
    "com.agilebits.onepassword",
    "com.1password.1password",
    "com.lastpass.lastpass",
    "com.bitwarden.desktop",
    "com.apple.keychainaccess",
    // Private messaging
    "net.whatsapp.whatsapp",
    "com.apple.mobilesms",
    "org.whispersystems.signal-desktop",
    "ru.keepcoder.telegram",
    "org.telegram.desktop",
    // Banking / finance
    "com.intuit.quickbooks",
    "com.paypal.here",
];

/// Per-observation safety label. Mirrors the reference desktop's three states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ShareDecision {
    /// Dropped on-device — never stored remotely or sent anywhere.
    Blocked,
    /// Kept in the local pipeline only; never reaches the team graph.
    LocalOnly,
    /// Eligible for the team graph (subject to the cluster policy gate).
    SharedAnon,
}

impl ShareDecision {
    pub fn label(&self) -> &'static str {
        match self {
            ShareDecision::Blocked => "BLOCKED",
            ShareDecision::LocalOnly => "LOCAL_ONLY",
            ShareDecision::SharedAnon => "SHARED_ANON",
        }
    }
}

/// Cluster-level sharing policy, controlled by the cluster manager. Resolved
/// from the backend (and overridable via `CONTINUUM_CLUSTER_SHARE_MODE` for
/// local testing). Defaults to [`Disabled`](Self::Disabled).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClusterSharePolicy {
    /// No member shares; the whole cluster is graph-silent.
    Disabled,
    /// Members share `SHARED_ANON` observations (the safety floor still applies).
    Members,
    /// Members share only if they have also locally opted in.
    OptIn,
}

impl Default for ClusterSharePolicy {
    fn default() -> Self {
        ClusterSharePolicy::Disabled
    }
}

impl ClusterSharePolicy {
    /// Parse a wire/env value: `disabled` | `members` | `opt_in`. Tolerates
    /// case and `-`/` ` separators. Unknown values resolve to `None`.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().replace([' ', '-'], "_").as_str() {
            "disabled" | "off" | "none" => Some(Self::Disabled),
            "members" | "all" | "all_members" | "on" => Some(Self::Members),
            "opt_in" | "optin" => Some(Self::OptIn),
            _ => None,
        }
    }

    /// Local override from `CONTINUUM_CLUSTER_SHARE_MODE` (for exercising the
    /// pipeline before the backend carries an explicit policy column).
    pub fn from_env() -> Option<Self> {
        std::env::var("CONTINUUM_CLUSTER_SHARE_MODE")
            .ok()
            .as_deref()
            .and_then(Self::parse)
    }
}

/// Context for [`classify`]. All fields are optional except the user blocklist.
pub struct ClassifyCtx<'a> {
    pub bundle_id: Option<&'a str>,
    pub app_name: Option<&'a str>,
    pub url: Option<&'a str>,
    pub window_title: Option<&'a str>,
    /// User has toggled manual Private Mode (Continuum's "incognito").
    pub private_mode: bool,
    /// The user's configured app/context blocklist.
    pub user_blocklist: &'a [String],
}

/// Label a descriptor `BLOCKED` / `LOCAL_ONLY` / `SHARED_ANON`. This is the
/// privacy floor: it can withhold but never force-share.
pub fn classify(descriptor: &Descriptor, ctx: &ClassifyCtx) -> ShareDecision {
    // 1. Hard blocks — sensitive app or sensitive content never leaves.
    if is_blocked_bundle(ctx.bundle_id) {
        return ShareDecision::Blocked;
    }
    if let Some(app) = ctx.app_name {
        if Blocklist::is_internal_app(app, ctx.bundle_id)
            || Blocklist::is_blocked(app, ctx.user_blocklist)
        {
            return ShareDecision::Blocked;
        }
    }
    if Blocklist::is_sensitive_context(ctx.url, ctx.window_title) {
        return ShareDecision::Blocked;
    }
    let blob = blob(descriptor, ctx.app_name);
    if contains_any(&blob, BLOCK_KEYWORDS) {
        return ShareDecision::Blocked;
    }

    // 2. Manual Private Mode keeps everything local.
    if ctx.private_mode {
        return ShareDecision::LocalOnly;
    }

    // 3. Personal-but-not-secret content stays on-device.
    if contains_any(&blob, LOCAL_ONLY_KEYWORDS) {
        return ShareDecision::LocalOnly;
    }

    // 4. Default: shareable work context (scrubbed before it leaves).
    ShareDecision::SharedAnon
}

/// Whether a classified observation should actually be pushed to the team
/// graph, combining the cluster policy with the per-observation decision.
pub fn allows_graph_push(
    policy: ClusterSharePolicy,
    decision: ShareDecision,
    local_opt_in: bool,
) -> bool {
    if decision != ShareDecision::SharedAnon {
        return false;
    }
    match policy {
        ClusterSharePolicy::Disabled => false,
        ClusterSharePolicy::Members => true,
        ClusterSharePolicy::OptIn => local_opt_in,
    }
}

/// Strip identity-revealing details from a descriptor before it leaves the
/// device. Ported from the reference desktop's `scrub`.
pub fn scrub(descriptor: &Descriptor) -> Descriptor {
    Descriptor {
        app: scrub_str(&descriptor.app),
        topic: scrub_str(&descriptor.topic),
        concept: scrub_str(&descriptor.concept),
        error_type: descriptor.error_type.as_deref().map(scrub_str),
    }
}

fn scrub_str(s: &str) -> String {
    let mut out = s.to_string();
    for (re, replacement) in scrubbers() {
        out = re.replace_all(&out, *replacement).into_owned();
    }
    out
}

/// Lazily-compiled scrubbing rules: home paths -> `~`, emails -> `<email>`,
/// long tokens/hashes -> `<token>`.
fn scrubbers() -> &'static [(Regex, &'static str)] {
    static RULES: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    RULES.get_or_init(|| {
        vec![
            (Regex::new(r"/(Users|home)/[^/\s]+").unwrap(), "~"),
            (
                Regex::new(r"[\w.+-]+@[\w-]+\.[\w.-]+").unwrap(),
                "<email>",
            ),
            (Regex::new(r"\b[A-Za-z0-9_-]{24,}\b").unwrap(), "<token>"),
        ]
    })
}

fn is_blocked_bundle(bundle_id: Option<&str>) -> bool {
    let Some(id) = bundle_id else {
        return false;
    };
    let id = id.to_lowercase();
    BUNDLE_BLOCKLIST
        .iter()
        .any(|e| id == *e || id.starts_with(&format!("{e}.")))
}

/// Lower-cased haystack of the descriptor JSON plus the app name, matching the
/// reference classifier's `blob`.
fn blob(descriptor: &Descriptor, app_name: Option<&str>) -> String {
    let json = serde_json::to_string(descriptor).unwrap_or_default();
    format!("{json} {}", app_name.unwrap_or("")).to_lowercase()
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| haystack.contains(n))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn desc(app: &str, topic: &str, concept: &str) -> Descriptor {
        Descriptor {
            app: app.to_string(),
            topic: topic.to_string(),
            concept: concept.to_string(),
            error_type: None,
        }
    }

    fn ctx<'a>(bundle: Option<&'a str>, app: Option<&'a str>, private_mode: bool) -> ClassifyCtx<'a> {
        ClassifyCtx {
            bundle_id: bundle,
            app_name: app,
            url: None,
            window_title: None,
            private_mode,
            user_blocklist: &[],
        }
    }

    #[test]
    fn shared_anon_for_ordinary_work() {
        let d = desc("VS Code", "rust pipeline", "editing the capture loop");
        assert_eq!(
            classify(&d, &ctx(Some("com.microsoft.vscode"), Some("VS Code"), false)),
            ShareDecision::SharedAnon
        );
    }

    #[test]
    fn blocks_sensitive_bundle() {
        let d = desc("1Password", "vault", "viewing logins");
        assert_eq!(
            classify(&d, &ctx(Some("com.1password.1password"), Some("1Password"), false)),
            ShareDecision::Blocked
        );
    }

    #[test]
    fn blocks_helper_process_of_sensitive_bundle() {
        let d = desc("1Password", "vault", "viewing logins");
        assert_eq!(
            classify(
                &d,
                &ctx(Some("com.1password.1password-helper"), Some("1Password"), false)
            ),
            ShareDecision::Blocked
        );
    }

    #[test]
    fn blocks_sensitive_keyword_in_content() {
        let d = desc("Safari", "finance", "checking my bank balance");
        assert_eq!(
            classify(&d, &ctx(Some("com.apple.safari"), Some("Safari"), false)),
            ShareDecision::Blocked
        );
    }

    #[test]
    fn private_mode_forces_local_only() {
        let d = desc("VS Code", "rust pipeline", "editing the capture loop");
        assert_eq!(
            classify(&d, &ctx(Some("com.microsoft.vscode"), Some("VS Code"), true)),
            ShareDecision::LocalOnly
        );
    }

    #[test]
    fn personal_keyword_is_local_only() {
        let d = desc("Notes", "personal", "my private journal entry");
        assert_eq!(
            classify(&d, &ctx(Some("com.apple.notes"), Some("Notes"), false)),
            ShareDecision::LocalOnly
        );
    }

    #[test]
    fn respects_user_blocklist() {
        let d = desc("Slack", "team chat", "discussing the launch");
        let blocklist = vec!["Slack".to_string()];
        let c = ClassifyCtx {
            bundle_id: Some("com.tinyspeck.slackmacgap"),
            app_name: Some("Slack"),
            url: None,
            window_title: None,
            private_mode: false,
            user_blocklist: &blocklist,
        };
        assert_eq!(classify(&d, &c), ShareDecision::Blocked);
    }

    #[test]
    fn policy_gate_default_disabled_blocks_push() {
        assert!(!allows_graph_push(
            ClusterSharePolicy::default(),
            ShareDecision::SharedAnon,
            true
        ));
    }

    #[test]
    fn policy_gate_members_allows_shared_anon_only() {
        assert!(allows_graph_push(
            ClusterSharePolicy::Members,
            ShareDecision::SharedAnon,
            false
        ));
        assert!(!allows_graph_push(
            ClusterSharePolicy::Members,
            ShareDecision::LocalOnly,
            false
        ));
    }

    #[test]
    fn policy_gate_opt_in_requires_local_flag() {
        assert!(!allows_graph_push(
            ClusterSharePolicy::OptIn,
            ShareDecision::SharedAnon,
            false
        ));
        assert!(allows_graph_push(
            ClusterSharePolicy::OptIn,
            ShareDecision::SharedAnon,
            true
        ));
    }

    #[test]
    fn parses_policy_values() {
        assert_eq!(ClusterSharePolicy::parse("members"), Some(ClusterSharePolicy::Members));
        assert_eq!(ClusterSharePolicy::parse("OPT-IN"), Some(ClusterSharePolicy::OptIn));
        assert_eq!(ClusterSharePolicy::parse("disabled"), Some(ClusterSharePolicy::Disabled));
        assert_eq!(ClusterSharePolicy::parse("bogus"), None);
    }

    #[test]
    fn scrub_redacts_paths_emails_tokens() {
        let d = Descriptor {
            app: "Terminal".to_string(),
            topic: "deploy".to_string(),
            concept: "ran /Users/kunj/script.sh, emailed dev@team.io, key mock_secret_token_value_of_length_24".to_string(),
            error_type: None,
        };
        let s = scrub(&d);
        assert!(s.concept.contains('~'));
        assert!(s.concept.contains("<email>"));
        assert!(s.concept.contains("<token>"));
        assert!(!s.concept.contains("kunj"));
        assert!(!s.concept.contains("dev@team.io"));
    }
}
