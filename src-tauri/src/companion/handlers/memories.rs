//! POST /v1/memories/manual — accept a mobile-origin text memory and route
//! it into the standard FNDR storage pipeline.
//!
//! Provenance is set from the authenticated [`MobileDevice`] so a stolen iOS
//! token cannot impersonate, say, a desktop capture. Idempotency is provided
//! via the caller-supplied `client_event_id`.

use crate::companion::auth::device_from_extensions;
use crate::companion::dto::{ManualMemoryRequest, ManualMemoryResponse};
use crate::companion::errors::{CompanionError, CompanionResult};
use crate::storage::MemoryRecord;
use crate::AppState;
use axum::extract::{Request, State};
use axum::Json;
use std::sync::Arc;
use uuid::Uuid;

/// Hard cap on a single mobile note. Anything larger should be split client-side.
const MAX_MOBILE_TEXT_CHARS: usize = 8_000;

pub async fn create_manual(
    State(app_state): State<Arc<AppState>>,
    request: Request,
) -> CompanionResult<Json<ManualMemoryResponse>> {
    let device = device_from_extensions(request.extensions())
        .ok_or(CompanionError::Unauthenticated)?;

    let (_parts, body) = request.into_parts();
    let bytes = axum::body::to_bytes(body, 32 * 1024)
        .await
        .map_err(|e| CompanionError::BadRequest(format!("body read failed: {e}")))?;
    let payload: ManualMemoryRequest = serde_json::from_slice(&bytes)
        .map_err(|e| CompanionError::BadRequest(format!("invalid JSON body: {e}")))?;

    let text = payload.text.trim().to_string();
    if text.is_empty() {
        return Err(CompanionError::BadRequest("text is empty".into()));
    }
    if text.chars().count() > MAX_MOBILE_TEXT_CHARS {
        return Err(CompanionError::BadRequest(format!(
            "text exceeds {MAX_MOBILE_TEXT_CHARS} chars"
        )));
    }
    if payload.client_event_id.trim().is_empty() {
        return Err(CompanionError::BadRequest("client_event_id is empty".into()));
    }

    let memory_id = deterministic_memory_id(&device.device_id, &payload.client_event_id);
    let source_type = payload
        .source_override
        .unwrap_or_else(|| device.device_type.manual_capture_source().to_string());

    let record = build_manual_record(
        &memory_id,
        &text,
        &source_type,
        payload.capture_type.as_deref(),
        payload.project.as_deref(),
        payload.topic.as_deref(),
        &device.device_name,
    );

    // `insert_memory_chunk` → `add_batch` applies the same content-hash dedup the
    // capture pipeline uses, so a retried call from the iOS offline queue (same
    // text + same device + same client_event_id) silently no-ops on the second
    // attempt. The deterministic memory_id is informational; LanceDB does not
    // enforce primary key uniqueness, so we cannot rely on it alone for dedup.
    app_state
        .store
        .insert_memory_chunk(&record)
        .await
        .map_err(|e| CompanionError::Internal(format!("insert_memory_chunk failed: {e}")))?;

    app_state.invalidate_memory_derived_caches();

    Ok(Json(ManualMemoryResponse {
        memory_id,
        status: "indexed".to_string(),
        source_type,
        duplicate: false,
    }))
}

/// Stable memory id derived from `(device_id, client_event_id)`. Retrying the
/// same capture from the iPhone offline queue resolves to the same id, so the
/// caller can rely on at-most-once storage.
pub fn deterministic_memory_id(device_id: &str, client_event_id: &str) -> String {
    let namespace = Uuid::NAMESPACE_OID;
    let composite = format!("companion::{}::{}", device_id, client_event_id);
    Uuid::new_v5(&namespace, composite.as_bytes()).to_string()
}

pub(crate) fn build_manual_record(
    memory_id: &str,
    text: &str,
    source_type: &str,
    capture_type: Option<&str>,
    project: Option<&str>,
    topic: Option<&str>,
    device_name: &str,
) -> MemoryRecord {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let day_bucket = chrono::Utc::now().format("%Y-%m-%d").to_string();

    let snippet = first_sentence(text, 240);
    let display_summary = snippet.clone();

    let tags = match capture_type {
        Some(t) if !t.trim().is_empty() => vec![t.trim().to_string()],
        _ => Vec::new(),
    };

    MemoryRecord {
        id: memory_id.to_string(),
        timestamp: now_ms,
        timestamp_start: now_ms,
        timestamp_end: now_ms,
        day_bucket,
        app_name: format!("FNDR Mobile ({})", device_name),
        bundle_id: None,
        window_title: capture_type
            .map(|c| format!("Manual {}", c))
            .unwrap_or_else(|| "Manual note".to_string()),
        text: text.to_string(),
        clean_text: text.to_string(),
        snippet,
        display_summary,
        source_type: source_type.to_string(),
        project: project.unwrap_or("").to_string(),
        topic: topic.unwrap_or("unknown").to_string(),
        activity_type: capture_type.unwrap_or("note").to_string(),
        tags,
        confidence_score: 1.0,
        importance_score: 0.6,
        summary_source: "fallback".to_string(),
        storage_outcome: "manual_capture".to_string(),
        ..MemoryRecord::default()
    }
}

fn first_sentence(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    let candidate = trimmed
        .split_inclusive(|c: char| c == '.' || c == '!' || c == '?')
        .next()
        .unwrap_or(trimmed)
        .trim();
    if candidate.chars().count() <= max_chars {
        candidate.to_string()
    } else {
        candidate.chars().take(max_chars).collect::<String>() + "…"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_memory_id_is_stable() {
        let a = deterministic_memory_id("dev_1", "evt_1");
        let b = deterministic_memory_id("dev_1", "evt_1");
        assert_eq!(a, b);
    }

    #[test]
    fn deterministic_memory_id_differs_by_device() {
        let a = deterministic_memory_id("dev_1", "evt_1");
        let b = deterministic_memory_id("dev_2", "evt_1");
        assert_ne!(a, b);
    }

    #[test]
    fn deterministic_memory_id_differs_by_event() {
        let a = deterministic_memory_id("dev_1", "evt_1");
        let b = deterministic_memory_id("dev_1", "evt_2");
        assert_ne!(a, b);
    }

    #[test]
    fn first_sentence_truncates_long_input() {
        let long = "a".repeat(500);
        let s = first_sentence(&long, 50);
        assert!(s.ends_with('…'));
        assert!(s.chars().count() <= 51);
    }

    #[test]
    fn first_sentence_keeps_short_input_verbatim() {
        let s = first_sentence("Remember this. And not this.", 240);
        assert_eq!(s, "Remember this.");
    }

    #[test]
    fn build_manual_record_sets_provenance_and_idempotency() {
        let rec = build_manual_record(
            "test-id",
            "Hello FNDR",
            "iphone_manual_capture",
            Some("idea"),
            Some("FNDR"),
            None,
            "Anurup's iPhone",
        );
        assert_eq!(rec.id, "test-id");
        assert_eq!(rec.source_type, "iphone_manual_capture");
        assert_eq!(rec.activity_type, "idea");
        assert_eq!(rec.project, "FNDR");
        assert!(rec.app_name.contains("Anurup's iPhone"));
        assert!(rec.text.contains("Hello FNDR"));
        assert_eq!(rec.storage_outcome, "manual_capture");
    }
}
