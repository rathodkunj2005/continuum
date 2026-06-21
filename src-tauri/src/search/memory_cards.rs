//! MemoryCard synthesis for search results.
//!
//! Groups hybrid search hits into human-readable cards, validates any local LLM
//! drafts against grounded snippets, and falls back to deterministic summaries.

use crate::config::{
    DEFAULT_MEMORY_CARD_GROUPING_TIMEOUT_MS, DEFAULT_MEMORY_CARD_LLM_TIMEOUT_MS,
    DEFAULT_MEMORY_CARD_MAX_GROUPS, DEFAULT_MEMORY_CARD_MAX_GROUP_SNIPPETS,
    DEFAULT_MEMORY_CARD_MAX_LLM_GROUPS,
};
use crate::inference::{InferenceEngine, MemoryCardDraft};
use crate::storage::SearchResult;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tokio::time::{timeout, Duration};

const MAX_GROUP_SNIPPETS: usize = DEFAULT_MEMORY_CARD_MAX_GROUP_SNIPPETS;
const GROUPING_TIMEOUT: Duration = Duration::from_millis(DEFAULT_MEMORY_CARD_GROUPING_TIMEOUT_MS);

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct MemoryCard {
    pub id: String,
    pub title: String,
    pub summary: String,
    #[serde(default)]
    pub display_summary: String,
    #[serde(default)]
    pub internal_context: String,
    pub action: String,
    pub context: Vec<String>,
    pub timestamp: i64,
    pub app_name: String,
    pub window_title: String,
    pub url: Option<String>,
    pub score: f32,
    pub source_count: usize,
    #[serde(default)]
    pub continuity: bool,
    pub raw_snippets: Vec<String>,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
    #[serde(default)]
    pub confidence: f32,
    #[serde(default)]
    pub anchor_coverage_score: f32,
    /// High-level activity category: "coding", "browsing", "communication", "docs", "design", "other"
    #[serde(default)]
    pub activity_type: String,
    /// Files or code symbols mentioned across this group's snippets
    #[serde(default)]
    pub files_touched: Vec<String>,
    /// Approximate session duration in minutes (0 if single capture)
    #[serde(default)]
    pub session_duration_mins: u32,
    /// Short id of the prior card this one continues from, parsed out of
    /// the durable `memory_context` "Continues from <short_id>" marker.
    /// Never persisted on its own — derived from `memory_context` metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation_of: Option<String>,
    /// URL / file:// / app:// link derived from typed persisted reopen provenance.
    /// Legacy `memory_context` marker parsing is fallback-only during migration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reopen_target: Option<String>,
    /// Insight-first layers (ADR 007), copied from search hits.
    #[serde(default)]
    pub insight_what_happened: String,
    #[serde(default)]
    pub insight_why_mattered: String,
    #[serde(default)]
    pub insight_what_changed: String,
    #[serde(default)]
    pub insight_context_thread: String,
    #[serde(default)]
    pub insight_spans_json: String,
    #[serde(default)]
    pub insight_card_confidence: f32,
    /// Content-derived coarse action for adaptive timeline UI.
    #[serde(default)]
    pub timeline_action_class: String,
    /// Persisted memory `project` field (best-effort), for project-scoped graph view.
    #[serde(default)]
    pub project: String,
    /// Count of insight `graph_nodes` rows citing this memory id in `source_memory_ids`.
    #[serde(default)]
    pub insight_kg_node_count: u32,
    /// Which pipeline branch produced this record: "vlm" | "llm" | "browser_semantic" | "fallback" | "url_only"
    #[serde(default)]
    pub synthesis_branch: String,
    /// Broad semantic category labels for cross-domain concept search.
    #[serde(default)]
    pub topic_categories: Vec<String>,
    /// Synonym/alias terms surfaced by synthesis.
    #[serde(default)]
    pub search_aliases: Vec<String>,
    /// Phase 3 — "Why this surfaced" populated by the composer when this card
    /// was produced by the agentic-graph-rag pipeline. Defaults to `None` for
    /// legacy code paths so existing frontend / serde consumers stay unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surfacing_reason: Option<crate::context_runtime::context_pack::SurfacingReason>,
    #[serde(default)]
    pub matched_routes: Vec<String>,
    #[serde(default)]
    pub matched_chunk_ids: Vec<String>,
    #[serde(default)]
    pub chunk_evidence: Vec<crate::storage::MatchedChunkEvidence>,
    /// Lifecycle status from `MemoryRecord.enrichment_status` — surfaced so the
    /// vault can render DEVELOPED / PENDING / REVIEW_FAILED chips deterministically.
    #[serde(default)]
    pub enrichment_status: String,
    /// Unix ms timestamp of the last successful review (0 = never).
    #[serde(default)]
    pub reviewed_at_ms: i64,
    /// Monotonic counter of successful review passes.
    #[serde(default)]
    pub reviewer_generation: u32,
    /// Persisted storage gate outcome, e.g. "visual_semantics_failed".
    #[serde(default)]
    pub storage_outcome: String,
}

#[derive(Debug, Clone)]
struct SessionGroup {
    members: Vec<SearchResult>,
}

pub struct MemoryCardSynthesizer;

impl MemoryCardSynthesizer {
    /// Product-named wrapper for the search-results -> MemoryCards boundary.
    pub async fn build_memory_cards(
        inference: Option<&InferenceEngine>,
        query: &str,
        results: &[SearchResult],
    ) -> Vec<MemoryCard> {
        Self::from_results(inference, query, results).await
    }

    pub async fn from_results(
        inference: Option<&InferenceEngine>,
        query: &str,
        results: &[SearchResult],
    ) -> Vec<MemoryCard> {
        Self::from_results_with_policy(
            inference,
            query,
            results,
            DEFAULT_MEMORY_CARD_MAX_GROUPS,
            DEFAULT_MEMORY_CARD_MAX_LLM_GROUPS,
            Duration::from_millis(DEFAULT_MEMORY_CARD_LLM_TIMEOUT_MS),
        )
        .await
    }

    pub async fn from_results_with_policy(
        inference: Option<&InferenceEngine>,
        query: &str,
        results: &[SearchResult],
        max_groups: usize,
        max_llm_groups: usize,
        llm_timeout: Duration,
    ) -> Vec<MemoryCard> {
        if results.is_empty() {
            return Vec::new();
        }

        tracing::info!("search_memory_cards:grouping:start");
        let groups = match timeout(
            GROUPING_TIMEOUT,
            tokio::task::spawn_blocking({
                let results = results.to_vec();
                let enforce_query_support = !query.trim().is_empty();
                move || {
                    group_results_with_query_support(&results, max_groups, enforce_query_support)
                }
            }),
        )
        .await
        {
            Ok(Ok(groups)) => groups,
            Ok(Err(err)) => {
                tracing::warn!("search_memory_cards:grouping:join_error err={}", err);
                results
                    .iter()
                    .take(max_groups)
                    .cloned()
                    .map(|r| SessionGroup { members: vec![r] })
                    .collect()
            }
            Err(_) => {
                tracing::warn!(
                    timeout_ms = GROUPING_TIMEOUT.as_millis(),
                    "search_memory_cards:grouping:timeout"
                );
                results
                    .iter()
                    .take(max_groups)
                    .cloned()
                    .map(|r| SessionGroup { members: vec![r] })
                    .collect()
            }
        };
        tracing::info!(groups = groups.len(), "search_memory_cards:grouping:done");
        let mut cards = Vec::with_capacity(groups.len());

        for (index, group) in groups.into_iter().enumerate() {
            let snippets = collect_group_snippets(&group.members);
            let grounded_snippets = collect_grounded_snippets(&group.members);
            let anchor = select_anchor(&group.members);
            let evidence_ids = collect_evidence_ids(&group.members, 4);

            let mut draft = None;
            if index < max_llm_groups {
                if let Some(engine) = inference {
                    tracing::info!(group_idx = index, "search_memory_cards:synthesis_llm:start");
                    draft = match timeout(
                        llm_timeout,
                        engine.synthesize_memory_card(
                            query,
                            &anchor.app_name,
                            &anchor.window_title,
                            &grounded_snippets,
                        ),
                    )
                    .await
                    {
                        Ok(value) => value,
                        Err(_) => {
                            tracing::warn!(
                                group_idx = index,
                                timeout_ms = llm_timeout.as_millis(),
                                "search_memory_cards:synthesis_llm:timeout"
                            );
                            None
                        }
                    };
                    tracing::info!(
                        group_idx = index,
                        used_llm = draft.is_some(),
                        "search_memory_cards:synthesis_llm:done"
                    );
                }
            }

            let (title, mut summary, action, mut context) = match draft.as_ref().and_then(|d| {
                validate_draft(d, query, &snippets, &anchor.app_name, &anchor.window_title)
            }) {
                Some(valid) => valid,
                None => deterministic_fallback(query, &anchor, &snippets),
            };

            let match_reason = build_match_reason(query, &group.members, &anchor);
            if !match_reason.is_empty()
                && !context
                    .iter()
                    .any(|existing| existing.eq_ignore_ascii_case(&match_reason))
            {
                context.insert(0, match_reason);
                context.truncate(4);
            }

            let mut score = aggregate_score(&group.members);
            let source_count = group.members.len();
            let confidence = grounding_confidence(query, &summary, score, &snippets);
            let anchor_coverage = aggregate_anchor_coverage(&group.members);
            let query_support = query_support_ratio(query, &snippets, &anchor);
            if !query.trim().is_empty() && query_support < 0.10 && confidence < 0.32 {
                continue;
            }
            if !query.trim().is_empty() {
                if query_support < 0.20 {
                    score *= 0.78;
                }
                if confidence < 0.30 && query_support < 0.12 {
                    score *= 0.62;
                }
            }
            if !query.trim().is_empty()
                && confidence < 0.42
                && !summary.to_lowercase().starts_with("low confidence:")
            {
                summary = format!("Low confidence: {}", summary);
            }

            let activity_type =
                infer_activity_type(&anchor.app_name, &anchor.window_title, &snippets);
            let files_touched = extract_files_touched(&snippets);
            let session_duration_mins = compute_session_duration(&group.members);

            let anchor_memory_context = if !anchor.memory_context.trim().is_empty() {
                anchor.memory_context.clone()
            } else {
                anchor.internal_context.clone()
            };
            let continuation_of = parse_continuation_of(&anchor_memory_context);
            let reopen_target = parse_reopen_target(&anchor_memory_context, &anchor);
            cards.push(MemoryCard {
                id: anchor.id.clone(),
                title,
                summary: summary.clone(),
                display_summary: summary,
                internal_context: anchor_memory_context,
                action,
                context,
                timestamp: anchor.timestamp,
                app_name: anchor.app_name.clone(),
                window_title: anchor.window_title.clone(),
                url: anchor.url.clone(),
                score,
                source_count,
                continuity: source_count > 1 || snippets.iter().any(|value| value.contains(" • ")),
                raw_snippets: snippets,
                evidence_ids,
                confidence,
                anchor_coverage_score: anchor_coverage,
                activity_type,
                files_touched,
                session_duration_mins,
                continuation_of,
                reopen_target,
                insight_what_happened: anchor.insight_what_happened.clone(),
                insight_why_mattered: anchor.insight_why_mattered.clone(),
                insight_what_changed: anchor.insight_what_changed.clone(),
                insight_context_thread: anchor.insight_context_thread.clone(),
                insight_spans_json: anchor.insight_spans_json.clone(),
                insight_card_confidence: anchor.insight_card_confidence,
                timeline_action_class: crate::timeline::classify_action_class(&anchor)
                    .as_str()
                    .to_string(),
                project: anchor.project.clone(),
                insight_kg_node_count: 0,
                synthesis_branch: anchor.synthesis_branch.clone(),
                topic_categories: anchor.topic_categories.clone(),
                search_aliases: anchor.search_aliases.clone(),
                surfacing_reason: surfacing_reason_from_result(&anchor),
                matched_routes: anchor.matched_routes.clone(),
                matched_chunk_ids: anchor.matched_chunk_ids.clone(),
                chunk_evidence: anchor.chunk_evidence.clone(),
                enrichment_status: anchor.enrichment_status.clone(),
                reviewed_at_ms: anchor.reviewed_at_ms,
                reviewer_generation: anchor.reviewer_generation,
                storage_outcome: anchor.storage_outcome.clone(),
            });
        }

        cards.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.timestamp.cmp(&a.timestamp))
        });
        apply_story_continuity(&mut cards);

        cards
    }

    pub fn deterministic_from_results(
        query: &str,
        results: &[SearchResult],
        limit: usize,
    ) -> Vec<MemoryCard> {
        let mut cards = Vec::new();
        let capped = limit.max(1);
        for result in results.iter().take(capped) {
            cards.push(fallback_card_for_result(query, result));
        }
        apply_story_continuity(&mut cards);
        cards
    }
}

#[cfg(test)]
fn group_results(results: &[SearchResult], max_groups: usize) -> Vec<SessionGroup> {
    group_results_with_query_support(results, max_groups, false)
}

fn group_results_with_query_support(
    results: &[SearchResult],
    max_groups: usize,
    enforce_query_support: bool,
) -> Vec<SessionGroup> {
    let mut sorted = results.to_vec();
    sorted.sort_by_key(|r| std::cmp::Reverse(r.timestamp));

    let mut groups: Vec<SessionGroup> = Vec::new();
    let mut key_to_group_idx: HashMap<String, usize> = HashMap::new();

    for result in sorted {
        let key = grouping_key(&result);
        if let Some(group_idx) = key_to_group_idx.get(&key).copied() {
            let anchor = &groups[group_idx].members[0];
            if should_group(anchor, &result, enforce_query_support) {
                groups[group_idx].members.push(result);
                continue;
            }
        }

        if groups.len() >= max_groups {
            continue;
        }
        let next_idx = groups.len();
        groups.push(SessionGroup {
            members: vec![result],
        });
        key_to_group_idx.insert(key, next_idx);
    }

    split_impure_groups(groups, enforce_query_support)
}

fn grouping_key(result: &SearchResult) -> String {
    if !result.session_key.trim().is_empty() {
        return result.session_key.clone();
    }

    let domain = extract_domain(result.url.as_deref()).unwrap_or_default();
    let title = normalize_for_dedup(&result.window_title);
    format!("{}:{}:{}", result.app_name.to_lowercase(), domain, title)
}

fn should_group(a: &SearchResult, b: &SearchResult, enforce_query_support: bool) -> bool {
    let within_time_window = (a.timestamp - b.timestamp).abs() <= 5 * 60 * 1000;
    if !within_time_window {
        return false;
    }

    if enforce_query_support {
        let query_support_ratio = ((a.anchor_coverage_score.clamp(0.0, 1.0)
            + b.anchor_coverage_score.clamp(0.0, 1.0))
            / 2.0)
            .clamp(0.0, 1.0);
        if query_support_ratio < 0.25 {
            return false;
        }
    }

    if !a.session_key.is_empty() && a.session_key == b.session_key {
        return true;
    }

    let same_app = a.app_name == b.app_name;
    let title_sim = token_overlap(&a.window_title, &b.window_title);
    let snippet_sim = token_overlap(&a.snippet, &b.snippet);
    let text_sim = token_overlap(&merged_text(a), &merged_text(b));
    let domain_match = extract_domain(a.url.as_deref()) == extract_domain(b.url.as_deref());
    let same_url = same_effective_url(a.url.as_deref(), b.url.as_deref());

    if same_app {
        return domain_match || title_sim >= 0.55 || text_sim >= 0.40;
    }

    if same_url && (text_sim >= 0.35 || snippet_sim >= 0.35 || title_sim >= 0.35) {
        return true;
    }

    false
}

fn split_impure_groups(
    groups: Vec<SessionGroup>,
    enforce_query_support: bool,
) -> Vec<SessionGroup> {
    if !enforce_query_support {
        return groups;
    }

    let mut out = Vec::new();
    for group in groups {
        if group.members.len() <= 1 {
            out.push(group);
            continue;
        }

        let mut current_members: Vec<SearchResult> = Vec::new();
        for member in group.members {
            if let Some(last) = current_members.last() {
                let entity_similarity = entity_overlap_similarity(last, &member);
                let lexical_similarity = token_overlap(&merged_text(last), &merged_text(&member));
                if entity_similarity < 0.35 && lexical_similarity < 0.20 {
                    out.push(SessionGroup {
                        members: std::mem::take(&mut current_members),
                    });
                }
            }
            current_members.push(member);
        }

        if !current_members.is_empty() {
            out.push(SessionGroup {
                members: current_members,
            });
        }
    }

    out
}

fn entity_overlap_similarity(a: &SearchResult, b: &SearchResult) -> f32 {
    let left = if a.extracted_entities.is_empty() {
        tokenize(&format!("{} {}", a.window_title, a.display_summary))
    } else {
        a.extracted_entities
            .iter()
            .map(|value| normalize_for_dedup(value))
            .filter(|value| !value.is_empty())
            .collect::<HashSet<_>>()
    };
    let right = if b.extracted_entities.is_empty() {
        tokenize(&format!("{} {}", b.window_title, b.display_summary))
    } else {
        b.extracted_entities
            .iter()
            .map(|value| normalize_for_dedup(value))
            .filter(|value| !value.is_empty())
            .collect::<HashSet<_>>()
    };

    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let intersection = left.intersection(&right).count() as f32;
    let union = left.union(&right).count() as f32;
    if union == 0.0 {
        0.0
    } else {
        intersection / union
    }
}

fn merged_text(result: &SearchResult) -> String {
    if !result.clean_text.trim().is_empty() {
        result.clean_text.clone()
    } else {
        result.text.clone()
    }
}

fn collect_group_snippets(results: &[SearchResult]) -> Vec<String> {
    let mut snippets = Vec::new();
    let mut seen = HashSet::new();

    for result in results {
        for evidence in &result.chunk_evidence {
            let text = evidence.text.trim();
            if text.is_empty() {
                continue;
            }
            let normalized = normalize_for_dedup(text);
            if seen.insert(normalized) {
                snippets.push(text.to_string());
            }
            if snippets.len() >= MAX_GROUP_SNIPPETS {
                break;
            }
        }
        if snippets.len() >= MAX_GROUP_SNIPPETS {
            break;
        }

        let primary = if !result.memory_context.trim().is_empty() {
            result.memory_context.trim().to_string()
        } else if !result.snippet.trim().is_empty() {
            result.snippet.trim().to_string()
        } else {
            merged_text(result)
                .lines()
                .next()
                .unwrap_or_default()
                .trim()
                .to_string()
        };

        if primary.is_empty() {
            continue;
        }

        let normalized = normalize_for_dedup(&primary);
        if seen.insert(normalized) {
            snippets.push(primary);
        }

        if snippets.len() >= MAX_GROUP_SNIPPETS {
            break;
        }
    }

    snippets
}

fn collect_grounded_snippets(results: &[SearchResult]) -> Vec<String> {
    let mut snippets = Vec::new();
    for result in results.iter().take(MAX_GROUP_SNIPPETS) {
        if let Some(evidence) = result
            .chunk_evidence
            .iter()
            .find(|evidence| !evidence.text.trim().is_empty())
        {
            snippets.push(format!(
                "[{}:{}] {}",
                short_id(&result.id),
                evidence.chunk_index,
                evidence.text.trim()
            ));
            continue;
        }

        let snippet = if !result.memory_context.trim().is_empty() {
            result.memory_context.trim().to_string()
        } else if !result.snippet.trim().is_empty() {
            result.snippet.trim().to_string()
        } else {
            merged_text(result)
                .lines()
                .next()
                .unwrap_or_default()
                .trim()
                .to_string()
        };

        if snippet.is_empty() {
            continue;
        }

        snippets.push(format!("[{}] {}", short_id(&result.id), snippet));
    }
    snippets
}

fn select_anchor(results: &[SearchResult]) -> SearchResult {
    results
        .iter()
        .max_by(|a, b| {
            a.score
                .partial_cmp(&b.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.timestamp.cmp(&b.timestamp))
        })
        .cloned()
        .unwrap_or_else(|| results[0].clone())
}

fn aggregate_score(results: &[SearchResult]) -> f32 {
    let mut weighted = 0.0f32;
    let mut total_w = 0.0f32;
    for (idx, result) in results.iter().enumerate() {
        let weight = 1.0 / (idx as f32 + 1.0);
        weighted += result.score * weight;
        total_w += weight;
    }

    let avg = if total_w > 0.0 {
        weighted / total_w
    } else {
        0.0
    };
    (avg + (results.len() as f32 * 0.04)).min(1.0)
}

fn aggregate_anchor_coverage(results: &[SearchResult]) -> f32 {
    if results.is_empty() {
        return 0.0;
    }

    let total = results
        .iter()
        .map(|result| result.anchor_coverage_score.clamp(0.0, 1.0))
        .sum::<f32>();
    (total / results.len() as f32).clamp(0.0, 1.0)
}

fn collect_evidence_ids(results: &[SearchResult], max_ids: usize) -> Vec<String> {
    let mut ranked = results.to_vec();
    ranked.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.timestamp.cmp(&a.timestamp))
    });

    ranked
        .iter()
        .take(max_ids.max(1))
        .map(|result| result.id.clone())
        .collect()
}

fn short_id(value: &str) -> String {
    value.chars().take(8).collect::<String>()
}

fn grounding_confidence(query: &str, summary: &str, base_score: f32, snippets: &[String]) -> f32 {
    if snippets.is_empty() {
        return 0.0;
    }

    let summary_terms = tokenize(summary)
        .into_iter()
        .filter(|term| !grounding_stop_words().contains(term.as_str()))
        .collect::<HashSet<_>>();
    if summary_terms.is_empty() {
        return 0.0;
    }

    let snippet_blob = normalize_for_dedup(&snippets.join(" "));
    let supported = summary_terms
        .iter()
        .filter(|term| snippet_blob.contains(term.as_str()))
        .count();
    let support_ratio = supported as f32 / summary_terms.len().max(1) as f32;

    let query_terms = tokenize(query)
        .into_iter()
        .filter(|term| !grounding_stop_words().contains(term.as_str()))
        .collect::<HashSet<_>>();
    let query_coverage = if query_terms.is_empty() {
        0.5
    } else {
        query_terms
            .iter()
            .filter(|term| snippet_blob.contains(term.as_str()))
            .count() as f32
            / query_terms.len() as f32
    };

    (base_score.clamp(0.0, 1.0) * 0.45 + support_ratio * 0.35 + query_coverage * 0.20)
        .clamp(0.0, 1.0)
}

fn query_support_ratio(query: &str, snippets: &[String], anchor: &SearchResult) -> f32 {
    if query.trim().is_empty() {
        return 1.0;
    }

    let query_terms = tokenize(query)
        .into_iter()
        .filter(|term| !grounding_stop_words().contains(term.as_str()))
        .collect::<HashSet<_>>();
    if query_terms.is_empty() {
        return 0.0;
    }

    let evidence_blob = normalize_for_dedup(&format!(
        "{} {} {} {}",
        snippets.join(" "),
        anchor.window_title,
        anchor.app_name,
        anchor.url.clone().unwrap_or_default()
    ));
    let supported = query_terms
        .iter()
        .filter(|term| evidence_blob.contains(term.as_str()))
        .count();

    supported as f32 / query_terms.len().max(1) as f32
}

fn grounding_stop_words() -> HashSet<&'static str> {
    [
        "the", "and", "for", "with", "that", "from", "this", "have", "into", "while", "open",
        "page", "about", "using", "user", "you", "your", "their", "then", "was", "what", "is",
        "are",
    ]
    .into_iter()
    .collect()
}

fn validate_draft(
    draft: &MemoryCardDraft,
    query: &str,
    snippets: &[String],
    app_name: &str,
    window_title: &str,
) -> Option<(String, String, String, Vec<String>)> {
    let title = sanitize_title(&draft.title, app_name, window_title);
    let summary = sanitize_summary(&draft.summary)?;
    if grounding_confidence(query, &summary, 0.6, snippets) < 0.38 {
        return None;
    }
    let action = sanitize_action(&draft.action);

    let mut context = draft
        .context
        .iter()
        .map(|value| normalize_sentence(value))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();

    context.retain(|value| !is_ui_chrome_phrase(value));
    context.dedup();
    context.truncate(4);

    if context.is_empty() {
        let fallback = truncate_words(window_title, 6);
        if fallback.is_empty() {
            context.push(app_name.to_string());
        } else {
            context.push(fallback);
        }
    }

    Some((title, summary, action, context))
}

fn deterministic_fallback(
    _query: &str,
    anchor: &SearchResult,
    snippets: &[String],
) -> (String, String, String, Vec<String>) {
    let title = sanitize_title("", &anchor.app_name, &anchor.window_title);
    let summary = build_story_summary(anchor, snippets);
    let action = build_action_summary(anchor, snippets);
    let context = build_context(anchor, snippets);

    (
        title,
        sanitize_summary(&summary).unwrap_or(summary),
        action,
        context,
    )
}

fn fallback_card_for_result(query: &str, result: &SearchResult) -> MemoryCard {
    let snippets = collect_group_snippets(std::slice::from_ref(result));
    let (title, mut summary, action, context) = deterministic_fallback(query, result, &snippets);
    let evidence_ids = vec![result.id.clone()];
    let confidence = grounding_confidence(query, &summary, result.score, &snippets);
    if !query.trim().is_empty()
        && confidence < 0.42
        && !summary.to_lowercase().starts_with("low confidence:")
    {
        summary = format!("Low confidence: {}", summary);
    }
    let activity_type = infer_activity_type(&result.app_name, &result.window_title, &snippets);
    let files_touched = extract_files_touched(&snippets);
    let anchor_memory_context = if !result.memory_context.trim().is_empty() {
        result.memory_context.clone()
    } else {
        result.internal_context.clone()
    };
    let continuation_of = parse_continuation_of(&anchor_memory_context);
    let reopen_target = parse_reopen_target(&anchor_memory_context, result);
    MemoryCard {
        id: result.id.clone(),
        title,
        summary: summary.clone(),
        display_summary: summary,
        internal_context: anchor_memory_context,
        action,
        context,
        timestamp: result.timestamp,
        app_name: result.app_name.clone(),
        window_title: result.window_title.clone(),
        url: result.url.clone(),
        score: result.score,
        source_count: 1,
        continuity: snippets.iter().any(|value| value.contains(" • ")),
        raw_snippets: snippets,
        evidence_ids,
        confidence,
        anchor_coverage_score: result.anchor_coverage_score.clamp(0.0, 1.0),
        activity_type,
        files_touched,
        session_duration_mins: 0,
        continuation_of,
        reopen_target,
        insight_what_happened: result.insight_what_happened.clone(),
        insight_why_mattered: result.insight_why_mattered.clone(),
        insight_what_changed: result.insight_what_changed.clone(),
        insight_context_thread: result.insight_context_thread.clone(),
        insight_spans_json: result.insight_spans_json.clone(),
        insight_card_confidence: result.insight_card_confidence,
        timeline_action_class: crate::timeline::classify_action_class(result)
            .as_str()
            .to_string(),
        project: result.project.clone(),
        insight_kg_node_count: 0,
        synthesis_branch: result.synthesis_branch.clone(),
        topic_categories: result.topic_categories.clone(),
        search_aliases: result.search_aliases.clone(),
        surfacing_reason: surfacing_reason_from_result(result),
        matched_routes: result.matched_routes.clone(),
        matched_chunk_ids: result.matched_chunk_ids.clone(),
        chunk_evidence: result.chunk_evidence.clone(),
        enrichment_status: result.enrichment_status.clone(),
        reviewed_at_ms: result.reviewed_at_ms,
        reviewer_generation: result.reviewer_generation,
        storage_outcome: result.storage_outcome.clone(),
    }
}

fn surfacing_reason_from_result(
    result: &SearchResult,
) -> Option<crate::context_runtime::context_pack::SurfacingReason> {
    if result.matched_routes.is_empty() {
        return None;
    }
    let mut routes = result.matched_routes.clone();
    for label in &result.embedding_reason_labels {
        if !routes.contains(label) {
            routes.push(label.clone());
        }
    }
    let headline = if result
        .matched_routes
        .iter()
        .any(|route| route.eq_ignore_ascii_case("chunk"))
    {
        "Matched a precise memory chunk".to_string()
    } else {
        format!("Matched in {} routes", result.matched_routes.len())
    };
    Some(crate::context_runtime::context_pack::SurfacingReason {
        headline,
        routes,
        graph_path: None,
        anchor_terms_hit: Vec::new(),
        recency_boost: 0.0,
    })
}

/// `pub(crate)` re-export used by `context_runtime::composer` to build cards
/// from a single `SearchResult` without duplicating field-mapping logic.
pub(crate) fn build_fallback_card(query: &str, result: &SearchResult) -> MemoryCard {
    fallback_card_for_result(query, result)
}

/// Extract the short id encoded by `build_durable_memory_context` after
/// `Continues from `. Returns the trimmed first whitespace-delimited token
/// (never an arbitrary substring). Empty when the marker is absent.
pub fn parse_continuation_of(memory_context: &str) -> Option<String> {
    for line in memory_context.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Continues from ") {
            let id = rest
                .split(|ch: char| ch == ':' || ch.is_whitespace())
                .next()
                .unwrap_or("")
                .trim();
            if !id.is_empty() {
                return Some(id.to_string());
            }
        }
    }
    None
}

/// Resolve reopen target from typed persisted fields. Legacy `memory_context`
/// marker parsing remains as migration fallback only.
pub fn parse_reopen_target(memory_context: &str, result: &SearchResult) -> Option<String> {
    match &result.reopen_kind {
        crate::memory::reopen::ReopenKind::BrowserUrl => {
            if let Some(url) = result.reopen_url.as_deref() {
                let trimmed = url.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
        crate::memory::reopen::ReopenKind::FilePath => {
            if let Some(path) = result.reopen_file_path.as_deref() {
                let trimmed = path.trim();
                if !trimmed.is_empty() {
                    return Some(format!("file://{}", trimmed.trim_start_matches("file://")));
                }
            }
        }
        crate::memory::reopen::ReopenKind::AppDeepLink => {
            if let Some(link) = result.reopen_app_deep_link.as_deref() {
                let trimmed = link.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
        crate::memory::reopen::ReopenKind::AppBundle => {
            if let Some(bundle) = result.reopen_app_bundle_id.as_deref() {
                let trimmed = bundle.trim();
                if !trimmed.is_empty() {
                    return Some(format!("app-bundle://{}", trimmed));
                }
            }
        }
        crate::memory::reopen::ReopenKind::Unknown => {}
    }

    for line in memory_context.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Reopen: ") {
            let value = rest.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }

    if let Some(u) = result
        .url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Some(u.to_string());
    }
    if let Some(first) = result
        .files_touched
        .iter()
        .map(|s| s.trim())
        .find(|s| !s.is_empty())
    {
        return Some(format!("file://{}", first));
    }
    None
}

/// Classify the high-level activity from content-derived signals only.
/// All cues are generic English / file-extension morphology — no app names,
/// no URL-host allowlists.
fn infer_activity_type(_app_name: &str, _window_title: &str, snippets: &[String]) -> String {
    let haystack = snippets.join(" ").to_lowercase();

    let mut scores = vec![("other", 0.04_f32)];
    if contains_any(&haystack, &["error", "failed", "trace", "exception"]) {
        scores.push(("debugging", 0.40));
    }
    if contains_any(
        &haystack,
        &["test", "build", "assert", "validation", "pipeline"],
    ) {
        scores.push(("testing_workflow", 0.30));
    }
    if contains_any(&haystack, &["plan", "decision", "todo", "next step"]) {
        scores.push(("planning", 0.28));
    }
    if contains_any(
        &haystack,
        &["function", ".rs", ".ts", ".py", "class ", "impl "],
    ) {
        scores.push(("coding", 0.34));
    }
    if contains_any(&haystack, &["http://", "https://"]) {
        scores.push(("researching", 0.24));
    }

    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scores
        .first()
        .map(|(label, _)| (*label).to_string())
        .unwrap_or_else(|| "other".to_string())
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| haystack.contains(n))
}

/// Extract file paths and code symbols from snippets (up to 6 unique entries).
fn extract_files_touched(snippets: &[String]) -> Vec<String> {
    let combined = snippets.join(" ");
    let mut found: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for word in combined.split_whitespace() {
        let w = word.trim_matches(|c: char| ",;:'\"()[]{}".contains(c));
        if w.len() < 4 {
            continue;
        }
        // File path heuristic: contains / or . with known extension
        let is_path = w.contains('/')
            || (w.contains('.')
                && (w.ends_with(".rs")
                    || w.ends_with(".ts")
                    || w.ends_with(".tsx")
                    || w.ends_with(".py")
                    || w.ends_with(".js")
                    || w.ends_with(".jsx")
                    || w.ends_with(".go")
                    || w.ends_with(".json")
                    || w.ends_with(".toml")
                    || w.ends_with(".md")
                    || w.ends_with(".sh")
                    || w.ends_with(".yaml")
                    || w.ends_with(".yml")));
        if is_path {
            // Normalize: strip leading slashes and keep only the last 2 components
            let parts: Vec<&str> = w.trim_start_matches('/').rsplitn(3, '/').collect();
            let normalized = if parts.len() >= 2 {
                parts[..2]
                    .iter()
                    .rev()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("/")
            } else {
                w.to_string()
            };
            let key = normalized.to_lowercase();
            if seen.insert(key) {
                found.push(normalized);
                if found.len() >= 6 {
                    break;
                }
            }
        }
    }

    found
}

/// Compute approximate session duration in minutes from member timestamps.
fn compute_session_duration(members: &[SearchResult]) -> u32 {
    if members.len() < 2 {
        return 0;
    }
    let min_ts = members.iter().map(|m| m.timestamp).min().unwrap_or(0);
    let max_ts = members.iter().map(|m| m.timestamp).max().unwrap_or(0);
    let diff_ms = max_ts.saturating_sub(min_ts);
    (diff_ms / 60_000).max(0) as u32
}

fn sanitize_title(raw: &str, app_name: &str, window_title: &str) -> String {
    let candidate = normalize_sentence(raw);
    if !candidate.is_empty() && !is_generic_title(&candidate) {
        return truncate_words(&candidate, 18);
    }

    let clean_window = normalize_sentence(window_title);
    if !clean_window.is_empty() && !is_generic_title(&clean_window) {
        return truncate_words(&clean_window, 18);
    }

    format!("{} activity", app_name)
}

fn sanitize_action(raw: &str) -> String {
    let cleaned = normalize_sentence(raw);
    if cleaned.is_empty() || is_ui_chrome_phrase(&cleaned) {
        "Reviewed key details".to_string()
    } else {
        truncate_words(&cleaned, 10)
    }
}

fn sanitize_summary(raw: &str) -> Option<String> {
    let summary = clean_story_fact(raw);
    if summary.is_empty() {
        return None;
    }

    if summary.contains('\n')
        || summary.contains('*')
        || summary.contains('#')
        || summary.contains('`')
    {
        return None;
    }

    let lower = summary.to_lowercase();
    if lower.starts_with("the screen shows") || lower.starts_with("i see") {
        return None;
    }
    if is_ui_chrome_phrase(&summary) {
        return None;
    }

    let mut sentences = split_sentences_preserving_decimals(&summary)
        .into_iter()
        .map(|s| normalize_sentence(&s))
        .map(|s| trim_trailing_fragment(&s))
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    if sentences.is_empty() {
        return None;
    }
    sentences.truncate(2);

    let total_words = sentences
        .iter()
        .map(|sentence| sentence.split_whitespace().count())
        .sum::<usize>();
    if !(8..=36).contains(&total_words) {
        return None;
    }

    for sentence in &sentences {
        let words = sentence.split_whitespace().count();
        if !(4..=22).contains(&words) {
            return None;
        }
    }

    let rendered = sentences
        .iter()
        .map(|s| ensure_sentence_period(s))
        .collect::<Vec<_>>()
        .join(" ");

    Some(rendered)
}

fn is_ui_chrome_phrase(value: &str) -> bool {
    let lower = value.to_lowercase();
    lower.contains("new tab")
        || lower.contains("toolbar")
        || lower.contains("tab strip")
        || lower == "home"
        || lower == "trending"
}

fn is_generic_title(value: &str) -> bool {
    matches!(
        value.to_lowercase().as_str(),
        "new tab" | "home" | "untitled" | "dashboard" | "settings"
    )
}

fn ensure_sentence_period(value: &str) -> String {
    let mut out = value.trim().to_string();
    if !out.ends_with('.') {
        out.push('.');
    }
    out
}

fn build_story_summary(anchor: &SearchResult, snippets: &[String]) -> String {
    let facts = extract_story_facts(snippets);

    if facts.is_empty() {
        let domain = extract_domain(anchor.url.as_deref());
        return if let Some(dom) = domain {
            format!(
                "Reviewed {} updates on {}.",
                truncate_words(&anchor.window_title, 6),
                dom
            )
        } else {
            format!("Reviewed {}.", truncate_words(&anchor.window_title, 8))
        };
    }

    let mut summary = ensure_sentence_period(&facts[0]);
    if let Some(second) = facts.get(1) {
        let second_sentence = ensure_sentence_period(second);
        if token_overlap(&summary, &second_sentence) < 0.72 {
            summary.push(' ');
            summary.push_str("Also, ");
            summary.push_str(second_sentence.trim_end_matches('.'));
            summary.push('.');
        }
    }

    summary
}

fn build_action_summary(anchor: &SearchResult, snippets: &[String]) -> String {
    if let Some(first) = extract_story_facts(snippets).first() {
        return sanitize_action(&truncate_words(first, 10));
    }

    if let Some(domain) = extract_domain(anchor.url.as_deref()) {
        return format!("Followed updates on {}", domain);
    }

    format!("Reviewed {}", truncate_words(&anchor.window_title, 5))
}

fn build_match_reason(query: &str, members: &[SearchResult], anchor: &SearchResult) -> String {
    let query_norm = normalize_for_dedup(query);
    if query_norm.is_empty() {
        return String::new();
    }

    let exact_phrase = members.iter().any(|member| {
        let text = normalize_for_dedup(&format!(
            "{} {} {}",
            member.window_title, member.snippet, member.clean_text
        ));
        !text.is_empty() && text.contains(&query_norm)
    });

    if exact_phrase {
        return "Exact phrase match".to_string();
    }

    let overlap = members
        .iter()
        .map(|member| {
            token_overlap(
                &query_norm,
                &normalize_for_dedup(&format!("{} {}", member.snippet, member.clean_text)),
            )
        })
        .fold(0.0_f32, f32::max);

    if overlap >= 0.55 {
        return "Strong semantic match".to_string();
    }

    if let Some(domain) = extract_domain(anchor.url.as_deref()) {
        let lowered_domain = domain.to_lowercase();
        if query_norm.contains(&lowered_domain) {
            return format!("Matched source {}", domain);
        }
    }

    format!("Related to {}", truncate_words(&anchor.window_title, 4))
}

fn build_context(anchor: &SearchResult, snippets: &[String]) -> Vec<String> {
    let mut context = Vec::new();
    let mut seen = HashSet::new();

    if let Some(domain) = extract_domain(anchor.url.as_deref()) {
        seen.insert(domain.to_lowercase());
        context.push(domain);
    }

    for snippet in snippets {
        for entity in extract_entities(snippet) {
            let key = entity.to_lowercase();
            if key.len() < 3 || seen.contains(&key) {
                continue;
            }
            seen.insert(key);
            context.push(entity);
            if context.len() >= 4 {
                break;
            }
        }
        if context.len() >= 4 {
            break;
        }
    }

    if context.is_empty() {
        context.push(truncate_words(&anchor.window_title, 6));
    }

    context
}

fn extract_story_facts(snippets: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut facts: Vec<String> = Vec::new();

    for snippet in snippets {
        let cleaned = clean_story_fact(snippet);
        if cleaned.is_empty() {
            continue;
        }
        let lower = cleaned.to_lowercase();
        if lower.starts_with("worked in ")
            || lower == "google chrome"
            || lower.contains("new tab")
            || is_ui_chrome_phrase(&cleaned)
        {
            continue;
        }

        let key = normalize_for_dedup(&cleaned);
        if key.is_empty() || !seen.insert(key) {
            continue;
        }

        let clipped = truncate_words(&cleaned, 18);
        if clipped.split_whitespace().count() >= 4
            && !facts
                .iter()
                .any(|existing| token_overlap(existing, &clipped) >= 0.78)
        {
            facts.push(clipped);
        }
        if facts.len() >= 2 {
            break;
        }
    }

    facts
}

fn apply_story_continuity(cards: &mut [MemoryCard]) {
    for card in cards.iter_mut() {
        if let Some(cleaned) = sanitize_summary(&card.summary) {
            card.summary = cleaned;
            continue;
        }

        let fallback =
            ensure_sentence_period(&trim_trailing_fragment(&clean_story_fact(&card.summary)));
        if fallback.split_whitespace().count() >= 4 {
            card.summary = fallback;
        }
    }
}

fn clean_story_fact(value: &str) -> String {
    let mut cleaned = normalize_sentence(value);
    cleaned = cleaned.replace(" • ", " ");
    cleaned = strip_leading_transitions(&cleaned);

    let tokens = cleaned
        .split_whitespace()
        .filter(|token| !looks_like_diff_stat(token))
        .collect::<Vec<_>>();
    trim_trailing_fragment(&normalize_sentence(&tokens.join(" ")))
}

fn split_sentences_preserving_decimals(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let bytes = value.as_bytes();

    for (idx, ch) in value.char_indices() {
        if !matches!(ch, '.' | '!' | '?') {
            continue;
        }

        // Keep decimal values like 35.1 intact.
        if ch == '.'
            && idx > 0
            && idx + 1 < value.len()
            && bytes[idx - 1].is_ascii_digit()
            && bytes[idx + 1].is_ascii_digit()
        {
            continue;
        }

        let candidate = value[start..=idx].trim();
        if !candidate.is_empty() {
            out.push(candidate.to_string());
        }
        start = idx + ch.len_utf8();
    }

    if start < value.len() {
        let tail = value[start..].trim();
        if !tail.is_empty() {
            out.push(tail.to_string());
        }
    }

    out
}

fn strip_leading_transitions(value: &str) -> String {
    let mut out = value.trim().to_string();
    let prefixes = [
        "then, ",
        "then ",
        "and then ",
        "after that, ",
        "after that ",
        "next, ",
        "next ",
    ];

    loop {
        let mut stripped = false;
        let trimmed = out.trim_start();
        for prefix in prefixes {
            if starts_with_ascii_case_insensitive(trimmed, prefix) {
                out = trimmed[prefix.len()..]
                    .trim_start_matches([' ', ',', ':', ';', '-'])
                    .to_string();
                stripped = true;
                break;
            }
        }
        if !stripped {
            break;
        }
    }
    out.trim().to_string()
}

fn starts_with_ascii_case_insensitive(value: &str, prefix: &str) -> bool {
    if value.len() < prefix.len() {
        return false;
    }
    value[..prefix.len()].eq_ignore_ascii_case(prefix)
}

fn looks_like_diff_stat(token: &str) -> bool {
    let trimmed = token.trim_matches(|ch: char| matches!(ch, ',' | ';' | ':' | '.' | ')' | '('));
    if trimmed.len() < 2 {
        return false;
    }
    let mut chars = trimmed.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if first != '+' && first != '-' {
        return false;
    }
    chars.all(|ch| ch.is_ascii_digit())
}

fn extract_entities(text: &str) -> Vec<String> {
    let stop = stop_words();
    let mut entities = Vec::new();

    for token in text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|tok| tok.len() > 2)
    {
        let lower = token.to_lowercase();
        if stop.contains(lower.as_str()) {
            continue;
        }
        if token.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        entities.push(token.to_string());
        if entities.len() >= 3 {
            break;
        }
    }

    entities
}

fn stop_words() -> HashSet<&'static str> {
    [
        "the", "and", "for", "with", "that", "from", "this", "have", "into", "while", "open",
        "page", "about", "using", "user", "you", "your", "their",
    ]
    .into_iter()
    .collect()
}

fn truncate_words(text: &str, max_words: usize) -> String {
    text.split_whitespace()
        .take(max_words)
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_sentence(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches(|ch: char| ch == '"' || ch == '\'' || ch == '`')
        .to_string()
}

fn trim_trailing_fragment(value: &str) -> String {
    let mut words = value
        .split_whitespace()
        .map(|token| token.trim_matches(|ch: char| matches!(ch, ',' | ';' | ':' | '-')))
        .filter(|token| !token.is_empty())
        .map(|token| token.to_string())
        .collect::<Vec<_>>();

    let dangling = [
        "for",
        "with",
        "and",
        "or",
        "to",
        "of",
        "in",
        "on",
        "by",
        "at",
        "from",
        "via",
        "about",
        "after",
        "before",
        "than",
        "then",
        "while",
        "during",
        "including",
    ];

    while let Some(last) = words.last() {
        let last_lc = last.to_lowercase();
        if dangling.contains(&last_lc.as_str()) {
            words.pop();
            continue;
        }
        break;
    }

    normalize_sentence(&words.join(" "))
}

fn token_overlap(a: &str, b: &str) -> f32 {
    let left = tokenize(a);
    let right = tokenize(b);
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }

    let intersection = left.intersection(&right).count() as f32;
    let union = left.union(&right).count() as f32;
    if union == 0.0 {
        0.0
    } else {
        intersection / union
    }
}

fn tokenize(text: &str) -> HashSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|tok| tok.len() > 2)
        .map(|tok| tok.to_string())
        .collect()
}

fn normalize_for_dedup(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || ch.is_whitespace() {
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

fn extract_domain(url: Option<&str>) -> Option<String> {
    let url = url?.trim();
    if url.is_empty() {
        return None;
    }

    let host = url
        .split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or_default()
        .trim();

    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

fn same_effective_url(left: Option<&str>, right: Option<&str>) -> bool {
    let Some(left) = left else {
        return false;
    };
    let Some(right) = right else {
        return false;
    };
    normalize_effective_url(left) == normalize_effective_url(right)
}

fn normalize_effective_url(raw: &str) -> String {
    let lowered = raw.trim().to_lowercase();
    if lowered.is_empty() {
        return String::new();
    }
    let no_scheme = lowered
        .strip_prefix("https://")
        .or_else(|| lowered.strip_prefix("http://"))
        .unwrap_or(&lowered);
    let no_query = no_scheme.split('?').next().unwrap_or(no_scheme);
    let no_fragment = no_query.split('#').next().unwrap_or(no_query);
    no_fragment.trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_nearby_same_session_hits() {
        let base = SearchResult {
            id: "1".to_string(),
            timestamp: 1_000_000,
            app_name: "Chrome".to_string(),
            bundle_id: None,
            window_title: "IPL 2026 highlights - YouTube".to_string(),
            session_id: "s1".to_string(),
            text: "IPL highlights and score recap".to_string(),
            clean_text: "IPL highlights and score recap".to_string(),
            ocr_confidence: 0.91,
            ocr_block_count: 8,
            snippet: "Watching IPL highlights on YouTube".to_string(),
            summary_source: "llm".to_string(),
            noise_score: 0.1,
            session_key: "chrome:youtube:ipl".to_string(),
            lexical_shadow: String::new(),
            score: 0.8,
            screenshot_path: None,
            url: Some("https://www.youtube.com/watch?v=123".to_string()),
            decay_score: 1.0,
            ..Default::default()
        };

        let mut second = base.clone();
        second.id = "2".to_string();
        second.timestamp -= 60_000;
        second.snippet = "Searching for cricket highlights".to_string();

        let groups = group_results(&[base, second], 6);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].members.len(), 2);
    }

    #[test]
    fn rejects_bad_summary_patterns() {
        assert!(sanitize_summary("The screen shows New Tab and toolbar labels.").is_none());
        assert!(sanitize_summary(
            "Reviewed IPL highlights on YouTube while comparing match statistics."
        )
        .is_some());
    }

    #[test]
    fn fallback_produces_contextual_summary() {
        let anchor = SearchResult {
            id: "1".to_string(),
            timestamp: 1,
            app_name: "Chrome".to_string(),
            bundle_id: None,
            window_title: "YouTube - Cricket".to_string(),
            session_id: "s".to_string(),
            text: "".to_string(),
            clean_text: "".to_string(),
            ocr_confidence: 0.8,
            ocr_block_count: 4,
            snippet: "".to_string(),
            summary_source: "fallback".to_string(),
            noise_score: 0.2,
            session_key: "chrome:youtube:cricket".to_string(),
            lexical_shadow: String::new(),
            score: 0.4,
            screenshot_path: None,
            url: Some("https://www.youtube.com/results?search_query=cricket".to_string()),
            decay_score: 1.0,
            ..Default::default()
        };

        let (_, summary, _, _) = deterministic_fallback(
            "cricket",
            &anchor,
            &["IPL highlights and score table".to_string()],
        );

        assert!(summary.matches('.').count() <= 2);
        assert!(!summary.to_lowercase().contains("new tab"));
        assert!(!summary.to_lowercase().contains("worked in google chrome"));
        assert!(
            summary.to_lowercase().contains("ipl") || summary.to_lowercase().contains("cricket")
        );
    }

    #[test]
    fn does_not_group_cross_app_without_shared_url() {
        let a = SearchResult {
            id: "a".to_string(),
            timestamp: 2_000_000,
            app_name: "Codex".to_string(),
            bundle_id: None,
            window_title: "Daily notes".to_string(),
            session_id: "s1".to_string(),
            text: "Updated memory pipeline and fixed tests".to_string(),
            clean_text: "Updated memory pipeline and fixed tests".to_string(),
            ocr_confidence: 0.9,
            ocr_block_count: 7,
            snippet: "Updated memory pipeline".to_string(),
            summary_source: "llm".to_string(),
            noise_score: 0.1,
            session_key: String::new(),
            lexical_shadow: String::new(),
            score: 0.7,
            screenshot_path: None,
            url: None,
            decay_score: 1.0,
            ..Default::default()
        };
        let mut b = a.clone();
        b.id = "b".to_string();
        b.app_name = "Google Chrome".to_string();
        b.url = Some("https://example.com/startup-evaluation".to_string());
        b.text = "Startup evaluation rubric and full points".to_string();
        b.clean_text = b.text.clone();
        b.snippet = "Startup evaluation rubric".to_string();

        let groups = group_results(&[a, b], 6);
        assert_eq!(groups.len(), 2);
    }

    #[tokio::test]
    async fn llm_group_policy_still_returns_deterministic_cards_without_inference() {
        let base = SearchResult {
            id: "r1".to_string(),
            timestamp: 3_000_000,
            app_name: "VS Code".to_string(),
            bundle_id: None,
            window_title: "memory_cards.rs".to_string(),
            session_id: "s42".to_string(),
            text: "Refined memory card synthesis fallback behavior".to_string(),
            clean_text: "Refined memory card synthesis fallback behavior".to_string(),
            ocr_confidence: 0.93,
            ocr_block_count: 7,
            snippet: "Refined memory card synthesis fallback behavior".to_string(),
            summary_source: "fallback".to_string(),
            noise_score: 0.08,
            session_key: "vscode:continuum:memory_cards".to_string(),
            lexical_shadow: String::new(),
            score: 0.84,
            screenshot_path: None,
            url: None,
            decay_score: 1.0,
            ..Default::default()
        };
        let mut second = base.clone();
        second.id = "r2".to_string();
        second.timestamp -= 120_000;
        second.snippet =
            "Added deterministic fallback when LLM synthesis is unavailable".to_string();

        let cards = MemoryCardSynthesizer::from_results_with_policy(
            None,
            "fallback",
            &[base, second],
            6,
            3,
            Duration::from_millis(2),
        )
        .await;

        assert!(!cards.is_empty());
        assert!(cards.iter().all(|card| !card.summary.trim().is_empty()));
        assert!(cards.iter().all(|card| card.source_count >= 1));
    }

    #[test]
    fn reopen_target_prefers_typed_provenance_over_legacy_marker() {
        let result = SearchResult {
            reopen_kind: crate::memory::reopen::ReopenKind::BrowserUrl,
            reopen_url: Some("https://typed.example/path".to_string()),
            url: Some("https://record.example".to_string()),
            files_touched: vec!["/tmp/legacy.txt".to_string()],
            ..Default::default()
        };

        let parsed = parse_reopen_target("Reopen: https://legacy.example/path", &result);
        assert_eq!(parsed.as_deref(), Some("https://typed.example/path"));
    }

    #[test]
    fn reopen_target_allows_legacy_marker_fallback_for_unknown_rows() {
        let result = SearchResult {
            reopen_kind: crate::memory::reopen::ReopenKind::Unknown,
            ..Default::default()
        };

        let parsed = parse_reopen_target("Reopen: https://legacy.example/path", &result);
        assert_eq!(parsed.as_deref(), Some("https://legacy.example/path"));
    }
}
