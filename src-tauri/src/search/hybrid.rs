//! Hybrid search combining semantic and keyword retrieval with query understanding.

use crate::capture::text_cleanup;
use crate::config::SearchConfig;
use crate::embed::{Embedder, EmbeddingBackend};
use crate::store::{SearchResult, Store};
use std::collections::{HashMap, HashSet};
use tokio::time::{timeout, Duration, Instant};

/// Hybrid searcher combining semantic + lexical retrieval and sentence-aware reranking.
pub struct HybridSearcher;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueryIntent {
    Definition,
    HowTo,
    Lookup,
    General,
}

#[derive(Debug, Clone)]
struct QueryProfile {
    raw: String,
    normalized: String,
    intent: QueryIntent,
    wants_recency: bool,
    primary_terms: Vec<String>,
    expanded_terms: Vec<String>,
    number_terms: HashSet<String>,
    phrase: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct FusionSignals {
    semantic_score: Option<f32>,
    snippet_score: Option<f32>,
    keyword_score: Option<f32>,
    lexical_score: f32,
    coverage: f32,
    phrase_score: f32,
}

impl QueryProfile {
    fn from_query(query: &str) -> Self {
        let normalized = normalize_text(query);
        let mut tokens = token_vec(&normalized);

        if tokens.is_empty() {
            return Self {
                raw: query.to_string(),
                normalized,
                intent: QueryIntent::General,
                wants_recency: false,
                primary_terms: Vec::new(),
                expanded_terms: Vec::new(),
                number_terms: HashSet::new(),
                phrase: None,
            };
        }

        let mut number_terms = HashSet::new();
        for token in &tokens {
            if token.chars().any(|ch| ch.is_ascii_digit()) {
                number_terms.insert(token.clone());
            }
        }

        let intent = detect_intent(&normalized);
        let wants_recency = query_wants_recency(&normalized);

        let mut primary_terms = tokens
            .iter()
            .filter(|token| !is_stop_word(token) || token.chars().any(|ch| ch.is_ascii_digit()))
            .cloned()
            .collect::<Vec<_>>();

        if primary_terms.is_empty() {
            primary_terms = tokens.clone();
        }

        // Keep sentence-level search usable by preserving high-signal content terms.
        primary_terms.truncate(8);

        let mut expanded_terms = Vec::new();
        for token in &primary_terms {
            push_unique(&mut expanded_terms, token);

            let stem = stem_token(token);
            if !stem.is_empty() {
                push_unique(&mut expanded_terms, &stem);
            }

            if token.len() > 4 && token.ends_with('s') {
                push_unique(&mut expanded_terms, &token[..token.len() - 1]);
            }
            if token.len() > 3 && !token.ends_with('s') {
                let plural = format!("{}s", token);
                push_unique(&mut expanded_terms, &plural);
            }
        }

        tokens.clear();

        let phrase = if normalized.split_whitespace().count() >= 2 {
            Some(normalized.clone())
        } else {
            None
        };

        Self {
            raw: query.to_string(),
            normalized,
            intent,
            wants_recency,
            primary_terms,
            expanded_terms,
            number_terms,
            phrase,
        }
    }

    fn is_empty(&self) -> bool {
        self.normalized.is_empty()
    }

    fn keyword_variants(&self, max_variants: usize) -> Vec<String> {
        let mut variants = Vec::new();

        if let Some(phrase) = self.phrase.as_ref() {
            push_unique(&mut variants, phrase);
        }

        if !self.number_terms.is_empty() {
            for value in &self.number_terms {
                push_unique(&mut variants, value);
            }
        }

        if !self.primary_terms.is_empty() {
            let joined = self
                .primary_terms
                .iter()
                .take(4)
                .cloned()
                .collect::<Vec<_>>()
                .join(" ");
            if !joined.is_empty() {
                push_unique(&mut variants, &joined);
            }

            for pair in self.primary_terms.windows(2).take(3) {
                push_unique(&mut variants, &pair.join(" "));
            }
        }

        let mut ranked_terms = self
            .primary_terms
            .iter()
            .filter(|term| !is_weak_query_term(term))
            .cloned()
            .collect::<Vec<_>>();
        ranked_terms
            .sort_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));

        for term in ranked_terms.iter().take(4) {
            push_unique(&mut variants, term);
        }
        for term in self.primary_terms.iter().take(6) {
            push_unique(&mut variants, term);
        }

        if variants.is_empty() && !self.raw.trim().is_empty() {
            variants.push(self.raw.trim().to_string());
        }

        variants.truncate(max_variants.max(1));
        variants
    }

    fn embedding_query(&self) -> String {
        let mut parts = Vec::new();

        if !self.raw.trim().is_empty() {
            parts.push(self.raw.trim().to_string());
        }

        let compact_terms = self
            .primary_terms
            .iter()
            .take(6)
            .cloned()
            .collect::<Vec<_>>();

        if !compact_terms.is_empty() {
            parts.push(compact_terms.join(" "));
        }

        let with_numbers = self
            .number_terms
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join(" ");
        if !with_numbers.is_empty() {
            parts.push(with_numbers);
        }

        let joined = parts.join(" ").trim().to_string();
        if joined.is_empty() {
            String::new()
        } else {
            format!(
                "Represent this sentence for searching relevant passages: {}",
                joined
            )
        }
    }

    fn is_short_intent_query(&self) -> bool {
        self.primary_terms.len() <= 2 && self.intent == QueryIntent::General
    }
}

impl HybridSearcher {
    /// Product-named wrapper for the core retrieval boundary.
    pub async fn search_hybrid_memories(
        store: &Store,
        embedder: &Embedder,
        query: &str,
        limit: usize,
        time_filter: Option<&str>,
        app_filter: Option<&str>,
        search_config: &SearchConfig,
    ) -> Result<Vec<SearchResult>, Box<dyn std::error::Error>> {
        Self::search_with_config(
            store,
            embedder,
            query,
            limit,
            time_filter,
            app_filter,
            search_config,
        )
        .await
    }

    /// Perform hybrid search with query understanding, weighted fusion, and reranking.
    pub async fn search(
        store: &Store,
        embedder: &Embedder,
        query: &str,
        limit: usize,
        time_filter: Option<&str>,
        app_filter: Option<&str>,
    ) -> Result<Vec<SearchResult>, Box<dyn std::error::Error>> {
        let search_config = SearchConfig::default();
        Self::search_with_config(
            store,
            embedder,
            query,
            limit,
            time_filter,
            app_filter,
            &search_config,
        )
        .await
    }

    /// Perform hybrid search with explicit runtime tuning.
    pub async fn search_with_config(
        store: &Store,
        embedder: &Embedder,
        query: &str,
        limit: usize,
        time_filter: Option<&str>,
        app_filter: Option<&str>,
        search_config: &SearchConfig,
    ) -> Result<Vec<SearchResult>, Box<dyn std::error::Error>> {
        let started = Instant::now();
        let profile = QueryProfile::from_query(query);
        if profile.is_empty() {
            return Ok(Vec::new());
        }
        let search_config = search_config.clone().normalized();
        let semantic_branch_timeout = Duration::from_millis(search_config.semantic_timeout_ms);
        let snippet_branch_timeout = Duration::from_millis(search_config.snippet_timeout_ms);
        let keyword_total_timeout = Duration::from_millis(search_config.keyword_timeout_ms);
        let keyword_variant_timeout =
            Duration::from_millis(search_config.keyword_variant_timeout_ms);

        tracing::info!(
            query = %query,
            limit,
            time_filter = ?time_filter,
            app_filter = ?app_filter,
            "hybrid_search:start"
        );

        let base_limit = limit.max(1);
        let semantic_branch_limit = (base_limit * search_config.candidate_multiplier)
            .min(search_config.max_semantic_branch_limit);
        let keyword_branch_limit = (base_limit * 2)
            .min(search_config.max_keyword_branch_limit)
            .max(base_limit);
        let semantic_enabled = matches!(embedder.backend(), EmbeddingBackend::Real);
        let query_embedding = if semantic_enabled {
            let embedding_query = profile.embedding_query();
            match embedder.embed_batch(&[embedding_query]) {
                Ok(vectors) => Some(vectors.into_iter().next().unwrap_or_default()),
                Err(err) => {
                    tracing::warn!(err = %err, "hybrid_search:embed_failed");
                    None
                }
            }
        } else {
            None
        };

        let allow_snippet_branch = query_embedding.is_some()
            && profile.primary_terms.len() >= search_config.min_snippet_query_terms;
        let semantic_fut = async {
            let branch_started = Instant::now();
            let mut timed_out = false;
            let results = if let Some(query_embedding) = query_embedding.as_ref() {
                match timeout(
                    semantic_branch_timeout,
                    store.vector_search(
                        query_embedding,
                        semantic_branch_limit,
                        time_filter,
                        app_filter,
                    ),
                )
                .await
                {
                    Ok(Ok(results)) => results,
                    Ok(Err(err)) => {
                        tracing::warn!(err = %err, "hybrid_search:semantic_failed");
                        Vec::new()
                    }
                    Err(_) => {
                        timed_out = true;
                        tracing::warn!(
                            timeout_ms = semantic_branch_timeout.as_millis(),
                            "hybrid_search:semantic_timeout"
                        );
                        Vec::new()
                    }
                }
            } else {
                Vec::new()
            };
            (results, branch_started.elapsed(), timed_out)
        };
        let snippet_fut = async {
            let branch_started = Instant::now();
            let mut timed_out = false;
            let results = if allow_snippet_branch {
                if let Some(query_embedding) = query_embedding.as_ref() {
                    match timeout(
                        snippet_branch_timeout,
                        store.snippet_vector_search(
                            query_embedding,
                            semantic_branch_limit,
                            time_filter,
                            app_filter,
                        ),
                    )
                    .await
                    {
                        Ok(Ok(results)) => results,
                        Ok(Err(err)) => {
                            tracing::warn!(err = %err, "hybrid_search:snippet_failed");
                            Vec::new()
                        }
                        Err(_) => {
                            timed_out = true;
                            tracing::warn!(
                                timeout_ms = snippet_branch_timeout.as_millis(),
                                "hybrid_search:snippet_timeout"
                            );
                            Vec::new()
                        }
                    }
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            };
            (results, branch_started.elapsed(), timed_out)
        };
        let keyword_fut = async {
            let branch_started = Instant::now();
            let mut timed_out = false;
            let results = match timeout(
                keyword_total_timeout,
                Self::keyword_search_with_budget(
                    store,
                    &profile,
                    keyword_branch_limit,
                    time_filter,
                    app_filter,
                    &search_config,
                    keyword_total_timeout,
                    keyword_variant_timeout,
                ),
            )
            .await
            {
                Ok(Ok(results)) => results,
                Ok(Err(err)) => {
                    tracing::warn!(err = %err, "hybrid_search:keyword_failed");
                    Vec::new()
                }
                Err(_) => {
                    timed_out = true;
                    tracing::warn!(
                        timeout_ms = keyword_total_timeout.as_millis(),
                        "hybrid_search:keyword_timeout"
                    );
                    Vec::new()
                }
            };
            (results, branch_started.elapsed(), timed_out)
        };

        let (
            (semantic_results, semantic_elapsed, semantic_timed_out),
            (snippet_results, snippet_elapsed, snippet_timed_out),
            (keyword_results, keyword_elapsed, keyword_timed_out),
        ) = tokio::join!(semantic_fut, snippet_fut, keyword_fut);

        tracing::info!(
            semantic_count = semantic_results.len(),
            snippet_count = snippet_results.len(),
            keyword_count = keyword_results.len(),
            semantic_ms = semantic_elapsed.as_millis(),
            snippet_ms = snippet_elapsed.as_millis(),
            keyword_ms = keyword_elapsed.as_millis(),
            semantic_timed_out,
            snippet_timed_out,
            keyword_timed_out,
            semantic_enabled,
            snippet_enabled = allow_snippet_branch,
            elapsed_ms = started.elapsed().as_millis(),
            "hybrid_search:branches_complete"
        );

        let fused = Self::hybrid_fusion(
            &profile,
            &semantic_results,
            &snippet_results,
            &keyword_results,
            &search_config,
        );
        let reranked = Self::rerank_with_profile(&profile, fused, limit, &search_config);
        tracing::info!(
            results = reranked.len(),
            elapsed_ms = started.elapsed().as_millis(),
            "hybrid_search:complete"
        );

        Ok(reranked)
    }

    async fn keyword_search_with_budget(
        store: &Store,
        profile: &QueryProfile,
        branch_limit: usize,
        time_filter: Option<&str>,
        app_filter: Option<&str>,
        search_config: &SearchConfig,
        keyword_total_timeout: Duration,
        keyword_variant_timeout: Duration,
    ) -> Result<Vec<SearchResult>, String> {
        let variants = profile.keyword_variants(search_config.max_keyword_variants);
        if variants.is_empty() {
            return Ok(Vec::new());
        }

        let started = Instant::now();
        let target_hits = branch_limit.min(18).max(8);
        let mut by_id: HashMap<String, SearchResult> = HashMap::new();

        for (variant_idx, variant) in variants.iter().enumerate() {
            if variant_idx > search_config.max_keyword_fallback_variants {
                break;
            }
            if by_id.len() >= target_hits && variant_idx > 0 {
                break;
            }
            if started.elapsed() >= keyword_total_timeout {
                tracing::warn!(
                    timeout_ms = keyword_total_timeout.as_millis(),
                    "hybrid_search:keyword_budget_exhausted"
                );
                break;
            }

            let hits = match timeout(
                keyword_variant_timeout,
                store.keyword_search(variant, branch_limit, time_filter, app_filter),
            )
            .await
            {
                Ok(Ok(hits)) => hits,
                Ok(Err(err)) => {
                    tracing::warn!(
                        variant_idx,
                        variant = %variant,
                        err = %err,
                        "hybrid_search:keyword_variant_failed"
                    );
                    continue;
                }
                Err(_) => {
                    tracing::warn!(
                        variant_idx,
                        variant = %variant,
                        timeout_ms = keyword_variant_timeout.as_millis(),
                        "hybrid_search:keyword_variant_timeout"
                    );
                    continue;
                }
            };

            let decay = if variant_idx == 0 {
                1.0
            } else {
                (1.0 - (variant_idx as f32 * 0.07)).max(0.84)
            };

            for mut hit in hits {
                hit.score *= decay;
                by_id
                    .entry(hit.id.clone())
                    .and_modify(|existing| {
                        if hit.score > existing.score
                            || (hit.score == existing.score && hit.timestamp > existing.timestamp)
                        {
                            *existing = hit.clone();
                        }
                    })
                    .or_insert(hit);
            }

            tracing::info!(
                variant_idx,
                variant = %variant,
                dedup_hits = by_id.len(),
                elapsed_ms = started.elapsed().as_millis(),
                "hybrid_search:keyword_variant_complete"
            );
        }

        let mut deduped = by_id.into_values().collect::<Vec<_>>();
        deduped.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.timestamp.cmp(&a.timestamp))
        });
        deduped.truncate(branch_limit.max(1));
        Ok(deduped)
    }

    /// Merge semantic + keyword candidates, then rerank with the standard policy.
    pub fn fuse_and_rerank(
        query: &str,
        semantic: &[SearchResult],
        keyword: &[SearchResult],
        limit: usize,
    ) -> Vec<SearchResult> {
        let profile = QueryProfile::from_query(query);
        let config = SearchConfig::default();
        let fused = Self::hybrid_fusion(&profile, semantic, &[], keyword, &config);
        Self::rerank_with_profile(&profile, fused, limit, &config)
    }

    fn hybrid_fusion(
        profile: &QueryProfile,
        semantic: &[SearchResult],
        snippet: &[SearchResult],
        keyword: &[SearchResult],
        search_config: &SearchConfig,
    ) -> Vec<SearchResult> {
        let mut signals: HashMap<String, FusionSignals> = HashMap::new();
        let mut candidates: HashMap<String, SearchResult> = HashMap::new();

        for result in semantic {
            candidates
                .entry(result.id.clone())
                .and_modify(|existing| {
                    if result.score > existing.score {
                        *existing = result.clone();
                    }
                })
                .or_insert_with(|| result.clone());

            signals
                .entry(result.id.clone())
                .and_modify(|signal| {
                    signal.semantic_score = Some(
                        signal
                            .semantic_score
                            .map(|current| current.max(result.score))
                            .unwrap_or(result.score),
                    );
                })
                .or_insert_with(|| FusionSignals {
                    semantic_score: Some(result.score),
                    ..FusionSignals::default()
                });
        }

        for result in snippet {
            candidates
                .entry(result.id.clone())
                .and_modify(|existing| {
                    if result.score > existing.score {
                        *existing = result.clone();
                    }
                })
                .or_insert_with(|| result.clone());

            signals
                .entry(result.id.clone())
                .and_modify(|signal| {
                    signal.snippet_score = Some(
                        signal
                            .snippet_score
                            .map(|current| current.max(result.score))
                            .unwrap_or(result.score),
                    );
                })
                .or_insert_with(|| FusionSignals {
                    snippet_score: Some(result.score),
                    ..FusionSignals::default()
                });
        }

        for result in keyword {
            candidates
                .entry(result.id.clone())
                .and_modify(|existing| {
                    if result.score > existing.score {
                        *existing = result.clone();
                    }
                })
                .or_insert_with(|| result.clone());

            signals
                .entry(result.id.clone())
                .and_modify(|signal| {
                    signal.keyword_score = Some(
                        signal
                            .keyword_score
                            .map(|current| current.max(result.score))
                            .unwrap_or(result.score),
                    );
                })
                .or_insert_with(|| FusionSignals {
                    keyword_score: Some(result.score),
                    ..FusionSignals::default()
                });
        }

        let mut docs = Vec::new();
        for result in candidates.values() {
            let merged = merged_candidate_text(result);
            let doc_tokens = token_vec(&merged);
            docs.push((result.id.clone(), merged, doc_tokens));
        }

        let avg_len = if docs.is_empty() {
            1.0
        } else {
            docs.iter()
                .map(|(_, _, tokens)| tokens.len() as f32)
                .sum::<f32>()
                / docs.len() as f32
        }
        .max(1.0);

        let doc_freq = build_doc_frequency(profile, &docs);

        for (id, text, tokens) in &docs {
            let lexical = bm25_like_score(profile, text, tokens, &doc_freq, docs.len(), avg_len)
                + signals.get(id).and_then(|s| s.keyword_score).unwrap_or(0.0) * 0.55;

            let coverage = term_coverage(profile, text);
            let phrase = phrase_alignment(profile, text);

            if let Some(signal) = signals.get_mut(id) {
                signal.lexical_score = lexical;
                signal.coverage = coverage;
                signal.phrase_score = phrase;
            }
        }

        let has_semantic_signals = !semantic.is_empty() || !snippet.is_empty();

        let semantic_values = signals
            .values()
            .map(|s| s.semantic_score.unwrap_or(0.0))
            .collect::<Vec<_>>();
        let snippet_values = signals
            .values()
            .map(|s| s.snippet_score.unwrap_or(0.0))
            .collect::<Vec<_>>();
        let lexical_values = signals
            .values()
            .map(|s| s.lexical_score)
            .collect::<Vec<_>>();

        let semantic_range = value_range(&semantic_values);
        let snippet_range = value_range(&snippet_values);
        let lexical_range = value_range(&lexical_values);

        let mut fused = Vec::new();
        for (id, mut result) in candidates {
            let signal = signals.get(&id).cloned().unwrap_or_default();

            let semantic_norm =
                normalize_range(signal.semantic_score.unwrap_or(0.0), semantic_range);
            let snippet_norm = normalize_range(signal.snippet_score.unwrap_or(0.0), snippet_range);
            let lexical_norm = normalize_range(signal.lexical_score, lexical_range);

            let (semantic_weight, snippet_weight, lexical_weight) =
                fusion_weights(profile, has_semantic_signals, search_config);
            let mut score = semantic_norm * semantic_weight
                + snippet_norm * snippet_weight
                + lexical_norm * lexical_weight;
            score += signal.coverage * 0.12;
            score += signal.phrase_score * 0.08;

            // Guardrail: pure semantic neighbors with weak lexical grounding
            // should not outrank anchored matches for vague/similar queries.
            if signal.keyword_score.is_none()
                && signal.coverage < 0.16
                && signal.phrase_score < 0.10
            {
                score *= 0.72;
            }

            if signal.semantic_score.is_some() && signal.keyword_score.is_some() {
                score += 0.05;
            }

            if profile.intent == QueryIntent::Definition
                && mentions_query_entities(profile, &merged_candidate_text(&result))
            {
                score += 0.04;
            }

            result.score = score.max(0.0);
            fused.push(result);
        }

        fused.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.timestamp.cmp(&a.timestamp))
        });

        fused
    }

    pub fn rerank(query: &str, candidates: Vec<SearchResult>, limit: usize) -> Vec<SearchResult> {
        let profile = QueryProfile::from_query(query);
        Self::rerank_with_profile(&profile, candidates, limit, &SearchConfig::default())
    }

    fn rerank_with_profile(
        profile: &QueryProfile,
        mut candidates: Vec<SearchResult>,
        limit: usize,
        search_config: &SearchConfig,
    ) -> Vec<SearchResult> {
        if candidates.is_empty() {
            return Vec::new();
        }

        candidates.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.timestamp.cmp(&a.timestamp))
        });
        candidates.truncate(search_config.max_rerank_pool.max(limit * 3));

        let query_lower = profile.normalized.clone();
        let code_query = is_code_query(&profile.raw);

        let mut session_counts: HashMap<String, usize> = HashMap::new();
        for candidate in &candidates {
            let key = session_key(candidate);
            *session_counts.entry(key).or_insert(0) += 1;
        }
        let max_timestamp = candidates
            .iter()
            .map(|candidate| candidate.timestamp)
            .max()
            .unwrap_or(0);
        let min_timestamp = candidates
            .iter()
            .map(|candidate| candidate.timestamp)
            .min()
            .unwrap_or(max_timestamp);
        let timestamp_span = (max_timestamp - min_timestamp).max(1) as f32;

        let now = chrono::Utc::now().timestamp_millis();
        let anchor_terms = query_anchor_terms(profile);
        let require_anchor = should_require_anchor(profile, &anchor_terms);

        for candidate in &mut candidates {
            let mut score = candidate.score;
            let text = candidate_text(candidate);
            let text_norm = normalize_text(&text);
            let title_norm = normalize_text(&candidate.window_title);
            let snippet_norm = normalize_text(&candidate.snippet);
            let merged_norm = format!("{} {} {}", title_norm, text_norm, snippet_norm);

            // Query-aware sentence reranker feature.
            let sentence_relevance = sentence_level_relevance(profile, &merged_norm);
            score *= 0.70 + sentence_relevance * 0.58;

            // Time decay (gentle).
            let age_hours =
                ((now - candidate.timestamp).max(0) as f32 / 3_600_000.0).min(24.0 * 30.0);
            let age_decay = if profile.wants_recency {
                1.0 / (1.0 + age_hours * 0.0048)
            } else {
                1.0 / (1.0 + age_hours * 0.0012)
            };
            score *= age_decay;
            if profile.wants_recency {
                let relative_freshness =
                    ((candidate.timestamp - min_timestamp).max(0) as f32) / timestamp_span;
                score *= 1.0 + relative_freshness * 0.28;
            }

            // Penalties.
            if is_generic_title(&candidate.window_title) {
                score *= 0.72;
            }
            if candidate.ocr_confidence > 0.0 {
                score *= 0.75 + candidate.ocr_confidence.min(1.0) * 0.35;
            }
            if candidate.noise_score > 0.0 {
                score *= (1.0 - (candidate.noise_score * 0.35)).max(0.45);
            }
            if text_cleanup::symbol_ratio(&text_norm) > 0.46 {
                score *= 0.7;
            }
            if snippet_norm.split_whitespace().count() < 4 {
                score *= 0.82;
            }
            if looks_like_browser_chrome(&text_norm, &title_norm) {
                score *= 0.62;
            }
            if !code_query
                && (text_cleanup::looks_like_file_inventory(&text_norm)
                    || looks_like_json_dump(&text_norm))
            {
                score *= 0.55;
            }

            // Boosts.
            let coverage = term_coverage(profile, &merged_norm);
            if coverage > 0.0 {
                score *= 1.0 + coverage.min(0.85) * 0.28;
            } else if !profile.primary_terms.is_empty() {
                score *= 0.68;
            }

            if require_anchor && !has_anchor_term(profile, &anchor_terms, &merged_norm) {
                score *= 0.22;
            }

            let source_alignment = query_source_alignment(
                profile,
                &title_norm,
                &candidate.app_name,
                candidate.url.as_deref(),
                &text_norm,
                &snippet_norm,
            );
            score *= 0.72 + source_alignment * 0.48;

            if profile.primary_terms.len() == 1 && source_alignment < 0.30 {
                score *= 0.68;
            }

            let entity_overlap = named_entity_overlap(&profile.raw, &text);
            if entity_overlap > 0 {
                score *= 1.0 + (entity_overlap as f32 * 0.06).min(0.18);
            }

            if !profile.number_terms.is_empty() && !mentions_query_entities(profile, &merged_norm) {
                score *= 0.5;
            }

            if let Some(url) = &candidate.url {
                let domain = extract_domain(url);
                if !domain.is_empty() && query_lower.contains(&domain) {
                    score *= 1.18;
                }
                if !domain.is_empty() && profile.primary_terms.iter().any(|t| domain.contains(t)) {
                    score *= 1.08;
                }
            }
            if !candidate.app_name.trim().is_empty() {
                let app_norm = normalize_text(&candidate.app_name);
                if !app_norm.is_empty() && query_lower.contains(&app_norm) {
                    score *= 1.16;
                }
            }

            if candidate.summary_source.eq_ignore_ascii_case("llm") {
                score *= 1.05;
            }

            let coherence = session_counts
                .get(&session_key(candidate))
                .copied()
                .unwrap_or(1);
            if coherence > 1 {
                score *= 1.0 + (coherence as f32 * 0.04).min(0.16);
            }

            let signal_strength = estimate_result_signal_strength(candidate);
            score *= 0.78 + signal_strength * 0.32;

            let age_days = ((now - candidate.timestamp).max(0) as f32 / 86_400_000.0).max(0.0);
            let retention_days = retention_days_for_signal(signal_strength);
            if age_days > retention_days {
                let overage_ratio =
                    ((age_days - retention_days) / retention_days.max(1.0)).clamp(0.0, 1.5);
                let stale_penalty = if query_requires_freshness(profile) {
                    (1.0 - 0.78 * overage_ratio).clamp(0.12, 1.0)
                } else {
                    (1.0 - 0.30 * overage_ratio).clamp(0.58, 1.0)
                };
                score *= stale_penalty;
            }

            // Ebbinghaus decay: recent + accessed records score higher.
            score *= candidate.decay_score.max(search_config.decay_floor);

            candidate.score = score;
        }

        candidates.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.timestamp.cmp(&a.timestamp))
        });

        // Session-level fuzzy dedup.
        let mut deduped = Vec::new();
        let mut seen_texts: Vec<String> = Vec::new();
        let mut seen_sessions: HashMap<String, usize> = HashMap::new();

        for candidate in candidates {
            let norm = normalize_text(&candidate_text(&candidate));
            if norm.is_empty() {
                continue;
            }

            let duplicate = seen_texts
                .iter()
                .any(|existing| fuzzy_sim(existing, &norm) >= 0.92);
            if duplicate {
                continue;
            }

            let sess = session_key(&candidate);
            let count = seen_sessions.entry(sess).or_insert(0);
            if *count >= 2 {
                continue;
            }

            *count += 1;
            seen_texts.push(norm);
            deduped.push(candidate);

            if deduped.len() >= limit.max(6).min(30) {
                break;
            }
        }

        diversify_results(
            profile,
            apply_relevance_gate(profile, deduped, search_config),
            limit.min(30),
            search_config,
        )
    }
}

fn apply_relevance_gate(
    profile: &QueryProfile,
    candidates: Vec<SearchResult>,
    search_config: &SearchConfig,
) -> Vec<SearchResult> {
    if candidates.is_empty() {
        return Vec::new();
    }

    let top_score = candidates[0].score;
    let absolute_floor = search_config.absolute_relevance_floor
        + if profile.primary_terms.len() >= 4 {
            0.04
        } else {
            0.0
        }
        + if profile.intent == QueryIntent::Definition {
            0.02
        } else {
            0.0
        };
    let effective_absolute_floor = absolute_floor.min((top_score * 0.90).max(0.01));
    let relative_floor = top_score * search_config.relative_relevance_floor;
    let min_coverage = if profile.primary_terms.len() >= 4 {
        0.30
    } else if profile.primary_terms.len() >= 3 {
        0.24
    } else if profile.primary_terms.len() >= 2 {
        0.17
    } else if profile.primary_terms.len() == 1 {
        0.18
    } else if profile.intent == QueryIntent::Definition && !profile.primary_terms.is_empty() {
        0.15
    } else {
        0.0
    };
    let anchor_terms = query_anchor_terms(profile);
    let require_anchor = should_require_anchor(profile, &anchor_terms);

    let mut filtered = Vec::new();
    for candidate in candidates {
        let merged = merged_candidate_text(&candidate);
        let coverage = term_coverage(profile, &merged);
        let has_entity = mentions_query_entities(profile, &merged);
        let has_anchor = has_anchor_term(profile, &anchor_terms, &merged);

        if candidate.score < effective_absolute_floor {
            continue;
        }

        if candidate.score < relative_floor && !filtered.is_empty() {
            continue;
        }

        if !profile.primary_terms.is_empty() && coverage < min_coverage && !has_entity {
            continue;
        }

        if require_anchor && !has_anchor {
            continue;
        }

        if !profile.number_terms.is_empty() && !has_entity {
            continue;
        }

        if query_requires_freshness(profile) {
            let age_days = ((chrono::Utc::now().timestamp_millis() - candidate.timestamp).max(0)
                as f32)
                / 86_400_000.0;
            let retention_days =
                retention_days_for_signal(estimate_result_signal_strength(&candidate));
            if age_days > retention_days {
                continue;
            }
        }

        filtered.push(candidate);
    }

    if filtered.is_empty() {
        return Vec::new();
    }

    filtered
}

fn candidate_text(result: &SearchResult) -> String {
    if !result.clean_text.trim().is_empty() {
        result.clean_text.clone()
    } else {
        result.text.clone()
    }
}

fn merged_candidate_text(result: &SearchResult) -> String {
    format!(
        "{} {} {} {}",
        result.window_title,
        candidate_text(result),
        result.snippet,
        result.url.clone().unwrap_or_default()
    )
}

fn session_key(result: &SearchResult) -> String {
    if !result.session_key.trim().is_empty() {
        return result.session_key.clone();
    }

    let domain = result
        .url
        .as_ref()
        .map(|url| extract_domain(url))
        .unwrap_or_default();
    format!(
        "{}:{}:{}",
        result.app_name.to_lowercase(),
        domain,
        normalize_text(&result.window_title)
    )
}

fn is_generic_title(title: &str) -> bool {
    matches!(
        normalize_text(title).as_str(),
        "new tab" | "home" | "untitled" | "dashboard" | "settings" | "preferences"
    )
}

fn looks_like_browser_chrome(text: &str, title: &str) -> bool {
    let lower = text.to_lowercase();
    let title_lower = title.to_lowercase();

    title_lower == "new tab"
        || lower.contains("new tab")
        || lower.contains("tab strip")
        || lower.contains("back forward")
        || lower.contains("home trending")
        || lower.contains("notifications")
}

fn looks_like_json_dump(text: &str) -> bool {
    (text.contains('{') && text.contains('}') && text.contains(':') && text.len() > 80)
        || text.contains("\"items\"")
        || text.contains("\"files\"")
}

fn named_entity_overlap(query: &str, text: &str) -> usize {
    let query_entities = extract_named_entities(query);
    if query_entities.is_empty() {
        return 0;
    }

    let text_lower = text.to_lowercase();
    query_entities
        .iter()
        .filter(|entity| text_lower.contains(entity.as_str()))
        .count()
}

fn extract_named_entities(text: &str) -> HashSet<String> {
    text.split_whitespace()
        .filter(|token| token.len() > 2)
        .filter_map(|token| {
            let clean = token
                .trim_matches(|ch: char| !ch.is_alphanumeric())
                .to_string();
            if clean.len() <= 2 {
                return None;
            }
            if clean.chars().next().is_some_and(|ch| ch.is_uppercase()) {
                Some(clean.to_lowercase())
            } else {
                None
            }
        })
        .collect()
}

fn normalize_text(value: &str) -> String {
    value
        .to_lowercase()
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

fn token_vec(value: &str) -> Vec<String> {
    normalize_text(value)
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .map(|token| token.to_string())
        .collect()
}

fn token_set_with_stems(value: &str) -> HashSet<String> {
    let mut set = HashSet::new();
    for token in token_vec(value) {
        set.insert(token.clone());
        let stem = stem_token(&token);
        if !stem.is_empty() {
            set.insert(stem);
        }
    }
    set
}

fn stem_token(token: &str) -> String {
    let lower = token.trim().to_lowercase();
    if lower.len() <= 2 {
        return lower;
    }

    if lower.len() > 4 && lower.ends_with("ies") {
        return format!("{}y", &lower[..lower.len() - 3]);
    }
    if lower.len() > 5 && lower.ends_with("ing") {
        return lower[..lower.len() - 3].to_string();
    }
    if lower.len() > 4 && lower.ends_with("ed") {
        return lower[..lower.len() - 2].to_string();
    }
    if lower.len() > 4 && lower.ends_with("es") {
        return lower[..lower.len() - 2].to_string();
    }
    if lower.len() > 3 && lower.ends_with('s') {
        return lower[..lower.len() - 1].to_string();
    }

    lower
}

fn is_stop_word(token: &str) -> bool {
    matches!(
        token,
        "a" | "an"
            | "and"
            | "are"
            | "as"
            | "at"
            | "be"
            | "by"
            | "for"
            | "from"
            | "how"
            | "i"
            | "in"
            | "is"
            | "it"
            | "me"
            | "my"
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

fn detect_intent(query: &str) -> QueryIntent {
    if query.starts_with("what is ")
        || query.starts_with("who is ")
        || query.starts_with("define ")
        || query.starts_with("explain ")
    {
        QueryIntent::Definition
    } else if query.starts_with("how to ") || query.starts_with("how do ") {
        QueryIntent::HowTo
    } else if query.starts_with("where ")
        || query.starts_with("when ")
        || query.contains(" before ")
        || query.contains(" after ")
    {
        QueryIntent::Lookup
    } else {
        QueryIntent::General
    }
}

fn query_wants_recency(query: &str) -> bool {
    query.contains("last")
        || query.contains("latest")
        || query.contains("recent")
        || query.contains("today")
        || query.contains("yesterday")
        || query.contains("just now")
        || query.contains("before")
        || query.contains("after")
}

fn query_requires_freshness(profile: &QueryProfile) -> bool {
    let query = profile.normalized.as_str();
    query.contains("today")
        || query.contains("yesterday")
        || query.contains("recent")
        || query.contains("latest")
        || query.contains("just now")
        || query.contains("currently")
}

fn push_unique(target: &mut Vec<String>, value: &str) {
    let candidate = value.trim();
    if candidate.is_empty() {
        return;
    }
    if !target.iter().any(|existing| existing == candidate) {
        target.push(candidate.to_string());
    }
}

fn build_doc_frequency(
    profile: &QueryProfile,
    docs: &[(String, String, Vec<String>)],
) -> HashMap<String, usize> {
    let mut df: HashMap<String, usize> = HashMap::new();
    for (_, text, _) in docs {
        let token_set = token_set_with_stems(text);
        for term in &profile.expanded_terms {
            if token_set.contains(term) {
                *df.entry(term.clone()).or_insert(0) += 1;
            }
        }
    }
    df
}

fn bm25_like_score(
    profile: &QueryProfile,
    text: &str,
    tokens: &[String],
    doc_freq: &HashMap<String, usize>,
    doc_count: usize,
    avg_len: f32,
) -> f32 {
    if profile.expanded_terms.is_empty() || doc_count == 0 {
        return 0.0;
    }

    let mut tf: HashMap<String, usize> = HashMap::new();
    for token in tokens {
        *tf.entry(token.clone()).or_insert(0) += 1;
        let stem = stem_token(token);
        if !stem.is_empty() {
            *tf.entry(stem).or_insert(0) += 1;
        }
    }

    let k1 = 1.2;
    let b = 0.75;
    let doc_len = tokens.len().max(1) as f32;

    let mut score = 0.0;
    for term in &profile.expanded_terms {
        let freq = tf.get(term).copied().unwrap_or(0) as f32;
        if freq <= 0.0 {
            continue;
        }

        let df = doc_freq.get(term).copied().unwrap_or(1) as f32;
        let idf = (((doc_count as f32 - df + 0.5) / (df + 0.5)) + 1.0).ln();
        let denom = freq + k1 * (1.0 - b + b * (doc_len / avg_len));
        score += idf * ((freq * (k1 + 1.0)) / denom.max(1e-6));
    }

    score += phrase_alignment(profile, text) * 0.9;
    score
}

fn term_coverage(profile: &QueryProfile, text: &str) -> f32 {
    if profile.primary_terms.is_empty() {
        return 0.0;
    }

    let token_set = token_set_with_stems(text);
    let mut matched = 0usize;

    for term in &profile.primary_terms {
        if token_set_matches_term(&token_set, term) {
            matched += 1;
        }
    }
    let mut denominator = profile.primary_terms.len().max(1);
    if !profile.number_terms.is_empty() {
        denominator += 1;
        if profile
            .number_terms
            .iter()
            .any(|number| !number.is_empty() && token_set.contains(number))
        {
            matched += 1;
        }
    }

    matched as f32 / denominator as f32
}

fn phrase_alignment(profile: &QueryProfile, text: &str) -> f32 {
    let Some(phrase) = profile.phrase.as_ref() else {
        return 0.0;
    };

    let normalized = normalize_text(text);
    if normalized.contains(phrase) {
        return 1.0;
    }

    let phrase_bigrams = bigrams(phrase);
    if phrase_bigrams.is_empty() {
        return 0.0;
    }

    let text_bigrams = bigrams(&normalized);
    if text_bigrams.is_empty() {
        return 0.0;
    }

    let overlap = phrase_bigrams
        .iter()
        .filter(|bigram| text_bigrams.contains(*bigram))
        .count();

    overlap as f32 / phrase_bigrams.len() as f32
}

fn bigrams(text: &str) -> HashSet<String> {
    let tokens = token_vec(text);
    if tokens.len() < 2 {
        return HashSet::new();
    }

    let mut out = HashSet::new();
    for pair in tokens.windows(2) {
        out.insert(format!("{} {}", pair[0], pair[1]));
    }
    out
}

fn sentence_level_relevance(profile: &QueryProfile, text: &str) -> f32 {
    let coverage = term_coverage(profile, text);
    let phrase = phrase_alignment(profile, text);
    let entity = if mentions_query_entities(profile, text) {
        1.0
    } else {
        0.0
    };

    (coverage * 0.58 + phrase * 0.28 + entity * 0.14).clamp(0.0, 1.0)
}

fn mentions_query_entities(profile: &QueryProfile, text: &str) -> bool {
    let token_set = token_set_with_stems(text);

    if !profile.number_terms.is_empty()
        && profile
            .number_terms
            .iter()
            .any(|number| token_set.contains(number))
    {
        return true;
    }

    profile
        .primary_terms
        .iter()
        .any(|term| token_set_matches_term(&token_set, term))
}

fn fusion_weights(
    profile: &QueryProfile,
    has_semantic_signals: bool,
    search_config: &SearchConfig,
) -> (f32, f32, f32) {
    if !has_semantic_signals {
        return (0.0, 0.0, 1.0);
    }

    if profile.is_short_intent_query() {
        // For short noun-like queries, lexical evidence should dominate.
        (0.24, 0.14, 0.62)
    } else {
        (
            search_config.vector_weight,
            search_config.snippet_weight,
            search_config.keyword_weight,
        )
    }
}

fn estimate_result_signal_strength(result: &SearchResult) -> f32 {
    let summary_weight = match result.summary_source.trim().to_ascii_lowercase().as_str() {
        "llm" => 1.0,
        "vlm" => 0.9,
        "fallback" => 0.66,
        _ => 0.58,
    };
    let snippet_density = (token_vec(&result.snippet).len().min(24) as f32 / 24.0).clamp(0.0, 1.0);
    let text_density =
        (token_vec(&candidate_text(result)).len().min(80) as f32 / 80.0).clamp(0.0, 1.0);

    (result.ocr_confidence.clamp(0.0, 1.0) * 0.24
        + (1.0 - result.noise_score.clamp(0.0, 1.0)) * 0.28
        + summary_weight * 0.18
        + snippet_density * 0.16
        + text_density * 0.14)
        .clamp(0.0, 1.0)
}

fn retention_days_for_signal(signal_strength: f32) -> f32 {
    14.0 + signal_strength.clamp(0.0, 1.0) * 46.0
}

fn diversify_results(
    profile: &QueryProfile,
    mut candidates: Vec<SearchResult>,
    limit: usize,
    search_config: &SearchConfig,
) -> Vec<SearchResult> {
    if candidates.is_empty() || limit == 0 {
        return Vec::new();
    }

    candidates.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.timestamp.cmp(&a.timestamp))
    });

    let desired = limit.min(30);
    let top_score = candidates
        .first()
        .map(|candidate| candidate.score)
        .unwrap_or(0.0);
    let strong_floor = search_config.strong_result_floor.min(top_score);
    let medium_floor = search_config.medium_result_floor.min(top_score);
    let preserve = search_config
        .diversity_preserve_top
        .min(desired)
        .min(candidates.len());

    let mut selected = Vec::with_capacity(desired);
    let mut app_counts: HashMap<String, usize> = HashMap::new();
    let mut source_counts: HashMap<String, usize> = HashMap::new();
    let mut time_bucket_counts: HashMap<i64, usize> = HashMap::new();

    for candidate in candidates.drain(..preserve) {
        remember_diversity_keys(
            &candidate,
            &mut app_counts,
            &mut source_counts,
            &mut time_bucket_counts,
        );
        selected.push(candidate);
    }

    while selected.len() < desired && !candidates.is_empty() {
        let floor = if selected.len() < 8.min(desired) {
            medium_floor
        } else {
            (medium_floor * 0.9).max(top_score * 0.42)
        };

        let best_idx = candidates
            .iter()
            .enumerate()
            .filter(|(_, candidate)| candidate.score >= floor)
            .max_by(|(_, left), (_, right)| {
                diversity_score(
                    profile,
                    left,
                    &app_counts,
                    &source_counts,
                    &time_bucket_counts,
                )
                .partial_cmp(&diversity_score(
                    profile,
                    right,
                    &app_counts,
                    &source_counts,
                    &time_bucket_counts,
                ))
                .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(idx, _)| idx);

        let Some(best_idx) = best_idx else {
            break;
        };

        let candidate = candidates.remove(best_idx);
        remember_diversity_keys(
            &candidate,
            &mut app_counts,
            &mut source_counts,
            &mut time_bucket_counts,
        );
        selected.push(candidate);
    }

    for candidate in candidates {
        if selected.len() >= desired {
            break;
        }
        if candidate.score < strong_floor && !selected.is_empty() {
            continue;
        }
        selected.push(candidate);
    }

    selected
}

fn diversity_score(
    profile: &QueryProfile,
    candidate: &SearchResult,
    app_counts: &HashMap<String, usize>,
    source_counts: &HashMap<String, usize>,
    time_bucket_counts: &HashMap<i64, usize>,
) -> f32 {
    let app_key = candidate.app_name.to_ascii_lowercase();
    let source_key = diversity_source_key(candidate);
    let time_bucket = candidate.timestamp / (30 * 60 * 1000);
    let app_hits = app_counts.get(&app_key).copied().unwrap_or(0);
    let source_hits = source_counts.get(&source_key).copied().unwrap_or(0);
    let time_hits = time_bucket_counts.get(&time_bucket).copied().unwrap_or(0);

    let mut score = candidate.score;
    score += if app_hits == 0 {
        0.10
    } else {
        -(app_hits as f32) * 0.10
    };
    score += if source_hits == 0 {
        0.07
    } else {
        -(source_hits as f32) * 0.08
    };
    score += if time_hits == 0 {
        if profile.wants_recency {
            0.02
        } else {
            0.04
        }
    } else {
        -(time_hits as f32) * 0.03
    };
    score += estimate_result_signal_strength(candidate) * 0.04;

    score
}

fn diversity_source_key(candidate: &SearchResult) -> String {
    let domain = candidate
        .url
        .as_ref()
        .map(|url| extract_domain(url))
        .unwrap_or_default();
    if domain.is_empty() {
        session_key(candidate)
    } else {
        format!("{}:{}", candidate.app_name.to_ascii_lowercase(), domain)
    }
}

fn remember_diversity_keys(
    candidate: &SearchResult,
    app_counts: &mut HashMap<String, usize>,
    source_counts: &mut HashMap<String, usize>,
    time_bucket_counts: &mut HashMap<i64, usize>,
) {
    *app_counts
        .entry(candidate.app_name.to_ascii_lowercase())
        .or_insert(0) += 1;
    *source_counts
        .entry(diversity_source_key(candidate))
        .or_insert(0) += 1;
    *time_bucket_counts
        .entry(candidate.timestamp / (30 * 60 * 1000))
        .or_insert(0) += 1;
}

fn token_set_matches_term(token_set: &HashSet<String>, term: &str) -> bool {
    let stem = stem_token(term);
    token_set.contains(term) || (!stem.is_empty() && token_set.contains(&stem))
}

fn is_weak_query_term(term: &str) -> bool {
    matches!(
        term,
        "last"
            | "time"
            | "times"
            | "first"
            | "recent"
            | "latest"
            | "watch"
            | "watched"
            | "watching"
            | "find"
            | "found"
            | "show"
            | "showed"
            | "showing"
            | "look"
            | "looked"
            | "open"
            | "opened"
    )
}

fn query_anchor_terms(profile: &QueryProfile) -> Vec<String> {
    let mut anchors = profile
        .primary_terms
        .iter()
        .filter(|term| term.len() >= 4 && !is_weak_query_term(term))
        .cloned()
        .collect::<Vec<_>>();

    if anchors.is_empty() {
        anchors = profile.primary_terms.iter().take(1).cloned().collect();
    }

    anchors
}

fn should_require_anchor(profile: &QueryProfile, anchors: &[String]) -> bool {
    if anchors.is_empty() {
        return false;
    }
    if profile.is_short_intent_query() {
        return true;
    }

    anchors.len() <= (profile.primary_terms.len().max(1) / 2).max(1)
}

fn has_anchor_term(profile: &QueryProfile, anchors: &[String], text: &str) -> bool {
    let token_set = token_set_with_stems(text);

    if profile
        .number_terms
        .iter()
        .any(|number| !number.is_empty() && token_set.contains(number))
    {
        return true;
    }

    for anchor in anchors {
        if token_set_matches_term(&token_set, anchor) {
            return true;
        }
    }

    false
}

fn query_source_alignment(
    profile: &QueryProfile,
    title_norm: &str,
    app_name: &str,
    url: Option<&str>,
    text_norm: &str,
    snippet_norm: &str,
) -> f32 {
    let candidates = profile
        .primary_terms
        .iter()
        .filter(|term| term.len() >= 3 && !is_weak_query_term(term))
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return 1.0;
    }

    let title_tokens = token_set_with_stems(title_norm);
    let app_tokens = token_set_with_stems(app_name);
    let source_tokens = token_set_with_stems(&format!("{text_norm} {snippet_norm}"));
    let domain = url.map(extract_domain).unwrap_or_default();
    let domain_tokens = token_set_with_stems(&domain);
    let url_tokens = token_set_with_stems(url.unwrap_or_default());

    let denom = candidates.len().max(1) as f32;
    let mut total = 0.0f32;
    for term in candidates {
        if token_set_matches_term(&app_tokens, term) || token_set_matches_term(&domain_tokens, term)
        {
            total += 1.0;
            continue;
        }
        if token_set_matches_term(&title_tokens, term) || token_set_matches_term(&url_tokens, term)
        {
            total += 0.85;
            continue;
        }
        if token_set_matches_term(&source_tokens, term) {
            total += 0.45;
        }
    }

    (total / denom).clamp(0.0, 1.0)
}

fn value_range(values: &[f32]) -> (f32, f32) {
    if values.is_empty() {
        return (0.0, 0.0);
    }
    let mut min = f32::INFINITY;
    let mut max = f32::NEG_INFINITY;
    for value in values {
        min = min.min(*value);
        max = max.max(*value);
    }
    (min, max)
}

fn normalize_range(value: f32, range: (f32, f32)) -> f32 {
    let (min, max) = range;
    if (max - min).abs() < 1e-6 {
        if value > 0.0 {
            1.0
        } else {
            0.0
        }
    } else {
        ((value - min) / (max - min)).clamp(0.0, 1.0)
    }
}

fn extract_domain(url: &str) -> String {
    url.split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or_default()
        .to_lowercase()
}

fn fuzzy_sim(a: &str, b: &str) -> f32 {
    let left = token_set_with_stems(a);
    let right = token_set_with_stems(b);
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }

    let inter = left.intersection(&right).count() as f32;
    let union = left.union(&right).count() as f32;
    if union == 0.0 {
        0.0
    } else {
        inter / union
    }
}

fn is_code_query(query: &str) -> bool {
    let lower = query.to_lowercase();
    lower.contains("code")
        || lower.contains("json")
        || lower.contains("stack trace")
        || lower.contains("rust")
        || lower.contains("typescript")
        || lower.contains("error")
        || lower.contains("file")
        || lower.contains("terminal")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sr(id: &str, title: &str, text: &str, score: f32) -> SearchResult {
        SearchResult {
            id: id.to_string(),
            timestamp: 2_000_000,
            app_name: "Chrome".to_string(),
            bundle_id: None,
            window_title: title.to_string(),
            session_id: "s1".to_string(),
            text: text.to_string(),
            clean_text: text.to_string(),
            ocr_confidence: 0.9,
            ocr_block_count: 7,
            snippet: text.to_string(),
            summary_source: "llm".to_string(),
            noise_score: 0.1,
            session_key: "chrome:test".to_string(),
            lexical_shadow: text.to_string(),
            score,
            screenshot_path: None,
            url: Some("https://example.com".to_string()),
            decay_score: 1.0,
            ..Default::default()
        }
    }

    #[test]
    fn query_profile_extracts_number_focus_for_definition_queries() {
        let profile = QueryProfile::from_query("what is 4000");
        assert_eq!(profile.intent, QueryIntent::Definition);
        assert!(profile.number_terms.contains("4000"));
        assert!(profile.expanded_terms.iter().any(|term| term == "4000"));
    }

    #[test]
    fn query_profile_expands_morphology_without_domain_aliases() {
        let profile = QueryProfile::from_query("playlist recap");
        assert!(profile.expanded_terms.iter().any(|term| term == "playlist"));
        assert!(profile
            .expanded_terms
            .iter()
            .any(|term| term == "playlists"));
        assert!(!profile.expanded_terms.iter().any(|term| term == "spotify"));
    }

    #[test]
    fn keyword_variants_include_adjacent_subqueries() {
        let profile = QueryProfile::from_query("knowledge graph display options");
        let variants = profile.keyword_variants(SearchConfig::default().max_keyword_variants);
        assert!(variants.iter().any(|variant| variant == "knowledge graph"));
        assert!(variants.iter().any(|variant| variant == "graph display"));
    }

    #[test]
    fn hybrid_fusion_merges_vector_and_keyword_hits() {
        let semantic = vec![
            sr(
                "semantic-only",
                "Vector note",
                "Semantic memory about the hybrid search ranking plan",
                0.82,
            ),
            sr(
                "shared",
                "Hybrid note",
                "Hybrid search ranking with keyword and vector evidence",
                0.74,
            ),
        ];
        let keyword = vec![
            sr(
                "keyword-only",
                "Keyword note",
                "Keyword exact match for hybrid search ranking",
                0.78,
            ),
            sr(
                "shared",
                "Hybrid note",
                "Hybrid search ranking with keyword and vector evidence",
                0.69,
            ),
        ];

        let profile = QueryProfile::from_query("hybrid search ranking");
        let fused = HybridSearcher::hybrid_fusion(
            &profile,
            &semantic,
            &[],
            &keyword,
            &SearchConfig::default(),
        );
        let ids = fused
            .iter()
            .map(|result| result.id.as_str())
            .collect::<Vec<_>>();

        assert!(ids.contains(&"semantic-only"));
        assert!(ids.contains(&"keyword-only"));
        assert_eq!(ids.iter().filter(|id| **id == "shared").count(), 1);
    }

    #[test]
    fn rerank_respects_score_order_for_equivalent_hits() {
        let lower = sr(
            "lower",
            "Ranking note",
            "Hybrid ranking notes mention vector keyword merge",
            0.62,
        );
        let higher = sr(
            "higher",
            "Ranking note",
            "Hybrid ranking notes mention vector keyword merge",
            0.78,
        );

        let ranked = HybridSearcher::rerank("hybrid ranking", vec![lower, higher], 2);
        assert_eq!(
            ranked.first().map(|result| result.id.as_str()),
            Some("higher")
        );
    }

    #[test]
    fn rerank_penalizes_generic_chrome_noise() {
        let mut noisy = sr("1", "New Tab", "New Tab Home Trending Notifications", 0.8);
        noisy.noise_score = 0.9;
        noisy.ocr_confidence = 0.4;

        let useful = sr(
            "2",
            "IPL 2026 Highlights",
            "Reviewed IPL highlights and match stats on YouTube",
            0.75,
        );

        let ranked = HybridSearcher::rerank("ipl highlights", vec![noisy, useful.clone()], 10);
        assert_eq!(ranked.first().map(|r| r.id.as_str()), Some("2"));
    }

    #[test]
    fn rerank_fuzzy_dedups_near_identical_snippets() {
        let a = sr(
            "1",
            "Title",
            "Reviewed onboarding checklist for FNDR launch",
            0.8,
        );
        let mut b = a.clone();
        b.id = "2".to_string();
        b.score = 0.79;

        let ranked = HybridSearcher::rerank("onboarding checklist", vec![a, b], 10);
        assert_eq!(ranked.len(), 1);
    }

    #[test]
    fn relevance_gate_drops_irrelevant_results() {
        let random = sr("1", "Weather", "Forecast and humidity in Herriman", 0.34);
        let noisy = sr(
            "2",
            "Activity Monitor",
            "Checked battery and CPU usage",
            0.33,
        );

        let ranked = HybridSearcher::rerank("what is cricket", vec![random, noisy], 10);
        assert!(ranked.is_empty());
    }

    #[test]
    fn relevance_gate_keeps_exact_entity_matches() {
        let relevant = sr(
            "1",
            "ChatGPT - 4000",
            "User asked what is 4000 in ChatGPT",
            0.28,
        );
        let other = sr("2", "Weather", "Freeze watch in Herriman", 0.45);

        let ranked = HybridSearcher::rerank("what is 4000", vec![other, relevant], 10);
        assert_eq!(ranked.first().map(|r| r.id.as_str()), Some("1"));
    }

    #[test]
    fn single_term_query_requires_anchor_match() {
        let irrelevant = sr(
            "1",
            "Codex Notes",
            "User ran embedding scripts and explored files in Codex app",
            0.92,
        );
        let relevant = sr(
            "2",
            "Spotify",
            "Played tracks and explored playlists on Spotify",
            0.61,
        );

        let ranked = HybridSearcher::rerank("spotify", vec![irrelevant, relevant], 10);
        assert_eq!(ranked.first().map(|r| r.id.as_str()), Some("2"));
        assert!(ranked
            .iter()
            .all(|r| merged_candidate_text(r).to_lowercase().contains("spotify")));
    }

    #[test]
    fn temporal_query_still_requires_topic_anchor() {
        let irrelevant = sr(
            "1",
            "Battery",
            "User monitored app energy impact over last 12 hours",
            0.9,
        );
        let relevant = sr(
            "2",
            "YouTube Cricket",
            "Watched IPL cricket highlights and match recap",
            0.57,
        );

        let ranked = HybridSearcher::rerank(
            "the last time i watched cricket",
            vec![irrelevant, relevant],
            10,
        );
        assert_eq!(ranked.first().map(|r| r.id.as_str()), Some("2"));
    }

    #[test]
    fn rerank_handles_diverse_query_styles() {
        let cases = vec![
            (
                "that startup evaluation page with full points",
                sr(
                    "a1",
                    "Course assignment",
                    "Startup evaluation rubric with full points and scoring criteria",
                    0.56,
                ),
                sr(
                    "a2",
                    "System stats",
                    "CPU pressure and battery usage over last 12 hours",
                    0.87,
                ),
                "a1",
            ),
            (
                "\"graphs should be miniaturized\"",
                sr(
                    "b1",
                    "Design notes",
                    "Graphs should be miniaturized before export to preserve readability",
                    0.52,
                ),
                sr(
                    "b2",
                    "General notes",
                    "Explored visual options and export behavior",
                    0.82,
                ),
                "b1",
            ),
            (
                "chrome weather in herriman",
                sr(
                    "c1",
                    "Weather in Herriman",
                    "Viewed weather forecast in Chrome for Herriman this weekend",
                    0.58,
                ),
                sr(
                    "c2",
                    "Editor window",
                    "Refactored rust search pipeline in terminal and editor",
                    0.78,
                ),
                "c1",
            ),
            (
                "what was i doing before the pitch deck edits",
                sr(
                    "d1",
                    "Pitch deck",
                    "Compared two versions of the pitch deck and revised slide order",
                    0.55,
                ),
                sr(
                    "d2",
                    "Music app",
                    "Played tracks and changed volume settings",
                    0.74,
                ),
                "d1",
            ),
        ];

        for (query, relevant, distractor, expected) in cases {
            let ranked = HybridSearcher::rerank(query, vec![distractor, relevant], 10);
            assert_eq!(
                ranked.first().map(|r| r.id.as_str()),
                Some(expected),
                "query: {query}"
            );
        }
    }

    #[test]
    fn recency_queries_prefer_newer_hits_when_relevance_is_close() {
        let now = chrono::Utc::now().timestamp_millis();
        let mut older = sr(
            "1",
            "Budget model",
            "Updated budget model and revised assumptions",
            0.58,
        );
        older.timestamp = now - (2 * 24 * 60 * 60 * 1000);

        let mut newer = sr(
            "2",
            "Budget model",
            "Updated budget model and revised assumptions",
            0.55,
        );
        newer.timestamp = now - (60 * 60 * 1000);

        let ranked = HybridSearcher::rerank("latest budget model updates", vec![older, newer], 10);
        assert_eq!(ranked.first().map(|r| r.id.as_str()), Some("2"));
    }

    #[test]
    fn diversity_rerank_surfaces_second_app_after_top_hit() {
        let mut chrome_primary = sr(
            "1",
            "Launch checklist",
            "Reviewed launch checklist and deployment notes",
            0.92,
        );
        chrome_primary.app_name = "Chrome".to_string();
        chrome_primary.session_key = "chrome:launch".to_string();

        let mut chrome_secondary = sr(
            "2",
            "Launch checklist",
            "Reviewed launch checklist and rollout notes",
            0.89,
        );
        chrome_secondary.app_name = "Chrome".to_string();
        chrome_secondary.session_key = "chrome:launch".to_string();

        let mut slack_hit = sr(
            "3",
            "Launch checklist",
            "Shared launch checklist updates with the team in Slack",
            0.84,
        );
        slack_hit.app_name = "Slack".to_string();
        slack_hit.session_key = "slack:launch".to_string();

        let ranked = HybridSearcher::rerank(
            "launch checklist updates",
            vec![chrome_primary, chrome_secondary, slack_hit],
            2,
        );
        assert_eq!(ranked.len(), 2);
        assert!(ranked.iter().any(|result| result.app_name == "Slack"));
    }

    #[test]
    fn freshness_queries_drop_stale_low_signal_hits() {
        let now = chrono::Utc::now().timestamp_millis();
        let mut stale = sr(
            "1",
            "Pipeline notes",
            "Pipeline testing notes and rough draft",
            0.95,
        );
        stale.timestamp = now - (55 * 24 * 60 * 60 * 1000);
        stale.summary_source = "fallback".to_string();
        stale.ocr_confidence = 0.35;
        stale.noise_score = 0.55;

        let mut fresh = sr(
            "2",
            "Pipeline notes",
            "Pipeline testing feature implemented for developers",
            0.74,
        );
        fresh.timestamp = now - (2 * 60 * 60 * 1000);

        let ranked =
            HybridSearcher::rerank("today pipeline testing feature", vec![stale, fresh], 5);
        assert_eq!(ranked.first().map(|result| result.id.as_str()), Some("2"));
        assert!(ranked.iter().all(|result| result.id != "1"));
    }

    #[test]
    fn app_hint_in_query_boosts_matching_source() {
        let chrome_hit = sr(
            "1",
            "Weather",
            "Viewed weather forecast and radar details",
            0.54,
        );

        let mut vscode_hit = sr(
            "2",
            "Weather",
            "Viewed weather forecast and radar details",
            0.59,
        );
        vscode_hit.app_name = "VS Code".to_string();
        vscode_hit.session_key = "vscode:test".to_string();

        let ranked =
            HybridSearcher::rerank("chrome weather forecast", vec![vscode_hit, chrome_hit], 10);
        assert_eq!(ranked.first().map(|r| r.app_name.as_str()), Some("Chrome"));
    }

    #[test]
    fn single_term_prefers_source_alignment_over_body_mention() {
        let mut body_mention = sr(
            "1",
            "Codex workspace",
            "User searched for spotify and updated memory pipeline",
            0.88,
        );
        body_mention.app_name = "Codex".to_string();
        body_mention.session_key = "codex:test".to_string();

        let mut source_match = sr("2", "Spotify", "Played songs and explored playlists", 0.62);
        source_match.app_name = "Spotify".to_string();
        source_match.session_key = "spotify:test".to_string();
        source_match.url = Some("https://open.spotify.com/track/123".to_string());

        let ranked = HybridSearcher::rerank("spotify", vec![body_mention, source_match], 10);
        assert_eq!(ranked.first().map(|r| r.id.as_str()), Some("2"));
    }

    #[test]
    fn temporal_query_with_no_topic_anchor_returns_empty() {
        let irrelevant = sr(
            "1",
            "Codex notes",
            "User monitored app energy impact over last 12 hours and updated files",
            0.92,
        );
        let ranked =
            HybridSearcher::rerank("the last time i watched cricket", vec![irrelevant], 10);
        assert!(ranked.is_empty());
    }
}
