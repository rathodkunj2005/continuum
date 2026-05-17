//! Phase 3 fusion stage: combine per-route hits into a single ranked list with
//! `FusionSignals` + `SurfacingReason` attached. Pure function — no I/O.

use crate::context_runtime::context_pack::{
    FusedHit, FusionSignals, FusionWeights, SurfacingReason,
};
use crate::context_runtime::query_plan::{QueryPlan, Route};
use crate::context_runtime::retrieval_routes::{PathStep, RouteHits};
use crate::telemetry::runtime_metrics;
use std::collections::HashMap;
use std::time::Instant;

const MAX_FUSED_HITS: usize = 50;

pub fn fuse(plan: &QueryPlan, hits: Vec<RouteHits>, weights: &FusionWeights) -> Vec<FusedHit> {
    let started = Instant::now();
    let mut agg: HashMap<String, Agg> = HashMap::new();

    for route_hits in &hits {
        let weight = weight_for(weights, route_hits.route);
        for hit in &route_hits.hits {
            let entry = agg.entry(hit.memory_id.clone()).or_default();
            let normalized = hit.score.clamp(0.0, 1.0);
            entry.add_route(route_hits.route, normalized, weight);
            if matches!(route_hits.route, Route::Graph) {
                if let Some(path) = &hit.graph_path {
                    if entry.graph_path.is_none() && !path.is_empty() {
                        entry.graph_path = Some(path.clone());
                    }
                }
            }
            entry.coverage = entry.coverage.max(coverage_from_hit(hit));
        }
    }

    let anchor_terms = plan_anchor_terms(plan);

    let mut fused: Vec<FusedHit> = agg
        .into_iter()
        .map(|(memory_id, entry)| {
            let recency_boost = entry.signals.temporal * weights.recency;
            let surfacing_reason = build_surfacing_reason(
                &entry.contributing_routes,
                &entry.graph_path,
                anchor_terms.clone(),
                recency_boost,
            );
            FusedHit {
                memory_id,
                score: entry.score,
                signals: FusionSignals {
                    recency: recency_boost,
                    coverage: entry.coverage,
                    ..entry.signals
                },
                surfacing_reason,
                contributing_routes: entry.contributing_routes.clone(),
            }
        })
        .collect();

    fused.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    fused.truncate(MAX_FUSED_HITS);

    runtime_metrics::record_ms("fndr.retrieval.fusion.ms", started.elapsed().as_millis() as u64);
    fused
}

#[derive(Default)]
struct Agg {
    signals: FusionSignals,
    score: f32,
    coverage: f32,
    graph_path: Option<Vec<PathStep>>,
    contributing_routes: Vec<Route>,
}

impl Agg {
    fn add_route(&mut self, route: Route, score: f32, weight: f32) {
        match route {
            Route::Vector => self.signals.vector = self.signals.vector.max(score),
            Route::Keyword => self.signals.keyword = self.signals.keyword.max(score),
            Route::Temporal => self.signals.temporal = self.signals.temporal.max(score),
            Route::Entity => self.signals.entity = self.signals.entity.max(score),
            Route::Graph => self.signals.graph = self.signals.graph.max(score),
        }
        self.score += score * weight;
        if !self.contributing_routes.contains(&route) {
            self.contributing_routes.push(route);
        }
    }
}

fn weight_for(weights: &FusionWeights, route: Route) -> f32 {
    match route {
        Route::Vector => weights.vector,
        Route::Keyword => weights.keyword,
        Route::Temporal => weights.temporal,
        Route::Entity => weights.entity,
        Route::Graph => weights.graph,
    }
}

fn coverage_from_hit(hit: &crate::context_runtime::retrieval_routes::RouteHit) -> f32 {
    hit.signals
        .search_result
        .as_ref()
        .map(|sr| sr.anchor_coverage_score)
        .unwrap_or(0.0)
}

fn plan_anchor_terms(plan: &QueryPlan) -> Vec<String> {
    let mut terms = Vec::new();
    for hint in &plan.target_entities {
        if !hint.label.trim().is_empty() {
            terms.push(hint.label.clone());
        }
    }
    for topic in &plan.target_topics {
        if !topic.trim().is_empty() {
            terms.push(topic.clone());
        }
    }
    terms.sort();
    terms.dedup();
    terms
}

fn build_surfacing_reason(
    contributing_routes: &[Route],
    graph_path: &Option<Vec<PathStep>>,
    anchor_terms_hit: Vec<String>,
    recency_boost: f32,
) -> SurfacingReason {
    let route_strings: Vec<String> = contributing_routes
        .iter()
        .map(|route| route_label(*route, graph_path))
        .collect();

    let headline = if let Some(path) = graph_path.as_ref().filter(|p| !p.is_empty()) {
        let last = path.last().unwrap();
        let first = path.first().unwrap();
        format!(
            "Reached via {} from {}",
            edge_label(&last.edge),
            first.from_label
        )
    } else if contributing_routes.contains(&Route::Temporal) && contributing_routes.len() >= 2 {
        format!("Most recent of {} this session", contributing_routes.len())
    } else {
        format!("Matched in {} routes", contributing_routes.len().max(1))
    };

    SurfacingReason {
        headline,
        routes: route_strings,
        graph_path: graph_path.clone(),
        anchor_terms_hit,
        recency_boost,
    }
}

fn route_label(route: Route, graph_path: &Option<Vec<PathStep>>) -> String {
    match route {
        Route::Vector => "vector".to_string(),
        Route::Keyword => "keyword".to_string(),
        Route::Temporal => "temporal".to_string(),
        Route::Entity => "entity".to_string(),
        Route::Graph => match graph_path.as_ref().filter(|p| !p.is_empty()) {
            Some(path) => format!(
                "graph({}-hop via {}:{})",
                path.len(),
                edge_label(&path.last().unwrap().edge),
                path.last().unwrap().to_label
            ),
            None => "graph".to_string(),
        },
    }
}

fn edge_label(edge: &crate::graph::schema::GraphEdgeType) -> String {
    format!("{edge:?}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context_runtime::query_plan::PlannerIntent;
    use crate::context_runtime::retrieval_routes::{RouteBranch, RouteHit, RouteSignals};

    fn hit(memory_id: &str, score: f32, branch: RouteBranch) -> RouteHit {
        RouteHit {
            memory_id: memory_id.to_string(),
            score,
            signals: RouteSignals {
                branch,
                confidence: score,
                search_result: None,
            },
            graph_path: None,
        }
    }

    fn dummy_plan() -> QueryPlan {
        QueryPlan {
            raw: "test".to_string(),
            intent: PlannerIntent::Lookup,
            target_project: None,
            target_topics: Vec::new(),
            target_entities: Vec::new(),
            time_window: None,
            needed_context: Default::default(),
            retrieval_routes: vec![Route::Vector, Route::Keyword],
            graph_expansion: crate::context_runtime::query_plan::GraphExpansion {
                max_hops: 1,
                seed_kinds: Vec::new(),
                allowed_edges: Vec::new(),
            },
            budget_tokens: 1200,
        }
    }

    #[test]
    fn fuse_combines_weighted_route_scores() {
        let plan = dummy_plan();
        let weights = FusionWeights::default();
        let hits = vec![
            RouteHits {
                route: Route::Vector,
                hits: vec![hit("a", 0.9, RouteBranch::Semantic), hit("b", 0.6, RouteBranch::Semantic)],
                elapsed_ms: 1,
            },
            RouteHits {
                route: Route::Keyword,
                hits: vec![hit("a", 0.4, RouteBranch::Keyword)],
                elapsed_ms: 1,
            },
        ];
        let fused = fuse(&plan, hits, &weights);
        assert_eq!(fused[0].memory_id, "a");
        assert!(fused[0].score > fused[1].score);
        assert_eq!(fused[0].contributing_routes.len(), 2);
        assert!(fused[0].surfacing_reason.routes.contains(&"vector".to_string()));
        assert!(fused[0].surfacing_reason.routes.contains(&"keyword".to_string()));
    }

    #[test]
    fn fuse_empty_input_returns_empty() {
        let plan = dummy_plan();
        let fused = fuse(&plan, Vec::new(), &FusionWeights::default());
        assert!(fused.is_empty());
    }

    #[test]
    fn for_intent_debug_boosts_graph_and_drops_vector() {
        let w = FusionWeights::for_intent(PlannerIntent::Debug);
        assert!((w.graph - 0.20).abs() < f32::EPSILON);
        assert!((w.vector - 0.35).abs() < f32::EPSILON);
    }
}
