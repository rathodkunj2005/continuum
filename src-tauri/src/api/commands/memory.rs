//! Single-memory Tauri commands.

use crate::AppState;
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub async fn delete_memory(
    state: State<'_, Arc<AppState>>,
    memory_id: String,
) -> Result<bool, String> {
    let existing = state
        .inner()
        .store
        .get_memory_by_id(&memory_id)
        .await
        .map_err(|e: Box<dyn std::error::Error>| e.to_string())?;

    let deleted = state
        .inner()
        .store
        .delete_memory_by_id(&memory_id)
        .await
        .map_err(|e: Box<dyn std::error::Error>| e.to_string())?;

    if deleted == 0 {
        return Ok(false);
    }

    if let Err(err) =
        super::todos::apply_memory_deletion_to_tasks(&state.inner().store, &memory_id).await
    {
        tracing::warn!(
            "Task cleanup after deleting memory {} failed: {}",
            memory_id,
            err
        );
    }

    state.invalidate_memory_derived_caches();

    if let Some(record) = existing {
        if let Some(path) = record.screenshot_path {
            if let Err(err) = std::fs::remove_file(&path) {
                tracing::warn!("Failed to delete screenshot artifact {}: {}", path, err);
            }
        }
    }

    tracing::info!("Deleted memory record {}", memory_id);
    Ok(true)
}
