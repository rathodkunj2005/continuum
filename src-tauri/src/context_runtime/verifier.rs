//! Phase 3 verifier: gate fused hits with explicit rules before composition.

use crate::context_runtime::context_pack::{EvidencePack, FusedHit, VerifyOutcome};
use crate::context_runtime::query_plan::{QueryPlan, Route};
use crate::telemetry::runtime_metrics;
use std::collections::HashSet;

const MIN_CONFIDENCE_NON_GRAPH: f32 = 0.3;
const MIN_DISTINCT_BACKERS: usize = 2;

pub fn verify(plan: &QueryPlan, fused: &[FusedHit], evidence: &EvidencePack) -> VerifyOutcome {
    if fused.is_empty() {
        return record(VerifyOutcome::NotEnoughEvidence {
            reason: "no fused hits".to_string(),
        });
    }

    let retained: Vec<&FusedHit> = fused
        .iter()
        .filter(|hit| {
            let from_graph = hit.contributing_routes.contains(&Route::Graph);
            let conf = hit.signals.coverage.max(hit.score.min(1.0));
            from_graph || conf >= MIN_CONFIDENCE_NON_GRAPH
        })
        .collect();

    if retained.is_empty() {
        return record(VerifyOutcome::NotEnoughEvidence {
            reason: "all hits below confidence threshold".to_string(),
        });
    }

    let weak_graph_only = retained.iter().filter(|hit| {
        hit.contributing_routes == vec![Route::Graph]
            && hit
                .surfacing_reason
                .graph_path
                .as_ref()
                .map(|p| p.len() == 2)
                .unwrap_or(false)
    });
    let weak_graph_only_count = weak_graph_only.count();

    let distinct_backers: HashSet<&str> = retained.iter().map(|h| h.memory_id.as_str()).collect();
    if distinct_backers.len() < MIN_DISTINCT_BACKERS {
        return record(VerifyOutcome::PartialAnswer {
            missing: vec!["only one memory backs the top result".to_string()],
        });
    }

    if plan.needed_context.files && evidence.files.is_empty() {
        return record(VerifyOutcome::PartialAnswer {
            missing: vec!["no file evidence for a file-seeking query".to_string()],
        });
    }

    let mut missing = Vec::new();
    if plan.needed_context.decisions && evidence.decisions.is_empty() {
        missing.push("no decision evidence".to_string());
    }
    if plan.needed_context.errors && evidence.errors.is_empty() {
        missing.push("no error evidence".to_string());
    }

    if !missing.is_empty() {
        return record(VerifyOutcome::PartialAnswer { missing });
    }

    let top_score = retained.first().map(|h| h.score).unwrap_or(0.0).min(1.0);
    let confidence = (top_score * 0.8 + (retained.len().min(5) as f32) * 0.04).min(1.0);
    let confidence = if weak_graph_only_count > 0 {
        (confidence * 0.85).min(1.0)
    } else {
        confidence
    };

    record(VerifyOutcome::Grounded { confidence })
}

fn record(outcome: VerifyOutcome) -> VerifyOutcome {
    let counter = match &outcome {
        VerifyOutcome::Grounded { .. } => "fndr.retrieval.verify.grounded",
        VerifyOutcome::PartialAnswer { .. } => "fndr.retrieval.verify.partial",
        VerifyOutcome::NotEnoughEvidence { .. } => "fndr.retrieval.verify.no_evidence",
    };
    runtime_metrics::bump(counter);
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context_runtime::context_pack::{FusionSignals, SurfacingReason};
    use crate::context_runtime::query_plan::{GraphExpansion, NeededContext, PlannerIntent};

    fn plan(files: bool) -> QueryPlan {
        QueryPlan {
            raw: "q".to_string(),
            intent: PlannerIntent::Lookup,
            target_project: None,
            target_topics: Vec::new(),
            target_entities: Vec::new(),
            time_window: None,
            needed_context: NeededContext {
                files,
                ..Default::default()
            },
            retrieval_routes: vec![Route::Vector],
            graph_expansion: GraphExpansion {
                max_hops: 0,
                seed_kinds: Vec::new(),
                allowed_edges: Vec::new(),
            },
            budget_tokens: 1200,
        }
    }

    fn hit(id: &str, score: f32, routes: Vec<Route>) -> FusedHit {
        FusedHit {
            memory_id: id.to_string(),
            score,
            signals: FusionSignals {
                coverage: score,
                ..Default::default()
            },
            surfacing_reason: SurfacingReason::default(),
            contributing_routes: routes,
        }
    }

    #[test]
    fn low_confidence_hits_yield_not_enough_evidence() {
        let outcome = verify(
            &plan(false),
            &[hit("a", 0.1, vec![Route::Vector])],
            &EvidencePack::default(),
        );
        assert!(matches!(outcome, VerifyOutcome::NotEnoughEvidence { .. }));
    }

    #[test]
    fn two_solid_backers_yield_grounded() {
        let outcome = verify(
            &plan(false),
            &[
                hit("a", 0.8, vec![Route::Vector]),
                hit("b", 0.7, vec![Route::Vector, Route::Keyword]),
            ],
            &EvidencePack::default(),
        );
        assert!(matches!(outcome, VerifyOutcome::Grounded { .. }));
    }

    #[test]
    fn needed_files_without_evidence_is_partial() {
        let outcome = verify(
            &plan(true),
            &[
                hit("a", 0.8, vec![Route::Vector]),
                hit("b", 0.7, vec![Route::Vector]),
            ],
            &EvidencePack::default(),
        );
        assert!(matches!(outcome, VerifyOutcome::PartialAnswer { .. }));
    }
}
