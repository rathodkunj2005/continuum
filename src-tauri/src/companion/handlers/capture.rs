//! POST /v1/capture/control — pause / resume / incognito.
//!
//! Wraps `AppState::pause` / `resume` and the `is_incognito` atomic. Incognito
//! is a timed mode in spirit; the Mac UI is expected to drive the timer (we
//! just set the flag here).

use crate::companion::dto::{CaptureAction, CaptureControlRequest, CaptureControlResponse};
use crate::companion::errors::{CompanionError, CompanionResult};
use crate::AppState;
use axum::extract::State;
use axum::Json;
use chrono::TimeZone;
use std::sync::atomic::Ordering;
use std::sync::Arc;

pub async fn control(
    State(app_state): State<Arc<AppState>>,
    body: Result<Json<CaptureControlRequest>, axum::extract::rejection::JsonRejection>,
) -> CompanionResult<Json<CaptureControlResponse>> {
    let Json(request) = body.map_err(|err| CompanionError::BadRequest(err.to_string()))?;

    let now_ms = chrono::Utc::now().timestamp_millis();
    let until_ms = request
        .duration_minutes
        .map(|m| now_ms + (m as i64) * 60_000);

    let reason = request
        .reason
        .as_deref()
        .unwrap_or("mobile_companion_request");

    match request.action {
        CaptureAction::Pause => {
            app_state.pause();
            tracing::info!(reason, "Mobile companion paused capture");
        }
        CaptureAction::Resume => {
            app_state.resume();
            app_state.is_incognito.store(false, Ordering::SeqCst);
            tracing::info!(reason, "Mobile companion resumed capture");
        }
        CaptureAction::Incognito => {
            app_state.is_incognito.store(true, Ordering::SeqCst);
            tracing::info!(reason, until_ms, "Mobile companion entered incognito");
        }
    }

    let is_paused = app_state.is_paused.load(Ordering::SeqCst);
    let is_incognito = app_state.is_incognito.load(Ordering::SeqCst);
    let capture_status = if is_incognito {
        "incognito"
    } else if is_paused {
        "paused"
    } else {
        "running"
    };

    let until = until_ms.and_then(|ms| {
        chrono::Utc
            .timestamp_millis_opt(ms)
            .single()
            .map(|dt| dt.to_rfc3339())
    });

    Ok(Json(CaptureControlResponse {
        capture_status: capture_status.to_string(),
        is_paused,
        is_incognito,
        until,
    }))
}
