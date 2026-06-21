//! Serializable cloud types shared between the auth client, session store, and
//! the Tauri IPC surface.

use serde::{Deserialize, Serialize};

/// An authenticated Supabase session. Persisted (refresh token) in the OS
/// keychain; the access token is short-lived and refreshed on demand.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudSession {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds at which `access_token` expires.
    pub expires_at: i64,
    pub user_id: String,
    #[serde(default)]
    pub email: Option<String>,
}

impl CloudSession {
    /// True when the access token is expired or within `skew_secs` of expiry.
    pub fn is_expired(&self, skew_secs: i64) -> bool {
        let now = chrono::Utc::now().timestamp();
        self.expires_at - skew_secs <= now
    }
}

/// The identity the sync pipeline needs: who the user is, which cluster their
/// shared observations land in, and the Letta agent provisioned at onboarding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudIdentity {
    pub user_id: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub cluster_id: Option<String>,
    #[serde(default)]
    pub letta_agent_id: Option<String>,
}

/// Lightweight status surfaced to the UI / onboarding gate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudStatus {
    /// Backend configured (SUPABASE_URL + anon key present in this build).
    pub configured: bool,
    /// A valid session is stored.
    pub signed_in: bool,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
}
