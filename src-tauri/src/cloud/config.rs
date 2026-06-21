//! Cloud (Supabase) configuration, read from the environment.
//!
//! Continuum stays local-first: when these variables are unset the cloud layer
//! reports `configured = false` and the app runs fully offline. They are loaded
//! from `.env` at startup via `dotenvy` (see `main.rs`).

use std::env;

/// Resolved Supabase endpoints for the desktop cloud-sync client.
#[derive(Debug, Clone)]
pub struct CloudConfig {
    /// Base project URL, e.g. `https://abc.supabase.co` (no trailing slash).
    pub supabase_url: String,
    /// Public anon key — safe to ship; RLS + user JWT scope all access.
    pub anon_key: String,
    /// Edge Functions base, e.g. `https://abc.supabase.co/functions/v1`.
    pub functions_url: String,
    /// Shared secret for the `agent-sync` Edge Function (`x-continuum-secret`).
    /// Required to push observations into the team graph; `None` disables the
    /// outbound graph sync while leaving sign-in and querying functional.
    pub agent_sync_secret: Option<String>,
}

impl CloudConfig {
    /// Build from `SUPABASE_URL` + `SUPABASE_ANON_KEY` (required) and an
    /// optional `SUPABASE_FUNCTIONS_URL`. Returns `None` when the backend is
    /// not configured, which keeps local-only development unblocked.
    pub fn from_env() -> Option<Self> {
        let supabase_url = non_empty(env::var("SUPABASE_URL").ok())?
            .trim_end_matches('/')
            .to_string();
        let anon_key = non_empty(env::var("SUPABASE_ANON_KEY").ok())?;
        let functions_url = non_empty(env::var("SUPABASE_FUNCTIONS_URL").ok())
            .map(|s| s.trim_end_matches('/').to_string())
            .unwrap_or_else(|| format!("{supabase_url}/functions/v1"));
        let agent_sync_secret = non_empty(env::var("AGENT_SYNC_SECRET").ok());
        Some(Self {
            supabase_url,
            anon_key,
            functions_url,
            agent_sync_secret,
        })
    }

    /// GoTrue auth endpoint: `{supabase_url}/auth/v1/{path}`.
    pub fn auth_url(&self, path: &str) -> String {
        format!("{}/auth/v1/{}", self.supabase_url, path.trim_start_matches('/'))
    }

    /// PostgREST endpoint: `{supabase_url}/rest/v1/{path}`.
    pub fn rest_url(&self, path: &str) -> String {
        format!("{}/rest/v1/{}", self.supabase_url, path.trim_start_matches('/'))
    }

    /// Edge Function endpoint: `{functions_url}/{name}`.
    pub fn function_url(&self, name: &str) -> String {
        format!("{}/{}", self.functions_url, name.trim_start_matches('/'))
    }
}

fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_urls() {
        let cfg = CloudConfig {
            supabase_url: "https://abc.supabase.co".to_string(),
            anon_key: "anon".to_string(),
            functions_url: "https://abc.supabase.co/functions/v1".to_string(),
            agent_sync_secret: None,
        };
        assert_eq!(cfg.auth_url("otp"), "https://abc.supabase.co/auth/v1/otp");
        assert_eq!(
            cfg.rest_url("profiles?select=id"),
            "https://abc.supabase.co/rest/v1/profiles?select=id"
        );
        assert_eq!(
            cfg.function_url("agent-sync"),
            "https://abc.supabase.co/functions/v1/agent-sync"
        );
    }

    #[test]
    fn non_empty_filters_blanks() {
        assert_eq!(non_empty(Some("  ".to_string())), None);
        assert_eq!(non_empty(Some(" x ".to_string())), Some("x".to_string()));
        assert_eq!(non_empty(None), None);
    }
}
