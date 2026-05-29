use crate::context_runtime::query_plan::{QueryPlan, Route};
use crate::context_runtime::retrieval_routes::{
    finish_route, memory_record_to_search_result, PathStep, RetrievalRoute, RouteBranch, RouteCtx,
    RouteHit, RouteHits, RouteSignals,
};
use crate::graph::schema::GraphNode;
use crate::memory_embedding_document::{
    embedding_retrieval_adjustment, search_embedding_provenance_from_metadata, EmbeddingRole,
};
use crate::storage::SearchResult;
use futures::future::BoxFuture;
use std::collections::HashMap;
use std::time::Instant;
use uuid::Uuid;

pub struct GraphRoute;

impl RetrievalRoute for GraphRoute {
    fn route(&self) -> Route {
        Route::Graph
    }

    fn run<'a>(&'a self, plan: &'a QueryPlan, ctx: &'a RouteCtx<'a>) -> BoxFuture<'a, RouteHits> {
        Box::pin(async move {
            let started = Instant::now();
            let Some(index) = ctx.graph_index else {
                return finish_route(Route::Graph, started, Vec::new());
            };
            if plan.graph_expansion.max_hops == 0 {
                return finish_route(Route::Graph, started, Vec::new());
            }

            let memory_to_nodes = memory_to_nodes(ctx.graph_nodes);
            let seeds = seed_memory_ids(ctx);
            if seeds.is_empty() {
                return finish_route(Route::Graph, started, Vec::new());
            }

            let mut by_id: HashMap<String, RouteHit> = HashMap::new();

            for (seed_memory_id, seed_score) in seeds {
                let Some(seed_nodes) = memory_to_nodes.get(&seed_memory_id) else {
                    continue;
                };
                for &seed_node_id in seed_nodes {
                    for neighbor in index.neighbors_with_paths(
                        seed_node_id,
                        &plan.graph_expansion.allowed_edges,
                        plan.graph_expansion.max_hops as usize,
                    ) {
                        let Some(neighbor_node) = index.node(neighbor.id) else {
                            continue;
                        };
                        let graph_path = neighbor
                            .path
                            .iter()
                            .map(|step| PathStep {
                                from_label: step.from_label.clone(),
                                edge: step.edge,
                                to_label: step.to_label.clone(),
                            })
                            .collect::<Vec<_>>();
                        if graph_path.is_empty() {
                            continue;
                        }
                        let graph_adjustment = graph_node_retrieval_adjustment(neighbor_node);

                        for memory_id in &neighbor_node.source_memory_ids {
                            if memory_id.trim().is_empty() || memory_id == &seed_memory_id {
                                continue;
                            }

                            let score = ((seed_score * 0.58 + neighbor.confidence * 0.42)
                                .clamp(0.0, 1.0)
                                * graph_adjustment.score_multiplier)
                                .clamp(0.0, 1.0);
                            let search_result = match ctx.store.get_memory_by_id(memory_id).await {
                                Ok(Some(record)) => Some(graph_search_result(
                                    &record,
                                    score,
                                    &graph_adjustment.reason_labels,
                                )),
                                Ok(None) => None,
                                Err(err) => {
                                    tracing::warn!(err = %err, memory_id = %memory_id, "retrieval_route:graph_memory_fetch_failed");
                                    None
                                }
                            };
                            insert_best(
                                &mut by_id,
                                RouteHit {
                                    memory_id: memory_id.clone(),
                                    score,
                                    signals: RouteSignals {
                                        branch: RouteBranch::Graph,
                                        confidence: score,
                                        search_result,
                                    },
                                    graph_path: Some(graph_path.clone()),
                                },
                            );
                        }
                    }
                }
            }

            let mut hits = by_id.into_values().collect::<Vec<_>>();
            hits.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            hits.truncate(25);
            finish_route(Route::Graph, started, hits)
        })
    }
}

fn memory_to_nodes(nodes: &[GraphNode]) -> HashMap<String, Vec<Uuid>> {
    let mut out: HashMap<String, Vec<Uuid>> = HashMap::new();
    for node in nodes {
        for memory_id in &node.source_memory_ids {
            if memory_id.trim().is_empty() {
                continue;
            }
            out.entry(memory_id.clone()).or_default().push(node.id);
        }
    }
    out
}

fn graph_node_retrieval_adjustment(
    node: &GraphNode,
) -> crate::memory_embedding_document::EmbeddingRetrievalAdjustment {
    let provenance = search_embedding_provenance_from_metadata(&node.metadata);
    embedding_retrieval_adjustment(
        provenance
            .as_ref()
            .and_then(|provenance| provenance.role(EmbeddingRole::GraphNode)),
        EmbeddingRole::GraphNode,
    )
}

fn graph_search_result(
    record: &crate::storage::MemoryRecord,
    score: f32,
    reason_labels: &[String],
) -> SearchResult {
    let mut result = memory_record_to_search_result(record, score);
    push_unique(&mut result.matched_routes, "Graph".to_string());
    for label in reason_labels {
        push_unique(&mut result.embedding_reason_labels, label.clone());
    }
    result
}

fn seed_memory_ids(ctx: &RouteCtx<'_>) -> Vec<(String, f32)> {
    let mut seeds = Vec::new();
    collect_top_route_seeds(ctx, Route::Vector, 5, &mut seeds);
    collect_top_route_seeds(ctx, Route::Keyword, 5, &mut seeds);
    collect_top_route_seeds(ctx, Route::Entity, usize::MAX, &mut seeds);
    seeds.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    seeds.dedup_by(|left, right| {
        if left.0 == right.0 {
            if right.1 > left.1 {
                left.1 = right.1;
            }
            true
        } else {
            false
        }
    });
    seeds
}

fn collect_top_route_seeds(
    ctx: &RouteCtx<'_>,
    route: Route,
    limit: usize,
    seeds: &mut Vec<(String, f32)>,
) {
    for route_hits in ctx
        .prior_route_hits
        .iter()
        .filter(|hits| hits.route == route)
    {
        let mut hits = route_hits.hits.clone();
        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for hit in hits.into_iter().take(limit) {
            seeds.push((hit.memory_id, hit.score));
        }
    }
}

fn insert_best(by_id: &mut HashMap<String, RouteHit>, hit: RouteHit) {
    by_id
        .entry(hit.memory_id.clone())
        .and_modify(|existing| {
            if hit.score > existing.score {
                *existing = hit.clone();
            }
        })
        .or_insert(hit);
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !value.trim().is_empty() && !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SearchConfig;
    use crate::context_runtime::retrieval_routes::{RouteHits, RouteSignals};
    use crate::graph::graph_index::GraphIndex;
    use crate::graph::schema::{GraphEdge, GraphEdgeType, GraphNode, GraphNodeType};
    use crate::memory_embedding_document::{annotate_graph_node_embedding, EmbeddingStatus};
    use crate::storage::MemoryRecord;
    use crate::storage::Store;
    use chrono::Utc;

    fn node(id: u128, label: &str, memory_id: &str) -> GraphNode {
        GraphNode {
            id: Uuid::from_u128(id),
            node_type: GraphNodeType::Concept,
            label: label.to_string(),
            confidence: 0.9,
            source_memory_ids: vec![memory_id.to_string()],
            embedding: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            stale: false,
            metadata: serde_json::json!({}),
        }
    }

    fn memory(id: &str) -> MemoryRecord {
        MemoryRecord {
            id: id.to_string(),
            text: "Graph route memory".to_string(),
            memory_context: "Graph route memory".to_string(),
            embedding: vec![0.1; crate::config::DEFAULT_TEXT_EMBEDDING_DIM],
            snippet_embedding: vec![0.1; crate::config::DEFAULT_TEXT_EMBEDDING_DIM],
            support_embedding: vec![0.1; crate::config::DEFAULT_TEXT_EMBEDDING_DIM],
            image_embedding: vec![0.0; crate::config::DEFAULT_IMAGE_EMBEDDING_DIM],
            embedding_dim: crate::config::DEFAULT_TEXT_EMBEDDING_DIM as u32,
            ..Default::default()
        }
    }

    fn edge(source: u128, target: u128) -> GraphEdge {
        GraphEdge {
            id: Uuid::new_v4(),
            source_id: Uuid::from_u128(source),
            target_id: Uuid::from_u128(target),
            edge_type: GraphEdgeType::SameTaskAs,
            confidence: 0.9,
            conflict_flag: false,
            created_at: Utc::now(),
            metadata: serde_json::json!({}),
        }
    }

    #[tokio::test]
    async fn graph_route_returns_connected_memory_with_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().to_path_buf();
        let store = tokio::task::spawn_blocking(move || Store::new(&path).expect("store"))
            .await
            .expect("store task");
        let config = SearchConfig::default().normalized();
        let nodes = vec![
            node(1, "seed", "seed-memory"),
            node(2, "neighbor", "graph-memory"),
        ];
        let edges = vec![edge(1, 2)];
        let index = GraphIndex::build(&nodes, &edges);
        let mut plan = crate::context_runtime::query_plan::plan(
            "related graph",
            &crate::context_runtime::query_plan::PlanHints::default(),
        );
        plan.graph_expansion.max_hops = 1;
        plan.graph_expansion.allowed_edges = vec![GraphEdgeType::SameTaskAs];
        let prior = vec![RouteHits {
            route: Route::Vector,
            hits: vec![RouteHit {
                memory_id: "seed-memory".to_string(),
                score: 0.9,
                signals: RouteSignals {
                    branch: RouteBranch::Semantic,
                    confidence: 0.9,
                    search_result: None,
                },
                graph_path: None,
            }],
            elapsed_ms: 1,
        }];
        let ctx = RouteCtx::new(&store, &config)
            .with_graph(&index, &nodes, &edges)
            .with_prior_route_hits(prior);

        let hits = GraphRoute.run(&plan, &ctx).await;
        assert_eq!(hits.route, Route::Graph);
        assert_eq!(hits.hits[0].memory_id, "graph-memory");
        assert!(!hits.hits[0].graph_path.as_ref().unwrap().is_empty());
    }

    #[tokio::test]
    async fn graph_route_penalizes_stale_graph_node_embedding_and_explains_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().to_path_buf();
        let store = tokio::task::spawn_blocking(move || Store::new(&path).expect("store"))
            .await
            .expect("store task");
        store
            .add_batch_preserving_ids(&[memory("graph-memory")])
            .await
            .expect("insert graph memory");
        let config = SearchConfig::default().normalized();
        let seed = node(1, "seed", "seed-memory");
        let mut neighbor = node(2, "neighbor", "graph-memory");
        annotate_graph_node_embedding(
            &mut neighbor,
            EmbeddingStatus::StaleSourceText,
            "node_type: concept\nlabel: neighbor",
            Some("graph node text hash no longer matches".to_string()),
        );
        let nodes = vec![seed, neighbor];
        let edges = vec![edge(1, 2)];
        let index = GraphIndex::build(&nodes, &edges);
        let mut plan = crate::context_runtime::query_plan::plan(
            "related graph",
            &crate::context_runtime::query_plan::PlanHints::default(),
        );
        plan.graph_expansion.max_hops = 1;
        plan.graph_expansion.allowed_edges = vec![GraphEdgeType::SameTaskAs];
        let prior = vec![RouteHits {
            route: Route::Vector,
            hits: vec![RouteHit {
                memory_id: "seed-memory".to_string(),
                score: 0.9,
                signals: RouteSignals {
                    branch: RouteBranch::Semantic,
                    confidence: 0.9,
                    search_result: None,
                },
                graph_path: None,
            }],
            elapsed_ms: 1,
        }];
        let ctx = RouteCtx::new(&store, &config)
            .with_graph(&index, &nodes, &edges)
            .with_prior_route_hits(prior);

        let hits = GraphRoute.run(&plan, &ctx).await;

        assert_eq!(hits.hits[0].memory_id, "graph-memory");
        assert!(hits.hits[0].score < 0.68);
        let result = hits.hits[0]
            .signals
            .search_result
            .as_ref()
            .expect("search result");
        assert!(result.matched_routes.contains(&"Graph".to_string()));
        assert!(result
            .embedding_reason_labels
            .contains(&"embedding:graph_node:stale_source_text".to_string()));
    }

    #[tokio::test]
    async fn graph_route_is_empty_without_seed_hits() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().to_path_buf();
        let store = tokio::task::spawn_blocking(move || Store::new(&path).expect("store"))
            .await
            .expect("store task");
        let config = SearchConfig::default().normalized();
        let nodes = vec![node(1, "seed", "seed-memory")];
        let index = GraphIndex::build(&nodes, &[]);
        let plan = crate::context_runtime::query_plan::plan(
            "related graph",
            &crate::context_runtime::query_plan::PlanHints::default(),
        );
        let ctx = RouteCtx::new(&store, &config).with_graph(&index, &nodes, &[]);

        let hits = GraphRoute.run(&plan, &ctx).await;
        assert!(hits.hits.is_empty());
    }
}
