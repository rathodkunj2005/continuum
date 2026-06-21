//! Desktop client for the `cluster-create` / `cluster-join` Edge Functions.
//!
//! Lets a signed-in user spin up a workspace (becoming its admin) or join one
//! by code without leaving the desktop — the same flow as the web Workspaces
//! page. The new membership is picked up by the sync runtime on its next
//! refresh, after which capture begins syncing into that cluster.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::cloud::{CloudConfig, CloudSession};

const USER_AGENT: &str = "Continuum/1.0";

/// A workspace the user now belongs to. `join_code` is returned when creating
/// (so the admin can share it); joining returns it as `None`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterMembership {
    pub cluster_id: String,
    pub name: String,
    pub role: String,
    #[serde(default)]
    pub join_code: Option<String>,
}

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("HTTP client init failed: {e}"))
}

/// Extract the Edge Function's `{ "error": "..." }` message, falling back to a
/// generic HTTP status message.
fn parse_error(status: reqwest::StatusCode, body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
        .unwrap_or_else(|| format!("workspace request failed (HTTP {status})"))
}

async fn post_cluster_fn(
    cfg: &CloudConfig,
    session: &CloudSession,
    function: &str,
    body: serde_json::Value,
) -> Result<ClusterMembership, String> {
    let client = http_client()?;
    let res = client
        .post(cfg.function_url(function))
        .header("apikey", &cfg.anon_key)
        .bearer_auth(&session.access_token)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Couldn't reach the workspace service: {e}"))?;

    let status = res.status();
    let text = res.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(parse_error(status, &text));
    }
    serde_json::from_str::<ClusterMembership>(&text)
        .map_err(|e| format!("unexpected workspace response: {e}"))
}

/// Create a workspace; the caller becomes its admin and gets a join code back.
pub async fn create_cluster(
    cfg: &CloudConfig,
    session: &CloudSession,
    name: &str,
) -> Result<ClusterMembership, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Enter a workspace name.".to_string());
    }
    post_cluster_fn(cfg, session, "cluster-create", serde_json::json!({ "name": name })).await
}

/// Join a workspace by code; the caller becomes a member.
pub async fn join_cluster(
    cfg: &CloudConfig,
    session: &CloudSession,
    join_code: &str,
) -> Result<ClusterMembership, String> {
    let code = join_code.trim();
    if code.is_empty() {
        return Err("Enter a join code.".to_string());
    }
    post_cluster_fn(cfg, session, "cluster-join", serde_json::json!({ "join_code": code })).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_create_response_with_join_code() {
        let json = r#"{"cluster_id":"c1","name":"Acme","join_code":"ABCD2345","role":"admin"}"#;
        let m: ClusterMembership = serde_json::from_str(json).unwrap();
        assert_eq!(m.cluster_id, "c1");
        assert_eq!(m.role, "admin");
        assert_eq!(m.join_code.as_deref(), Some("ABCD2345"));
    }

    #[test]
    fn parses_join_response_without_join_code() {
        let json = r#"{"cluster_id":"c2","name":"Beta","role":"member"}"#;
        let m: ClusterMembership = serde_json::from_str(json).unwrap();
        assert_eq!(m.cluster_id, "c2");
        assert_eq!(m.role, "member");
        assert!(m.join_code.is_none());
    }

    #[test]
    fn parse_error_prefers_function_message() {
        let msg = parse_error(reqwest::StatusCode::NOT_FOUND, r#"{"error":"invalid join code"}"#);
        assert_eq!(msg, "invalid join code");
    }

    #[test]
    fn parse_error_falls_back_to_status() {
        let msg = parse_error(reqwest::StatusCode::INTERNAL_SERVER_ERROR, "boom");
        assert!(msg.contains("500"));
    }
}
