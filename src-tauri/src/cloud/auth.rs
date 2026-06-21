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

/// Sign in by exchanging a Supabase **magic link** (or its raw token) for a
/// session. The desktop's primary flow is the 6-digit OTP, but when a project's
/// email template sends a link instead of a code, the user can paste the whole
/// link. We replicate a browser click: GET the verify URL *without following*
/// the redirect and read the session out of the returned `Location` fragment.
pub async fn verify_magic_link(cfg: &CloudConfig, input: &str) -> Result<CloudSession, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("Paste the sign-in link from your email.".to_string());
    }
    // A bare token (no URL) → verify via the token_hash POST path.
    if !input.contains("://") {
        return verify_token_hash(cfg, input, "magiclink").await;
    }

    let http = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("HTTP client init failed: {e}"))?;

    let res = http
        .get(input)
        .header("apikey", &cfg.anon_key)
        .send()
        .await
        .map_err(|e| format!("Could not reach the sign-in service: {e}"))?;

    // Success answers with a redirect carrying the session (or an error) in the
    // URL fragment; some setups answer 200 with JSON instead.
    let location = res
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let Some(location) = location else {
        if res.status().is_success() {
            let body: TokenResponse = res
                .json()
                .await
                .map_err(|e| format!("Unexpected sign-in response: {e}"))?;
            return Ok(session_from_token(body));
        }
        return Err(gotrue_error(res).await);
    };

    let params = parse_fragment(&location);
    if let Some(err) = params.get("error_description").or_else(|| params.get("error")) {
        return Err(humanize_link_error(err));
    }
    let access_token = params.get("access_token").cloned().ok_or_else(|| {
        "The link didn't return a session — request a fresh sign-in email.".to_string()
    })?;
    let refresh_token = params.get("refresh_token").cloned().unwrap_or_default();
    let expires_at = params
        .get("expires_at")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);

    let (user_id, email) = fetch_user(cfg, &http, &access_token).await?;
    let now = chrono::Utc::now().timestamp();
    Ok(CloudSession {
        access_token,
        refresh_token,
        expires_at: if expires_at > 0 { expires_at } else { now + 3600 },
        user_id,
        email,
    })
}

/// Verify a token_hash (e.g. a bare token pasted without the URL).
async fn verify_token_hash(
    cfg: &CloudConfig,
    token_hash: &str,
    link_type: &str,
) -> Result<CloudSession, String> {
    let res = client()?
        .post(cfg.auth_url("verify"))
        .header("apikey", &cfg.anon_key)
        .json(&serde_json::json!({ "type": link_type, "token_hash": token_hash }))
        .send()
        .await
        .map_err(|e| format!("Could not verify the link: {e}"))?;
    if !res.status().is_success() {
        return Err(gotrue_error(res).await);
    }
    let body: TokenResponse = res
        .json()
        .await
        .map_err(|e| format!("Unexpected sign-in response: {e}"))?;
    Ok(session_from_token(body))
}

/// Read `{ id, email }` for the bearer of `access_token` via GoTrue `/user`.
async fn fetch_user(
    cfg: &CloudConfig,
    http: &reqwest::Client,
    access_token: &str,
) -> Result<(String, Option<String>), String> {
    let res = http
        .get(cfg.auth_url("user"))
        .header("apikey", &cfg.anon_key)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("Could not load your account: {e}"))?;
    if !res.status().is_success() {
        return Err(gotrue_error(res).await);
    }
    let user: GotrueUser = res
        .json()
        .await
        .map_err(|e| format!("Unexpected account response: {e}"))?;
    Ok((user.id, user.email))
}

/// Parse the `k=v&...` params out of a URL fragment (`...#access_token=...`).
fn parse_fragment(location: &str) -> std::collections::HashMap<String, String> {
    let frag = location.split_once('#').map(|(_, f)| f).unwrap_or("");
    frag.split('&')
        .filter_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            Some((k.to_string(), urldecode(v)))
        })
        .collect()
}

/// Minimal `application/x-www-form-urlencoded` decode (dependency-free).
fn urldecode(s: &str) -> String {
    let s = s.replace('+', " ");
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Supabase already returns human-readable error text; just clean it up.
fn humanize_link_error(err: &str) -> String {
    let e = err.trim();
    if e.is_empty() {
        "The sign-in link was rejected. Request a fresh one.".to_string()
    } else {
        e.to_string()
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

    // Optional pin: sync to a specific cluster (e.g. a personal workspace)
    // instead of the earliest-joined one. Falls back to the first membership
    // when unset/empty.
    let pinned = std::env::var("CONTINUUM_CLUSTER_ID")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let cluster_id = match pinned {
        Some(id) => Some(id),
        None => match http
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
        },
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
