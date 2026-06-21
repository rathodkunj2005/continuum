//! Session persistence. The refresh token (whole session JSON) lives in the OS
//! keychain via the `keyring` crate; an in-memory cache avoids a keychain read
//! on every status check. The short-lived access token is never written to
//! disk in plaintext.

use std::sync::OnceLock;

use parking_lot::RwLock;

use crate::cloud::types::CloudSession;

const KEYRING_SERVICE: &str = "com.continuum.app";
const KEYRING_ACCOUNT: &str = "supabase-session";

fn cache() -> &'static RwLock<Option<CloudSession>> {
    static CACHE: OnceLock<RwLock<Option<CloudSession>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(None))
}

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)
        .map_err(|e| format!("keychain unavailable: {e}"))
}

/// Load the persisted session from the keychain into the cache (called once at
/// startup). Returns the session if one exists.
pub fn load_persisted() -> Option<CloudSession> {
    let raw = entry().ok()?.get_password().ok()?;
    let session: CloudSession = serde_json::from_str(&raw).ok()?;
    *cache().write() = Some(session.clone());
    Some(session)
}

/// Persist a session to the keychain and update the cache.
pub fn store(session: &CloudSession) -> Result<(), String> {
    let raw = serde_json::to_string(session).map_err(|e| e.to_string())?;
    entry()?
        .set_password(&raw)
        .map_err(|e| format!("could not save session: {e}"))?;
    *cache().write() = Some(session.clone());
    Ok(())
}

/// Remove the persisted session (sign out).
pub fn clear() -> Result<(), String> {
    *cache().write() = None;
    match entry()?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("could not clear session: {e}")),
    }
}

/// Current session from cache, falling back to a keychain read.
pub fn current() -> Option<CloudSession> {
    if let Some(s) = cache().read().clone() {
        return Some(s);
    }
    load_persisted()
}
