use crate::context_runtime::query_plan::{QueryPlan, Route};
use crate::context_runtime::retrieval_routes::{
    finish_route, hit_from_search_result, RetrievalRoute, RouteBranch, RouteCtx, RouteHits,
};
use crate::embedding::EmbeddingBackend;
use crate::search::QueryProfile;
use crate::telemetry::runtime_metrics;
use futures::future::BoxFuture;
use std::time::{Duration, Instant};
use tokio::time::timeout;

pub struct VectorRoute;

impl RetrievalRoute for VectorRoute {
    fn route(&self) -> Route {
        Route::Vector
    }

    fn run<'a>(&'a self, _plan: &'a QueryPlan, ctx: &'a RouteCtx<'a>) -> BoxFuture<'a, RouteHits> {
        Box::pin(async move {
            let route_started = Instant::now();
            let Some(embedder) = ctx.embedder else {
                return finish_route(Route::Vector, route_started, Vec::new());
            };

            let profile = QueryProfile::from_query(&_plan.raw);
            if profile.is_empty() {
                return finish_route(Route::Vector, route_started, Vec::new());
            }
            if matches!(embedder.backend(), EmbeddingBackend::Mock) && !ctx.allow_mock_vectors {
                return finish_route(Route::Vector, route_started, Vec::new());
            }

            let base_limit = ctx.limit.max(1);
            let branch_limit = (base_limit * ctx.search_config.candidate_multiplier)
                .min(ctx.search_config.max_semantic_branch_limit);
            let semantic_timeout = Duration::from_millis(ctx.search_config.semantic_timeout_ms);
            let snippet_timeout = Duration::from_millis(ctx.search_config.snippet_timeout_ms);

            let embedding_query = profile.embedding_query_with_extras(ctx.expansion);
            let embed_started = Instant::now();
            let query_embedding = match embedder.embed_batch(&[embedding_query]) {
                Ok(vectors) => vectors.into_iter().next().unwrap_or_default(),
                Err(err) => {
                    tracing::warn!(err = %err, "hybrid_search:embed_failed");
                    runtime_metrics::record_ms(
                        "hybrid.embed_query_ms",
                        embed_started.elapsed().as_millis() as u64,
                    );
                    return finish_route(Route::Vector, route_started, Vec::new());
                }
            };
            runtime_metrics::record_ms(
                "hybrid.embed_query_ms",
                embed_started.elapsed().as_millis() as u64,
            );

            let semantic_started = Instant::now();
            let mut semantic_timed_out = false;
            let semantic_results = match timeout(
                semantic_timeout,
                ctx.store.vector_search(
                    &query_embedding,
                    branch_limit,
                    ctx.time_filter,
                    ctx.app_filter,
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
                    semantic_timed_out = true;
                    tracing::warn!(
                        timeout_ms = semantic_timeout.as_millis(),
                        "hybrid_search:semantic_timeout"
                    );
                    Vec::new()
                }
            };
            let semantic_elapsed = semantic_started.elapsed();

            let allow_snippet_branch =
                profile.primary_terms_len() >= ctx.search_config.min_snippet_query_terms;
            let snippet_started = Instant::now();
            let mut snippet_timed_out = false;
            let snippet_results = if allow_snippet_branch {
                match timeout(
                    snippet_timeout,
                    ctx.store.snippet_vector_search(
                        &query_embedding,
                        branch_limit,
                        ctx.time_filter,
                        ctx.app_filter,
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
                        snippet_timed_out = true;
                        tracing::warn!(
                            timeout_ms = snippet_timeout.as_millis(),
                            "hybrid_search:snippet_timeout"
                        );
                        Vec::new()
                    }
                }
            } else {
                Vec::new()
            };
            let snippet_elapsed = snippet_started.elapsed();

            runtime_metrics::record_ms("hybrid.semantic_ms", semantic_elapsed.as_millis() as u64);
            runtime_metrics::record_ms("hybrid.snippet_ms", snippet_elapsed.as_millis() as u64);
            if semantic_timed_out {
                runtime_metrics::bump("hybrid.semantic_timeout");
            }
            if snippet_timed_out {
                runtime_metrics::bump("hybrid.snippet_timeout");
            }

            let mut hits = semantic_results
                .into_iter()
                .map(|result| hit_from_search_result(Route::Vector, RouteBranch::Semantic, result))
                .collect::<Vec<_>>();
            hits.extend(
                snippet_results.into_iter().map(|result| {
                    hit_from_search_result(Route::Vector, RouteBranch::Snippet, result)
                }),
            );

            finish_route(Route::Vector, route_started, hits)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{SearchConfig, DEFAULT_IMAGE_EMBEDDING_DIM};
    use crate::embedding::{Embedder, EMBEDDING_DIM};
    use crate::storage::{MemoryRecord, Store};

    fn record(id: &str, text: &str, embedding: Vec<f32>) -> MemoryRecord {
        MemoryRecord {
            id: id.to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            app_name: "Terminal".to_string(),
            window_title: "Route Test".to_string(),
            session_id: "route-session".to_string(),
            text: text.to_string(),
            clean_text: text.to_string(),
            snippet: text.to_string(),
            summary_source: "llm".to_string(),
            embedding: embedding.clone(),
            snippet_embedding: embedding,
            image_embedding: vec![0.0; DEFAULT_IMAGE_EMBEDDING_DIM],
            support_embedding: vec![0.0; EMBEDDING_DIM],
            decay_score: 1.0,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn vector_route_returns_seeded_semantic_hits() {
        std::env::set_var("CONTINUUM_ALLOW_MOCK_EMBEDDER", "1");
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().to_path_buf();
        let store = tokio::task::spawn_blocking(move || Store::new(&path).expect("store"))
            .await
            .expect("store task");
        let embedder = Embedder::new().expect("embedder");
        let text = "planner graph route vector recall";
        let embedding = embedder
            .embed_batch(&[text.to_string()])
            .expect("embedding")
            .into_iter()
            .next()
            .expect("vector");
        store
            .add_batch(&[record("vector-1", text, embedding)])
            .await
            .expect("add");

        let config = SearchConfig::default().normalized();
        let plan = crate::context_runtime::query_plan::plan(
            "planner graph route vector recall",
            &crate::context_runtime::query_plan::PlanHints::default(),
        );
        let ctx = RouteCtx::new(&store, &config)
            .with_embedder(&embedder)
            .allowing_mock_vectors();

        let hits = VectorRoute.run(&plan, &ctx).await;
        assert_eq!(hits.route, Route::Vector);
        assert!(!hits.hits.is_empty());
    }

    #[tokio::test]
    async fn vector_route_is_empty_without_embedder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().to_path_buf();
        let store = tokio::task::spawn_blocking(move || Store::new(&path).expect("store"))
            .await
            .expect("store task");
        let config = SearchConfig::default().normalized();
        let plan = crate::context_runtime::query_plan::plan(
            "planner",
            &crate::context_runtime::query_plan::PlanHints::default(),
        );
        let ctx = RouteCtx::new(&store, &config);

        let hits = VectorRoute.run(&plan, &ctx).await;
        assert!(hits.hits.is_empty());
    }
}
