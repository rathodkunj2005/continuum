use crate::config::SearchConfig;
use crate::context_runtime::query_plan::{QueryPlan, Route};
use crate::context_runtime::retrieval_routes::{
    finish_route, hit_from_search_result, RetrievalRoute, RouteBranch, RouteCtx, RouteHits,
};
use crate::search::QueryProfile;
use crate::storage::{SearchResult, Store};
use crate::telemetry::runtime_metrics;
use futures::future::BoxFuture;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::time::timeout;

pub struct KeywordRoute;

impl RetrievalRoute for KeywordRoute {
    fn route(&self) -> Route {
        Route::Keyword
    }

    fn run<'a>(&'a self, plan: &'a QueryPlan, ctx: &'a RouteCtx<'a>) -> BoxFuture<'a, RouteHits> {
        Box::pin(async move {
            let route_started = Instant::now();
            let profile = QueryProfile::from_query(&plan.raw);
            if profile.is_empty() {
                return finish_route(Route::Keyword, route_started, Vec::new());
            }

            let base_limit = ctx.limit.max(1);
            let branch_limit = (base_limit * 2)
                .min(ctx.search_config.max_keyword_branch_limit)
                .max(base_limit);
            let keyword_total_timeout = Duration::from_millis(ctx.search_config.keyword_timeout_ms);
            let keyword_variant_timeout =
                Duration::from_millis(ctx.search_config.keyword_variant_timeout_ms);

            let branch_started = Instant::now();
            let mut timed_out = false;
            let results = match timeout(
                keyword_total_timeout,
                keyword_search_with_budget(
                    ctx.store,
                    &profile,
                    branch_limit,
                    ctx.time_filter,
                    ctx.app_filter,
                    ctx.search_config,
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
            let keyword_elapsed = branch_started.elapsed();
            runtime_metrics::record_ms("hybrid.keyword_ms", keyword_elapsed.as_millis() as u64);
            if timed_out {
                runtime_metrics::bump("hybrid.keyword_timeout");
            }

            let hits = results
                .into_iter()
                .map(|result| hit_from_search_result(Route::Keyword, RouteBranch::Keyword, result))
                .collect();
            finish_route(Route::Keyword, route_started, hits)
        })
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{SearchConfig, DEFAULT_IMAGE_EMBEDDING_DIM};
    use crate::embedding::EMBEDDING_DIM;
    use crate::storage::{MemoryRecord, Store};

    fn record(id: &str, text: &str) -> MemoryRecord {
        MemoryRecord {
            id: id.to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            app_name: "Terminal".to_string(),
            window_title: "Keyword Route".to_string(),
            session_id: "keyword-session".to_string(),
            text: text.to_string(),
            clean_text: text.to_string(),
            snippet: text.to_string(),
            summary_source: "llm".to_string(),
            embedding: vec![0.0; EMBEDDING_DIM],
            snippet_embedding: vec![0.0; EMBEDDING_DIM],
            support_embedding: vec![0.0; EMBEDDING_DIM],
            image_embedding: vec![0.0; DEFAULT_IMAGE_EMBEDDING_DIM],
            decay_score: 1.0,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn keyword_route_returns_seeded_hits() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().to_path_buf();
        let store = tokio::task::spawn_blocking(move || Store::new(&path).expect("store"))
            .await
            .expect("store task");
        store
            .add_batch(&[record("keyword-1", "route runner keyword planning")])
            .await
            .expect("add");
        let config = SearchConfig {
            keyword_timeout_ms: 5_000,
            keyword_variant_timeout_ms: 2_000,
            ..SearchConfig::default()
        }
        .normalized();
        let plan = crate::context_runtime::query_plan::plan(
            "keyword planning",
            &crate::context_runtime::query_plan::PlanHints::default(),
        );
        let ctx = RouteCtx::new(&store, &config);

        let hits = KeywordRoute.run(&plan, &ctx).await;
        assert_eq!(hits.route, Route::Keyword);
        assert!(!hits.hits.is_empty());
    }

    #[tokio::test]
    async fn keyword_route_is_empty_for_blank_query() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().to_path_buf();
        let store = tokio::task::spawn_blocking(move || Store::new(&path).expect("store"))
            .await
            .expect("store task");
        let config = SearchConfig::default().normalized();
        let plan = crate::context_runtime::query_plan::plan(
            "",
            &crate::context_runtime::query_plan::PlanHints::default(),
        );
        let ctx = RouteCtx::new(&store, &config);

        let hits = KeywordRoute.run(&plan, &ctx).await;
        assert!(hits.hits.is_empty());
    }
}
