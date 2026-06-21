//! Supabase GoTrue auth over REST (email OTP) plus identity resolution via
//! PostgREST. Kept dependency-light (reqwest + serde) so the desktop owns its
//! own session and token storage instead of embedding a JS client.
//!
//! Flow: `request_otp` emails a 6-digit code; `verify_otp` exchanges the code
//! for a session; `refresh` renews an expired access token. `resolve_identity`
//! reads the signed-in user's profile + first cluster (RLS-scoped by the JWT).

use std::time::Duration;

use serde::Deserialize;

use crate::cloud::config::CloudConfig;
use crate::cloud::types::{CloudIdentity, CloudSession};

const USER_AGENT: &str = "Continuum/1.0";

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("HTTP client init failed: {e}"))
}

/// Send a one-time login code to `email`. `create_user: true` matches the web
/// app's magic-link signup behavior (new emails are provisioned on first code).
pub async fn request_otp(cfg: &CloudConfig, email: &str) -> Result<(), String> {
    let res = client()?
        .post(cfg.auth_url("otp"))
        .header("apikey", &cfg.anon_key)
        .json(&serde_json::json!({ "email": email, "create_user": true }))
        .send()
        .await
        .map_err(|e| format!("Could not reach the sign-in service: {e}"))?;
    if res.status().is_success() {
        Ok(())
    } else {
        Err(gotrue_error(res).await)
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    #[serde(default)]
    expires_at: i64,
    #[serde(default)]
    expires_in: i64,
    user: GotrueUser,
}

#[derive(Deserialize)]
struct GotrueUser {
    id: String,
    #[serde(default)]
    email: Option<String>,
}

/// Exchange an emailed code for a session.
pub async fn verify_otp(cfg: &CloudConfig, email: &str, token: &str) -> Result<CloudSession, String> {
    let res = client()?
        .post(cfg.auth_url("verify"))
        .header("apikey", &cfg.anon_key)
        .json(&serde_json::json!({ "type": "email", "email": email, "token": token }))
        .send()
        .await
        .map_err(|e| format!("Could not verify the code: {e}"))?;
    if !res.status().is_success() {
        return Err(gotrue_error(res).await);
    }
    let body: TokenResponse = res
        .json()
        .await
        .map_err(|e| format!("Unexpected sign-in response: {e}"))?;
    Ok(session_from_token(body))
}

/// Renew an expired access token using the stored refresh token.
pub async fn refresh(cfg: &CloudConfig, refresh_token: &str) -> Result<CloudSession, String> {
    let res = client()?
        .post(format!("{}?grant_type=refresh_token", cfg.auth_url("token")))
        .header("apikey", &cfg.anon_key)
        .json(&serde_json::json!({ "refresh_token": refresh_token }))
        .send()
        .await
        .map_err(|e| format!("Could not refresh the session: {e}"))?;
    if !res.status().is_success() {
        return Err(gotrue_error(res).await);
    }
    let body: TokenResponse = res
        .json()
        .await
        .map_err(|e| format!("Unexpected refresh response: {e}"))?;
    Ok(session_from_token(body))
}

fn session_from_token(t: TokenResponse) -> CloudSession {
    let now = chrono::Utc::now().timestamp();
    let expires_at = if t.expires_at > 0 {
        t.expires_at
    } else {
        now + t.expires_in.max(0)
    };
    CloudSession {
        access_token: t.access_token,
        refresh_token: t.refresh_token,
        expires_at,
        user_id: t.user.id,
        email: t.user.email,
    }
}

/// Resolve `{ cluster_id, letta_agent_id }` for the signed-in user. Best-effort:
/// missing rows (e.g. user not yet in a cluster) resolve to `None` rather than
/// erroring, so a freshly-signed-in user can still complete onboarding.
pub async fn resolve_identity(
    cfg: &CloudConfig,
    session: &CloudSession,
) -> Result<CloudIdentity, String> {
    let http = client()?;

    let letta_agent_id = match http
        .get(cfg.rest_url(&format!(
            "profiles?select=letta_agent_id&id=eq.{}",
            session.user_id
        )))
        .header("apikey", &cfg.anon_key)
        .bearer_auth(&session.access_token)
        .send()
        .await
    {
        Ok(r) => first_string_field(r, "letta_agent_id").await,
        Err(_) => None,
    };

    let cluster_id = match http
        .get(cfg.rest_url(&format!(
            "cluster_members?select=cluster_id&user_id=eq.{}&order=joined_at.asc&limit=1",
            session.user_id
        )))
        .header("apikey", &cfg.anon_key)
        .bearer_auth(&session.access_token)
        .send()
        .await
    {
        Ok(r) => first_string_field(r, "cluster_id").await,
        Err(_) => None,
    };

    Ok(CloudIdentity {
        user_id: session.user_id.clone(),
        email: session.email.clone(),
        cluster_id,
        letta_agent_id,
    })
}

/// Read `field` from the first row of a PostgREST JSON array response.
async fn first_string_field(res: reqwest::Response, field: &str) -> Option<String> {
    if !res.status().is_success() {
        return None;
    }
    let rows: Vec<serde_json::Value> = res.json().await.ok()?;
    rows.into_iter()
        .next()?
        .get(field)?
        .as_str()
        .map(|s| s.to_string())
}

/// Extract a human-readable message from a GoTrue error body.
async fn gotrue_error(res: reqwest::Response) -> String {
    let status = res.status();
    let text = res.text().await.unwrap_or_default();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
        let msg = v
            .get("error_description")
            .or_else(|| v.get("msg"))
            .or_else(|| v.get("error"))
            .and_then(|x| x.as_str());
        if let Some(m) = msg {
            return m.to_string();
        }
    }
    format!("Sign-in failed (HTTP {status})")
}
