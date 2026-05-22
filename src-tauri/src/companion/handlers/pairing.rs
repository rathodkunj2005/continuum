//! Pairing endpoints — the only routes mounted *without* the auth middleware.
//!
//! `POST /v1/pair/start` is reserved for the Mac-side React UI and is mounted
//! on a loopback-only sub-router. iOS only ever hits `POST /v1/pair/complete`
//! with the short code it scanned from the Mac's QR.

use crate::companion::dto::{PairCompleteRequest, PairCompleteResponse, PairStartResponse};
use crate::companion::errors::{CompanionError, CompanionResult};
use crate::companion::pairing::{PairingEndpointHint, PairingService};
use axum::extract::State;
use axum::Json;
use std::sync::Arc;

/// Shared state carried into the pairing routes.
pub struct PairingHttpState {
    pub service: Arc<PairingService>,
    pub endpoint_hint: PairingEndpointHint,
}

pub async fn start(
    State(state): State<Arc<PairingHttpState>>,
) -> CompanionResult<Json<PairStartResponse>> {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let resp = state.service.start(now_ms, &state.endpoint_hint);
    Ok(Json(resp))
}

pub async fn complete(
    State(state): State<Arc<PairingHttpState>>,
    body: Result<Json<PairCompleteRequest>, axum::extract::rejection::JsonRejection>,
) -> CompanionResult<Json<PairCompleteResponse>> {
    let Json(request) = body.map_err(|err| CompanionError::BadRequest(err.to_string()))?;
    let now_ms = chrono::Utc::now().timestamp_millis();
    let resp = state
        .service
        .complete(now_ms, &state.endpoint_hint.mac_name, request)?;
    Ok(Json(resp))
}
