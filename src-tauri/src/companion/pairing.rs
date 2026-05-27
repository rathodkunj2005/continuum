//! Pairing protocol: the Mac generates a short-lived code, the iPhone
//! completes the handshake, and a fresh access token is issued and stored
//! in the device registry.
//!
//! Codes are single-use, expire after `PAIRING_TTL`, and live only in memory
//! (no disk persistence — a pairing in flight does not survive a restart).

use crate::companion::device_registry::DeviceRegistry;
use crate::companion::dto::{
    DeviceType, MobileDevice, PairCompleteRequest, PairCompleteResponse, PairStartResponse,
};
use crate::companion::errors::{CompanionError, CompanionResult};
use parking_lot::Mutex;
use rand::distr::Alphanumeric;
use rand::RngExt;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;

/// Time-to-live for an outstanding pairing code.
pub const PAIRING_TTL_MS: i64 = 5 * 60_000;

const PAIRING_CODE_LEN: usize = 6;
const ACCESS_TOKEN_LEN: usize = 48;
const DEFAULT_PERMISSIONS: &[&str] = &["ask", "search", "manual_capture", "capture_control"];

/// Single outstanding pairing offer keyed by the short numeric code.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct PairingOffer {
    /// Duplicated from the map key for diagnostic logging.
    code: String,
    issued_at_ms: i64,
    expires_at_ms: i64,
}

/// What the QR code on the Mac encodes. iOS decodes this JSON and posts back
/// to `/v1/pair/complete`.
#[derive(Debug, Clone, Serialize)]
pub struct QrPayload {
    pub version: u32,
    pub mac_name: String,
    pub host: String,
    pub port: u16,
    pub tls: bool,
    pub cert_fingerprint_sha256: Option<String>,
    pub pairing_code: String,
    pub expires_at_ms: i64,
}

#[derive(Clone)]
pub struct PairingEndpointHint {
    pub host: String,
    pub port: u16,
    pub tls: bool,
    pub mac_name: String,
    pub cert_fingerprint_sha256: Option<String>,
}

pub struct PairingService {
    /// Outstanding pairing offers, keyed by code. Bounded by `PAIRING_TTL_MS`
    /// (expired entries are pruned on every lookup).
    offers: Mutex<HashMap<String, PairingOffer>>,
    registry: Arc<DeviceRegistry>,
}

impl PairingService {
    pub fn new(registry: Arc<DeviceRegistry>) -> Self {
        Self {
            offers: Mutex::new(HashMap::new()),
            registry,
        }
    }

    /// Issue a new pairing code + QR payload. Called by the Mac UI when the
    /// user hits "Pair a device".
    pub fn start(&self, now_ms: i64, hint: &PairingEndpointHint) -> PairStartResponse {
        let code = generate_pairing_code();
        let expires_at_ms = now_ms + PAIRING_TTL_MS;

        let offer = PairingOffer {
            code: code.clone(),
            issued_at_ms: now_ms,
            expires_at_ms,
        };

        {
            let mut offers = self.offers.lock();
            prune_expired(&mut offers, now_ms);
            offers.insert(code.clone(), offer);
        }

        let qr = QrPayload {
            version: 1,
            mac_name: hint.mac_name.clone(),
            host: hint.host.clone(),
            port: hint.port,
            tls: hint.tls,
            cert_fingerprint_sha256: hint.cert_fingerprint_sha256.clone(),
            pairing_code: code.clone(),
            expires_at_ms,
        };
        let qr_payload = serde_json::to_string(&qr).unwrap_or_default();

        PairStartResponse {
            pairing_code: code,
            qr_payload,
            expires_at_ms,
            host: hint.host.clone(),
            port: hint.port,
            cert_fingerprint_sha256: hint.cert_fingerprint_sha256.clone(),
        }
    }

    /// Consume the code, issue a fresh access token, and register the device.
    pub fn complete(
        &self,
        now_ms: i64,
        mac_name: &str,
        request: PairCompleteRequest,
    ) -> CompanionResult<PairCompleteResponse> {
        let code = request.pairing_code.trim().to_string();
        if code.is_empty() {
            return Err(CompanionError::PairingCodeInvalid);
        }
        let device_name = request.device_name.trim();
        if device_name.is_empty() {
            return Err(CompanionError::BadRequest("device_name is empty".into()));
        }

        // Pop the offer atomically so two simultaneous completes can't both win.
        let offer = {
            let mut offers = self.offers.lock();
            prune_expired(&mut offers, now_ms);
            offers.remove(&code)
        };
        let offer = offer.ok_or(CompanionError::PairingCodeInvalid)?;
        if offer.expires_at_ms <= now_ms {
            return Err(CompanionError::PairingCodeInvalid);
        }

        let device_id = format!("dev_{}_{}", device_type_short(request.device_type), short_id());
        let access_token = generate_access_token();

        let device = MobileDevice {
            device_id: device_id.clone(),
            device_name: device_name.to_string(),
            device_type: request.device_type,
            access_token: access_token.clone(),
            paired_at_ms: now_ms,
            last_seen_at_ms: now_ms,
            permissions: DEFAULT_PERMISSIONS.iter().map(|s| s.to_string()).collect(),
            revoked_at_ms: None,
            public_key: request.public_key,
            app_version: request.app_version,
        };
        self.registry.insert(device)?;

        Ok(PairCompleteResponse {
            device_id,
            access_token,
            mac_name: mac_name.to_string(),
            permissions: DEFAULT_PERMISSIONS.iter().map(|s| s.to_string()).collect(),
        })
    }

    #[cfg(test)]
    pub fn outstanding_count(&self) -> usize {
        self.offers.lock().len()
    }
}

fn prune_expired(offers: &mut HashMap<String, PairingOffer>, now_ms: i64) {
    offers.retain(|_, offer| offer.expires_at_ms > now_ms);
}

fn generate_pairing_code() -> String {
    let mut rng = rand::rng();
    (0..PAIRING_CODE_LEN)
        .map(|_| rng.random_range(0..10).to_string())
        .collect()
}

fn generate_access_token() -> String {
    rand::rng()
        .sample_iter(&Alphanumeric)
        .take(ACCESS_TOKEN_LEN)
        .map(char::from)
        .collect()
}

fn short_id() -> String {
    let mut rng = rand::rng();
    (0..8)
        .map(|_| {
            let n: u32 = rng.random_range(0..16);
            std::char::from_digit(n, 16).unwrap()
        })
        .collect()
}

fn device_type_short(t: DeviceType) -> &'static str {
    match t {
        DeviceType::Iphone => "iphone",
        DeviceType::Watch => "watch",
        DeviceType::Other => "mobile",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StateStore;
    use tempfile::TempDir;

    fn fresh_service() -> (TempDir, Arc<PairingService>) {
        let tmp = TempDir::new().unwrap();
        let store = Arc::new(StateStore::new(tmp.path()).unwrap());
        let registry = Arc::new(DeviceRegistry::new(store).unwrap());
        (tmp, Arc::new(PairingService::new(registry)))
    }

    fn hint() -> PairingEndpointHint {
        PairingEndpointHint {
            host: "127.0.0.1".to_string(),
            port: 47812,
            tls: true,
            mac_name: "Test Mac".to_string(),
            cert_fingerprint_sha256: Some("abc".to_string()),
        }
    }

    fn complete_request(code: &str) -> PairCompleteRequest {
        PairCompleteRequest {
            pairing_code: code.to_string(),
            device_name: "Anurup's iPhone".to_string(),
            device_type: DeviceType::Iphone,
            public_key: None,
            app_version: Some("0.1.0 (1)".to_string()),
        }
    }

    #[test]
    fn start_produces_six_digit_code_and_qr_payload() {
        let (_tmp, svc) = fresh_service();
        let resp = svc.start(1_700_000_000_000, &hint());
        assert_eq!(resp.pairing_code.len(), PAIRING_CODE_LEN);
        assert!(resp.pairing_code.chars().all(|c| c.is_ascii_digit()));
        let qr: serde_json::Value = serde_json::from_str(&resp.qr_payload).unwrap();
        assert_eq!(qr["pairing_code"], resp.pairing_code);
        assert_eq!(qr["host"], "127.0.0.1");
        assert_eq!(qr["tls"], true);
        assert_eq!(qr["version"], 1);
    }

    #[test]
    fn complete_with_valid_code_registers_device() {
        let (_tmp, svc) = fresh_service();
        let start = svc.start(1_700_000_000_000, &hint());
        let resp = svc
            .complete(
                1_700_000_001_000,
                "Test Mac",
                complete_request(&start.pairing_code),
            )
            .unwrap();
        assert!(resp.access_token.len() >= ACCESS_TOKEN_LEN);
        assert!(resp.device_id.starts_with("dev_iphone_"));
        assert_eq!(resp.permissions.len(), DEFAULT_PERMISSIONS.len());
        assert_eq!(svc.outstanding_count(), 0, "code must be single-use");
    }

    #[test]
    fn complete_rejects_unknown_code() {
        let (_tmp, svc) = fresh_service();
        let err = svc
            .complete(1, "Mac", complete_request("000000"))
            .unwrap_err();
        assert!(matches!(err, CompanionError::PairingCodeInvalid));
    }

    #[test]
    fn complete_rejects_expired_code() {
        let (_tmp, svc) = fresh_service();
        let start = svc.start(1_700_000_000_000, &hint());
        let after_ttl = 1_700_000_000_000 + PAIRING_TTL_MS + 1;
        let err = svc
            .complete(after_ttl, "Mac", complete_request(&start.pairing_code))
            .unwrap_err();
        assert!(matches!(err, CompanionError::PairingCodeInvalid));
    }

    #[test]
    fn complete_cannot_be_replayed() {
        let (_tmp, svc) = fresh_service();
        let start = svc.start(1_700_000_000_000, &hint());
        let first = svc
            .complete(
                1_700_000_001_000,
                "Test Mac",
                complete_request(&start.pairing_code),
            )
            .unwrap();
        assert!(!first.access_token.is_empty());

        let err = svc
            .complete(
                1_700_000_002_000,
                "Test Mac",
                complete_request(&start.pairing_code),
            )
            .unwrap_err();
        assert!(matches!(err, CompanionError::PairingCodeInvalid));
    }

    #[test]
    fn complete_rejects_empty_device_name() {
        let (_tmp, svc) = fresh_service();
        let start = svc.start(1_700_000_000_000, &hint());
        let mut req = complete_request(&start.pairing_code);
        req.device_name = "   ".to_string();
        let err = svc.complete(1_700_000_001_000, "Mac", req).unwrap_err();
        assert!(matches!(err, CompanionError::BadRequest(_)));
    }

    #[test]
    fn expired_codes_are_pruned_on_next_start() {
        let (_tmp, svc) = fresh_service();
        let _ = svc.start(1_700_000_000_000, &hint());
        let _ = svc.start(1_700_000_001_000, &hint());
        assert_eq!(svc.outstanding_count(), 2);
        // Time-travel past the TTL of BOTH offers (the second is
        // offset by 1s, so we add to that one) then issue a new code.
        let _ = svc.start(1_700_000_001_000 + PAIRING_TTL_MS + 1, &hint());
        // Only the newest survives.
        assert_eq!(svc.outstanding_count(), 1);
    }

    #[test]
    fn two_starts_produce_different_codes() {
        let (_tmp, svc) = fresh_service();
        let a = svc.start(1_700_000_000_000, &hint());
        let b = svc.start(1_700_000_000_001, &hint());
        // Probabilistically unique — 1e-6 collision per attempt is acceptable for a 6-digit code.
        assert_ne!(a.pairing_code, b.pairing_code);
    }
}
