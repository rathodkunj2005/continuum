//! Blocklist, wipe-all-data, proactive privacy alerts.

use super::common::push_unique_case_insensitive;
use crate::privacy::Blocklist;
use crate::AppState;
use std::sync::Arc;
use tauri::State;

/// Get blocklist
#[tauri::command]
pub async fn get_blocklist(state: State<'_, Arc<AppState>>) -> Result<Vec<String>, String> {
    let config = state.inner().config.read();
    Ok(config.blocklist.clone())
}

/// Set blocklist
#[tauri::command]
pub async fn set_blocklist(
    state: State<'_, Arc<AppState>>,
    apps: Vec<String>,
) -> Result<(), String> {
    let mut config = state.inner().config.write();
    config.blocklist = apps;
    config.blocklist = config
        .blocklist
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .fold(Vec::new(), |mut acc, value| {
            if !acc
                .iter()
                .any(|existing: &String| existing.eq_ignore_ascii_case(&value))
            {
                acc.push(value);
            }
            acc
        });
    config
        .save()
        .map_err(|e: Box<dyn std::error::Error>| e.to_string())?;
    Ok(())
}

/// Delete all data
#[tauri::command]
pub async fn delete_all_data(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    // 1. Clear memory records
    state
        .inner()
        .store
        .delete_all()
        .await
        .map_err(|e: Box<dyn std::error::Error>| e.to_string())?;
    state.invalidate_memory_derived_caches();

    // 2. Clear knowledge graph
    if let Err(e) = state.inner().graph.clear_all().await {
        tracing::warn!("Failed to clear graph store during delete_all: {}", e);
    }

    // 3. Delete persisted capture artifacts
    for artifact_dir in ["frames", "screenshots", "meetings"] {
        let path = state.inner().store.data_dir().join(artifact_dir);
        if path.exists() {
            if let Err(e) = std::fs::remove_dir_all(&path) {
                tracing::warn!("Failed to remove {} dir: {}", artifact_dir, e);
            }
        }
    }

    tracing::info!("All FNDR data deleted");
    Ok(())
}

// ── Proactive Privacy Shield Commands ───────────────────────────────────────────

#[tauri::command]
pub async fn get_privacy_alerts(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<crate::PrivacyAlert>, String> {
    let pending = state.pending_privacy_alerts.read();
    Ok(pending.clone())
}

#[tauri::command]
pub async fn dismiss_privacy_alert(
    site: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let site_key = privacy_site_key(&site);
    {
        let mut pending = state.pending_privacy_alerts.write();
        pending.retain(|a| !privacy_site_matches(&a.domain_or_title, &site_key));
    }
    {
        let mut snoozed = state.snoozed_privacy_alerts.write();
        // Keep the in-memory cache aligned until the persisted dismissal is reloaded.
        let expire = chrono::Local::now().timestamp() + (30 * 24 * 60 * 60);
        snoozed.insert(site_key.clone(), expire);
    }
    {
        let mut config = state.config.write();
        push_unique_case_insensitive(&mut config.dismissed_privacy_alerts, site_key);
        config.save().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn add_to_blocklist(site: String, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let site_key = privacy_site_key(&site);

    // 1. Remove from pending alerts
    {
        let mut pending = state.pending_privacy_alerts.write();
        pending.retain(|a| !privacy_site_matches(&a.domain_or_title, &site_key));
    }

    // 2. Add to config blocklist
    {
        let mut config = state.config.write();
        if !config
            .blocklist
            .iter()
            .any(|b| b.eq_ignore_ascii_case(&site_key))
        {
            config.blocklist.push(site_key.clone());
        }
        config
            .dismissed_privacy_alerts
            .retain(|value| !privacy_site_matches(value, &site_key));
        config.save().map_err(|e| e.to_string())?;
    }

    // 3. Retroactively delete memories with this site if we grabbed it during the alert period
    if let Err(e) = state.store.delete_memories_by_domain(&site_key).await {
        tracing::error!(
            "Failed to retroactively delete memories for blocked site {}: {}",
            site_key,
            e
        );
    } else {
        state.invalidate_memory_derived_caches();
    }

    Ok(())
}

fn privacy_site_key(site: &str) -> String {
    Blocklist::context_key(Some(site), Some(site)).unwrap_or_else(|| site.trim().to_string())
}

fn privacy_site_matches(value: &str, site_key: &str) -> bool {
    if value.eq_ignore_ascii_case(site_key) {
        return true;
    }
    let site_values = vec![site_key.to_string()];
    let value_values = vec![value.to_string()];
    Blocklist::is_context_blocked(Some(value), Some(value), &site_values)
        || Blocklist::is_context_blocked(Some(site_key), Some(site_key), &value_values)
}
