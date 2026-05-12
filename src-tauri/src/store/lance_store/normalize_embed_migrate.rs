//! Record normalization, embedding text, alias generation, dedupe helpers, and JSON migrations.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arrow_array::{
    Array, BooleanArray, FixedSizeListArray, Float32Array, Int64Array, ListArray, RecordBatch,
    RecordBatchIterator, RecordBatchReader, StringArray, UInt32Array,
};
use arrow_schema::{ArrowError, DataType, Schema};
use chrono::{Datelike, Local, NaiveDate, TimeZone};
use futures::TryStreamExt;
use lancedb::query::{ExecutableQuery, QueryBase};
use lancedb::table::{AddDataMode, NewColumnTransform};
use lancedb::{Connection, Table};

use crate::capture::text_cleanup;
use crate::config::{DEFAULT_IMAGE_EMBEDDING_DIM, DEFAULT_TEXT_EMBEDDING_DIM};
use crate::memory_compaction::{build_lexical_shadow, compact_memory_record_payload};
use crate::memory_quality::{
    classify_storage_outcome, default_memory_quality_config, deterministic_dedup_fingerprint,
    is_supported_dedup_fingerprint, quality_gate_reason as shared_quality_gate_reason,
};
use crate::store::schema::{
    ExtractedEntity, GraphEdge, GraphNode, IntentAnalysis, IntentCandidate, MemoryActionItem,
    MemoryRecord, MeetingSegment, MeetingSession, SearchResult, Task,
};

use super::arrow_and_filters::{
    compute_content_hash, edge_to_batch, extract_domain, meeting_to_batch, node_to_batch,
    records_to_batch, segment_to_batch, task_to_batch,
};
use super::schemas::*;
use super::text_kw::{
    canonicalize_index_url, char_ngrams, drop_middle_char, is_keyword_stop_word, keyword_terms,
    normalize_keyword_text, trim_chars,
};
use super::{
    ACTIVITY_EVENTS_TABLE, CONTEXT_DELTAS_TABLE, CONTEXT_PACKS_TABLE, DECISION_LEDGER_TABLE,
    EDGES_TABLE, ENTITY_ALIASES_TABLE, INDEX_NOISE_HOSTS, KNOWLEDGE_PAGES_TABLE, MEETINGS_TABLE,
    MEMORIES_TABLE,
    NODES_TABLE, PROJECT_CONTEXTS_TABLE, SEARCH_RESULT_COLUMNS, SEGMENTS_TABLE, TASKS_TABLE,
    IMAGE_EMBED_DIM, TEXT_EMBED_DIM,
};

pub(super) fn lexical_keyword_score(terms: &[String], result: &SearchResult) -> f32 {
    if terms.is_empty() {
        return 0.0;
    }

    let title = normalize_keyword_text(&result.window_title);
    let snippet = normalize_keyword_text(&result.snippet);
    let memory_context = normalize_keyword_text(&result.memory_context);
    let lexical_shadow = normalize_keyword_text(&result.lexical_shadow);
    let alias_blob = normalize_keyword_text(&result.search_aliases.join(" "));
    let clean = normalize_keyword_text(if !result.clean_text.trim().is_empty() {
        &result.clean_text
    } else {
        &result.text
    });
    let app = normalize_keyword_text(&result.app_name);
    let url = result
        .url
        .as_ref()
        .map(|value| normalize_keyword_text(value))
        .unwrap_or_default();
    let merged = format!(
        "{} {} {} {} {} {} {}",
        title, snippet, memory_context, clean, lexical_shadow, alias_blob, url
    );

    let mut matched_terms = 0usize;
    let mut weighted = 0.0f32;

    for (idx, term) in terms.iter().enumerate() {
        let mut matched = false;
        if title.contains(term) {
            weighted += 1.8;
            matched = true;
        }
        if snippet.contains(term) {
            weighted += 1.35;
            matched = true;
        }
        if clean.contains(term) {
            weighted += 1.1;
            matched = true;
        }
        if memory_context.contains(term) {
            weighted += 1.25;
            matched = true;
        }
        if lexical_shadow.contains(term) {
            weighted += 1.05;
            matched = true;
        }
        if alias_blob.contains(term) {
            weighted += 1.0;
            matched = true;
        }
        if app.contains(term) {
            weighted += 0.75;
            matched = true;
        }
        if !url.is_empty() && url.contains(term) {
            weighted += 0.95;
            matched = true;
        }

        // Reward full sentence/phrase hits for sentence queries.
        if idx == 0 && term.split_whitespace().count() >= 2 && merged.contains(term) {
            weighted += 1.1;
            matched = true;
        }

        if matched {
            matched_terms += 1;
        }
    }

    let coverage = matched_terms as f32 / terms.len() as f32;
    let normalized = (weighted / (terms.len() as f32 * 2.8)).min(1.0);
    (normalized * 0.7 + coverage * 0.3).clamp(0.0, 1.0)
}

pub(super) fn recency_score(now_ms: i64, timestamp_ms: i64) -> f32 {
    let age_hours = ((now_ms - timestamp_ms).max(0) as f32 / 3_600_000.0).min(24.0 * 30.0);
    (1.0 / (1.0 + age_hours * 0.03)).clamp(0.0, 1.0)
}

pub(super) fn estimate_signal_strength(
    summary_source: &str,
    ocr_confidence: f32,
    noise_score: f32,
    snippet: &str,
    clean_text: &str,
) -> f32 {
    let summary_weight = match summary_source.trim().to_ascii_lowercase().as_str() {
        "llm" => 1.0,
        "vlm" => 0.9,
        "fallback" => 0.66,
        _ => 0.58,
    };
    let snippet_density = (normalize_keyword_text(snippet)
        .split_whitespace()
        .count()
        .min(24) as f32
        / 24.0)
        .clamp(0.0, 1.0);
    let text_density = (normalize_keyword_text(clean_text)
        .split_whitespace()
        .count()
        .min(80) as f32
        / 80.0)
        .clamp(0.0, 1.0);

    (ocr_confidence.clamp(0.0, 1.0) * 0.24
        + (1.0 - noise_score.clamp(0.0, 1.0)) * 0.28
        + summary_weight * 0.18
        + snippet_density * 0.16
        + text_density * 0.14)
        .clamp(0.0, 1.0)
}

pub(super) fn estimate_record_signal_strength(record: &MemoryRecord) -> f32 {
    estimate_signal_strength(
        &record.summary_source,
        record.ocr_confidence,
        record.noise_score,
        &record.snippet,
        &record.clean_text,
    )
}

pub(super) fn normalize_record_for_index(record: &MemoryRecord) -> MemoryRecord {
    let lexical_shadow = if record.lexical_shadow.trim().is_empty() {
        build_lexical_shadow(
            &record.window_title,
            &record.snippet,
            &record.clean_text,
            record.url.as_deref(),
        )
    } else {
        record.lexical_shadow.clone()
    };
    let mut normalized = compact_memory_record_payload(record);
    normalized.url = sanitize_index_url(
        normalized.url.as_deref(),
        &normalized.window_title,
        &normalized.snippet,
    );
    normalized.session_key = build_index_session_key(&normalized);
    normalized.lexical_shadow = lexical_shadow;
    normalized.embedding = normalize_vector_dim(
        &normalized.id,
        "embedding",
        &normalized.embedding,
        TEXT_EMBED_DIM as usize,
    );
    normalized.snippet_embedding = normalize_vector_dim(
        &normalized.id,
        "snippet_embedding",
        &normalized.snippet_embedding,
        TEXT_EMBED_DIM as usize,
    );
    normalized.support_embedding = normalize_vector_dim(
        &normalized.id,
        "support_embedding",
        &normalized.support_embedding,
        TEXT_EMBED_DIM as usize,
    );
    normalized.image_embedding = normalize_vector_dim(
        &normalized.id,
        "image_embedding",
        &normalized.image_embedding,
        IMAGE_EMBED_DIM as usize,
    );
    if normalized.display_summary.trim().is_empty() {
        normalized.display_summary = normalized.snippet.clone();
    }
    if normalized.internal_context.trim().is_empty() {
        normalized.internal_context = normalized.clean_text.clone();
    }
    normalized.clean_text = strip_low_conf_markers(&normalized.clean_text);
    normalized.snippet = strip_low_conf_markers(&normalized.snippet);
    normalized.display_summary = strip_low_conf_markers(&normalized.display_summary);
    normalized.internal_context = strip_low_conf_markers(&normalized.internal_context);
    normalized.memory_context = strip_low_conf_markers(&normalized.memory_context);
    normalized.embedding_text = strip_low_conf_markers(&normalized.embedding_text);
    if normalized.timestamp_start <= 0 {
        normalized.timestamp_start = normalized.timestamp;
    }
    if normalized.timestamp_end <= 0 {
        normalized.timestamp_end = normalized.timestamp;
    }
    if normalized.source_type.trim().is_empty() {
        normalized.source_type = infer_source_type(&normalized);
    }
    normalize_event_fields(&mut normalized);

    if normalized.memory_context.trim().is_empty() {
        normalized.memory_context = derive_memory_context(&normalized);
    }
    if normalized.user_intent.trim().is_empty()
        || normalized.intent_analysis.intent_label.is_empty()
    {
        let analysis = infer_intent_analysis(&normalized);
        normalized.user_intent = analysis.intent_label.clone();
        normalized.intent_analysis = analysis;
    }
    if normalized.topic.trim().is_empty() || normalized.topic == "unknown" {
        normalized.topic = infer_topic(&normalized);
    }
    if normalized.workflow.trim().is_empty() || normalized.workflow == "unknown" {
        normalized.workflow = infer_workflow(&normalized);
    }
    if normalized.embedding_text.trim().is_empty() {
        normalized.embedding_text = compose_embedding_text(&normalized);
    }
    if !is_supported_dedup_fingerprint(&normalized.dedup_fingerprint) {
        normalized.dedup_fingerprint =
            deterministic_dedup_fingerprint(&normalized, Some(&normalized.memory_context));
    }
    if normalized.search_aliases.is_empty() {
        normalized.search_aliases = generate_search_aliases(&normalized);
    }
    if normalized.raw_evidence.trim().is_empty() {
        normalized.raw_evidence = build_raw_evidence_payload(&normalized);
    }
    if normalized.extracted_entities_structured.is_empty() {
        normalized.extracted_entities_structured = derive_structured_entities(&normalized);
    }
    if normalized.action_items.is_empty() {
        normalized.action_items = derive_action_items(&normalized);
    }
    if normalized.topic_confidence <= 0.0 {
        normalized.topic_confidence = estimate_topic_confidence(&normalized);
    }
    if normalized.workflow_confidence <= 0.0 {
        normalized.workflow_confidence = estimate_workflow_confidence(&normalized);
    }
    if normalized.project_confidence <= 0.0 {
        normalized.project_confidence = estimate_project_confidence(&normalized);
    }
    if normalized.ocr_noise_score <= 0.0 {
        normalized.ocr_noise_score = normalized.noise_score.clamp(0.0, 1.0);
    }
    if normalized.evidence_confidence <= 0.0 {
        normalized.evidence_confidence = normalized
            .ocr_confidence
            .max(normalized.extraction_confidence)
            .clamp(0.0, 1.0);
    }
    if normalized.specificity_score <= 0.0 {
        normalized.specificity_score = estimate_specificity_score(&normalized);
    }
    if normalized.intent_score <= 0.0 {
        normalized.intent_score = normalized.intent_analysis.confidence.clamp(0.0, 1.0);
    }
    if normalized.entity_score <= 0.0 {
        normalized.entity_score = estimate_entity_score(&normalized);
    }
    if normalized.importance_score <= 0.0 {
        normalized.importance_score = estimate_importance_score(&normalized);
    }
    if normalized.agent_usefulness_score <= 0.0 {
        normalized.agent_usefulness_score = estimate_agent_usefulness_score(&normalized);
    }
    if normalized.confidence_score <= 0.0 {
        normalized.confidence_score = ((normalized.evidence_confidence * 0.55)
            + (normalized.intent_analysis.confidence * 0.20)
            + (normalized.extraction_confidence.clamp(0.0, 1.0) * 0.25))
            .clamp(0.0, 1.0);
    }
    if normalized.graph_readiness_score <= 0.0 {
        normalized.graph_readiness_score = ((normalized.entity_score * 0.4)
            + (normalized.evidence_confidence * 0.35)
            + (normalized.specificity_score * 0.25))
            .clamp(0.0, 1.0);
    }
    if normalized.retrieval_value_score <= 0.0 {
        let salience_concentration = estimate_salience_concentration(&normalized);
        let topic_clarity = estimate_topic_clarity(&normalized);
        let pollution = estimate_pollution_ratio(&normalized);
        normalized.retrieval_value_score = ((normalized.agent_usefulness_score * 0.35)
            + (normalized.specificity_score * 0.25)
            + (normalized.intent_score * 0.15)
            + (salience_concentration * 0.15)
            + (topic_clarity * 0.10)
            - (pollution * 0.20))
            .clamp(0.0, 1.0);
    }
    if normalized.storage_outcome.trim().is_empty() {
        normalized.storage_outcome =
            classify_storage_outcome(&normalized, &default_memory_quality_config());
    }
    if normalized.quality_gate_reason.trim().is_empty() {
        normalized.quality_gate_reason = shared_quality_gate_reason(&normalized);
    }
    normalized.anchor_coverage_score = normalized.anchor_coverage_score.clamp(0.0, 1.0);
    if normalized.content_hash.trim().is_empty() {
        normalized.content_hash = compute_content_hash(
            normalized.url.as_deref(),
            &normalized.window_title,
            normalized.timestamp,
        );
    }
    normalized
}

pub(super) fn strip_low_conf_markers(value: &str) -> String {
    value
        .replace("[LOW_CONF]", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn normalize_event_fields(record: &mut MemoryRecord) {
    if record.topic.trim().is_empty() {
        record.topic = "unknown".to_string();
    }
    if record.workflow.trim().is_empty() {
        record.workflow = "unknown".to_string();
    }
    if record.source_type.trim().is_empty() {
        record.source_type = "screen".to_string();
    }
}

pub(super) fn infer_source_type(record: &MemoryRecord) -> String {
    if record.url.is_some() {
        "browser".to_string()
    } else {
        "screen".to_string()
    }
}

pub(super) fn derive_memory_context(record: &MemoryRecord) -> String {
    let quality_config = default_memory_quality_config();
    derive_memory_context_with_config(record, &quality_config)
}

pub(super) fn derive_memory_context_with_config(
    record: &MemoryRecord,
    config: &crate::config::MemoryQualityConfig,
) -> String {
    let summary = if !record.display_summary.trim().is_empty() {
        record.display_summary.trim()
    } else {
        record.snippet.trim()
    };
    let detail = if !record.internal_context.trim().is_empty() {
        record.internal_context.trim()
    } else {
        record.clean_text.trim()
    };

    let mut parts = Vec::new();
    if !summary.is_empty() {
        parts.push(summary.to_string());
    }

    let intent = if !record.user_intent.trim().is_empty() {
        record.user_intent.trim().to_string()
    } else if !record.activity_type.trim().is_empty() {
        record.activity_type.trim().to_string()
    } else {
        String::new()
    };
    let mut context_bits = Vec::new();
    if !intent.is_empty() {
        context_bits.push(format!("Intent: {intent}"));
    }
    if !record.project.trim().is_empty() {
        context_bits.push(format!("Project: {}", record.project.trim()));
    }
    if !record.topic.trim().is_empty() && record.topic != "unknown" {
        context_bits.push(format!("Topic: {}", record.topic.trim()));
    }
    if !record.workflow.trim().is_empty() && record.workflow != "unknown" {
        context_bits.push(format!("Workflow: {}", record.workflow.trim()));
    }
    if !record.files_touched.is_empty() {
        context_bits.push(format!(
            "Files: {}",
            record
                .files_touched
                .iter()
                .take(4)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !record.decisions.is_empty() {
        context_bits.push(format!(
            "Decisions: {}",
            record
                .decisions
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    if !record.errors.is_empty() {
        context_bits.push(format!(
            "Errors: {}",
            record
                .errors
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    if !record.next_steps.is_empty() {
        context_bits.push(format!(
            "Next actions: {}",
            record
                .next_steps
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    if !context_bits.is_empty() {
        parts.push(context_bits.join(" | "));
    }

    if !detail.is_empty() && !summary.eq_ignore_ascii_case(detail) {
        parts.push(trim_chars(detail, 700));
    }
    if parts.is_empty() {
        return trim_chars(&record.window_title, 220);
    }
    trim_chars(&parts.join("\n"), config.memory_context_max_chars as usize)
}

pub(super) fn infer_topic(record: &MemoryRecord) -> String {
    if !record.project.trim().is_empty() {
        return trim_chars(record.project.trim(), 80);
    }
    if let Some(url) = record.url.as_deref() {
        if let Some(path) = extract_path_segments(url, 2) {
            let normalized = path.replace('/', " ").replace('_', " ");
            let topic = normalized
                .split_whitespace()
                .take(4)
                .collect::<Vec<_>>()
                .join(" ");
            if !topic.is_empty() {
                return topic;
            }
        }
    }
    let from_title = normalize_keyword_text(&record.window_title)
        .split_whitespace()
        .filter(|token| token.len() >= 4 && !is_keyword_stop_word(token))
        .take(5)
        .collect::<Vec<_>>()
        .join(" ");
    if !from_title.is_empty() {
        return from_title;
    }
    "unknown".to_string()
}

pub(super) fn infer_workflow(record: &MemoryRecord) -> String {
    let intent = infer_intent_analysis(record);
    let label = intent.intent_label;
    if label.is_empty() {
        "unknown".to_string()
    } else {
        label
    }
}

pub(super) fn infer_intent_analysis(record: &MemoryRecord) -> crate::store::IntentAnalysis {
    let mut scores: HashMap<&str, f32> = HashMap::new();
    let mut evidence: HashMap<&str, Vec<String>> = HashMap::new();
    let text = normalize_keyword_text(&format!(
        "{} {} {} {}",
        record.window_title, record.clean_text, record.internal_context, record.memory_context
    ));

    let add = |scores: &mut HashMap<&str, f32>,
               evidence: &mut HashMap<&str, Vec<String>>,
               label: &'static str,
               weight: f32,
               reason: &str| {
        *scores.entry(label).or_insert(0.0) += weight;
        evidence.entry(label).or_default().push(reason.to_string());
    };

    if !record.files_touched.is_empty() {
        add(
            &mut scores,
            &mut evidence,
            "coding",
            0.42,
            "code/IDE artifacts present",
        );
    }
    if !record.errors.is_empty() || text.contains("failed") || text.contains("error") {
        add(
            &mut scores,
            &mut evidence,
            "debugging",
            0.45,
            "failure/error signals present",
        );
    }
    if !record.decisions.is_empty() || text.contains("decided") || text.contains("chose") {
        add(
            &mut scores,
            &mut evidence,
            "planning",
            0.32,
            "decision/planning language detected",
        );
    }
    if record.url.is_some() {
        add(
            &mut scores,
            &mut evidence,
            "researching",
            0.22,
            "web/document context detected",
        );
    }
    if !record.commands.is_empty() {
        add(
            &mut scores,
            &mut evidence,
            "testing_workflow",
            0.30,
            "terminal/command evidence present",
        );
    }
    if text.contains("todo") || !record.next_steps.is_empty() {
        add(
            &mut scores,
            &mut evidence,
            "organizing_information",
            0.22,
            "explicit next-step/task language",
        );
    }
    if scores.is_empty() {
        add(
            &mut scores,
            &mut evidence,
            "unknown",
            0.20,
            "insufficient high-confidence intent evidence",
        );
    }

    let mut ranked = scores.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let top = ranked.first().copied().unwrap_or(("unknown", 0.0));
    let total: f32 = ranked
        .iter()
        .map(|(_, score)| *score)
        .sum::<f32>()
        .max(0.001);
    let confidence = (top.1 / total).clamp(0.0, 1.0);
    let competing = ranked
        .iter()
        .skip(1)
        .take(2)
        .map(|(label, score)| crate::store::IntentCandidate {
            label: (*label).to_string(),
            confidence: (*score / total).clamp(0.0, 1.0),
        })
        .collect::<Vec<_>>();

    crate::store::IntentAnalysis {
        intent_label: top.0.to_string(),
        confidence,
        supporting_evidence: evidence.get(top.0).cloned().unwrap_or_default(),
        competing_intents: competing,
    }
}

pub fn compose_embedding_text(record: &MemoryRecord) -> String {
    let mut segments = Vec::new();
    push_text_segment(&mut segments, &record.user_intent, "intent");
    push_text_segment(&mut segments, &record.project, "project");
    push_text_segment(&mut segments, &record.topic, "topic");
    push_text_segment(&mut segments, &record.workflow, "workflow");
    // The durable memory_context anchors retrieval; keep it immediately after
    // the structured anchors and ahead of enumerated fields.
    push_text_segment(&mut segments, &record.memory_context, "context");

    let entity_blob = record.entities.join(", ");
    push_text_segment(&mut segments, &entity_blob, "entities");
    let alias_blob = record.search_aliases.join(", ");
    push_text_segment(&mut segments, &alias_blob, "aliases");
    let decisions = record.decisions.join("; ");
    push_text_segment(&mut segments, &decisions, "decisions");
    let errors = record.errors.join("; ");
    push_text_segment(&mut segments, &errors, "errors");
    let blockers = record.blockers.join("; ");
    push_text_segment(&mut segments, &blockers, "blockers");
    let todos = if !record.todos.is_empty() {
        record.todos.join("; ")
    } else {
        record.next_steps.join("; ")
    };
    push_text_segment(&mut segments, &todos, "todos");
    let results = record.results.join("; ");
    push_text_segment(&mut segments, &results, "results");
    push_text_segment(&mut segments, &record.files_touched.join(", "), "files");
    if let Some(url) = record.url.as_deref() {
        push_text_segment(&mut segments, url, "urls");
    }
    push_text_segment(&mut segments, &record.commands.join("; "), "commands");
    let evidence = crate::capture::text_cleanup::compress_to_salient_evidence(
        &record.clean_text,
        &record.app_name,
        360,
    );
    push_text_segment(&mut segments, &evidence, "raw_evidence");

    trim_chars(&segments.join("\n"), 2_000)
}

pub(super) fn push_text_segment(out: &mut Vec<String>, value: &str, label: &str) {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("unknown") {
        return;
    }
    out.push(format!("{label}: {trimmed}"));
}

/// Source noun-phrase candidates for alias generation from the strongest
/// semantic fields: topic, workflow, decisions, next_steps, and the head of
/// memory_context. Returns trimmed phrase strings.
pub(super) fn alias_noun_phrase_sources(record: &MemoryRecord) -> Vec<String> {
    let mut phrases: Vec<String> = Vec::new();
    let push_if_non_empty = |out: &mut Vec<String>, value: &str| {
        let trimmed = value.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("unknown") {
            return;
        }
        out.push(trimmed.to_string());
    };
    push_if_non_empty(&mut phrases, &record.topic);
    push_if_non_empty(&mut phrases, &record.workflow);
    push_if_non_empty(&mut phrases, &record.project);
    for decision in record.decisions.iter().take(3) {
        push_if_non_empty(&mut phrases, decision);
    }
    for step in record.next_steps.iter().take(3) {
        push_if_non_empty(&mut phrases, step);
    }
    // memory_context head (first salient span) reuses the same scoring helper
    // so the noun-phrase set stays consistent with the embedding tail.
    let context_head: String = record
        .memory_context
        .split("\n\n")
        .next()
        .unwrap_or("")
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(140)
        .collect();
    push_if_non_empty(&mut phrases, &context_head);
    phrases
}

pub(super) fn acronym_for_phrase(value: &str) -> Option<String> {
    let tokens: Vec<&str> = value
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .collect();
    let capital_tokens = tokens
        .iter()
        .filter(|tok| tok.chars().next().map(|c| c.is_uppercase()).unwrap_or(false))
        .count();
    // Only emit acronyms for proper-noun compounds (≥2 capitalized tokens).
    if capital_tokens < 2 || tokens.len() < 2 {
        return None;
    }
    let acronym: String = tokens
        .iter()
        .filter_map(|tok| tok.chars().next())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if acronym.len() >= 2 {
        Some(acronym)
    } else {
        None
    }
}

/// Public re-export so capture/api rebuild paths share one alias generator.
pub fn generate_search_aliases_public(record: &MemoryRecord) -> Vec<String> {
    generate_search_aliases(record)
}

/// Diagnostics-facing wrappers around the new retrieval-aware estimators.
pub fn salience_concentration_score(record: &MemoryRecord) -> f32 {
    estimate_salience_concentration(record)
}

pub fn topic_clarity_score(record: &MemoryRecord) -> f32 {
    estimate_topic_clarity(record)
}

pub fn pollution_ratio_score(record: &MemoryRecord) -> f32 {
    estimate_pollution_ratio(record)
}

pub(super) fn generate_search_aliases(record: &MemoryRecord) -> Vec<String> {
    let mut aliases: HashSet<String> = HashSet::new();
    let push_alias = |aliases: &mut HashSet<String>, value: String| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return;
        }
        aliases.insert(trimmed.to_ascii_lowercase());
    };

    // Phrase-based sources keep aliases anchored to noun-phrases.
    for phrase in alias_noun_phrase_sources(record) {
        push_alias(&mut aliases, phrase.clone());
        let underscored = phrase.replace('_', " ");
        if underscored != phrase {
            push_alias(&mut aliases, underscored);
        }
        let dashed = phrase.replace('-', " ");
        if dashed != phrase {
            push_alias(&mut aliases, dashed);
        }
        if let Some(acronym) = acronym_for_phrase(&phrase) {
            push_alias(&mut aliases, acronym);
        }
        // Compact form (no spaces) only when the phrase is ≤3 tokens to avoid
        // generating opaque alphabet soup from long sentences.
        let token_count = phrase.split_whitespace().count();
        if token_count > 0 && token_count <= 3 {
            let compact = normalize_keyword_text(&phrase).replace(' ', "");
            if compact.len() >= 3 {
                push_alias(&mut aliases, compact);
            }
        }
    }

    // Explicit names — entities, files, tags, related tools — surface as-is
    // but never get acronymized, which is the source of the historical
    // `df`/`lco`/`mce` noise.
    for value in record
        .entities
        .iter()
        .chain(record.tags.iter())
        .chain(record.files_touched.iter())
        .chain(record.related_tools.iter())
    {
        let base = value.trim();
        if base.is_empty() {
            continue;
        }
        push_alias(&mut aliases, base.to_string());
        let underscored = base.replace('_', " ");
        if underscored != base {
            push_alias(&mut aliases, underscored);
        }
        let dashed = base.replace('-', " ");
        if dashed != base {
            push_alias(&mut aliases, dashed);
        }
    }

    let mut out = aliases.into_iter().collect::<Vec<_>>();
    out.sort();
    out.truncate(24);
    out
}

pub(super) fn build_raw_evidence_payload(record: &MemoryRecord) -> String {
    serde_json::json!({
        "timestamp_start": record.timestamp_start,
        "timestamp_end": record.timestamp_end,
        "app_name": record.app_name,
        "window_title": record.window_title,
        "url": record.url,
        "source_type": record.source_type,
        "ocr_confidence": record.ocr_confidence,
        "noise_score": record.noise_score,
        "clean_text_excerpt": trim_chars(&record.clean_text, 400),
    })
    .to_string()
}

pub(super) fn derive_structured_entities(record: &MemoryRecord) -> Vec<crate::store::ExtractedEntity> {
    let mut entities = Vec::new();
    let now = chrono::Utc::now().timestamp_millis();
    let base_evidence = vec![format!("memory_id={}", record.id), format!("ts={}", now)];

    for entity in dedup_strings(record.entities.clone()) {
        entities.push(crate::store::ExtractedEntity {
            text: entity.clone(),
            normalized_name: normalize_keyword_text(&entity),
            entity_type: "entity".to_string(),
            confidence: 0.72,
            source: "memory_context".to_string(),
            evidence: base_evidence.clone(),
            aliases: vec![entity.to_ascii_lowercase()],
            related_entity_ids: Vec::new(),
        });
    }
    for file in dedup_strings(record.files_touched.clone()) {
        entities.push(crate::store::ExtractedEntity {
            text: file.clone(),
            normalized_name: normalize_keyword_text(&file),
            entity_type: "file".to_string(),
            confidence: 0.86,
            source: "path".to_string(),
            evidence: vec!["detected file path".to_string()],
            aliases: vec![basename(&file)],
            related_entity_ids: Vec::new(),
        });
    }
    if let Some(url) = record.url.as_ref() {
        entities.push(crate::store::ExtractedEntity {
            text: url.clone(),
            normalized_name: normalize_keyword_text(url),
            entity_type: "url".to_string(),
            confidence: 0.9,
            source: "url".to_string(),
            evidence: vec!["record url metadata".to_string()],
            aliases: extract_domain(url).into_iter().collect(),
            related_entity_ids: Vec::new(),
        });
    }
    entities
}

pub(super) fn derive_action_items(record: &MemoryRecord) -> Vec<crate::store::MemoryActionItem> {
    let now = chrono::Utc::now().timestamp_millis();
    let mut items = Vec::new();
    for decision in dedup_strings(record.decisions.clone()) {
        items.push(build_action_item(
            "decision", decision, "observed", 0.8, &record.id, now,
        ));
    }
    for err in dedup_strings(record.errors.clone()) {
        items.push(build_action_item(
            "error", err, "observed", 0.85, &record.id, now,
        ));
    }
    for blocker in dedup_strings(record.blockers.clone()) {
        items.push(build_action_item(
            "blocker", blocker, "inferred", 0.68, &record.id, now,
        ));
    }
    for todo in dedup_strings(record.todos.clone()) {
        items.push(build_action_item(
            "todo", todo, "observed", 0.78, &record.id, now,
        ));
    }
    for todo in dedup_strings(record.next_steps.clone()) {
        items.push(build_action_item(
            "todo", todo, "inferred", 0.66, &record.id, now,
        ));
    }
    for result in dedup_strings(record.results.clone()) {
        items.push(build_action_item(
            "result", result, "observed", 0.74, &record.id, now,
        ));
    }
    items
}

pub(super) fn build_action_item(
    kind: &str,
    text: String,
    status: &str,
    confidence: f32,
    memory_id: &str,
    now: i64,
) -> crate::store::MemoryActionItem {
    crate::store::MemoryActionItem {
        kind: kind.to_string(),
        text: trim_chars(text.trim(), 220),
        confidence: confidence.clamp(0.0, 1.0),
        status: status.to_string(),
        evidence: vec![format!("source_memory={memory_id}")],
        source_memory_id: memory_id.to_string(),
        related_entities: Vec::new(),
        related_files: Vec::new(),
        related_urls: Vec::new(),
        created_at: now,
        updated_at: now,
    }
}

pub(super) fn estimate_topic_confidence(record: &MemoryRecord) -> f32 {
    let mut score: f32 = 0.18;
    if !record.topic.trim().is_empty() && record.topic != "unknown" {
        score += 0.22;
    }
    if !record.project.trim().is_empty() {
        score += 0.20;
    }
    if record.url.is_some() {
        score += 0.14;
    }
    if !record.files_touched.is_empty() {
        score += 0.18;
    }
    score.clamp(0.0, 1.0)
}

pub(super) fn estimate_workflow_confidence(record: &MemoryRecord) -> f32 {
    let mut score: f32 = 0.2;
    if !record.workflow.trim().is_empty() && record.workflow != "unknown" {
        score += 0.22;
    }
    score += record.intent_analysis.confidence * 0.36;
    if !record.commands.is_empty() || !record.errors.is_empty() {
        score += 0.18;
    }
    score.clamp(0.0, 1.0)
}

pub(super) fn estimate_project_confidence(record: &MemoryRecord) -> f32 {
    let mut score: f32 = 0.12;
    if !record.project.trim().is_empty() {
        score += 0.30;
    }
    if !record.files_touched.is_empty() {
        score += 0.22;
    }
    if record.url.is_some() {
        score += 0.14;
    }
    if !record.related_projects.is_empty() {
        score += 0.14;
    }
    score.clamp(0.0, 1.0)
}

pub(super) fn estimate_specificity_score(record: &MemoryRecord) -> f32 {
    let context_tokens = normalize_keyword_text(&record.memory_context)
        .split_whitespace()
        .count()
        .min(120) as f32
        / 120.0;
    let artifact_count = (record.files_touched.len()
        + record.entities.len()
        + record.decisions.len()
        + record.errors.len()
        + record.next_steps.len()) as f32;
    let artifact_score = (artifact_count / 16.0).clamp(0.0, 1.0);
    (context_tokens * 0.55 + artifact_score * 0.45).clamp(0.0, 1.0)
}

pub(super) fn estimate_entity_score(record: &MemoryRecord) -> f32 {
    let count = (record.entities.len()
        + record.files_touched.len()
        + usize::from(record.url.is_some())) as f32;
    (count / 12.0).clamp(0.0, 1.0)
}

pub(super) fn estimate_importance_score(record: &MemoryRecord) -> f32 {
    let mut score: f32 = 0.25;
    if !record.errors.is_empty() {
        score += 0.22;
    }
    if !record.decisions.is_empty() {
        score += 0.2;
    }
    if !record.next_steps.is_empty() || !record.todos.is_empty() {
        score += 0.18;
    }
    if !record.project.trim().is_empty() {
        score += 0.14;
    }
    score.clamp(0.0, 1.0)
}

/// Top-k span concentration on `clean_text`. Higher means a few dense spans
/// carry the document's signal — a strong indicator that retrieval against
/// this record will surface meaningful matches.
pub(super) fn estimate_salience_concentration(record: &MemoryRecord) -> f32 {
    crate::capture::text_cleanup::salience_concentration(&record.clean_text, &record.app_name)
}

/// Crude topic-clarity signal: non-empty/non-"unknown" topic with at least
/// some token presence in `memory_context` or `entities`. Range [0, 1].
pub(super) fn estimate_topic_clarity(record: &MemoryRecord) -> f32 {
    let topic = record.topic.trim();
    if topic.is_empty() || topic.eq_ignore_ascii_case("unknown") {
        return 0.0;
    }
    let topic_norm = normalize_keyword_text(topic);
    let topic_tokens: HashSet<&str> = topic_norm
        .split_whitespace()
        .filter(|t| t.len() >= 3)
        .collect();
    if topic_tokens.is_empty() {
        return 0.25;
    }
    let context_norm = normalize_keyword_text(&record.memory_context);
    let entities_norm = normalize_keyword_text(&record.entities.join(" "));
    let mut hits = 0usize;
    for token in &topic_tokens {
        if context_norm.contains(token) || entities_norm.contains(token) {
            hits += 1;
        }
    }
    let coverage = hits as f32 / topic_tokens.len() as f32;
    (0.30 + coverage * 0.70).clamp(0.0, 1.0)
}

/// Pollution ratio: blends OCR noise with the inverse of salience
/// concentration. Higher → noisier / less useful for retrieval.
pub(super) fn estimate_pollution_ratio(record: &MemoryRecord) -> f32 {
    let noise = record.ocr_noise_score.clamp(0.0, 1.0);
    let concentration = estimate_salience_concentration(record);
    let diffusion = (1.0 - concentration).clamp(0.0, 1.0);
    ((noise * 0.55) + (diffusion * 0.45)).clamp(0.0, 1.0)
}

pub(super) fn estimate_agent_usefulness_score(record: &MemoryRecord) -> f32 {
    let mut score: f32 = 0.20;
    if !record.memory_context.trim().is_empty() {
        score += 0.22;
    }
    score += estimate_specificity_score(record) * 0.24;
    score += estimate_entity_score(record) * 0.18;
    score += record.intent_analysis.confidence * 0.16;
    score += record.evidence_confidence * 0.10;
    score -= record.noise_score.clamp(0.0, 1.0) * 0.14;
    score.clamp(0.0, 1.0)
}

pub(super) fn basename(path: &str) -> String {
    path.replace('\\', "/")
        .rsplit('/')
        .find(|segment| !segment.trim().is_empty())
        .unwrap_or(path)
        .to_string()
}

pub(super) fn dedup_strings(values: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        let key = trimmed.to_ascii_lowercase();
        if seen.insert(key) {
            out.push(trimmed.to_string());
        }
    }
    out
}

pub(super) fn is_indexable_memory_record(record: &MemoryRecord) -> bool {
    let text_signal = format!(
        "{} {} {} {}",
        record.clean_text, record.snippet, record.memory_context, record.lexical_shadow
    );
    let alnum = text_signal
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .count();
    if alnum < 6 {
        tracing::debug!(
            memory_id = %record.id,
            "memory_record:skip_low_signal_text"
        );
        return false;
    }
    true
}

pub(super) fn normalize_vector_dim(id: &str, field: &str, vector: &[f32], expected_dim: usize) -> Vec<f32> {
    if vector.len() == expected_dim {
        return vector.to_vec();
    }

    tracing::warn!(
        memory_id = %id,
        field,
        actual_dim = vector.len(),
        expected_dim,
        "memory_record:vector_dimension_mismatch"
    );

    if vector.is_empty() || vector.iter().all(|value| *value == 0.0) {
        return vec![0.0; expected_dim];
    }

    let mut repaired = vec![0.0; expected_dim];
    let copy_len = vector.len().min(expected_dim);
    repaired[..copy_len].copy_from_slice(&vector[..copy_len]);
    repaired
}

pub(super) fn sanitize_index_url(url: Option<&str>, title: &str, snippet: &str) -> Option<String> {
    let raw = url?.trim();
    if raw.is_empty() {
        return None;
    }

    let normalized = canonicalize_index_url(raw);
    let domain = extract_domain(&normalized)?;
    let context = normalize_keyword_text(&format!("{title} {snippet}"));
    let path = extract_path_segments(&normalized, 3).unwrap_or_default();

    if INDEX_NOISE_HOSTS.iter().any(|host| domain == *host) {
        return None;
    }
    if looks_like_auth_or_error_context(&context) {
        return None;
    }
    if !path.is_empty() && is_low_entropy_path(&path) && context.split_whitespace().count() < 6 {
        return None;
    }

    Some(normalized)
}

pub(super) fn build_index_session_key(record: &MemoryRecord) -> String {
    if record.session_key.starts_with("meeting:") {
        return record.session_key.clone();
    }

    let app = normalize_app_key(&record.app_name);
    if let Some(url) = record.url.as_deref() {
        if let Some(domain) = extract_domain(url) {
            if let Some(path) = extract_path_segments(url, 2) {
                if !path.is_empty() {
                    return format!("{app}:{domain}:{path}");
                }
            }
            return format!("{app}:{domain}");
        }
    }

    let title_key = normalize_anchor_key(&record.window_title);
    if !title_key.is_empty() {
        return format!("{app}:title:{title_key}");
    }

    let snippet_key = normalize_anchor_key(&record.snippet);
    if !snippet_key.is_empty() {
        return format!("{app}:snippet:{snippet_key}");
    }

    if !record.session_key.trim().is_empty() {
        return record.session_key.clone();
    }

    app
}

pub(super) fn normalize_app_key(app_name: &str) -> String {
    let normalized = app_name
        .to_ascii_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect::<String>();
    let compact = normalized
        .split('_')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    if compact.is_empty() {
        "unknown".to_string()
    } else {
        compact
    }
}

pub(super) fn normalize_anchor_key(text: &str) -> String {
    normalize_keyword_text(text)
        .split_whitespace()
        .filter(|token| token.len() > 2)
        .take(8)
        .collect::<Vec<_>>()
        .join("_")
}

pub(super) fn extract_path_segments(url: &str, count: usize) -> Option<String> {
    let without_scheme = url.split("://").nth(1).unwrap_or(url);
    let mut parts = without_scheme.split('/');
    let _host = parts.next()?;
    let segments = parts
        .filter(|segment| !segment.trim().is_empty())
        .map(|segment| {
            normalize_keyword_text(segment)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join("_")
        })
        .filter(|segment| !segment.is_empty())
        .take(count)
        .collect::<Vec<_>>();

    if segments.is_empty() {
        None
    } else {
        Some(segments.join("/"))
    }
}

pub(super) fn is_low_entropy_path(path: &str) -> bool {
    let normalized = normalize_keyword_text(path);
    let tokens = normalized
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    if tokens.is_empty() {
        return true;
    }

    let unique = tokens
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>()
        .len();
    unique <= 2
        && tokens.iter().all(|token| {
            matches!(
                *token,
                "404" | "500" | "account" | "auth" | "error" | "login" | "signin"
            )
        })
}

pub(super) fn looks_like_auth_or_error_context(context: &str) -> bool {
    context.contains("sign in")
        || context.contains("log in")
        || context.contains("authenticate")
        || context.contains("authorization")
        || context.contains("404")
        || context.contains("500")
        || context.contains("not found")
        || context.starts_with("error ")
}

pub(super) fn dedup_records_for_insert(records: &[MemoryRecord]) -> Vec<MemoryRecord> {
    let mut by_key: HashMap<String, MemoryRecord> = HashMap::new();

    for record in records {
        let key = record_insert_dedup_key(record);
        by_key
            .entry(key)
            .and_modify(|existing| {
                let existing_rank = estimate_record_signal_strength(existing);
                let incoming_rank = estimate_record_signal_strength(record);
                if incoming_rank > existing_rank
                    || (incoming_rank == existing_rank && record.timestamp > existing.timestamp)
                {
                    *existing = record.clone();
                }
            })
            .or_insert_with(|| record.clone());
    }

    by_key.into_values().collect()
}

pub(super) fn dedup_search_results(mut results: Vec<SearchResult>, limit: usize) -> Vec<SearchResult> {
    if results.is_empty() {
        return results;
    }

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.timestamp.cmp(&a.timestamp))
    });

    let mut by_key: HashMap<String, SearchResult> = HashMap::new();
    for result in results {
        let key = search_result_dedup_key(&result);
        by_key
            .entry(key)
            .and_modify(|existing| {
                if result.score > existing.score
                    || (result.score == existing.score && result.timestamp > existing.timestamp)
                {
                    *existing = result.clone();
                }
            })
            .or_insert(result);
    }

    let mut out: Vec<SearchResult> = by_key.into_values().collect();
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.timestamp.cmp(&a.timestamp))
    });
    out.truncate(limit.max(1));
    out
}

pub(super) fn record_insert_dedup_key(record: &MemoryRecord) -> String {
    if !record.content_hash.trim().is_empty() {
        return record.content_hash.trim().to_string();
    }
    compute_content_hash(
        record.url.as_deref(),
        &record.window_title,
        record.timestamp,
    )
}

pub(super) fn search_result_dedup_key(result: &SearchResult) -> String {
    if !result.content_hash.trim().is_empty() {
        return result.content_hash.trim().to_string();
    }
    compute_content_hash(
        result.url.as_deref(),
        &result.window_title,
        result.timestamp,
    )
}

/// Escape single quotes for SQL string literals.
pub(super) fn sql_escape(s: &str) -> String {
    s.replace('\'', "''")
}

// ── DB initialization ─────────────────────────────────────────────────────────

pub(super) async fn open_all_tables(
    db_path: &Path,
) -> Result<
    (
        Table,
        Option<Table>,
        Table,
        Table,
        Table,
        Table,
        Table,
        Table,
        Table,
        Table,
        Table,
        Table,
        Table,
        Table,
    ),
    lancedb::Error,
> {
    let uri = db_path.to_string_lossy();
    let conn: Connection = lancedb::connect(&uri).execute().await?;
    let names = conn.table_names().execute().await?;

    let (table, legacy_table) = if names.contains(&"memories_v2_1024".to_string()) {
        let table = conn.open_table("memories_v2_1024").execute().await?;
        let legacy_table = if names.contains(&MEMORIES_TABLE.to_string()) {
            Some(conn.open_table(MEMORIES_TABLE).execute().await?)
        } else {
            None
        };
        (table, legacy_table)
    } else if names.contains(&MEMORIES_TABLE.to_string()) {
        let existing = conn.open_table(MEMORIES_TABLE).execute().await?;
        let schema = existing.schema().await?;
        let is_384 = schema.fields().iter().any(|f| {
            if f.name() == "embedding" {
                if let DataType::FixedSizeList(_, dim) = f.data_type() {
                    return *dim == 384;
                }
            }
            false
        });

        if is_384 && TEXT_EMBED_DIM == 1024 {
            let table = open_or_create_named_table(
                &conn,
                &names,
                "memories_v2_1024",
                Arc::new(memory_schema()),
            )
            .await?;
            (table, Some(existing))
        } else {
            (existing, None)
        }
    } else {
        (
            open_or_create_named_table(&conn, &names, MEMORIES_TABLE, Arc::new(memory_schema()))
                .await?,
            None,
        )
    };
    ensure_memory_schema_columns(&table).await?;

    let tasks =
        open_or_create_named_table(&conn, &names, TASKS_TABLE, Arc::new(task_schema())).await?;
    let meetings =
        open_or_create_named_table(&conn, &names, MEETINGS_TABLE, Arc::new(meeting_schema()))
            .await?;
    let segments =
        open_or_create_named_table(&conn, &names, SEGMENTS_TABLE, Arc::new(segment_schema()))
            .await?;
    let nodes =
        open_or_create_named_table(&conn, &names, NODES_TABLE, Arc::new(node_schema())).await?;
    let edges =
        open_or_create_named_table(&conn, &names, EDGES_TABLE, Arc::new(edge_schema())).await?;
    let activity_events = open_or_create_named_table(
        &conn,
        &names,
        ACTIVITY_EVENTS_TABLE,
        Arc::new(activity_event_schema()),
    )
    .await?;
    let project_contexts = open_or_create_named_table(
        &conn,
        &names,
        PROJECT_CONTEXTS_TABLE,
        Arc::new(project_context_schema()),
    )
    .await?;
    let decision_ledger = open_or_create_named_table(
        &conn,
        &names,
        DECISION_LEDGER_TABLE,
        Arc::new(decision_ledger_schema()),
    )
    .await?;
    let context_packs = open_or_create_named_table(
        &conn,
        &names,
        CONTEXT_PACKS_TABLE,
        Arc::new(context_pack_schema()),
    )
    .await?;
    let context_deltas = open_or_create_named_table(
        &conn,
        &names,
        CONTEXT_DELTAS_TABLE,
        Arc::new(context_delta_schema()),
    )
    .await?;
    let entity_aliases = open_or_create_named_table(
        &conn,
        &names,
        ENTITY_ALIASES_TABLE,
        Arc::new(entity_alias_schema()),
    )
    .await?;
    let knowledge_pages = open_or_create_named_table(
        &conn,
        &names,
        KNOWLEDGE_PAGES_TABLE,
        Arc::new(knowledge_page_schema()),
    )
    .await?;

    Ok((
        table,
        legacy_table,
        tasks,
        meetings,
        segments,
        nodes,
        edges,
        activity_events,
        project_contexts,
        decision_ledger,
        context_packs,
        context_deltas,
        entity_aliases,
        knowledge_pages,
    ))
}

pub(super) async fn open_or_create_named_table(
    conn: &Connection,
    existing_tables: &[String],
    name: &str,
    schema: Arc<Schema>,
) -> Result<Table, lancedb::Error> {
    if existing_tables.contains(&name.to_string()) {
        conn.open_table(name).execute().await
    } else {
        let empty = RecordBatchIterator::new(
            std::iter::empty::<Result<RecordBatch, ArrowError>>(),
            schema,
        );
        conn.create_table(name, Box::new(empty) as Box<dyn RecordBatchReader + Send>)
            .execute()
            .await
    }
}

pub(super) async fn ensure_memory_schema_columns(table: &Table) -> Result<(), lancedb::Error> {
    let schema = table.schema().await?;
    let existing: std::collections::HashSet<String> = schema
        .fields()
        .iter()
        .map(|field| field.name().to_string())
        .collect();

    let mut transforms: Vec<(String, String)> = Vec::new();
    if !existing.contains("clean_text") {
        transforms.push(("clean_text".to_string(), "text".to_string()));
    }
    if !existing.contains("ocr_confidence") {
        transforms.push((
            "ocr_confidence".to_string(),
            "CAST(0.0 AS FLOAT)".to_string(),
        ));
    }
    if !existing.contains("ocr_block_count") {
        transforms.push((
            "ocr_block_count".to_string(),
            "CAST(0 AS BIGINT)".to_string(),
        ));
    }
    if !existing.contains("summary_source") {
        transforms.push(("summary_source".to_string(), "'fallback'".to_string()));
    }
    if !existing.contains("display_summary") {
        transforms.push(("display_summary".to_string(), "snippet".to_string()));
    }
    if !existing.contains("internal_context") {
        transforms.push(("internal_context".to_string(), "clean_text".to_string()));
    }
    if !existing.contains("noise_score") {
        transforms.push(("noise_score".to_string(), "CAST(0.0 AS FLOAT)".to_string()));
    }
    if !existing.contains("session_key") {
        transforms.push(("session_key".to_string(), "''".to_string()));
    }
    if !existing.contains("lexical_shadow") {
        transforms.push(("lexical_shadow".to_string(), "''".to_string()));
    }
    if !existing.contains("snippet_embedding") {
        // Placeholder zeros — will be computed properly for new captures.
        transforms.push(("snippet_embedding".to_string(), "embedding".to_string()));
    }
    if !existing.contains("support_embedding") {
        transforms.push(("support_embedding".to_string(), "embedding".to_string()));
    }
    if !existing.contains("decay_score") {
        transforms.push(("decay_score".to_string(), "CAST(1.0 AS FLOAT)".to_string()));
    }
    if !existing.contains("last_accessed_at") {
        transforms.push(("last_accessed_at".to_string(), "timestamp".to_string()));
    }
    if !existing.contains("timestamp_start") {
        transforms.push(("timestamp_start".to_string(), "timestamp".to_string()));
    }
    if !existing.contains("timestamp_end") {
        transforms.push(("timestamp_end".to_string(), "timestamp".to_string()));
    }
    if !existing.contains("source_type") {
        transforms.push(("source_type".to_string(), "'screen'".to_string()));
    }
    if !existing.contains("topic") {
        transforms.push(("topic".to_string(), "'unknown'".to_string()));
    }
    if !existing.contains("workflow") {
        transforms.push(("workflow".to_string(), "'unknown'".to_string()));
    }
    if !existing.contains("user_intent") {
        transforms.push(("user_intent".to_string(), "''".to_string()));
    }
    if !existing.contains("intent_analysis") {
        transforms.push(("intent_analysis".to_string(), "'{}'".to_string()));
    }
    if !existing.contains("memory_context") {
        transforms.push(("memory_context".to_string(), "display_summary".to_string()));
    }
    if !existing.contains("commands") {
        transforms.push(("commands".to_string(), "[]".to_string()));
    }
    if !existing.contains("blockers") {
        transforms.push(("blockers".to_string(), "[]".to_string()));
    }
    if !existing.contains("todos") {
        transforms.push(("todos".to_string(), "[]".to_string()));
    }
    if !existing.contains("open_questions") {
        transforms.push(("open_questions".to_string(), "[]".to_string()));
    }
    if !existing.contains("results") {
        transforms.push(("results".to_string(), "[]".to_string()));
    }
    if !existing.contains("related_tools") {
        transforms.push(("related_tools".to_string(), "[]".to_string()));
    }
    if !existing.contains("related_agents") {
        transforms.push(("related_agents".to_string(), "[]".to_string()));
    }
    if !existing.contains("related_projects") {
        transforms.push(("related_projects".to_string(), "[]".to_string()));
    }
    if !existing.contains("raw_evidence") {
        transforms.push(("raw_evidence".to_string(), "'{}'".to_string()));
    }
    if !existing.contains("search_aliases") {
        transforms.push(("search_aliases".to_string(), "[]".to_string()));
    }
    if !existing.contains("related_memory_ids") {
        transforms.push(("related_memory_ids".to_string(), "[]".to_string()));
    }
    if !existing.contains("graph_node_ids") {
        transforms.push(("graph_node_ids".to_string(), "[]".to_string()));
    }
    if !existing.contains("graph_edge_ids") {
        transforms.push(("graph_edge_ids".to_string(), "[]".to_string()));
    }
    if !existing.contains("project_confidence") {
        transforms.push((
            "project_confidence".to_string(),
            "CAST(0.0 AS FLOAT)".to_string(),
        ));
    }
    if !existing.contains("topic_confidence") {
        transforms.push((
            "topic_confidence".to_string(),
            "CAST(0.0 AS FLOAT)".to_string(),
        ));
    }
    if !existing.contains("workflow_confidence") {
        transforms.push((
            "workflow_confidence".to_string(),
            "CAST(0.0 AS FLOAT)".to_string(),
        ));
    }
    if !existing.contains("project_evidence") {
        transforms.push(("project_evidence".to_string(), "[]".to_string()));
    }
    if !existing.contains("related_project_ids") {
        transforms.push(("related_project_ids".to_string(), "[]".to_string()));
    }
    if !existing.contains("evidence_confidence") {
        transforms.push((
            "evidence_confidence".to_string(),
            "CAST(0.0 AS FLOAT)".to_string(),
        ));
    }
    if !existing.contains("confidence_score") {
        transforms.push((
            "confidence_score".to_string(),
            "CAST(0.0 AS FLOAT)".to_string(),
        ));
    }
    if !existing.contains("importance_score") {
        transforms.push((
            "importance_score".to_string(),
            "CAST(0.0 AS FLOAT)".to_string(),
        ));
    }
    if !existing.contains("specificity_score") {
        transforms.push((
            "specificity_score".to_string(),
            "CAST(0.0 AS FLOAT)".to_string(),
        ));
    }
    if !existing.contains("intent_score") {
        transforms.push(("intent_score".to_string(), "CAST(0.0 AS FLOAT)".to_string()));
    }
    if !existing.contains("entity_score") {
        transforms.push(("entity_score".to_string(), "CAST(0.0 AS FLOAT)".to_string()));
    }
    if !existing.contains("agent_usefulness_score") {
        transforms.push((
            "agent_usefulness_score".to_string(),
            "CAST(0.0 AS FLOAT)".to_string(),
        ));
    }
    if !existing.contains("ocr_noise_score") {
        transforms.push(("ocr_noise_score".to_string(), "noise_score".to_string()));
    }
    if !existing.contains("graph_readiness_score") {
        transforms.push((
            "graph_readiness_score".to_string(),
            "CAST(0.0 AS FLOAT)".to_string(),
        ));
    }
    if !existing.contains("retrieval_value_score") {
        transforms.push((
            "retrieval_value_score".to_string(),
            "CAST(0.0 AS FLOAT)".to_string(),
        ));
    }
    if !existing.contains("storage_outcome") {
        transforms.push((
            "storage_outcome".to_string(),
            "'enriched_memory_card'".to_string(),
        ));
    }
    if !existing.contains("quality_gate_reason") {
        transforms.push(("quality_gate_reason".to_string(), "''".to_string()));
    }
    if !existing.contains("extracted_entities_structured") {
        transforms.push((
            "extracted_entities_structured".to_string(),
            "'[]'".to_string(),
        ));
    }
    if !existing.contains("action_items") {
        transforms.push(("action_items".to_string(), "'[]'".to_string()));
    }
    if !existing.contains("anchor_coverage_score") {
        transforms.push((
            "anchor_coverage_score".to_string(),
            "CAST(0.0 AS FLOAT)".to_string(),
        ));
    }
    if !existing.contains("content_hash") {
        transforms.push(("content_hash".to_string(), "''".to_string()));
    }

    if !transforms.is_empty() {
        tracing::info!(
            "Migrating LanceDB memories table schema with {} new columns",
            transforms.len()
        );
        table
            .add_columns(NewColumnTransform::SqlExpressions(transforms), None)
            .await?;
    }

    validate_memory_vector_schema(table).await?;

    Ok(())
}

pub(super) async fn validate_memory_vector_schema(table: &Table) -> Result<(), lancedb::Error> {
    let schema = table.schema().await?;
    for (column, expected_dim) in [
        ("embedding", TEXT_EMBED_DIM),
        ("snippet_embedding", TEXT_EMBED_DIM),
        ("support_embedding", TEXT_EMBED_DIM),
        ("image_embedding", IMAGE_EMBED_DIM),
    ] {
        let Some(field) = schema.field_with_name(column).ok() else {
            continue;
        };
        let actual_dim = fixed_size_list_dim(field.data_type());
        if actual_dim != Some(expected_dim) {
            return Err(lancedb::Error::Schema {
                message: format!(
                    "LanceDB table '{}' column '{}' has vector dimension {:?}, but FNDR is configured for {}. Existing 384-dimensional tables must be migrated or reset before using the 1024-dimensional embedding path. To reset local prototype data, stop FNDR and remove the app data LanceDB directory.",
                    MEMORIES_TABLE,
                    column,
                    actual_dim,
                    expected_dim
                ),
            });
        }
    }
    Ok(())
}

pub(super) fn fixed_size_list_dim(data_type: &DataType) -> Option<i32> {
    match data_type {
        DataType::FixedSizeList(_, dim) => Some(*dim),
        _ => None,
    }
}

// ── Migration from legacy JSON store ─────────────────────────────────────────

pub(super) async fn migrate_from_json(table: &Table, json_path: &Path) {
    let result: Result<(), Box<dyn std::error::Error>> = (async {
        let data = std::fs::read(json_path)?;
        let mut records: Vec<MemoryRecord> = serde_json::from_slice(&data)?;
        if records.is_empty() {
            return Ok(());
        }

        // Backfill day_bucket for legacy records that predate the field.
        for r in &mut records {
            if r.day_bucket.is_empty() {
                r.day_bucket = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(r.timestamp)
                    .unwrap_or_else(chrono::Utc::now)
                    .format("%Y-%m-%d")
                    .to_string();
            }
        }
        records = records.iter().map(normalize_record_for_index).collect();
        records = dedup_records_for_insert(&records);

        tracing::info!(
            "Migrating {} records from memories.json to LanceDB",
            records.len()
        );

        // Insert in chunks to avoid huge Arrow batches.
        for chunk in records.chunks(500) {
            let batch = records_to_batch(chunk)?;
            let schema = Arc::new(memory_schema());
            let iter = RecordBatchIterator::new(vec![Ok(batch)], schema);
            table
                .add(Box::new(iter) as Box<dyn RecordBatchReader + Send>)
                .execute()
                .await?;
        }

        // Remove the legacy JSON source once migration has completed successfully.
        std::fs::remove_file(json_path)?;

        tracing::info!("Migration complete");
        Ok(())
    })
    .await;

    if let Err(e) = result {
        tracing::warn!("JSON migration failed (data not lost): {}", e);
    }
}
pub(super) async fn migrate_tasks_from_json(table: &Table, json_path: &Path) {
    let result: Result<(), Box<dyn std::error::Error>> = (async {
        let data = std::fs::read(json_path)?;
        let tasks: Vec<Task> = serde_json::from_slice(&data)?;
        if tasks.is_empty() {
            return Ok(());
        }
        tracing::info!("Migrating {} tasks to LanceDB", tasks.len());
        let batch = task_to_batch(&tasks)?;
        let schema = Arc::new(task_schema());
        let iter = RecordBatchIterator::new(vec![Ok(batch)], schema);
        table
            .add(Box::new(iter) as Box<dyn RecordBatchReader + Send>)
            .mode(AddDataMode::Overwrite)
            .execute()
            .await?;
        std::fs::remove_file(json_path)?;
        Ok(())
    })
    .await;
    if let Err(e) = result {
        tracing::warn!("Task migration failed: {}", e);
    }
}

pub(super) async fn migrate_meetings_from_json(table: &Table, json_path: &Path) {
    let result: Result<(), Box<dyn std::error::Error>> = (async {
        let data = std::fs::read(json_path)?;
        let meetings: Vec<MeetingSession> = serde_json::from_slice(&data)?;
        if meetings.is_empty() {
            return Ok(());
        }
        tracing::info!("Migrating {} meetings to LanceDB", meetings.len());
        let batch = meeting_to_batch(&meetings)?;
        let schema = Arc::new(meeting_schema());
        let iter = RecordBatchIterator::new(vec![Ok(batch)], schema);
        table
            .add(Box::new(iter) as Box<dyn RecordBatchReader + Send>)
            .mode(AddDataMode::Overwrite)
            .execute()
            .await?;
        std::fs::remove_file(json_path)?;
        Ok(())
    })
    .await;
    if let Err(e) = result {
        tracing::warn!("Meeting migration failed: {}", e);
    }
}

pub(super) async fn migrate_segments_from_json(table: &Table, json_path: &Path) {
    let result: Result<(), Box<dyn std::error::Error>> = (async {
        let data = std::fs::read(json_path)?;
        let segments: Vec<MeetingSegment> = serde_json::from_slice(&data)?;
        if segments.is_empty() {
            return Ok(());
        }
        tracing::info!("Migrating {} segments to LanceDB", segments.len());
        let batch = segment_to_batch(&segments)?;
        let schema = Arc::new(segment_schema());
        let iter = RecordBatchIterator::new(vec![Ok(batch)], schema);
        table
            .add(Box::new(iter) as Box<dyn RecordBatchReader + Send>)
            .mode(AddDataMode::Overwrite)
            .execute()
            .await?;
        std::fs::remove_file(json_path)?;
        Ok(())
    })
    .await;
    if let Err(e) = result {
        tracing::warn!("Segment migration failed: {}", e);
    }
}

pub(super) async fn migrate_graph_from_json(nodes_table: &Table, edges_table: &Table, json_path: &Path) {
    #[derive(serde::Deserialize)]
    struct LegacyGraph {
        nodes: Vec<GraphNode>,
        edges: Vec<GraphEdge>,
    }

    let result: Result<(), Box<dyn std::error::Error>> = (async {
        let data = std::fs::read(json_path)?;
        let graph: LegacyGraph = serde_json::from_slice(&data)?;
        if !graph.nodes.is_empty() {
            tracing::info!("Migrating {} graph nodes to LanceDB", graph.nodes.len());
            let batch = node_to_batch(&graph.nodes)?;
            let schema = Arc::new(node_schema());
            let iter = RecordBatchIterator::new(vec![Ok(batch)], schema);
            nodes_table
                .add(Box::new(iter) as Box<dyn RecordBatchReader + Send>)
                .mode(AddDataMode::Overwrite)
                .execute()
                .await?;
        }
        if !graph.edges.is_empty() {
            tracing::info!("Migrating {} graph edges to LanceDB", graph.edges.len());
            let batch = edge_to_batch(&graph.edges)?;
            let schema = Arc::new(edge_schema());
            let iter = RecordBatchIterator::new(vec![Ok(batch)], schema);
            edges_table
                .add(Box::new(iter) as Box<dyn RecordBatchReader + Send>)
                .mode(AddDataMode::Overwrite)
                .execute()
                .await?;
        }
        std::fs::remove_file(json_path)?;
        Ok(())
    })
    .await;
    if let Err(e) = result {
        tracing::warn!("Graph migration failed: {}", e);
    }
}
