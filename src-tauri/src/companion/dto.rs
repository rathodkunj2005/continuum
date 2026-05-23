//! Request and response DTOs for the Companion API.
//!
//! Shapes match `fndr_ios_watch_mvp_prd.md` §13. Field names are stable —
//! treat any change as a versioned API change (bump path prefix from /v1 to /v2).

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Pairing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairStartResponse {
    pub pairing_code: String,
    pub qr_payload: String,
    pub expires_at_ms: i64,
    pub host: String,
    pub port: u16,
    pub cert_fingerprint_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairCompleteRequest {
    pub pairing_code: String,
    pub device_name: String,
    pub device_type: DeviceType,
    /// Optional client-supplied public key (base64). Reserved for future
    /// per-request signing; we accept and store it but do not verify yet.
    #[serde(default)]
    pub public_key: Option<String>,
    /// App version string for diagnostics ("1.0.0 (42)").
    #[serde(default)]
    pub app_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairCompleteResponse {
    pub device_id: String,
    pub access_token: String,
    pub mac_name: String,
    pub permissions: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceType {
    Iphone,
    Watch,
    Other,
}

impl DeviceType {
    pub fn manual_capture_source(self) -> &'static str {
        match self {
            DeviceType::Iphone => "iphone_manual_capture",
            DeviceType::Watch => "watch_manual_capture",
            DeviceType::Other => "mobile_manual_capture",
        }
    }
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusResponse {
    pub capture_status: String,
    pub runtime_status: String,
    pub last_memory_at_ms: Option<i64>,
    pub storage_status: String,
    pub model_status: String,
    pub active_project: Option<String>,
    pub mac_name: String,
    pub app_version: String,
}

// ---------------------------------------------------------------------------
// Capture control
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureControlRequest {
    pub action: CaptureAction,
    /// Optional duration. For pause/incognito; ignored for resume.
    #[serde(default)]
    pub duration_minutes: Option<u32>,
    /// Diagnostic string written into the Mac log.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureAction {
    Pause,
    Resume,
    Incognito,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureControlResponse {
    pub capture_status: String,
    pub is_paused: bool,
    pub is_incognito: bool,
    /// ISO-8601 for human-readable display in the mobile UI.
    pub until: Option<String>,
}

// ---------------------------------------------------------------------------
// Manual memory capture
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualMemoryRequest {
    /// The free-form text the user typed/dictated.
    pub text: String,
    /// Idempotency key — Mac dedups any repeat with the same id.
    pub client_event_id: String,
    /// Optional category ("idea" | "todo" | "decision" | "note" | "link" | "question").
    #[serde(default)]
    pub capture_type: Option<String>,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub topic: Option<String>,
    /// Override the source string. Defaults are filled in from the authenticated device type.
    #[serde(default)]
    pub source_override: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualMemoryResponse {
    pub memory_id: String,
    pub status: String,
    pub source_type: String,
    pub duplicate: bool,
}

// ---------------------------------------------------------------------------
// Ask + search + memory detail
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskRequest {
    pub query: String,
    #[serde(default)]
    pub limit: Option<usize>,
    /// "short" | "detailed" | "context_pack" (currently advisory only).
    #[serde(default)]
    pub answer_style: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskResponse {
    pub query: String,
    pub answer: String,
    pub verify_outcome: String,
    pub source_cards: Vec<CompanionMemoryCard>,
    pub latency_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySearchRequest {
    pub query: String,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub time_filter: Option<String>,
    #[serde(default)]
    pub app_filter: Option<String>,
    #[serde(default)]
    pub project_filter: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySearchResponse {
    pub query: String,
    pub cards: Vec<CompanionMemoryCard>,
    pub total: usize,
    pub latency_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryDetailResponse {
    pub card: CompanionMemoryCard,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompanionMemoryCard {
    pub memory_id: String,
    pub title: String,
    pub summary: String,
    pub display_summary: String,
    pub internal_context: String,
    pub timestamp: i64,
    pub app_name: String,
    pub window_title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub score: f32,
    pub source_count: usize,
    pub confidence: f32,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub topic: String,
    #[serde(default)]
    pub activity_type: String,
    #[serde(default)]
    pub files_touched: Vec<String>,
    #[serde(default)]
    pub raw_snippets: Vec<String>,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
}

// ---------------------------------------------------------------------------
// Feedback
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackRequest {
    /// e.g. "thumbs_up" | "thumbs_down" | "opened_source" | "copied_answer"
    pub event: String,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub memory_id: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackResponse {
    pub status: String,
}

// ---------------------------------------------------------------------------
// Device registry (also persisted via StateStore)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MobileDevice {
    pub device_id: String,
    pub device_name: String,
    pub device_type: DeviceType,
    pub access_token: String,
    pub paired_at_ms: i64,
    pub last_seen_at_ms: i64,
    pub permissions: Vec<String>,
    /// When set, requests using this device's token return 403.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_at_ms: Option<i64>,
    /// Reserved for future per-request signing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_version: Option<String>,
}

impl MobileDevice {
    pub fn is_active(&self) -> bool {
        self.revoked_at_ms.is_none()
    }
}

/// Public projection of a paired device, returned to the React settings UI.
/// Excludes the access token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceListEntry {
    pub device_id: String,
    pub device_name: String,
    pub device_type: DeviceType,
    pub paired_at_ms: i64,
    pub last_seen_at_ms: i64,
    pub revoked_at_ms: Option<i64>,
    pub app_version: Option<String>,
}

impl From<&MobileDevice> for DeviceListEntry {
    fn from(value: &MobileDevice) -> Self {
        Self {
            device_id: value.device_id.clone(),
            device_name: value.device_name.clone(),
            device_type: value.device_type,
            paired_at_ms: value.paired_at_ms,
            last_seen_at_ms: value.last_seen_at_ms,
            revoked_at_ms: value.revoked_at_ms,
            app_version: value.app_version.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Endpoint metadata (also written to ~/.fndr/companion.json)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompanionEndpoint {
    pub host: String,
    pub port: u16,
    pub base_url: String,
    pub tls: bool,
    pub cert_fingerprint_sha256: Option<String>,
    pub mac_name: String,
    pub app_version: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_type_serializes_as_lowercase() {
        let body = serde_json::to_string(&DeviceType::Iphone).unwrap();
        assert_eq!(body, "\"iphone\"");
    }

    #[test]
    fn device_type_maps_to_provenance_source() {
        assert_eq!(
            DeviceType::Iphone.manual_capture_source(),
            "iphone_manual_capture"
        );
        assert_eq!(
            DeviceType::Watch.manual_capture_source(),
            "watch_manual_capture"
        );
    }

    #[test]
    fn capture_action_serializes_snake_case() {
        let body = serde_json::to_string(&CaptureAction::Incognito).unwrap();
        assert_eq!(body, "\"incognito\"");
    }

    #[test]
    fn device_list_entry_omits_token() {
        let device = MobileDevice {
            device_id: "dev_abc".to_string(),
            device_name: "iPhone".to_string(),
            device_type: DeviceType::Iphone,
            access_token: "SECRET".to_string(),
            paired_at_ms: 1,
            last_seen_at_ms: 2,
            permissions: vec!["ask".to_string()],
            revoked_at_ms: None,
            public_key: None,
            app_version: None,
        };
        let entry = DeviceListEntry::from(&device);
        let body = serde_json::to_string(&entry).unwrap();
        assert!(!body.contains("SECRET"));
        assert!(body.contains("dev_abc"));
    }

    #[test]
    fn manual_memory_round_trip() {
        let req = ManualMemoryRequest {
            text: "Remember this".to_string(),
            client_event_id: "evt_1".to_string(),
            capture_type: Some("idea".to_string()),
            project: Some("FNDR".to_string()),
            topic: None,
            source_override: None,
        };
        let body = serde_json::to_string(&req).unwrap();
        let parsed: ManualMemoryRequest = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed.text, "Remember this");
        assert_eq!(parsed.capture_type.as_deref(), Some("idea"));
    }

    #[test]
    fn ask_request_round_trip() {
        let req = AskRequest {
            query: "What did I work on today?".to_string(),
            limit: Some(8),
            answer_style: Some("short".to_string()),
        };
        let body = serde_json::to_string(&req).unwrap();
        let parsed: AskRequest = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed.query, req.query);
        assert_eq!(parsed.limit, Some(8));
        assert_eq!(parsed.answer_style.as_deref(), Some("short"));
    }

    #[test]
    fn memory_search_request_round_trip() {
        let req = MemorySearchRequest {
            query: "fn companion".to_string(),
            limit: Some(10),
            time_filter: Some("today".to_string()),
            app_filter: Some("Cursor".to_string()),
            project_filter: Some("FNDR".to_string()),
        };
        let body = serde_json::to_string(&req).unwrap();
        let parsed: MemorySearchRequest = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed.query, "fn companion");
        assert_eq!(parsed.project_filter.as_deref(), Some("FNDR"));
    }

    #[test]
    fn feedback_request_round_trip() {
        let req = FeedbackRequest {
            event: "thumbs_up".to_string(),
            query: Some("what was I doing".to_string()),
            memory_id: Some("mem_1".to_string()),
            note: None,
        };
        let body = serde_json::to_string(&req).unwrap();
        let parsed: FeedbackRequest = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed.event, "thumbs_up");
        assert_eq!(parsed.memory_id.as_deref(), Some("mem_1"));
    }
}
