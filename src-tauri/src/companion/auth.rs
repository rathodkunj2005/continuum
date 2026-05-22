//! Bearer-token authentication middleware for the Companion API.
//!
//! Resolves the token against [`DeviceRegistry`] on every request. The matched
//! [`MobileDevice`] is stored in request extensions so handlers can read which
//! device made the call (for provenance, audit, and `last_seen_at` updates).

use crate::companion::device_registry::DeviceRegistry;
use crate::companion::dto::MobileDevice;
use crate::companion::errors::CompanionError;
use axum::extract::{Request, State};
use axum::http::header;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::sync::Arc;

/// Shared state plumbed into every middleware + handler. Held in `Arc` to keep
/// the router cheap to clone.
pub struct CompanionAuthState {
    pub registry: Arc<DeviceRegistry>,
}

/// Extract a bearer token from an Authorization header.
///
/// Public + tested separately so it can be reused by the WebSocket path later
/// (Sec-WebSocket-Protocol carries the same value in some clients).
pub fn extract_bearer(header_value: Option<&str>) -> Option<String> {
    let raw = header_value?;
    let raw = raw.trim();
    let token = raw.strip_prefix("Bearer ").or_else(|| raw.strip_prefix("bearer "))?;
    let token = token.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

/// Axum middleware fn — runs before every authenticated route.
pub async fn require_device(
    State(state): State<Arc<CompanionAuthState>>,
    mut request: Request,
    next: Next,
) -> Response {
    let header_value = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    let Some(token) = extract_bearer(header_value) else {
        return CompanionError::Unauthenticated.into_response();
    };

    let now_ms = chrono::Utc::now().timestamp_millis();
    let Some(device) = state.registry.find_by_token(&token, now_ms) else {
        // Token may have been issued and then revoked. Either way, 403 — the
        // client should drop the token and re-pair, not retry.
        return CompanionError::Forbidden.into_response();
    };

    request.extensions_mut().insert(Arc::new(device));
    next.run(request).await
}

/// Pull the authenticated device out of request extensions inside a handler.
pub fn device_from_extensions(req_extensions: &axum::http::Extensions) -> Option<Arc<MobileDevice>> {
    req_extensions.get::<Arc<MobileDevice>>().cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::companion::device_registry::DeviceRegistry;
    use crate::companion::dto::{DeviceType, MobileDevice};
    use crate::storage::StateStore;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::routing::get;
    use axum::Router;
    use tempfile::TempDir;
    use tower::util::ServiceExt;

    #[test]
    fn extract_bearer_handles_canonical_form() {
        assert_eq!(
            extract_bearer(Some("Bearer abc.def.ghi")),
            Some("abc.def.ghi".to_string())
        );
    }

    #[test]
    fn extract_bearer_is_case_insensitive_on_scheme() {
        assert_eq!(extract_bearer(Some("bearer xyz")), Some("xyz".to_string()));
    }

    #[test]
    fn extract_bearer_rejects_missing_or_empty() {
        assert_eq!(extract_bearer(None), None);
        assert_eq!(extract_bearer(Some("")), None);
        assert_eq!(extract_bearer(Some("Bearer ")), None);
        assert_eq!(extract_bearer(Some("NotBearer abc")), None);
    }

    async fn handler_ok(req: Request<Body>) -> Response {
        let device = device_from_extensions(req.extensions()).expect("device in extensions");
        (
            StatusCode::OK,
            format!("device:{}", device.device_id),
        )
            .into_response()
    }

    fn build_router(reg: Arc<DeviceRegistry>) -> Router {
        let state = Arc::new(CompanionAuthState { registry: reg });
        Router::new()
            .route("/ping", get(handler_ok))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                require_device,
            ))
            .with_state(state)
    }

    fn sample_device(token: &str) -> MobileDevice {
        MobileDevice {
            device_id: "dev_iphone_test".to_string(),
            device_name: "iPhone".to_string(),
            device_type: DeviceType::Iphone,
            access_token: token.to_string(),
            paired_at_ms: 1,
            last_seen_at_ms: 1,
            permissions: vec!["ask".to_string()],
            revoked_at_ms: None,
            public_key: None,
            app_version: None,
        }
    }

    fn fresh_registry() -> (TempDir, Arc<DeviceRegistry>) {
        let tmp = TempDir::new().unwrap();
        let store = Arc::new(StateStore::new(tmp.path()).unwrap());
        let reg = Arc::new(DeviceRegistry::new(store).unwrap());
        (tmp, reg)
    }

    /// `StateStore::new` builds its own current-thread tokio runtime to do a
    /// one-shot LanceDB open. That panics when called from inside an outer
    /// tokio runtime (which `#[tokio::test]` provides). `spawn_blocking` moves
    /// the construction to the dedicated blocking pool where building a fresh
    /// runtime is allowed.
    async fn fresh_registry_async() -> (TempDir, Arc<DeviceRegistry>) {
        tokio::task::spawn_blocking(fresh_registry).await.unwrap()
    }

    #[tokio::test]
    async fn missing_header_returns_401() {
        let (_tmp, reg) = fresh_registry_async().await;
        reg.insert(sample_device("tok_a")).unwrap();
        let app = build_router(reg);
        let resp = app
            .oneshot(Request::builder().uri("/ping").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn valid_token_passes_through_with_device_in_extensions() {
        let (_tmp, reg) = fresh_registry_async().await;
        reg.insert(sample_device("tok_valid")).unwrap();
        let app = build_router(reg);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/ping")
                    .header(header::AUTHORIZATION, "Bearer tok_valid")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 256).await.unwrap();
        assert_eq!(&body[..], b"device:dev_iphone_test");
    }

    #[tokio::test]
    async fn revoked_token_returns_403() {
        let (_tmp, reg) = fresh_registry_async().await;
        reg.insert(sample_device("tok_revoked")).unwrap();
        reg.revoke("dev_iphone_test", 100).unwrap();
        let app = build_router(reg);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/ping")
                    .header(header::AUTHORIZATION, "Bearer tok_revoked")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn unknown_token_returns_403() {
        let (_tmp, reg) = fresh_registry_async().await;
        let app = build_router(reg);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/ping")
                    .header(header::AUTHORIZATION, "Bearer wrong")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
