use crate::storage::MemoryRecord;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ActivityType {
    Researching,
    Coding,
    Debugging,
    Reading,
    Writing,
    Designing,
    Planning,
    Reviewing,
    Communicating,
    Configuring,
    Browsing,
    Unknown,
}

impl ActivityType {
    pub fn from_label(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "research" | "researching" => Self::Researching,
            "coding" => Self::Coding,
            "debugging" => Self::Debugging,
            "reading" | "studying" | "reading_results" => Self::Reading,
            "writing" => Self::Writing,
            "designing" => Self::Designing,
            "planning" => Self::Planning,
            "reviewing" | "reviewing_agent_output" => Self::Reviewing,
            "communicating" | "communication" => Self::Communicating,
            "configuring" | "configuring_tool" => Self::Configuring,
            "browsing" | "watching_or_listening" => Self::Browsing,
            _ => Self::Unknown,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Researching => "researching",
            Self::Coding => "coding",
            Self::Debugging => "debugging",
            Self::Reading => "reading",
            Self::Writing => "writing",
            Self::Designing => "designing",
            Self::Planning => "planning",
            Self::Reviewing => "reviewing",
            Self::Communicating => "communicating",
            Self::Configuring => "configuring",
            Self::Browsing => "browsing",
            Self::Unknown => "unknown",
        }
    }
}

impl Default for ActivityType {
    fn default() -> Self {
        Self::Unknown
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OcrQualityStats {
    pub raw_blocks: usize,
    pub kept_blocks: usize,
    pub dropped_blocks: usize,
    pub confidence_min: Option<f32>,
    pub confidence_mean: Option<f32>,
    pub confidence_weighted_by_chars: Option<f32>,
    pub confidence_max: Option<f32>,
    pub unknown_confidence_count: usize,
    pub content_density_score: f32,
    pub ui_chrome_ratio: f32,
    pub low_signal_ratio: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CapturedObservation {
    pub app_name: String,
    pub bundle_id: Option<String>,
    pub window_title: String,
    pub source_url: Option<String>,
    pub file_path: Option<String>,
    pub timestamp_ms: i64,
    pub source_kind: String,
    pub clean_text: String,
    pub ocr_quality: OcrQualityStats,
    pub raw_evidence: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EvidenceSpan {
    pub text: String,
    pub score: f32,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DroppedSpan {
    pub text: String,
    pub reason: String,
    pub score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CleanedEvidence {
    pub clean_text: String,
    pub salient_spans: Vec<EvidenceSpan>,
    pub dropped_spans: Vec<DroppedSpan>,
    pub dropped_reason_counts: HashMap<String, usize>,
    pub evidence_quality: f32,
    pub contamination_score: f32,
    pub ui_chrome_ratio: f32,
    pub repeated_text_ratio: f32,
    pub content_density_score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DistilledMemory {
    pub title: String,
    pub topic: String,
    pub summary_short: String,
    pub memory_context: String,
    pub activity_type: ActivityType,
    pub workflow: String,
    pub project: String,
    pub entities: Vec<String>,
    pub actions: Vec<String>,
    pub user_intent: String,
    pub confidence: f32,
    pub quality_flags: Vec<String>,
    #[serde(default)]
    pub topic_categories: Vec<String>,
    #[serde(default)]
    pub search_aliases: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ValidatedMemory {
    pub title: String,
    pub topic: String,
    pub summary_short: String,
    pub memory_context: String,
    pub activity_type: ActivityType,
    pub workflow: String,
    pub project: String,
    pub entities: Vec<String>,
    pub actions: Vec<String>,
    pub user_intent: String,
    pub confidence: f32,
    pub grounding_confidence: f32,
    pub evidence_quality: f32,
    pub contamination_score: f32,
    pub quality_flags: Vec<String>,
    pub topic_categories: Vec<String>,
    pub search_aliases: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MemoryDecision {
    Store(ValidatedMemory),
    Quarantine(DiagnosticObservation),
    Skip(SkipReason),
    RepairExisting(RepairPlan),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DiagnosticObservation {
    pub reason: String,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RepairPlan {
    pub memory_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SkipReason {
    InternalSurface,
    LowGrounding,
    HighContamination,
    MissingCoreFields,
    WeakEvidence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MemoryDecisionKind {
    Store,
    Quarantine,
    Skip,
    RepairExisting,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QualityScores {
    pub grounding_confidence: f32,
    pub evidence_quality: f32,
    pub contamination_score: f32,
    pub topic_clarity: f32,
    pub pollution_ratio: f32,
    pub retrieval_value: f32,
    pub graph_readiness: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QualityDecision {
    pub decision: String,
    pub passed: bool,
    pub reasons: Vec<String>,
    pub scores: QualityScores,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GraphCandidate {
    pub label: String,
    pub activity_type: String,
    pub topic: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EmbeddingDocument {
    pub text: String,
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StoredMemoryCard {
    pub record: MemoryRecord,
    pub quality: QualityDecision,
}
