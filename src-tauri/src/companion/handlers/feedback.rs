//! POST /v1/feedback — lightweight mobile feedback ingestion.

use crate::companion::auth::device_from_extensions;
use crate::companion::dto::{FeedbackRequest, FeedbackResponse};
use crate::companion::errors::{CompanionError, CompanionResult};
use axum::extract::Request;
use axum::Json;

pub async fn submit_feedback(request: Request) -> CompanionResult<Json<FeedbackResponse>> {
    let device =
        device_from_extensions(request.extensions()).ok_or(CompanionError::Unauthenticated)?;

    let (_parts, body) = request.into_parts();
    let bytes = axum::body::to_bytes(body, 32 * 1024)
        .await
        .map_err(|e| CompanionError::BadRequest(format!("body read failed: {e}")))?;
    let payload: FeedbackRequest = serde_json::from_slice(&bytes)
        .map_err(|e| CompanionError::BadRequest(format!("invalid JSON body: {e}")))?;

    let event = payload.event.trim();
    if event.is_empty() {
        return Err(CompanionError::BadRequest("event is empty".to_string()));
    }
    let (has_query, note_chars) = redacted_feedback_meta(&payload);

    tracing::info!(
        device_id = %device.device_id,
        device_name = %device.device_name,
        event,
        has_query,
        memory_id = payload.memory_id.as_deref().unwrap_or(""),
        note_chars,
        "companion.feedback"
    );

    Ok(Json(FeedbackResponse {
        status: "ok".to_string(),
    }))
}

fn redacted_feedback_meta(payload: &FeedbackRequest) -> (bool, usize) {
    let has_query = payload
        .query
        .as_ref()
        .map(|q| !q.trim().is_empty())
        .unwrap_or(false);
    let note_chars = payload
        .note
        .as_ref()
        .map(|n| n.chars().count())
        .unwrap_or(0);
    (has_query, note_chars)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feedback_log_meta_exposes_only_redacted_shape() {
        let payload = FeedbackRequest {
            event: "thumbs_down".to_string(),
            query: Some("sensitive text".to_string()),
            memory_id: Some("mem_1".to_string()),
            note: Some("why this was weak".to_string()),
        };
        let (has_query, note_chars) = redacted_feedback_meta(&payload);
        assert!(has_query);
        assert_eq!(note_chars, "why this was weak".chars().count());
    }
}
