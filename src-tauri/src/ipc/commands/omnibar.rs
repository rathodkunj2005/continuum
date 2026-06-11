//! Spotlight-style omnibar: global shortcut, frameless always-on-top window.

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

const OMNIBAR_LABEL: &str = "omnibar";
const OMNIBAR_WIDTH: f64 = 680.0;
const OMNIBAR_HEIGHT: f64 = 480.0;
pub const OMNIBAR_SHORTCUT: &str = "Alt+Space";

/// Pre-create the omnibar window at startup (hidden) so it is fully loaded
/// before the first hotkey press. Called once from main.rs setup.
pub fn create_omnibar_window<R: tauri::Runtime>(app: &AppHandle<R>) {
    let url = tauri::WebviewUrl::App("omnibar.html".into());
    match tauri::WebviewWindowBuilder::new(app, OMNIBAR_LABEL, url)
        .title("FNDR")
        .inner_size(OMNIBAR_WIDTH, OMNIBAR_HEIGHT)
        .center()
        .decorations(false)
        .always_on_top(true)
        .resizable(false)
        .skip_taskbar(true)
        .shadow(true)
        .visible(false)
        .build()
    {
        Ok(_) => tracing::info!("omnibar window pre-created (hidden)"),
        Err(err) => tracing::warn!("failed to pre-create omnibar window: {err}"),
    }
}

/// Register the omnibar global shortcut. Must be re-invoked after any
/// `global_shortcut().unregister_all()` (see `register_autofill_shortcut`).
pub fn register_omnibar_shortcut<R: tauri::Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let shortcut: Shortcut = OMNIBAR_SHORTCUT
        .parse()
        .map_err(|err| format!("Invalid omnibar shortcut '{OMNIBAR_SHORTCUT}': {err}"))?;

    let handle = app.clone();
    app.global_shortcut()
        .on_shortcut(shortcut, move |_app, _shortcut, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }
            toggle_omnibar(&handle);
        })
        .map_err(|err| err.to_string())
}

fn toggle_omnibar<R: tauri::Runtime>(app: &AppHandle<R>) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Some(window) = handle.get_webview_window(OMNIBAR_LABEL) else {
            tracing::warn!("omnibar: window not found at hotkey time");
            return;
        };
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
        } else {
            let _ = window.center();
            let _ = window.show();
            let _ = window.set_focus();
            let _ = handle.emit_to(OMNIBAR_LABEL, "omnibar://focus", ());
        }
    });
}

/// Hide the omnibar window (Esc / blur from the frontend).
#[tauri::command]
pub async fn dismiss_omnibar(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(OMNIBAR_LABEL) {
        window.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Open a memory from the omnibar: hide the omnibar, focus the main window,
/// and tell it to open the vault on the given memory.
#[tauri::command]
pub async fn omnibar_open_memory(app: AppHandle, memory_id: String) -> Result<(), String> {
    if let Some(omnibar) = app.get_webview_window(OMNIBAR_LABEL) {
        let _ = omnibar.hide();
    }
    let main = app
        .get_webview_window("main")
        .ok_or_else(|| "main window not found".to_string())?;
    main.show().map_err(|e| e.to_string())?;
    main.set_focus().map_err(|e| e.to_string())?;
    app.emit_to("main", "omnibar://open-memory", memory_id)
        .map_err(|e| e.to_string())?;
    Ok(())
}
