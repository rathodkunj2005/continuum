//! Clipboard history — read indexed clips back out of the graph and
//! copy/paste them from the omnibar.

use crate::storage::NodeType;
use crate::AppState;
use serde::Serialize;
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Manager, State};

const HISTORY_DEFAULT_LIMIT: usize = 50;
const HISTORY_MAX_LIMIT: usize = 200;
/// Upper bound on nodes scanned per request; clips beyond this are stale.
const HISTORY_SCAN_LIMIT: usize = 500;

#[derive(Debug, Clone, Serialize)]
pub struct ClipboardEntry {
    pub id: String,
    pub text: String,
    pub app_name: Option<String>,
    pub window_title: Option<String>,
    pub timestamp: i64,
}

#[tauri::command]
pub async fn get_clipboard_history(
    state: State<'_, Arc<AppState>>,
    limit: Option<usize>,
    query: Option<String>,
) -> Result<Vec<ClipboardEntry>, String> {
    let limit = limit.unwrap_or(HISTORY_DEFAULT_LIMIT).min(HISTORY_MAX_LIMIT);
    let nodes = state
        .store
        .get_nodes_by_type(NodeType::Clipboard, HISTORY_SCAN_LIMIT)
        .await
        .map_err(|e| e.to_string())?;

    let needle = query.unwrap_or_default().trim().to_lowercase();
    let entries = nodes
        .into_iter()
        .filter_map(|node| {
            let text = node
                .metadata
                .get("full_text")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| node.label.clone());
            if text.trim().is_empty() {
                return None;
            }
            if !needle.is_empty() && !text.to_lowercase().contains(&needle) {
                return None;
            }
            let meta_str = |key: &str| {
                node.metadata
                    .get(key)
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
            };
            Some(ClipboardEntry {
                app_name: meta_str("app_name"),
                window_title: meta_str("window_title"),
                id: node.id,
                text,
                timestamp: node.created_at,
            })
        })
        .take(limit)
        .collect();
    Ok(entries)
}

/// Put a past clip back on the system clipboard.
#[tauri::command]
pub async fn copy_clipboard_entry(text: String) -> Result<(), String> {
    write_pasteboard(&text)?;
    crate::capture::clipboard::note_self_copy(&text);
    Ok(())
}

/// Paste a past clip into the frontmost app: hide the omnibar so focus
/// returns to the previous app, then put the clip on the clipboard and
/// synthesize Cmd+V.
#[tauri::command]
pub async fn paste_clipboard_entry(app: AppHandle, text: String) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("omnibar") {
        let _ = window.hide();
    }
    write_pasteboard(&text)?;
    crate::capture::clipboard::note_self_copy(&text);
    // Let macOS finish the focus handoff before the keystroke.
    tokio::time::sleep(Duration::from_millis(180)).await;

    let script = r#"tell application "System Events" to keystroke "v" using command down"#;
    let status = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .status()
        .map_err(|e| format!("osascript paste failed to start: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("osascript paste returned non-zero exit code".to_string())
    }
}

fn write_pasteboard(text: &str) -> Result<(), String> {
    let mut child = std::process::Command::new("pbcopy")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to start pbcopy: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(text.as_bytes())
            .map_err(|e| format!("Failed to write to pbcopy: {e}"))?;
    }
    child
        .wait()
        .map_err(|e| format!("pbcopy did not finish: {e}"))?;
    Ok(())
}
