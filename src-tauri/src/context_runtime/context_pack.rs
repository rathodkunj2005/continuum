//! Shared types for the Phase 3 retrieval pipeline (fusion → evidence → verify → compose).
//!
//! The legacy [`crate::storage::schema::ContextPack`] remains the canonical persisted
//! pack; this module adds the new orchestration types (surfacing reasons, fusion
//! signals, evidence pack, verifier outcome, composed answer) that flow between
//! the new stages without changing the persisted schema.

use crate::context_runtime::query_plan::{PlannerIntent, Route};
use crate::context_runtime::retrieval_routes::PathStep;
use crate::search::memory_cards::MemoryCard;
use serde::{Deserialize, Serialize};
use specta::Type;

/// Per-card "Why this surfaced" rendered deterministically by the composer.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type, PartialEq)]
pub struct SurfacingReason {
    pub headline: String,
    pub routes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph_path: Option<Vec<PathStep>>,
    #[serde(default)]
    pub anchor_terms_hit: Vec<String>,
    #[serde(default)]
    pub recency_boost: f32,
}

/// Numeric per-route signals retained alongside the fused score so downstream
/// stages (verifier, composer) can reason about provenance.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type, PartialEq)]
pub struct FusionSignals {
    pub chunk: f32,
    pub vector: f32,
    pub keyword: f32,
    pub temporal: f32,
    pub entity: f32,
    pub graph: f32,
    pub recency: f32,
    pub coverage: f32,
    pub phrase: f32,
}

/// Weights applied to each route's contribution. `FusionWeights::for_intent`
/// returns intent-tuned defaults.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq)]
pub struct FusionWeights {
    pub chunk: f32,
    pub vector: f32,
    pub keyword: f32,
    pub temporal: f32,
    pub entity: f32,
    pub graph: f32,
    pub recency: f32,
}

impl Default for FusionWeights {
    fn default() -> Self {
        Self {
            chunk: 0.45,
            vector: 0.45,
            keyword: 0.20,
            temporal: 0.10,
            entity: 0.10,
            graph: 0.10,
            recency: 0.05,
        }
    }
}

impl FusionWeights {
    pub fn for_intent(intent: PlannerIntent) -> Self {
        let base = Self::default();
        match intent {
            PlannerIntent::Debug => Self {
                chunk: 0.40,
                vector: 0.35,
                graph: 0.20,
                ..base
            },
            PlannerIntent::Lookup => Self {
                chunk: 0.40,
                vector: 0.35,
                keyword: 0.30,
                ..base
            },
            PlannerIntent::Timeline => Self {
                chunk: 0.35,
                vector: 0.30,
                temporal: 0.25,
                ..base
            },
            PlannerIntent::ResumeWork => Self {
                chunk: 0.40,
                vector: 0.35,
                temporal: 0.20,
                graph: 0.15,
                ..base
            },
            PlannerIntent::RelatedTo => Self {
                chunk: 0.40,
                vector: 0.35,
                graph: 0.20,
                ..base
            },
            _ => base,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
pub struct FusedHit {
    pub memory_id: String,
    pub score: f32,
    pub signals: FusionSignals,
    pub surfacing_reason: SurfacingReason,
    /// Routes that contributed to this hit (e.g. `[Vector, Graph]`).
    #[serde(default)]
    pub contributing_routes: Vec<Route>,
}

/// Evidence references collected from the underlying `MemoryRecord` columns.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type, PartialEq)]
pub struct EvidencePack {
    pub files: Vec<FileRef>,
    pub commands: Vec<CommandRef>,
    pub decisions: Vec<DecisionRef>,
    pub errors: Vec<ErrorRef>,
    pub todos: Vec<TaskRef>,
    pub urls: Vec<UrlRef>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type, PartialEq)]
pub struct FileRef {
    pub path: String,
    pub memory_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type, PartialEq)]
pub struct CommandRef {
    pub command: String,
    pub memory_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type, PartialEq)]
pub struct DecisionRef {
    pub decision: String,
    pub memory_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type, PartialEq)]
pub struct ErrorRef {
    pub error: String,
    pub memory_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type, PartialEq)]
pub struct TaskRef {
    pub task: String,
    pub memory_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type, PartialEq)]
pub struct UrlRef {
    pub url: String,
    pub memory_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VerifyOutcome {
    Grounded { confidence: f32 },
    PartialAnswer { missing: Vec<String> },
    NotEnoughEvidence { reason: String },
}

impl Default for VerifyOutcome {
    fn default() -> Self {
        VerifyOutcome::NotEnoughEvidence {
            reason: "no fused hits".to_string(),
        }
    }
}

/// Result of [`crate::context_runtime::composer::compose_answer`].
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
pub struct ComposedAnswer {
    pub query: String,
    pub answer: String,
    pub evidence: EvidencePack,
    pub cards: Vec<MemoryCard>,
    pub verify_outcome: VerifyOutcome,
    pub surfacing_reasons: Vec<SurfacingReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debug_trace: Option<serde_json::Value>,
}
