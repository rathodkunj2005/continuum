//! Typed errors for the Companion API.
//!
//! Each variant maps to a stable HTTP status + JSON error body so iOS / Watch
//! clients can react without parsing strings.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CompanionError {
    #[error("missing or malformed Authorization header")]
    Unauthenticated,

    #[error("device is revoked or unknown")]
    Forbidden,

    #[error("pairing code is invalid or expired")]
    PairingCodeInvalid,

    #[error("pairing code already consumed")]
    PairingCodeUsed,

    #[error("request body is invalid: {0}")]
    BadRequest(String),

    #[error("resource not found")]
    NotFound,

    #[error("internal error: {0}")]
    Internal(String),
}

impl CompanionError {
    fn status_and_code(&self) -> (StatusCode, &'static str) {
        match self {
            CompanionError::Unauthenticated => (StatusCode::UNAUTHORIZED, "unauthenticated"),
            CompanionError::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            CompanionError::PairingCodeInvalid => {
                (StatusCode::BAD_REQUEST, "pairing_code_invalid")
            }
            CompanionError::PairingCodeUsed => (StatusCode::CONFLICT, "pairing_code_used"),
            CompanionError::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            CompanionError::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            CompanionError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
        }
    }
}

impl IntoResponse for CompanionError {
    fn into_response(self) -> Response {
        let (status, code) = self.status_and_code();
        let body = Json(json!({
            "error": code,
            "message": self.to_string(),
        }));
        (status, body).into_response()
    }
}

pub type CompanionResult<T> = Result<T, CompanionError>;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    #[tokio::test]
    async fn unauthenticated_returns_401_with_stable_code() {
        let resp = CompanionError::Unauthenticated.into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let body = to_bytes(resp.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "unauthenticated");
    }

    #[tokio::test]
    async fn pairing_code_used_returns_409() {
        let resp = CompanionError::PairingCodeUsed.into_response();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn bad_request_passes_through_message() {
        let resp = CompanionError::BadRequest("text is empty".to_string()).into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let body = to_bytes(resp.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "bad_request");
        assert!(json["message"].as_str().unwrap().contains("text is empty"));
    }
}
