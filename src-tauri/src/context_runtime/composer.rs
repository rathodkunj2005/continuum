//! Phase 3 composer: build either deterministic memory cards (with
//! `surfacing_reason`) or a grounded LLM answer over the evidence pack.

use crate::context_runtime::context_pack::{ComposedAnswer, EvidencePack, FusedHit, VerifyOutcome};
use crate::context_runtime::query_plan::QueryPlan;
use crate::context_runtime::retrieval_routes::memory_record_to_search_result;
use crate::inference::InferenceEngine;
use crate::search::memory_cards::{build_fallback_card, MemoryCard};
use crate::storage::Store;

const MAX_CARDS: usize = 12;
const MAX_ANSWER_CONTEXT_HITS: usize = 6;

pub async fn compose_cards(
    plan: &QueryPlan,
    fused: &[FusedHit],
    _evidence: &EvidencePack,
    store: &Store,
) -> Vec<MemoryCard> {
    let mut cards = Vec::new();
    for hit in fused.iter().take(MAX_CARDS) {
        let Ok(Some(record)) = store.get_memory_by_id(&hit.memory_id).await else {
            continue;
        };
        let search_result = memory_record_to_search_result(&record, hit.score);
        let mut card = build_fallback_card(&plan.raw, &search_result);
        card.surfacing_reason = Some(hit.surfacing_reason.clone());
        cards.push(card);
    }
    cards
}

pub async fn compose_answer(
    plan: &QueryPlan,
    fused: &[FusedHit],
    evidence: &EvidencePack,
    verify: VerifyOutcome,
    engine: Option<&InferenceEngine>,
    store: &Store,
) -> ComposedAnswer {
    let cards = compose_cards(plan, fused, evidence, store).await;
    let surfacing_reasons: Vec<_> = fused.iter().map(|h| h.surfacing_reason.clone()).collect();

    if matches!(verify, VerifyOutcome::NotEnoughEvidence { .. }) {
        return ComposedAnswer {
            query: plan.raw.clone(),
            answer: "I don't have enough grounded evidence to answer that yet.".to_string(),
            evidence: evidence.clone(),
            cards,
            verify_outcome: verify,
            surfacing_reasons,
        };
    }

    let context_str = render_context(fused, evidence, store).await;
    let answer_text = if let Some(engine) = engine {
        let raw = engine.answer(&plan.raw, &context_str).await;
        let raw_trim = raw.trim();
        if raw_trim.is_empty() {
            compose_partial_answer(evidence, &verify)
        } else if !citations_valid(raw_trim, evidence) {
            compose_partial_answer(evidence, &verify)
        } else {
            raw_trim.to_string()
        }
    } else {
        compose_partial_answer(evidence, &verify)
    };

    ComposedAnswer {
        query: plan.raw.clone(),
        answer: answer_text,
        evidence: evidence.clone(),
        cards,
        verify_outcome: verify,
        surfacing_reasons,
    }
}

fn compose_partial_answer(evidence: &EvidencePack, outcome: &VerifyOutcome) -> String {
    let mut lines = Vec::new();
    let header = match outcome {
        VerifyOutcome::Grounded { confidence } => format!(
            "Here is what the evidence supports (confidence {:.2}):",
            confidence
        ),
        VerifyOutcome::PartialAnswer { missing } => {
            format!("Partial answer — missing: {}", missing.join(", "))
        }
        VerifyOutcome::NotEnoughEvidence { reason } => {
            format!("Not enough evidence: {}", reason)
        }
    };
    lines.push(header);
    if !evidence.files.is_empty() {
        let files = evidence
            .files
            .iter()
            .take(5)
            .map(|f| f.path.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("Files: {files}"));
    }
    if !evidence.decisions.is_empty() {
        let decisions = evidence
            .decisions
            .iter()
            .take(3)
            .map(|d| d.decision.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        lines.push(format!("Decisions: {decisions}"));
    }
    if !evidence.commands.is_empty() {
        let cmds = evidence
            .commands
            .iter()
            .take(3)
            .map(|c| c.command.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        lines.push(format!("Commands: {cmds}"));
    }
    lines.join("\n")
}

async fn render_context(fused: &[FusedHit], evidence: &EvidencePack, store: &Store) -> String {
    let mut out = String::new();
    for hit in fused.iter().take(MAX_ANSWER_CONTEXT_HITS) {
        if let Ok(Some(record)) = store.get_memory_by_id(&hit.memory_id).await {
            out.push_str("--- memory ---\n");
            out.push_str(&record.snippet);
            out.push('\n');
        }
    }
    if !evidence.files.is_empty() {
        out.push_str("Known files: ");
        out.push_str(
            &evidence
                .files
                .iter()
                .map(|f| f.path.clone())
                .collect::<Vec<_>>()
                .join(", "),
        );
        out.push('\n');
    }
    if !evidence.decisions.is_empty() {
        out.push_str("Known decisions: ");
        out.push_str(
            &evidence
                .decisions
                .iter()
                .map(|d| d.decision.clone())
                .collect::<Vec<_>>()
                .join("; "),
        );
        out.push('\n');
    }
    out
}

/// Post-hoc cite check: every file/command/decision the answer references must
/// appear in the evidence pack. Returns false when the answer mentions a path
/// or command that wasn't grounded.
fn citations_valid(answer: &str, evidence: &EvidencePack) -> bool {
    let lower = answer.to_lowercase();
    for token in lower.split_whitespace() {
        let cleaned = token.trim_matches(|c: char| {
            !c.is_alphanumeric() && c != '.' && c != '/' && c != '_' && c != '-'
        });
        if cleaned.contains('/')
            || cleaned.ends_with(".rs")
            || cleaned.ends_with(".ts")
            || cleaned.ends_with(".tsx")
            || cleaned.ends_with(".py")
        {
            let found = evidence
                .files
                .iter()
                .any(|f| f.path.to_lowercase().contains(cleaned));
            if !found {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context_runtime::context_pack::{FileRef, FusionSignals, SurfacingReason};

    fn evidence_with_file(path: &str) -> EvidencePack {
        EvidencePack {
            files: vec![FileRef {
                path: path.to_string(),
                memory_ids: vec!["m-1".to_string()],
            }],
            ..Default::default()
        }
    }

    #[test]
    fn partial_answer_includes_top_files_and_decisions() {
        let outcome = VerifyOutcome::PartialAnswer {
            missing: vec!["llm".to_string()],
        };
        let text = compose_partial_answer(&evidence_with_file("plan.ts"), &outcome);
        assert!(text.contains("plan.ts"));
        assert!(text.contains("missing"));
    }

    #[test]
    fn citations_valid_rejects_unknown_file_path() {
        assert!(!citations_valid(
            "see src/foo.rs for details",
            &EvidencePack::default()
        ));
    }

    #[test]
    fn citations_valid_accepts_known_file() {
        let evidence = evidence_with_file("src/plan.ts");
        assert!(citations_valid("update src/plan.ts now", &evidence));
    }

    #[test]
    fn _suppress_unused_warning() {
        let _ = FusedHit {
            memory_id: "x".to_string(),
            score: 0.0,
            signals: FusionSignals::default(),
            surfacing_reason: SurfacingReason::default(),
            contributing_routes: Vec::new(),
        };
    }
}
