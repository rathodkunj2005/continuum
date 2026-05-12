//! Memory quality, debug inspector, rebuild, retrieval eval.

use super::common::truncate_chars;
use crate::context_runtime;
use crate::memory_quality::classify_storage_outcome;
use crate::search::QueryContext;
use crate::store::{MemoryRecord, SearchResult};
use crate::AppState;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Arc;
use tauri::State;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryScoreBreakdown {
    pub specificity: f32,
    pub intent: f32,
    pub entity: f32,
    pub usefulness: f32,
    pub evidence: f32,
    pub ocr_noise: f32,
    pub graph_readiness: f32,
    pub retrieval_value: f32,
    /// Top-k span concentration on clean_text — higher means a few dense spans
    /// carry the document's signal.
    #[serde(default)]
    pub salience_concentration: f32,
    /// Non-empty topic that meaningfully overlaps memory_context / entities.
    #[serde(default)]
    pub topic_clarity: f32,
    /// Composite of OCR noise + (1 − salience_concentration). Higher means
    /// the record is harder to retrieve usefully.
    #[serde(default)]
    pub pollution_ratio: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryGraphSnapshot {
    pub nodes: Vec<serde_json::Value>,
    pub edges: Vec<serde_json::Value>,
    pub weak_evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryDebugInspector {
    pub memory_id: String,
    pub memory_context: String,
    pub project: String,
    pub topic: String,
    pub workflow: String,
    pub activity_type: String,
    pub user_intent: String,
    pub entities: Vec<String>,
    pub actions: Vec<String>,
    pub quality_scores: MemoryScoreBreakdown,
    pub grounding_confidence: f32,
    pub extraction_issues: Vec<String>,
    pub ocr_quality_stats: OcrQualityStats,
    pub embedding_diagnostics: EmbeddingDiagnostics,
    pub embedding_text: String,
    pub search_aliases: Vec<String>,
    pub raw_ocr_evidence: serde_json::Value,
    pub graph: MemoryGraphSnapshot,
    pub storage_outcome: String,
    pub quality_gate_reason: String,
    pub query_match_reasons: Vec<String>,
    pub related_knowledge_pages: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OcrQualityStats {
    pub total_lines: usize,
    pub kept_lines: usize,
    pub low_conf_lines: usize,
    pub dropped_noise_lines: usize,
    pub dropped_low_signal_lines: usize,
    pub avg_line_score: f32,
    pub ocr_confidence: f32,
    pub ocr_blocks: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EmbeddingDiagnostics {
    pub structured_prefix_ratio: f32,
    pub evidence_tail_chars: usize,
    pub dominated_by_raw_ocr: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryQualityFlag {
    pub memory_id: String,
    pub summary: String,
    pub app_name: String,
    pub timestamp: i64,
    pub storage_outcome: String,
    pub issues: Vec<String>,
    pub scores: MemoryScoreBreakdown,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryValidationReport {
    pub generated_at: i64,
    pub lookback_minutes: u32,
    pub total_memories: usize,
    pub flagged_memories: Vec<MemoryQualityFlag>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RebuildMemoryPreview {
    pub memory_id: String,
    pub before_memory_context: String,
    pub after_memory_context: String,
    pub before_embedding_text: String,
    pub after_embedding_text: String,
    pub before_aliases: Vec<String>,
    pub after_aliases: Vec<String>,
    pub before_storage_outcome: String,
    pub after_storage_outcome: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RebuildMemoryContextReport {
    pub dry_run: bool,
    pub start: i64,
    pub end: i64,
    pub scanned: usize,
    pub changed: usize,
    pub previews: Vec<RebuildMemoryPreview>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RetrievalEvalCase {
    pub category: String,
    pub query: String,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RetrievalEvalRow {
    pub category: String,
    pub query: String,
    pub top_1_relevant: bool,
    pub top_5_relevant: bool,
    pub matched_by_semantic: bool,
    pub matched_by_prefix: bool,
    pub matched_by_fuzzy: bool,
    pub matched_by_ngram: bool,
    pub matched_by_graph: bool,
    pub matched_by_alias: bool,
    pub query_expansion_terms: Vec<String>,
    pub bad_match_reason: Option<String>,
    pub top_memory_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RetrievalEvalReport {
    pub generated_at: i64,
    pub total_queries: usize,
    pub top1_hits: usize,
    pub top5_hits: usize,
    pub rows: Vec<RetrievalEvalRow>,
}
fn normalize_debug_text(value: &str) -> String {
    value
        .to_ascii_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch.is_ascii_whitespace() {
                ch
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn dedupe_trimmed_strings(values: impl IntoIterator<Item = String>, limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        let key = normalize_debug_text(trimmed);
        if key.is_empty() || !seen.insert(key) {
            continue;
        }
        out.push(trimmed.to_string());
        if out.len() >= limit {
            break;
        }
    }
    out
}

fn raw_evidence_json(raw_evidence: &str) -> Option<Value> {
    let trimmed = raw_evidence.trim();
    if trimmed.is_empty() {
        return None;
    }
    serde_json::from_str::<Value>(trimmed).ok()
}

fn derive_grounding_confidence(memory: &MemoryRecord, evidence: Option<&Value>) -> f32 {
    evidence
        .and_then(|value| {
            value
                .get("extraction_grounding_confidence")
                .and_then(Value::as_f64)
                .map(|value| value as f32)
        })
        .unwrap_or(memory.extraction_confidence.clamp(0.0, 1.0))
        .clamp(0.0, 1.0)
}

fn derive_extraction_issues(evidence: Option<&Value>) -> Vec<String> {
    evidence
        .and_then(|value| value.get("extraction_issues"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .take(24)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn derive_ocr_quality_stats(memory: &MemoryRecord, evidence: Option<&Value>) -> OcrQualityStats {
    let quality = evidence
        .and_then(|value| value.get("ocr_quality"))
        .cloned()
        .unwrap_or(Value::Null);
    OcrQualityStats {
        total_lines: quality
            .get("total_lines")
            .and_then(Value::as_u64)
            .unwrap_or(memory.ocr_block_count as u64) as usize,
        kept_lines: quality
            .get("kept_lines")
            .and_then(Value::as_u64)
            .unwrap_or(memory.ocr_block_count as u64) as usize,
        low_conf_lines: quality
            .get("low_conf_lines")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize,
        dropped_noise_lines: quality
            .get("dropped_noise_lines")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize,
        dropped_low_signal_lines: quality
            .get("dropped_low_signal_lines")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize,
        avg_line_score: quality
            .get("avg_line_score")
            .and_then(Value::as_f64)
            .unwrap_or(0.0) as f32,
        ocr_confidence: memory.ocr_confidence,
        ocr_blocks: memory.ocr_block_count,
    }
}

fn derive_embedding_diagnostics(memory: &MemoryRecord) -> EmbeddingDiagnostics {
    let embedding_norm = normalize_debug_text(&memory.embedding_text);
    let clean_norm = normalize_debug_text(&memory.clean_text);
    let evidence_index = embedding_norm
        .find("evidence ")
        .unwrap_or(embedding_norm.len());
    let prefix = &embedding_norm[..evidence_index];
    let structured_markers = [
        "intent", "project", "topic", "workflow", "context", "entities", "files",
    ];
    let marker_hits = structured_markers
        .iter()
        .filter(|marker| prefix.contains(*marker))
        .count();
    let structured_prefix_ratio =
        (marker_hits as f32 / structured_markers.len() as f32).clamp(0.0, 1.0);
    let clean_prefix = clean_norm.chars().take(42).collect::<String>();
    let dominated = embedding_norm.len() > 60
        && !clean_prefix.is_empty()
        && embedding_norm.starts_with(&clean_prefix);
    EmbeddingDiagnostics {
        structured_prefix_ratio,
        evidence_tail_chars: embedding_norm
            .split("evidence:")
            .nth(1)
            .map(str::trim)
            .map(str::len)
            .unwrap_or(0),
        dominated_by_raw_ocr: dominated,
    }
}

fn is_vague_memory_context(value: &str) -> bool {
    let normalized = normalize_debug_text(value);
    if normalized.split_whitespace().count() < 10 {
        return true;
    }
    let vague_markers = [
        "user engaged with",
        "low confidence",
        "ui elements",
        "window and saw",
        "general browsing",
        "some activity",
    ];
    vague_markers
        .iter()
        .any(|marker| normalized.contains(marker))
}

fn compose_rebuild_embedding_text(record: &MemoryRecord) -> String {
    let mut parts = Vec::new();
    if !record.memory_context.trim().is_empty() {
        parts.push(format!("context: {}", record.memory_context.trim()));
    }
    if !record.user_intent.trim().is_empty() {
        parts.push(format!("intent: {}", record.user_intent.trim()));
    }
    if !record.project.trim().is_empty() {
        parts.push(format!("project: {}", record.project.trim()));
    }
    if !record.topic.trim().is_empty() {
        parts.push(format!("topic: {}", record.topic.trim()));
    }
    if !record.workflow.trim().is_empty() {
        parts.push(format!("workflow: {}", record.workflow.trim()));
    }
    if !record.entities.is_empty() {
        parts.push(format!("entities: {}", record.entities.join(", ")));
    }
    if !record.files_touched.is_empty() {
        parts.push(format!("files: {}", record.files_touched.join(", ")));
    }
    if !record.decisions.is_empty() {
        parts.push(format!("decisions: {}", record.decisions.join("; ")));
    }
    if !record.errors.is_empty() {
        parts.push(format!("errors: {}", record.errors.join("; ")));
    }
    if !record.next_steps.is_empty() {
        parts.push(format!("next_steps: {}", record.next_steps.join("; ")));
    }
    if !record.clean_text.trim().is_empty() {
        parts.push(format!(
            "evidence: {}",
            truncate_chars(record.clean_text.trim(), 420)
        ));
    }
    truncate_chars(&parts.join(" | "), 2_200)
}

fn regenerate_search_aliases_basic(record: &MemoryRecord) -> Vec<String> {
    // Delegate to the store-side noun-phrase generator so capture and rebuild
    // paths produce identical alias sets. Falls back to a small heuristic seed
    // when the structured fields are empty (legacy rows).
    let aliases = crate::store::generate_search_aliases_public(record);
    if !aliases.is_empty() {
        return aliases;
    }
    dedupe_trimmed_strings(
        vec![
            record.app_name.clone(),
            record.window_title.clone(),
            record.memory_context.clone(),
        ],
        24,
    )
}

fn derive_rebuild_memory_context(
    previous: Option<&MemoryRecord>,
    current: &MemoryRecord,
    next: Option<&MemoryRecord>,
) -> String {
    if !current.memory_context.trim().is_empty()
        && !is_vague_memory_context(&current.memory_context)
    {
        return current.memory_context.trim().to_string();
    }

    let mut clauses = Vec::new();
    let intent = if !current.user_intent.trim().is_empty() {
        current.user_intent.trim().to_string()
    } else {
        current.activity_type.trim().to_string()
    };
    if !intent.is_empty() {
        clauses.push(format!("The user was {}.", intent));
    }
    if !current.project.trim().is_empty() {
        clauses.push(format!(
            "This belonged to the {} project.",
            current.project.trim()
        ));
    } else if !current.topic.trim().is_empty() && current.topic != "unknown" {
        clauses.push(format!("Topic focus: {}.", current.topic.trim()));
    }

    let artifacts = dedupe_trimmed_strings(
        current
            .files_touched
            .iter()
            .cloned()
            .chain(current.entities.iter().cloned())
            .chain(current.related_tools.iter().cloned())
            .collect::<Vec<_>>(),
        5,
    );
    if !artifacts.is_empty() {
        clauses.push(format!("Key artifacts involved: {}.", artifacts.join(", ")));
    }

    let outcomes = dedupe_trimmed_strings(
        current
            .decisions
            .iter()
            .cloned()
            .chain(current.errors.iter().cloned())
            .chain(current.blockers.iter().cloned())
            .chain(current.todos.iter().cloned())
            .chain(current.results.iter().cloned())
            .chain(current.next_steps.iter().cloned())
            .collect::<Vec<_>>(),
        4,
    );
    if !outcomes.is_empty() {
        clauses.push(format!(
            "Important decisions, blockers, or results: {}.",
            outcomes.join("; ")
        ));
    }

    if clauses.is_empty() {
        clauses.push(format!(
            "The user was active in {} reviewing {}.",
            current.app_name,
            truncate_chars(&current.window_title, 120)
        ));
    }

    if let Some(prev) = previous {
        if prev.session_id == current.session_id && !prev.memory_context.trim().is_empty() {
            clauses.push(format!(
                "Earlier in this session: {}",
                truncate_chars(prev.memory_context.trim(), 160)
            ));
        }
    }
    if let Some(next) = next {
        if next.session_id == current.session_id && !next.memory_context.trim().is_empty() {
            clauses.push(format!(
                "Then it continued with: {}",
                truncate_chars(next.memory_context.trim(), 160)
            ));
        }
    }

    truncate_chars(&clauses.join(" "), 900)
}

pub(crate) fn classify_storage_outcome_with_config(
    record: &MemoryRecord,
    config: &crate::config::MemoryQualityConfig,
) -> String {
    classify_storage_outcome(record, config)
}

fn memory_quality_scores(memory: &MemoryRecord) -> MemoryScoreBreakdown {
    MemoryScoreBreakdown {
        specificity: memory.specificity_score,
        intent: memory.intent_score,
        entity: memory.entity_score,
        usefulness: memory.agent_usefulness_score,
        evidence: memory.evidence_confidence,
        ocr_noise: memory.ocr_noise_score,
        graph_readiness: memory.graph_readiness_score,
        retrieval_value: memory.retrieval_value_score,
        salience_concentration: crate::store::salience_concentration_score(memory),
        topic_clarity: crate::store::topic_clarity_score(memory),
        pollution_ratio: crate::store::pollution_ratio_score(memory),
    }
}

fn build_query_match_reasons(memory: &MemoryRecord, query: &str) -> Vec<String> {
    let normalized_query = normalize_debug_text(query);
    if normalized_query.is_empty() {
        return Vec::new();
    }
    let context = normalize_debug_text(&format!(
        "{} {} {} {} {}",
        memory.memory_context,
        memory.display_summary,
        memory.clean_text,
        memory.search_aliases.join(" "),
        memory.project
    ));
    let mut reasons = Vec::new();
    if context.contains(&normalized_query) {
        reasons.push("exact_query_phrase".to_string());
    }
    let query_tokens = normalized_query
        .split_whitespace()
        .filter(|token| token.len() > 1)
        .collect::<Vec<_>>();
    let mut matched_tokens = 0usize;
    for token in &query_tokens {
        if context.contains(token) {
            matched_tokens += 1;
        }
    }
    if !query_tokens.is_empty() && matched_tokens > 0 {
        reasons.push(format!(
            "token_overlap:{}/{}",
            matched_tokens,
            query_tokens.len()
        ));
    }
    if memory
        .search_aliases
        .iter()
        .any(|alias| normalize_debug_text(alias).contains(&normalized_query))
    {
        reasons.push("matched_alias".to_string());
    }
    if let Some(url) = memory.url.as_deref() {
        if normalize_debug_text(url).contains(&normalized_query) {
            reasons.push("matched_url".to_string());
        }
    }
    reasons
}

async fn build_memory_graph_snapshot(
    state: &AppState,
    memory: &MemoryRecord,
) -> Result<MemoryGraphSnapshot, String> {
    let nodes = state
        .store
        .get_all_nodes()
        .await
        .map_err(|e| e.to_string())?;
    let edges = state
        .store
        .get_all_edges()
        .await
        .map_err(|e| e.to_string())?;
    let memory_node_id = format!("memory:{}", memory.id);

    let selected_nodes = nodes
        .iter()
        .filter(|node| {
            node.id == memory_node_id
                || memory.graph_node_ids.iter().any(|id| id == &node.id)
                || node
                    .metadata
                    .get("memory_id")
                    .and_then(serde_json::Value::as_str)
                    .map(|value| value == memory.id)
                    .unwrap_or(false)
        })
        .map(|node| {
            serde_json::json!({
                "id": node.id,
                "type": format!("{:?}", node.node_type),
                "label": node.label,
                "created_at": node.created_at
            })
        })
        .collect::<Vec<_>>();

    let selected_edges = edges
        .iter()
        .filter(|edge| {
            edge.source == memory_node_id
                || edge.target == memory_node_id
                || memory.graph_edge_ids.iter().any(|id| id == &edge.id)
        })
        .map(|edge| {
            serde_json::json!({
                "id": edge.id,
                "type": format!("{:?}", edge.edge_type),
                "source": edge.source,
                "target": edge.target,
                "timestamp": edge.timestamp,
                "metadata": edge.metadata
            })
        })
        .collect::<Vec<_>>();

    let weak_evidence = selected_edges
        .iter()
        .filter_map(|edge| {
            let target = edge.get("target").and_then(serde_json::Value::as_str)?;
            let target_lower = target.to_ascii_lowercase();
            if target_lower.ends_with(":ui")
                || target_lower.ends_with(":chrome")
                || target_lower.ends_with(":window")
            {
                Some(format!("generic_target:{target}"))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    Ok(MemoryGraphSnapshot {
        nodes: selected_nodes,
        edges: selected_edges,
        weak_evidence,
    })
}

#[tauri::command]
pub async fn get_memory_debug_inspector(
    state: State<'_, Arc<AppState>>,
    memory_id: String,
    query: Option<String>,
) -> Result<MemoryDebugInspector, String> {
    let memory = state
        .inner()
        .store
        .get_memory_by_id(&memory_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Memory not found: {memory_id}"))?;
    let graph = build_memory_graph_snapshot(state.inner().as_ref(), &memory).await?;

    let actions = dedupe_trimmed_strings(
        memory
            .action_items
            .iter()
            .map(|item| item.text.clone())
            .chain(memory.decisions.iter().cloned())
            .chain(memory.errors.iter().cloned())
            .chain(memory.todos.iter().cloned())
            .chain(memory.blockers.iter().cloned())
            .chain(memory.results.iter().cloned())
            .chain(memory.next_steps.iter().cloned())
            .collect::<Vec<_>>(),
        16,
    );

    let entities = dedupe_trimmed_strings(
        memory
            .extracted_entities_structured
            .iter()
            .map(|item| item.text.clone())
            .chain(memory.entities.iter().cloned())
            .chain(memory.files_touched.iter().cloned())
            .collect::<Vec<_>>(),
        20,
    );
    let related_knowledge_pages = state
        .inner()
        .store
        .list_knowledge_pages(80, None, None)
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|page| page.supporting_memory_ids.iter().any(|id| id == &memory.id))
        .take(12)
        .map(|page| {
            serde_json::json!({
                "page_id": page.page_id,
                "page_type": page.page_type,
                "title": page.title,
                "stability": page.stability,
                "confidence_score": page.confidence_score
            })
        })
        .collect::<Vec<_>>();
    let evidence = raw_evidence_json(&memory.raw_evidence);
    let grounding_confidence = derive_grounding_confidence(&memory, evidence.as_ref());
    let extraction_issues = derive_extraction_issues(evidence.as_ref());
    let ocr_quality_stats = derive_ocr_quality_stats(&memory, evidence.as_ref());
    let embedding_diagnostics = derive_embedding_diagnostics(&memory);

    Ok(MemoryDebugInspector {
        memory_id: memory.id.clone(),
        memory_context: memory.memory_context.clone(),
        project: memory.project.clone(),
        topic: memory.topic.clone(),
        workflow: memory.workflow.clone(),
        activity_type: memory.activity_type.clone(),
        user_intent: memory.user_intent.clone(),
        entities,
        actions,
        quality_scores: memory_quality_scores(&memory),
        grounding_confidence,
        extraction_issues,
        ocr_quality_stats,
        embedding_diagnostics,
        embedding_text: memory.embedding_text.clone(),
        search_aliases: memory.search_aliases.clone(),
        raw_ocr_evidence: serde_json::json!({
            "text": truncate_chars(&memory.text, 1800),
            "clean_text": truncate_chars(&memory.clean_text, 1800),
            "internal_context": truncate_chars(&memory.internal_context, 1400),
            "raw_evidence": truncate_chars(&memory.raw_evidence, 2200),
            "parsed_raw_evidence": evidence,
        }),
        graph,
        storage_outcome: memory.storage_outcome.clone(),
        quality_gate_reason: memory.quality_gate_reason.clone(),
        query_match_reasons: query
            .as_deref()
            .map(|q| build_query_match_reasons(&memory, q))
            .unwrap_or_default(),
        related_knowledge_pages,
    })
}

#[tauri::command]
pub async fn evaluate_recent_memory_quality(
    state: State<'_, Arc<AppState>>,
    lookback_minutes: Option<u32>,
    limit: Option<usize>,
) -> Result<MemoryValidationReport, String> {
    let lookback = lookback_minutes.unwrap_or(180).clamp(5, 72 * 60);
    let cap = limit.unwrap_or(180).clamp(10, 1_500);
    let now = chrono::Utc::now().timestamp_millis();
    let start = now - chrono::Duration::minutes(lookback as i64).num_milliseconds();
    let rows = state
        .inner()
        .store
        .get_search_results_in_range(start, now)
        .await
        .map_err(|e| e.to_string())?;
    let scanned_total = rows.len().min(cap);
    let memory_quality_cfg = state.inner().config.read().memory_quality.clone();

    let mut flagged = Vec::new();
    for row in rows.into_iter().rev().take(cap) {
        let Some(memory) = state
            .inner()
            .store
            .get_memory_by_id(&row.id)
            .await
            .map_err(|e| e.to_string())?
        else {
            continue;
        };

        let graph = build_memory_graph_snapshot(state.inner().as_ref(), &memory).await?;
        let mut issues = Vec::new();
        if is_vague_memory_context(&memory.memory_context) {
            issues.push("vague_memory_context".to_string());
        }
        if memory.user_intent.trim().is_empty() {
            issues.push("missing_intent".to_string());
        }
        if memory.project.trim().is_empty()
            && (memory.topic.trim().is_empty() || memory.topic == "unknown")
        {
            issues.push("missing_project_or_topic".to_string());
        }
        if memory.entities.is_empty() && memory.files_touched.is_empty() {
            issues.push("missing_entities".to_string());
        }
        if memory.agent_usefulness_score < 0.60 {
            issues.push("low_agent_usefulness".to_string());
        }
        if memory.ocr_noise_score > 0.50 {
            issues.push("high_ocr_noise".to_string());
        }
        if memory.storage_outcome == "primary_memory_card"
            && classify_storage_outcome_with_config(&memory, &memory_quality_cfg)
                != "primary_memory_card"
        {
            issues.push("primary_should_be_low_quality_evidence".to_string());
        }
        if !graph.weak_evidence.is_empty() {
            issues.push("graph_events_with_weak_evidence".to_string());
        }
        let embedding_norm = normalize_debug_text(&memory.embedding_text);
        let clean_norm = normalize_debug_text(&memory.clean_text);
        let clean_prefix = clean_norm.chars().take(42).collect::<String>();
        if embedding_norm.len() > 60
            && !clean_prefix.is_empty()
            && embedding_norm.starts_with(&clean_prefix)
        {
            issues.push("embedding_text_dominated_by_raw_ocr".to_string());
        }
        if issues.is_empty() {
            continue;
        }
        flagged.push(MemoryQualityFlag {
            memory_id: memory.id.clone(),
            summary: if !memory.memory_context.trim().is_empty() {
                truncate_chars(&memory.memory_context, 180)
            } else {
                truncate_chars(&memory.display_summary, 180)
            },
            app_name: memory.app_name.clone(),
            timestamp: memory.timestamp,
            storage_outcome: memory.storage_outcome.clone(),
            issues,
            scores: memory_quality_scores(&memory),
        });
    }

    Ok(MemoryValidationReport {
        generated_at: now,
        lookback_minutes: lookback,
        total_memories: scanned_total,
        flagged_memories: flagged,
    })
}

#[tauri::command]
pub async fn rebuild_memory_context_for_range(
    state: State<'_, Arc<AppState>>,
    start: i64,
    end: i64,
    dry_run: bool,
) -> Result<RebuildMemoryContextReport, String> {
    if start > end {
        return Err("start must be <= end".to_string());
    }
    let mut all = state
        .inner()
        .store
        .list_all_memories()
        .await
        .map_err(|e| e.to_string())?;
    let quality_cfg = state.inner().config.read().memory_quality.clone();
    let mut previews = Vec::new();
    let mut changed = 0usize;

    for index in 0..all.len() {
        let ts = all[index].timestamp;
        if ts < start || ts > end {
            continue;
        }

        let previous = if index > 0 {
            Some(&all[index - 1])
        } else {
            None
        };
        let next = all.get(index + 1);
        let before = all[index].clone();
        let mut rebuilt = all[index].clone();
        rebuilt.memory_context = derive_rebuild_memory_context(previous, &rebuilt, next);
        rebuilt.embedding_text = compose_rebuild_embedding_text(&rebuilt);
        rebuilt.search_aliases = regenerate_search_aliases_basic(&rebuilt);
        if rebuilt.raw_evidence.trim().is_empty() {
            rebuilt.raw_evidence = serde_json::json!({
                "text": truncate_chars(&rebuilt.text, 1600),
                "clean_text": truncate_chars(&rebuilt.clean_text, 1600),
                "window_title": rebuilt.window_title,
                "app_name": rebuilt.app_name
            })
            .to_string();
        }
        rebuilt.storage_outcome = classify_storage_outcome_with_config(&rebuilt, &quality_cfg);

        let different = rebuilt.memory_context != before.memory_context
            || rebuilt.embedding_text != before.embedding_text
            || rebuilt.search_aliases != before.search_aliases
            || rebuilt.storage_outcome != before.storage_outcome;
        if !different {
            continue;
        }
        changed += 1;
        previews.push(RebuildMemoryPreview {
            memory_id: rebuilt.id.clone(),
            before_memory_context: before.memory_context.clone(),
            after_memory_context: rebuilt.memory_context.clone(),
            before_embedding_text: truncate_chars(&before.embedding_text, 240),
            after_embedding_text: truncate_chars(&rebuilt.embedding_text, 240),
            before_aliases: before.search_aliases.clone(),
            after_aliases: rebuilt.search_aliases.clone(),
            before_storage_outcome: before.storage_outcome.clone(),
            after_storage_outcome: rebuilt.storage_outcome.clone(),
        });
        all[index] = rebuilt;
    }

    if !dry_run && changed > 0 {
        state
            .inner()
            .store
            .replace_all_memories_preserving_ids(&all)
            .await
            .map_err(|e| e.to_string())?;
        let touched = all
            .iter()
            .filter(|memory| memory.timestamp >= start && memory.timestamp <= end)
            .cloned()
            .collect::<Vec<_>>();
        let _ = context_runtime::sync_memory_records(
            state.inner().as_ref(),
            &touched,
            Some("backfill"),
        )
        .await;
        state.inner().invalidate_memory_derived_caches();
    }

    Ok(RebuildMemoryContextReport {
        dry_run,
        start,
        end,
        scanned: previews.len(),
        changed,
        previews: previews.into_iter().take(80).collect(),
    })
}

fn result_relevant_for_query(result: &SearchResult, query_ctx: &QueryContext) -> bool {
    let haystack = normalize_debug_text(&format!(
        "{} {} {} {} {} {} {}",
        result.window_title,
        result.snippet,
        result.display_summary,
        result.memory_context,
        result.search_aliases.join(" "),
        result.project,
        result.topic
    ));
    if haystack.is_empty() {
        return false;
    }
    query_ctx
        .expanded_terms
        .iter()
        .filter(|term| term.len() > 1)
        .any(|term| haystack.contains(&normalize_debug_text(term)))
}

#[tauri::command]
pub async fn run_memory_retrieval_eval(
    state: State<'_, Arc<AppState>>,
) -> Result<RetrievalEvalReport, String> {
    let fixture = include_str!("../fixtures/retrieval_eval_queries.json");
    let cases: Vec<RetrievalEvalCase> = serde_json::from_str(fixture)
        .map_err(|err| format!("Invalid retrieval eval fixture: {err}"))?;
    let now = chrono::Utc::now().timestamp_millis();
    let mut rows = Vec::new();
    let mut top1 = 0usize;
    let mut top5 = 0usize;

    for case in cases {
        let query_ctx = QueryContext::from_query(&case.query);
        let hits = super::search::run_search_query(state.inner().as_ref(), &case.query, None, None, 5).await?;
        let top_1_relevant = hits
            .first()
            .map(|hit| result_relevant_for_query(hit, &query_ctx))
            .unwrap_or(false);
        let top_5_relevant = hits
            .iter()
            .any(|hit| result_relevant_for_query(hit, &query_ctx));
        if top_1_relevant {
            top1 += 1;
        }
        if top_5_relevant {
            top5 += 1;
        }
        let top = hits.first();
        let top_blob = top
            .map(|hit| {
                normalize_debug_text(&format!(
                    "{} {} {}",
                    hit.window_title,
                    hit.snippet,
                    hit.search_aliases.join(" ")
                ))
            })
            .unwrap_or_default();
        let matched_by_alias = top
            .map(|hit| {
                hit.search_aliases.iter().any(|alias| {
                    let n = normalize_debug_text(alias);
                    !n.is_empty() && query_ctx.expanded_terms.iter().any(|term| n.contains(term))
                })
            })
            .unwrap_or(false);
        let matched_by_prefix = query_ctx
            .prefix_variants
            .iter()
            .any(|prefix| top_blob.contains(prefix));
        let matched_by_fuzzy = query_ctx
            .fuzzy_variants
            .iter()
            .any(|variant| top_blob.contains(variant));
        let matched_by_ngram = query_ctx
            .ngram_variants
            .iter()
            .take(6)
            .any(|gram| top_blob.contains(gram));
        let matched_by_graph = top
            .map(|hit| !hit.related_memory_ids.is_empty() || !hit.extracted_entities.is_empty())
            .unwrap_or(false);
        let matched_by_semantic = top.map(|hit| hit.score >= 0.52).unwrap_or(false);
        let bad_match_reason = if top_5_relevant {
            None
        } else if hits.is_empty() {
            Some("no_results".to_string())
        } else {
            Some("no_query_grounding_in_top_results".to_string())
        };

        rows.push(RetrievalEvalRow {
            category: case.category,
            query: case.query,
            top_1_relevant,
            top_5_relevant,
            matched_by_semantic,
            matched_by_prefix,
            matched_by_fuzzy,
            matched_by_ngram,
            matched_by_graph,
            matched_by_alias,
            query_expansion_terms: query_ctx.expanded_terms,
            bad_match_reason,
            top_memory_id: top.map(|hit| hit.id.clone()),
        });
    }

    Ok(RetrievalEvalReport {
        generated_at: now,
        total_queries: rows.len(),
        top1_hits: top1,
        top5_hits: top5,
        rows,
    })
}
