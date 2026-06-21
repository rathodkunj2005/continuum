//! Tauri IPC handlers for cloud sign-in (Supabase email OTP) and identity.
//!
//! These back the onboarding "Account" step and the app-level auth gate. Auth
//! state lives in the OS keychain (see `crate::cloud::session`), not in the
//! webview, so the Rust sync pipeline owns the session.

use std::sync::Arc;

use tauri::State;

use crate::cloud::query::ClusterAnswer;
use crate::cloud::sync::CloudSyncStatus;
use crate::cloud::{self, CloudConfig, CloudIdentity, CloudStatus};
use crate::AppState;

fn config_or_err() -> Result<CloudConfig, String> {
    CloudConfig::from_env().ok_or_else(|| {
        "Cloud sync is not configured in this build (missing SUPABASE_URL / SUPABASE_ANON_KEY)."
            .to_string()
    })
}

/// Whether cloud is configured and whether a session is stored. Cheap; safe to
/// poll from the UI gate.
#[tauri::command]
pub async fn cloud_status() -> Result<CloudStatus, String> {
    let configured = CloudConfig::from_env().is_some();
    let session = cloud::session::current();
    Ok(CloudStatus {
        configured,
        signed_in: session.is_some(),
        email: session.as_ref().and_then(|s| s.email.clone()),
        user_id: session.as_ref().map(|s| s.user_id.clone()),
    })
}

/// Email a one-time sign-in code.
#[tauri::command]
pub async fn cloud_request_otp(email: String) -> Result<(), String> {
    let cfg = config_or_err()?;
    let email = email.trim().to_string();
    if email.is_empty() {
        return Err("Please enter your email.".to_string());
    }
    cloud::auth::request_otp(&cfg, &email).await
}

/// Verify the emailed code, persist the session, and return the new status.
#[tauri::command]
pub async fn cloud_verify_otp(email: String, code: String) -> Result<CloudStatus, String> {
    let cfg = config_or_err()?;
    let email = email.trim().to_string();
    let code = code.trim().to_string();
    if email.is_empty() || code.is_empty() {
        return Err("Enter both your email and the code.".to_string());
    }
    let session = cloud::auth::verify_otp(&cfg, &email, &code).await?;
    cloud::session::store(&session)?;
    Ok(CloudStatus {
        configured: true,
        signed_in: true,
        email: session.email.clone(),
        user_id: Some(session.user_id.clone()),
    })
}

/// Sign out: clear the persisted session.
#[tauri::command]
pub async fn cloud_sign_out() -> Result<(), String> {
    cloud::session::clear()
}

/// Resolve `{ cluster_id, letta_agent_id }` for the signed-in user (refreshing
/// the token if needed).
#[tauri::command]
pub async fn cloud_get_identity() -> Result<CloudIdentity, String> {
    let cfg = config_or_err()?;
    let session = cloud::ensure_fresh_session(&cfg).await?;
    cloud::auth::resolve_identity(&cfg, &session).await
}

/// Ask the user's cluster a question. Calls the same `query-synthesize` Edge
/// Function the web app uses, authenticated with the stored JWT, and returns a
/// grounded answer plus the teammate nodes it cited.
#[tauri::command]
pub async fn cloud_query_cluster(query: String) -> Result<ClusterAnswer, String> {
    let cfg = config_or_err()?;
    let session = cloud::ensure_fresh_session(&cfg).await?;
    let identity = cloud::auth::resolve_identity(&cfg, &session).await?;
    let cluster_id = identity
        .cluster_id
        .ok_or_else(|| "You haven't joined a cluster yet.".to_string())?;
    cloud::query::query_cluster(&cfg, &session, &cluster_id, &query).await
}

/// Snapshot of the outbound team-graph sync pipeline (queue depth, counters,
/// resolved cluster policy, last error). Cheap; safe to poll from the UI.
#[tauri::command]
pub async fn cloud_sync_status(state: State<'_, Arc<AppState>>) -> Result<CloudSyncStatus, String> {
    Ok(state.inner().cloud_sync.status())
}
