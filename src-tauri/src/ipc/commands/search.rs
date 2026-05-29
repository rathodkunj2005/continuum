//! Search-related Tauri commands and helpers.

use super::common::{shared_embedder, strip_internal_fndr_results, truncate_chars};
use crate::graph::graph_store::GraphStore;
use crate::privacy::Blocklist;
use crate::search::{
    rerank_results, HybridSearcher, MemoryCard, MemoryCardSynthesizer, QueryContext,
};
use crate::storage::SearchResult;
use crate::AppState;
use std::collections::HashSet;
use std::sync::Arc;
use tauri::State;
use tokio::time::{timeout, Duration, Instant};

const SYNTHESIS_TIMEOUT: Duration = Duration::from_millis(2400);
const MEMORY_GRAPH_LIMIT: usize = 1_500;
const MEMORY_DERIVED_CACHE_TTL_MS: i64 = 30_000;

pub(super) async fn run_search_query(
    state: &AppState,
    query: &str,
    time_filter: Option<&str>,
    app_filter: Option<&str>,
    limit: usize,
) -> Result<Vec<SearchResult>, String> {
    let limit = limit.clamp(1, 50);

    if !state
        .store
        .has_memories()
        .await
        .map_err(|e| e.to_string())?
    {
        return Ok(Vec::new());
    }

    let search_config = {
        let config = state.config.read();
        config.search.clone()
    };

    // When an InferenceEngine is loaded, route through the expansion variant
    // so abstract concept queries ("sport", "design") can semantically reach
    // domain-specific captures that don't contain the literal query term.
    let engine_arc = state.inference_engine();
    let engine_ref = engine_arc.as_deref();

    let results = match shared_embedder() {
        Ok(embedder) => match HybridSearcher::search_with_expansion(
            &state.store,
            embedder,
            engine_ref,
            query,
            limit,
            time_filter,
            app_filter,
            &search_config,
        )
        .await
        .map_err(|err| err.to_string())
        {
            Ok(results) => results,
            Err(err) => {
                tracing::warn!(
                    "Hybrid search failed; falling back to keyword-only search: {}",
                    err
                );
                state
                    .store
                    .keyword_search(query, limit, time_filter, app_filter)
                    .await
                    .map_err(|e| e.to_string())?
            }
        },
        Err(err) => {
            tracing::warn!(
                "Semantic embedder unavailable for raw search; falling back to keyword-only: {}",
                err
            );
            state
                .store
                .keyword_search(query, limit, time_filter, app_filter)
                .await
                .map_err(|e| e.to_string())?
        }
    };

    Ok(strip_internal_fndr_results(results))
}

pub(super) fn cache_is_fresh(computed_at_ms: i64) -> bool {
    let age_ms = chrono::Utc::now().timestamp_millis() - computed_at_ms;
    age_ms >= 0 && age_ms <= MEMORY_DERIVED_CACHE_TTL_MS
}

pub(super) fn card_domain(url: &str) -> Option<String> {
    let no_scheme = url.split("://").nth(1).unwrap_or(url);
    let host = no_scheme.split('/').next()?.trim();
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

pub(super) fn is_low_signal_title(title: &str, app_name: &str) -> bool {
    let normalized = title.trim().to_lowercase();
    if normalized.is_empty() {
        return true;
    }

    let app = app_name.trim().to_lowercase();
    if normalized == app || normalized == format!("{app} activity") {
        return true;
    }

    let tokens = normalized.split_whitespace().count();
    if tokens <= 1 {
        return true;
    }

    // Generic shell / empty-window titles only (no per-app keyword lists).
    matches!(
        normalized.as_str(),
        "new chat"
            | "chat"
            | "activity"
            | "home"
            | "dashboard"
            | "new tab"
            | "settings"
            | "untitled"
            | "untitled document"
    )
}

pub(super) fn is_low_signal_summary(summary: &str, app_name: &str) -> bool {
    let normalized = summary.trim().to_lowercase();
    if normalized.is_empty() {
        return true;
    }

    let app = app_name.trim().to_lowercase();
    if normalized == app {
        return true;
    }

    let words = normalized.split_whitespace().count();
    words <= 2
}

pub(super) fn title_from_summary(summary: &str, app_name: &str) -> Option<String> {
    let trimmed = summary.trim().trim_end_matches('.');
    if trimmed.is_empty() {
        return None;
    }

    let cleaned = if let Some(rest) = trimmed.strip_prefix("Reviewed ") {
        rest.trim()
    } else if let Some(rest) = trimmed.strip_prefix("reviewed ") {
        rest.trim()
    } else {
        trimmed
    };

    if cleaned.is_empty() {
        return None;
    }

    let candidate = cleaned.to_string();
    if is_low_signal_title(&candidate, app_name) {
        None
    } else {
        Some(candidate)
    }
}

pub(super) fn card_summary(result: &SearchResult) -> String {
    let display = result.display_summary.trim();
    let snippet = result.snippet.trim();
    let clean = result.clean_text.trim();

    let base = if !display.is_empty() && !is_low_signal_summary(display, &result.app_name) {
        display
    } else if !snippet.is_empty() && !is_low_signal_summary(snippet, &result.app_name) {
        snippet
    } else if !clean.is_empty() && !is_low_signal_summary(clean, &result.app_name) {
        clean
    } else if !snippet.is_empty() {
        snippet
    } else {
        clean
    };

    if base.is_empty() {
        format!("Captured activity in {}", result.app_name)
    } else {
        base.to_string()
    }
}

fn has_continuity_signal(result: &SearchResult) -> bool {
    result.snippet.contains(" • ") || result.clean_text.contains(" • ")
}

pub(super) fn card_title(result: &SearchResult, summary: &str) -> String {
    let title = result.window_title.trim();
    if !title.is_empty() {
        let candidate = title.to_string();
        if !is_low_signal_title(&candidate, &result.app_name) {
            return candidate;
        }
    }

    if let Some(from_summary) = title_from_summary(summary, &result.app_name) {
        return from_summary;
    }

    if let Some(domain) = result.url.as_deref().and_then(card_domain) {
        return format!("{} · {}", result.app_name, domain);
    }

    format!("{} memory", result.app_name)
}

async fn enrich_insight_kg_node_counts(
    store: std::sync::Arc<crate::storage::Store>,
    cards: &mut [MemoryCard],
) {
    let gs = GraphStore::new(store);
    let Ok(nodes) = gs.all_nodes().await else {
        return;
    };
    let mut counts: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for n in &nodes {
        for mid in &n.source_memory_ids {
            *counts.entry(mid.clone()).or_insert(0) += 1;
        }
    }
    for c in cards {
        c.insight_kg_node_count = *counts.get(&c.id).unwrap_or(&0);
    }
}

pub(super) fn memory_card_from_result(result: SearchResult) -> MemoryCard {
    let memory_id = result.id.clone();
    let score = result.score;
    let app_name = result.app_name.clone();
    let window_title = result.window_title.clone();
    let url = result.url.clone();
    let summary = card_summary(&result);
    let title = card_title(&result, &summary);
    let mut context = Vec::new();
    if let Some(domain) = url.as_deref().and_then(card_domain) {
        context.push(format!("Site: {}", domain));
    }
    if !result.activity_type.trim().is_empty() && result.activity_type != "other" {
        context.push(format!("Activity: {}", result.activity_type.trim()));
    }
    for file in result.files_touched.iter().take(3) {
        let trimmed = file.trim();
        if !trimmed.is_empty() {
            context.push(format!("File: {}", trimmed));
        }
    }
    let memory_context = result.memory_context.trim();
    if !memory_context.is_empty() {
        let excerpt: String = memory_context.chars().take(220).collect();
        if !context.iter().any(|line| line.contains(&excerpt)) {
            context.push(excerpt);
        }
    } else if !result.internal_context.trim().is_empty() {
        let excerpt: String = result.internal_context.chars().take(220).collect();
        context.push(excerpt);
    }

    let fallback_snippet = summary.clone();
    let action = if result.url.is_some() {
        "Open source".to_string()
    } else {
        "Revisit context".to_string()
    };
    let anchor_memory_context = if !result.memory_context.trim().is_empty() {
        result.memory_context.clone()
    } else {
        result.internal_context.clone()
    };
    let continuation_of = crate::search::parse_continuation_of(&anchor_memory_context);
    let reopen_target = crate::search::parse_reopen_target(&anchor_memory_context, &result);
    let storage_outcome = card_storage_outcome(&result);
    let enrichment_status = card_enrichment_status(&result, &storage_outcome);

    MemoryCard {
        id: memory_id.clone(),
        title,
        summary: summary.clone(),
        display_summary: if result.display_summary.trim().is_empty() {
            summary
        } else {
            result.display_summary.clone()
        },
        internal_context: anchor_memory_context,
        action,
        context,
        timestamp: result.timestamp,
        app_name,
        window_title,
        url,
        score,
        source_count: 1,
        continuity: has_continuity_signal(&result),
        raw_snippets: vec![fallback_snippet],
        evidence_ids: vec![memory_id],
        confidence: card_confidence(&result),
        anchor_coverage_score: result.anchor_coverage_score.clamp(0.0, 1.0),
        activity_type: result.activity_type.clone(),
        files_touched: result.files_touched.clone(),
        session_duration_mins: result.session_duration_mins,
        continuation_of,
        reopen_target,
        insight_what_happened: result.insight_what_happened.clone(),
        insight_why_mattered: result.insight_why_mattered.clone(),
        insight_what_changed: result.insight_what_changed.clone(),
        insight_context_thread: result.insight_context_thread.clone(),
        insight_spans_json: result.insight_spans_json.clone(),
        insight_card_confidence: result.insight_card_confidence,
        timeline_action_class: crate::timeline::classify_action_class(&result)
            .as_str()
            .to_string(),
        project: result.project.clone(),
        insight_kg_node_count: 0,
        synthesis_branch: result.synthesis_branch.clone(),
        topic_categories: result.topic_categories.clone(),
        search_aliases: result.search_aliases.clone(),
        surfacing_reason: if result.matched_routes.is_empty() {
            None
        } else {
            let mut routes = result.matched_routes.clone();
            for label in &result.embedding_reason_labels {
                if !routes.contains(label) {
                    routes.push(label.clone());
                }
            }
            Some(crate::context_runtime::context_pack::SurfacingReason {
                headline: if result
                    .matched_routes
                    .iter()
                    .any(|route| route.eq_ignore_ascii_case("chunk"))
                {
                    "Matched a precise memory chunk".to_string()
                } else {
                    format!("Matched in {} routes", result.matched_routes.len())
                },
                routes,
                graph_path: None,
                anchor_terms_hit: Vec::new(),
                recency_boost: 0.0,
            })
        },
        matched_routes: result.matched_routes.clone(),
        matched_chunk_ids: result.matched_chunk_ids.clone(),
        chunk_evidence: result.chunk_evidence.clone(),
        enrichment_status,
        reviewed_at_ms: result.reviewed_at_ms,
        reviewer_generation: result.reviewer_generation,
        storage_outcome,
    }
}

fn card_confidence(result: &SearchResult) -> f32 {
    let stored_quality = result
        .confidence_score
        .max(result.insight_card_confidence)
        .max(result.evidence_confidence)
        .max(result.extraction_confidence)
        .clamp(0.0, 1.0);
    if is_low_evidence_visual_fallback_result(result) {
        return stored_quality.min(0.45);
    }
    if stored_quality > 0.0 {
        stored_quality
    } else {
        result.score.clamp(0.0, 1.0)
    }
}

fn card_storage_outcome(result: &SearchResult) -> String {
    if is_low_evidence_visual_fallback_result(result)
        && matches!(
            result.storage_outcome.as_str(),
            "" | "primary_memory_card" | "enriched_memory_card"
        )
    {
        return "low_quality_evidence".to_string();
    }
    result.storage_outcome.clone()
}

fn card_enrichment_status(result: &SearchResult, storage_outcome: &str) -> String {
    if storage_outcome == "low_quality_evidence"
        && is_low_evidence_visual_fallback_result(result)
        && matches!(
            result.enrichment_status.as_str(),
            "" | "pending" | "review_failed"
        )
    {
        return "pending_visual_semantics".to_string();
    }
    result.enrichment_status.clone()
}

fn is_low_evidence_visual_fallback_result(result: &SearchResult) -> bool {
    result
        .synthesis_branch
        .eq_ignore_ascii_case("llm_ocr_grounded_visual_fallback")
        && result.ocr_block_count == 0
        && result.ocr_confidence <= 0.01
}

pub(super) fn refine_memory_card_title(card: &mut MemoryCard) {
    if !is_low_signal_title(&card.title, &card.app_name) {
        return;
    }

    let window_title = card.window_title.trim();
    if !window_title.is_empty() && !is_low_signal_title(window_title, &card.app_name) {
        card.title = window_title.to_string();
        return;
    }

    if let Some(from_summary) = title_from_summary(&card.summary, &card.app_name) {
        card.title = from_summary;
        return;
    }

    if let Some(domain) = card.url.as_deref().and_then(card_domain) {
        card.title = format!("{} · {}", card.app_name, domain);
        return;
    }

    card.title = format!("{} memory", card.app_name);
}

pub(super) fn refine_memory_card_titles(cards: &mut [MemoryCard]) {
    for card in cards {
        refine_memory_card_title(card);
    }
}

/// Search for memories
#[tauri::command]
pub async fn search(
    state: State<'_, Arc<AppState>>,
    query: String,
    time_filter: Option<String>,
    app_filter: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<SearchResult>, String> {
    run_search_query(
        state.inner(),
        &query,
        time_filter.as_deref(),
        app_filter.as_deref(),
        limit.unwrap_or(20),
    )
    .await
}

/// Search and return synthesized memory cards for UI rendering
#[tauri::command]
pub async fn search_memory_cards(
    state: State<'_, Arc<AppState>>,
    query: String,
    time_filter: Option<String>,
    app_filter: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<MemoryCard>, String> {
    let limit = limit.unwrap_or(20).clamp(1, 50);
    let started = Instant::now();
    tracing::info!(
        query = %query,
        time_filter = ?time_filter,
        app_filter = ?app_filter,
        limit,
        "search_memory_cards:start"
    );

    if !state
        .store
        .has_memories()
        .await
        .map_err(|e| e.to_string())?
    {
        tracing::info!("search_memory_cards:complete total_ms=0 cards=0");
        return Ok(Vec::new());
    }

    let memory_card_config = {
        let config = state.config.read();
        config.memory_cards.clone()
    };
    let fallback_cards = |raw_results: &[SearchResult]| {
        MemoryCardSynthesizer::deterministic_from_results(
            &query,
            raw_results,
            limit.min(memory_card_config.max_groups),
        )
    };

    let raw_limit = limit.max(18).min(50);
    let mut raw_results = run_search_query(
        state.inner(),
        &query,
        time_filter.as_deref(),
        app_filter.as_deref(),
        raw_limit,
    )
    .await?;
    raw_results.truncate(raw_limit);
    let query_context = QueryContext::from_query(&query);
    let (reranked, rerank_stats) = rerank_results(&query_context, raw_results);
    let mut raw_results = reranked;
    if rerank_stats.excluded_for_coverage > 0 {
        tracing::info!(
            excluded_for_coverage = rerank_stats.excluded_for_coverage,
            query = %query_context.raw_query,
            "search_memory_cards:coverage_gate"
        );
    }
    raw_results.truncate(raw_limit);
    tracing::info!(count = raw_results.len(), "search_memory_cards:rerank:done");
    if raw_results.is_empty() {
        tracing::info!(
            "search_memory_cards:complete total_ms={} cards=0",
            started.elapsed().as_millis()
        );
        return Ok(Vec::new());
    }

    // Never block live search on model loading. If inference isn't already warm,
    // synthesis falls back to deterministic card generation immediately.
    let inference = state.inner().inference_engine();

    tracing::info!("search_memory_cards:synthesis:start");
    let synthesis_future = MemoryCardSynthesizer::from_results_with_policy(
        inference.as_deref(),
        &query,
        &raw_results,
        memory_card_config.max_groups,
        memory_card_config.max_llm_groups,
        Duration::from_millis(memory_card_config.llm_timeout_ms),
    );
    let mut cards = match timeout(SYNTHESIS_TIMEOUT, synthesis_future).await {
        Ok(generated) => {
            tracing::info!(
                count = generated.len(),
                "search_memory_cards:synthesis:done"
            );
            generated
        }
        Err(_) => {
            tracing::warn!(
                timeout_ms = SYNTHESIS_TIMEOUT.as_millis(),
                "search_memory_cards:synthesis:timeout"
            );
            fallback_cards(&raw_results)
        }
    };

    if cards.is_empty() {
        cards = fallback_cards(&raw_results);
    }
    refine_memory_card_titles(&mut cards);
    cards.retain(|card| !Blocklist::is_internal_app(&card.app_name, None));
    cards.truncate(limit);
    enrich_insight_kg_node_counts(state.store.clone(), &mut cards).await;
    tracing::info!(
        total_ms = started.elapsed().as_millis(),
        cards = cards.len(),
        "search_memory_cards:complete"
    );
    Ok(cards)
}

/// List memory cards in newest→oldest order for browsing.
#[tauri::command]
pub async fn list_memory_cards(
    state: State<'_, Arc<AppState>>,
    app_filter: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<MemoryCard>, String> {
    let limit = limit.unwrap_or(MEMORY_GRAPH_LIMIT).clamp(1, 2_000);
    let results = state
        .inner()
        .store
        .list_recent_results(limit, app_filter.as_deref())
        .await
        .map_err(|e| e.to_string())?;

    let mut cards: Vec<MemoryCard> = strip_internal_fndr_results(results)
        .into_iter()
        .map(memory_card_from_result)
        .collect();
    refine_memory_card_titles(&mut cards);
    enrich_insight_kg_node_counts(state.store.clone(), &mut cards).await;
    Ok(cards)
}
#[tauri::command]
pub async fn search_raw_results(
    state: State<'_, Arc<AppState>>,
    query: String,
    time_filter: Option<String>,
    app_filter: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<SearchResult>, String> {
    search(state, query, time_filter, app_filter, limit).await
}

/// Image-to-image retrieval: given a seed memory id, return the most visually
/// similar stored memories by cosine over the CLIP `image_embedding` column.
///
/// Returns an empty list when the seed is unknown or carries the legacy
/// zero image vector (older captures from before CLIP was wired into the
/// screen-capture loop, or rows where CLIP failed at capture time). Cross-modal
/// text->image retrieval is intentionally not exposed here pending an explicit
/// privacy review (see ADR-004 / ADR-005).
#[tauri::command]
pub async fn find_visually_similar_memories(
    state: State<'_, Arc<AppState>>,
    seed_memory_id: String,
    limit: Option<usize>,
    time_filter: Option<String>,
    app_filter: Option<String>,
) -> Result<Vec<SearchResult>, String> {
    let clamped = limit.unwrap_or(8).clamp(1, 50);
    state
        .store
        .similar_by_image_embedding(
            &seed_memory_id,
            clamped,
            time_filter.as_deref(),
            app_filter.as_deref(),
        )
        .await
        .map_err(|e| e.to_string())
}

/// Summarize search results using AI
#[tauri::command]
pub async fn summarize_search(
    _state: State<'_, Arc<AppState>>,
    query: String,
    results_snippets: Vec<String>,
) -> Result<String, String> {
    if results_snippets.is_empty() {
        return Ok(String::new());
    }

    let evidence = parse_summary_evidence(&results_snippets);
    let summary = build_grounded_search_summary(&query, &evidence);
    Ok(summary)
}

#[derive(Debug, Clone)]
struct SummaryEvidence {
    score: f32,
    text: String,
}

fn parse_summary_evidence(snippets: &[String]) -> Vec<SummaryEvidence> {
    let mut evidence = Vec::new();
    for raw in snippets {
        let score = extract_bracket_value(raw, "score")
            .and_then(|value| value.parse::<f32>().ok())
            .unwrap_or(0.5);
        let text = strip_bracket_prefixes(raw);
        if text.is_empty() {
            continue;
        }
        evidence.push(SummaryEvidence { score, text });
    }
    evidence
}

fn extract_bracket_value(raw: &str, key: &str) -> Option<String> {
    let prefix = format!("[{}:", key);
    let start = raw.find(&prefix)?;
    let rest = &raw[start + prefix.len()..];
    let end = rest.find(']')?;
    let value = rest[..end].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn strip_bracket_prefixes(raw: &str) -> String {
    let mut remaining = raw.trim();
    while remaining.starts_with('[') {
        let Some(end) = remaining.find(']') else {
            break;
        };
        remaining = remaining[end + 1..].trim_start();
    }
    remaining.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn summary_terms(query: &str) -> Vec<String> {
    query
        .to_lowercase()
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|term| term.len() > 1)
        .filter(|term| !summary_stop_word(term))
        .map(|term| term.to_string())
        .collect()
}

fn summary_stop_word(term: &str) -> bool {
    matches!(
        term,
        "a" | "an"
            | "and"
            | "are"
            | "as"
            | "at"
            | "be"
            | "for"
            | "from"
            | "how"
            | "in"
            | "is"
            | "it"
            | "of"
            | "on"
            | "or"
            | "that"
            | "the"
            | "this"
            | "to"
            | "was"
            | "what"
            | "when"
            | "where"
            | "who"
            | "why"
            | "with"
    )
}

fn evidence_relevance(
    query_terms: &[String],
    query_numbers: &HashSet<String>,
    text: &str,
    score: f32,
) -> f32 {
    let normalized = text.to_lowercase();

    let coverage = if query_terms.is_empty() {
        0.5
    } else {
        query_terms
            .iter()
            .filter(|term| normalized.contains(term.as_str()))
            .count() as f32
            / query_terms.len() as f32
    };

    let number_overlap = if query_numbers.is_empty() {
        0.0
    } else if query_numbers
        .iter()
        .any(|number| normalized.contains(number.as_str()))
    {
        1.0
    } else {
        0.0
    };

    (coverage * 0.58 + score.clamp(0.0, 1.0) * 0.30 + number_overlap * 0.12).clamp(0.0, 1.0)
}

fn clean_summary_fragment(text: &str) -> String {
    truncate_chars(
        &text
            .replace('\n', " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .trim_matches('"')
            .trim_matches('\'')
            .trim()
            .to_string(),
        180,
    )
}

fn ensure_period(sentence: &str) -> String {
    let mut out = sentence.trim().trim_end_matches('.').to_string();
    if !out.ends_with('.') {
        out.push('.');
    }
    out
}

fn build_grounded_search_summary(query: &str, evidence: &[SummaryEvidence]) -> String {
    if evidence.is_empty() {
        return String::new();
    }

    let query_terms = summary_terms(query);
    let query_numbers = query_terms
        .iter()
        .filter(|term| term.chars().any(|ch| ch.is_ascii_digit()))
        .cloned()
        .collect::<HashSet<_>>();

    let mut scored = evidence
        .iter()
        .map(|item| {
            let relevance =
                evidence_relevance(&query_terms, &query_numbers, &item.text, item.score);
            (item, relevance)
        })
        .collect::<Vec<_>>();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let selected = scored
        .iter()
        .filter(|(_, relevance)| *relevance >= 0.22)
        .take(2)
        .collect::<Vec<_>>();

    if selected.len() < 2 {
        tracing::debug!(
            query = %query,
            selected = selected.len(),
            "summarize_search:suppressed_insufficient_evidence"
        );
        return String::new();
    }

    let mut fragments = Vec::new();
    let mut confidence = 0.0f32;
    for (item, relevance) in &selected {
        fragments.push(clean_summary_fragment(&item.text));
        confidence += *relevance;
    }
    confidence /= selected.len() as f32;

    let mut summary = ensure_period(
        fragments
            .first()
            .map(|text| text.as_str())
            .unwrap_or("Found related activity"),
    );
    if let Some(second) = fragments.get(1) {
        summary.push_str(" Then ");
        summary.push_str(&ensure_period(second));
    }

    if confidence < 0.36 {
        tracing::debug!(
            query = %query,
            confidence,
            "summarize_search:suppressed_low_confidence"
        );
        return String::new();
    }

    summary
}
