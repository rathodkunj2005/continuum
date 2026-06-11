use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[derive(Default)]
pub enum ReopenKind {
    BrowserUrl,
    FilePath,
    AppBundle,
    AppDeepLink,
    #[default]
    Unknown,
}


impl ReopenKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::BrowserUrl => "browser_url",
            Self::FilePath => "file_path",
            Self::AppBundle => "app_bundle",
            Self::AppDeepLink => "app_deep_link",
            Self::Unknown => "unknown",
        }
    }

    pub fn from_label(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "browser_url" => Self::BrowserUrl,
            "file_path" => Self::FilePath,
            "app_bundle" => Self::AppBundle,
            "app_deep_link" => Self::AppDeepLink,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[derive(Default)]
pub enum ReopenValidationStatus {
    Valid,
    Invalid,
    #[default]
    Unchecked,
}


impl ReopenValidationStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::Invalid => "invalid",
            Self::Unchecked => "unchecked",
        }
    }

    pub fn from_label(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "valid" => Self::Valid,
            "invalid" => Self::Invalid,
            _ => Self::Unchecked,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReopenTarget {
    pub kind: ReopenKind,
    pub url: Option<String>,
    pub file_path: Option<String>,
    pub app_bundle_id: Option<String>,
    pub app_name: Option<String>,
    pub app_deep_link: Option<String>,
    pub captured_at_ms: i64,
    pub confidence: f32,
    pub validation_status: ReopenValidationStatus,
}

fn is_http_url(value: &str) -> bool {
    let lower = value.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

pub fn build_reopen_target(
    source_url: Option<&str>,
    first_file_path: Option<&str>,
    bundle_id: Option<&str>,
    app_name: &str,
    captured_at_ms: i64,
) -> ReopenTarget {
    if let Some(url) = source_url.map(str::trim).filter(|v| is_http_url(v)) {
        return ReopenTarget {
            kind: ReopenKind::BrowserUrl,
            url: Some(url.to_string()),
            captured_at_ms,
            confidence: 0.95,
            validation_status: ReopenValidationStatus::Valid,
            ..Default::default()
        };
    }

    if let Some(path) = first_file_path.map(str::trim).filter(|v| !v.is_empty()) {
        return ReopenTarget {
            kind: ReopenKind::FilePath,
            file_path: Some(path.to_string()),
            captured_at_ms,
            confidence: 0.85,
            validation_status: ReopenValidationStatus::Unchecked,
            ..Default::default()
        };
    }

    if let Some(bundle) = bundle_id.map(str::trim).filter(|v| !v.is_empty()) {
        return ReopenTarget {
            kind: ReopenKind::AppBundle,
            app_bundle_id: Some(bundle.to_string()),
            app_name: (!app_name.trim().is_empty()).then_some(app_name.trim().to_string()),
            captured_at_ms,
            confidence: 0.70,
            validation_status: ReopenValidationStatus::Unchecked,
            ..Default::default()
        };
    }

    ReopenTarget {
        kind: ReopenKind::Unknown,
        app_name: (!app_name.trim().is_empty()).then_some(app_name.trim().to_string()),
        captured_at_ms,
        confidence: 0.0,
        validation_status: ReopenValidationStatus::Invalid,
        ..Default::default()
    }
}

pub fn serialize_reopen_target(target: &ReopenTarget) -> String {
    serde_json::to_string(target).unwrap_or_else(|_| "{}".to_string())
}

pub fn deserialize_reopen_target(value: &str) -> Option<ReopenTarget> {
    serde_json::from_str::<ReopenTarget>(value).ok()
}
