//! GET /v1/status — what mobile shows in the Status tab.
//!
//! Wraps existing app-state introspection (`AppState::is_capturing`,
//! is_paused, is_incognito) and adds a couple of mobile-friendly summary
//! fields. Does not include per-reason capture-pipeline counters — those are
//! a desktop-only diagnostic surface.

use crate::companion::dto::StatusResponse;
use crate::companion::errors::CompanionResult;
use crate::AppState;
use axum::extract::State;
use axum::Json;
use std::sync::atomic::Ordering;
use std::sync::Arc;

pub async fn get_status(
    State(app_state): State<Arc<AppState>>,
) -> CompanionResult<Json<StatusResponse>> {
    let capture_status = if app_state.is_incognito.load(Ordering::SeqCst) {
        "incognito"
    } else if app_state.is_paused.load(Ordering::SeqCst) {
        "paused"
    } else {
        "running"
    };

    let runtime_status = if app_state.ai_model_loaded() {
        "available"
    } else if app_state.ai_model_available() {
        "loading"
    } else {
        "unavailable"
    };

    let last_capture_ms = app_state.last_capture_time.load(Ordering::Relaxed);
    let last_memory_at_ms = if last_capture_ms == 0 {
        None
    } else {
        Some(last_capture_ms as i64)
    };

    let mac_name = mac_display_name();
    let app_version = env!("CARGO_PKG_VERSION").to_string();

    Ok(Json(StatusResponse {
        capture_status: capture_status.to_string(),
        runtime_status: runtime_status.to_string(),
        last_memory_at_ms,
        storage_status: "healthy".to_string(),
        model_status: runtime_status.to_string(),
        active_project: None,
        mac_name,
        app_version,
    }))
}

pub fn mac_display_name() -> String {
    std::env::var("CONTINUUM_MAC_NAME")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(hostname_lookup)
        .unwrap_or_else(|| "Continuum Mac".to_string())
}

fn hostname_lookup() -> Option<String> {
    // Prefer macOS scutil hostname when available — it's the user-visible
    // sharing name. Fall back to the unix hostname. Both are best-effort;
    // if they fail we return None and the caller substitutes a default.
    if cfg!(target_os = "macos") {
        if let Ok(output) = std::process::Command::new("scutil")
            .args(["--get", "ComputerName"])
            .output()
        {
            if output.status.success() {
                let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !name.is_empty() {
                    return Some(name);
                }
            }
        }
    }

    if let Ok(output) = std::process::Command::new("hostname").output() {
        if output.status.success() {
            let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}
