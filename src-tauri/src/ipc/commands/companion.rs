//! Tauri commands that expose the Companion API to the desktop React UI:
//! pairing, device listing, revocation, and server status.

use crate::companion::dto::{
    CompanionEndpoint, DeviceListEntry, PairStartResponse,
};
use crate::AppState;
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

#[derive(Debug, Clone, Serialize)]
pub struct CompanionStatusPayload {
    pub running: bool,
    pub host: String,
    pub port: u16,
    pub tls: bool,
    pub base_url: String,
    pub mac_name: String,
    pub last_error: Option<String>,
}

#[tauri::command]
pub async fn companion_get_status() -> Result<CompanionStatusPayload, String> {
    let s = crate::companion::status();
    Ok(CompanionStatusPayload {
        running: s.running,
        host: s.host,
        port: s.port,
        tls: s.tls,
        base_url: s.base_url,
        mac_name: s.mac_name,
        last_error: s.last_error,
    })
}

#[tauri::command]
pub async fn companion_get_endpoint() -> Result<Option<CompanionEndpoint>, String> {
    Ok(crate::companion::endpoint())
}

#[tauri::command]
pub async fn companion_start_server(
    state: State<'_, Arc<AppState>>,
    port: Option<u16>,
) -> Result<CompanionStatusPayload, String> {
    let app_state = state.inner().clone();
    let s = crate::companion::start(app_state, None, port).await?;
    Ok(CompanionStatusPayload {
        running: s.running,
        host: s.host,
        port: s.port,
        tls: s.tls,
        base_url: s.base_url,
        mac_name: s.mac_name,
        last_error: s.last_error,
    })
}

#[tauri::command]
pub async fn companion_stop_server() -> Result<CompanionStatusPayload, String> {
    let s = crate::companion::stop().await;
    Ok(CompanionStatusPayload {
        running: s.running,
        host: s.host,
        port: s.port,
        tls: s.tls,
        base_url: s.base_url,
        mac_name: s.mac_name,
        last_error: s.last_error,
    })
}

#[tauri::command]
pub async fn companion_start_pairing() -> Result<PairStartResponse, String> {
    let svc = crate::companion::pairing_service()
        .ok_or_else(|| "Companion API is not running".to_string())?;
    let endpoint = crate::companion::endpoint()
        .ok_or_else(|| "Companion API endpoint unavailable".to_string())?;
    let hint = crate::companion::pairing::PairingEndpointHint {
        host: endpoint.host,
        port: endpoint.port,
        tls: endpoint.tls,
        mac_name: endpoint.mac_name,
        cert_fingerprint_sha256: endpoint.cert_fingerprint_sha256,
    };
    let now_ms = chrono::Utc::now().timestamp_millis();
    Ok(svc.start(now_ms, &hint))
}

#[tauri::command]
pub async fn companion_list_devices() -> Result<Vec<DeviceListEntry>, String> {
    let reg = crate::companion::device_registry()
        .ok_or_else(|| "Companion API is not running".to_string())?;
    Ok(reg
        .list()
        .iter()
        .map(DeviceListEntry::from)
        .collect())
}

#[tauri::command]
pub async fn companion_revoke_device(device_id: String) -> Result<bool, String> {
    let reg = crate::companion::device_registry()
        .ok_or_else(|| "Companion API is not running".to_string())?;
    let now_ms = chrono::Utc::now().timestamp_millis();
    reg.revoke(&device_id, now_ms)
        .map_err(|e| format!("{e:?}"))
}
