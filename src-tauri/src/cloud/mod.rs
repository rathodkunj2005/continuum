//! Cloud sync layer for the Continuum team knowledge graph.
//!
//! Continuum is local-first: capture, OCR, embeddings, and the insight graph
//! all stay on-device. This module adds an opt-in (per build config) bridge to
//! the Supabase backend so a privacy-filtered subset of observations can join a
//! shared team graph. This first slice covers authentication and identity;
//! descriptor derivation, the share-policy classifier, and the sync queue land
//! on top of it.
//!
//! - [`config`]       — Supabase endpoints from the environment.
//! - [`types`]        — serializable session / identity / status.
//! - [`auth`]         — GoTrue email-OTP auth + PostgREST identity resolution.
//! - [`session`]      — keychain-backed session persistence.
//! - [`descriptor`]   — `MemoryRecord` → `{app,topic,concept,error_type}`.
//! - [`share_policy`] — privacy classifier + cluster/manager sharing gate.
//! - [`dedup`]        — L1 in-memory recent-observation dedup.
//! - [`sync`]         — outbound queue + `agent-sync` push worker.
//! - [`query`]        — `query-synthesize` cluster Q&A client.

pub mod auth;
pub mod config;
pub mod dedup;
pub mod descriptor;
pub mod embed;
pub mod query;
pub mod session;
pub mod share_policy;
pub mod sync;
pub mod types;

pub use config::CloudConfig;
pub use types::{CloudIdentity, CloudSession, CloudStatus};

/// Return the current session, refreshing the access token if it is expired or
/// near expiry. Persists the refreshed session. Errors when not signed in or
/// the refresh fails (caller should treat that as "needs sign-in").
pub async fn ensure_fresh_session(cfg: &CloudConfig) -> Result<CloudSession, String> {
    let session = session::current().ok_or_else(|| "Not signed in".to_string())?;
    if !session.is_expired(60) {
        return Ok(session);
    }
    let refreshed = auth::refresh(cfg, &session.refresh_token).await?;
    session::store(&refreshed)?;
    Ok(refreshed)
}
