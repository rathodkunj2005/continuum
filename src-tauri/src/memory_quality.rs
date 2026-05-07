//! Shared memory-quality helpers used by capture, store normalization, and API diagnostics.

use crate::config::{
    MemoryQualityConfig, DEFAULT_PRIMARY_MEMORY_AGENT_USEFULNESS_MIN,
    DEFAULT_PRIMARY_MEMORY_INTENT_MIN, DEFAULT_PRIMARY_MEMORY_OCR_NOISE_MAX,
    DEFAULT_PRIMARY_MEMORY_SPECIFICITY_MIN,
};
use crate::store::MemoryRecord;

pub fn default_memory_quality_config() -> MemoryQualityConfig {
    MemoryQualityConfig {
        primary_memory_specificity_min: DEFAULT_PRIMARY_MEMORY_SPECIFICITY_MIN,
        primary_memory_intent_min: DEFAULT_PRIMARY_MEMORY_INTENT_MIN,
        primary_memory_agent_usefulness_min: DEFAULT_PRIMARY_MEMORY_AGENT_USEFULNESS_MIN,
        primary_memory_ocr_noise_max: DEFAULT_PRIMARY_MEMORY_OCR_NOISE_MAX,
    }
}

pub fn classify_storage_outcome(record: &MemoryRecord, config: &MemoryQualityConfig) -> String {
    let primary = record.specificity_score >= config.primary_memory_specificity_min
        && record.intent_score >= config.primary_memory_intent_min
        && record.agent_usefulness_score >= config.primary_memory_agent_usefulness_min
        && record.ocr_noise_score <= config.primary_memory_ocr_noise_max;
    if primary {
        "primary_memory_card".to_string()
    } else if !record.dedup_fingerprint.trim().is_empty()
        && record.specificity_score < config.primary_memory_specificity_min
    {
        "merge_into_existing_memory".to_string()
    } else if !record.related_memory_ids.is_empty()
        && record.retrieval_value_score < 0.35
        && record.evidence_confidence < 0.50
    {
        "discard_duplicate".to_string()
    } else if record.agent_usefulness_score >= 0.45 {
        "enriched_memory_card".to_string()
    } else if record.evidence_confidence >= 0.45 {
        "low_quality_evidence".to_string()
    } else {
        "defer_until_more_context".to_string()
    }
}

pub fn quality_gate_reason(record: &MemoryRecord) -> String {
    format!(
        "specificity={:.2}, intent={:.2}, entities={:.2}, usefulness={:.2}, evidence={:.2}, noise={:.2}",
        record.specificity_score,
        record.intent_score,
        record.entity_score,
        record.agent_usefulness_score,
        record.evidence_confidence,
        record.ocr_noise_score
    )
}

pub fn is_supported_dedup_fingerprint(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.len() < 6 || trimmed.len() > 120 {
        return false;
    }
    trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | ':' | '.' | '/'))
}

pub fn deterministic_dedup_fingerprint(
    record: &MemoryRecord,
    evidence_hint: Option<&str>,
) -> String {
    let app = normalize_tokenish(&record.app_name);
    let title = normalize_tokenish(&record.window_title);
    let project = normalize_tokenish(&record.project);
    let topic = normalize_tokenish(&record.topic);
    let activity = normalize_tokenish(&record.activity_type);
    let url = record
        .url
        .as_deref()
        .map(extract_domain)
        .unwrap_or_default();
    let signal_source = evidence_hint
        .filter(|hint| !hint.trim().is_empty())
        .unwrap_or_else(|| record.clean_text.as_str());
    let evidence = stable_signal_terms(signal_source, 6);
    let mut parts = vec![
        if app.is_empty() {
            "app".to_string()
        } else {
            app
        },
        if url.is_empty() { title } else { url },
    ];
    if !project.is_empty() {
        parts.push(project);
    } else if !topic.is_empty() && topic != "unknown" {
        parts.push(topic);
    }
    if !activity.is_empty() && activity != "unknown" {
        parts.push(activity);
    }
    if !evidence.is_empty() {
        parts.push(evidence.join("_"));
    }
    parts
        .into_iter()
        .filter(|part| !part.trim().is_empty())
        .take(5)
        .collect::<Vec<_>>()
        .join(":")
}

fn normalize_tokenish(value: &str) -> String {
    value
        .to_ascii_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .take(8)
        .collect::<Vec<_>>()
        .join("_")
}

fn stable_signal_terms(value: &str, max_terms: usize) -> Vec<String> {
    let mut out = Vec::new();
    for token in value
        .to_ascii_lowercase()
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| token.len() >= 3)
    {
        if STOP_TOKENS.contains(&token) {
            continue;
        }
        if !out.iter().any(|item| item == token) {
            out.push(token.to_string());
        }
        if out.len() >= max_terms {
            break;
        }
    }
    out
}

fn extract_domain(url: &str) -> String {
    let raw = url.trim();
    if raw.is_empty() {
        return String::new();
    }
    raw.split("://")
        .nth(1)
        .unwrap_or(raw)
        .split('/')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

const STOP_TOKENS: &[&str] = &[
    "the", "and", "for", "with", "from", "this", "that", "into", "your", "you", "user", "www",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_fingerprint_fallback_is_stable_and_supported() {
        let record = MemoryRecord {
            app_name: "Codex".to_string(),
            window_title: "capture/mod.rs".to_string(),
            project: "FNDR".to_string(),
            topic: "memory cards".to_string(),
            activity_type: "debugging".to_string(),
            clean_text: "Investigated OCR quality regressions in capture loop".to_string(),
            ..Default::default()
        };
        let value =
            deterministic_dedup_fingerprint(&record, Some("Fixed OCR grounding confidence"));
        assert!(
            is_supported_dedup_fingerprint(&value),
            "generated fallback should be valid: {value}"
        );
    }

    #[test]
    fn storage_outcome_primary_when_scores_are_high() {
        let cfg = default_memory_quality_config();
        let record = MemoryRecord {
            specificity_score: 0.9,
            intent_score: 0.9,
            agent_usefulness_score: 0.9,
            ocr_noise_score: 0.1,
            ..Default::default()
        };
        let outcome = classify_storage_outcome(&record, &cfg);
        assert_eq!(outcome, "primary_memory_card");
    }

    #[test]
    fn storage_outcome_merges_when_dedup_present_but_specificity_low() {
        let cfg = default_memory_quality_config();
        let record = MemoryRecord {
            specificity_score: 0.18,
            intent_score: 0.85,
            agent_usefulness_score: 0.8,
            ocr_noise_score: 0.2,
            dedup_fingerprint: "fndr:capture:ocr".to_string(),
            ..Default::default()
        };
        let outcome = classify_storage_outcome(&record, &cfg);
        assert_eq!(outcome, "merge_into_existing_memory");
    }
}
