use crate::inference::qwen_vl_memory::MemorySynthesisOutput;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct TaskCandidate {
    pub title: String,
    pub source_memory_id: String,
    pub confidence: f32,
    pub due_date: Option<DateTime<Utc>>,
    pub evidence: String,
}

/// Extract task candidates from memory output fields.
/// Uses decisions, errors, and next_steps — does NOT call any model.
pub fn extract_task_candidates(
    memory_id: &str,
    output: &MemorySynthesisOutput,
) -> Vec<TaskCandidate> {
    let mut candidates = Vec::new();

    for step in &output.next_steps {
        if step.trim().is_empty() {
            continue;
        }
        candidates.push(TaskCandidate {
            title: step.trim().to_string(),
            source_memory_id: memory_id.to_string(),
            confidence: output.confidence_score * 0.85,
            due_date: None,
            evidence: format!("next_step from: {}", output.summary_short),
        });
    }

    for decision in &output.decisions {
        if decision.trim().is_empty() {
            continue;
        }
        let lower = decision.to_ascii_lowercase();
        if lower.contains("todo") || lower.contains("need to") || lower.contains("will ") {
            candidates.push(TaskCandidate {
                title: decision.trim().to_string(),
                source_memory_id: memory_id.to_string(),
                confidence: output.confidence_score * 0.70,
                due_date: None,
                evidence: format!("decision from: {}", output.summary_short),
            });
        }
    }

    for error in &output.errors {
        if error.trim().is_empty() {
            continue;
        }
        candidates.push(TaskCandidate {
            title: format!("Fix: {}", error.trim()),
            source_memory_id: memory_id.to_string(),
            confidence: output.confidence_score * 0.65,
            due_date: None,
            evidence: format!("error from: {}", output.summary_short),
        });
    }

    candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_next_steps_as_tasks() {
        let output = MemorySynthesisOutput {
            summary_short: "PR review".to_string(),
            next_steps: vec!["merge PR #42".to_string(), "update docs".to_string()],
            confidence_score: 0.8,
            ..Default::default()
        };
        let tasks = extract_task_candidates("mem-001", &output);
        assert!(tasks.iter().any(|t| t.title == "merge PR #42"));
        assert!(tasks.iter().any(|t| t.title == "update docs"));
    }

    #[test]
    fn extracts_errors_as_fix_tasks() {
        let output = MemorySynthesisOutput {
            summary_short: "debugging session".to_string(),
            errors: vec!["connection refused on port 5432".to_string()],
            confidence_score: 0.7,
            ..Default::default()
        };
        let tasks = extract_task_candidates("mem-002", &output);
        assert!(tasks.iter().any(|t| t.title.starts_with("Fix:")));
    }

    #[test]
    fn empty_output_produces_no_tasks() {
        let output = MemorySynthesisOutput::default();
        let tasks = extract_task_candidates("mem-003", &output);
        assert!(tasks.is_empty());
    }
}
