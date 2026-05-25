use crate::config::SearchConfig;
use crate::context_runtime::query_plan::{QueryPlan, Route};
use crate::embedding::Embedder;
use crate::graph::graph_index::GraphIndex;
use crate::graph::schema::{GraphEdge, GraphEdgeType, GraphNode};
use crate::inference::InferenceEngine;
use crate::storage::{MemoryRecord, SearchResult, Store};
use crate::telemetry::runtime_metrics;
use futures::future::{join_all, BoxFuture};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashSet;
use std::time::Instant;

#[derive(Clone)]
pub struct RouteCtx<'a> {
    pub store: &'a Store,
    pub graph_index: Option<&'a GraphIndex>,
    pub graph_nodes: &'a [GraphNode],
    pub graph_edges: &'a [GraphEdge],
    pub inference: Option<&'a InferenceEngine>,
    pub embedder: Option<&'a Embedder>,
    pub search_config: &'a SearchConfig,
    pub limit: usize,
    pub time_filter: Option<&'a str>,
    pub app_filter: Option<&'a str>,
    pub expansion: &'a [String],
    pub prior_route_hits: Vec<RouteHits>,
    pub allow_mock_vectors: bool,
    pub now_ms: i64,
}

impl<'a> RouteCtx<'a> {
    pub fn new(store: &'a Store, search_config: &'a SearchConfig) -> Self {
        Self {
            store,
            graph_index: None,
            graph_nodes: &[],
            graph_edges: &[],
            inference: None,
            embedder: None,
            search_config,
            limit: 12,
            time_filter: None,
            app_filter: None,
            expansion: &[],
            prior_route_hits: Vec::new(),
            allow_mock_vectors: false,
            now_ms: chrono::Utc::now().timestamp_millis(),
        }
    }

    pub fn with_embedder(mut self, embedder: &'a Embedder) -> Self {
        self.embedder = Some(embedder);
        self
    }

    pub fn with_inference(mut self, inference: Option<&'a InferenceEngine>) -> Self {
        self.inference = inference;
        self
    }

    pub fn with_graph(
        mut self,
        graph_index: &'a GraphIndex,
        graph_nodes: &'a [GraphNode],
        graph_edges: &'a [GraphEdge],
    ) -> Self {
        self.graph_index = Some(graph_index);
        self.graph_nodes = graph_nodes;
        self.graph_edges = graph_edges;
        self
    }

    pub fn with_limits(
        mut self,
        limit: usize,
        time_filter: Option<&'a str>,
        app_filter: Option<&'a str>,
        expansion: &'a [String],
    ) -> Self {
        self.limit = limit.max(1);
        self.time_filter = time_filter;
        self.app_filter = app_filter;
        self.expansion = expansion;
        self
    }

    pub fn with_prior_route_hits(mut self, prior_route_hits: Vec<RouteHits>) -> Self {
        self.prior_route_hits = prior_route_hits;
        self
    }

    pub fn allowing_mock_vectors(mut self) -> Self {
        self.allow_mock_vectors = true;
        self
    }

    pub fn with_now_ms(mut self, now_ms: i64) -> Self {
        self.now_ms = now_ms;
        self
    }
}

pub trait RetrievalRoute: Send + Sync {
    fn route(&self) -> Route;
    fn run<'a>(&'a self, plan: &'a QueryPlan, ctx: &'a RouteCtx<'a>) -> BoxFuture<'a, RouteHits>;
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
pub enum RouteBranch {
    Chunk,
    Semantic,
    Snippet,
    Keyword,
    Temporal,
    Entity,
    Graph,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct RouteSignals {
    pub branch: RouteBranch,
    pub confidence: f32,
    #[serde(skip)]
    #[specta(skip)]
    pub search_result: Option<SearchResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
pub struct PathStep {
    pub from_label: String,
    pub edge: GraphEdgeType,
    pub to_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct RouteHit {
    pub memory_id: String,
    pub score: f32,
    pub signals: RouteSignals,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph_path: Option<Vec<PathStep>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct RouteHits {
    pub route: Route,
    pub hits: Vec<RouteHit>,
    pub elapsed_ms: u64,
}

impl RouteHits {
    pub fn empty(route: Route) -> Self {
        Self {
            route,
            hits: Vec::new(),
            elapsed_ms: 0,
        }
    }
}

pub struct RouteRunner;

impl RouteRunner {
    pub async fn dispatch(plan: &QueryPlan, ctx: &RouteCtx<'_>) -> Vec<RouteHits> {
        let requested = requested_routes(plan);
        let mut first_wave = Vec::new();

        for route in requested
            .iter()
            .copied()
            .filter(|route| *route != Route::Graph)
        {
            first_wave.push(run_route(route, plan, ctx));
        }

        let mut route_hits = join_all(first_wave).await;

        if requested.contains(&Route::Graph) {
            let graph_ctx = ctx.clone().with_prior_route_hits(route_hits.clone());
            route_hits.push(run_route(Route::Graph, plan, &graph_ctx).await);
        }

        route_hits
    }
}

fn requested_routes(plan: &QueryPlan) -> Vec<Route> {
    let mut seen = HashSet::new();
    let mut routes = Vec::new();
    for route in &plan.retrieval_routes {
        if seen.insert(*route) {
            routes.push(*route);
        }
    }
    routes
}

fn run_route<'a>(
    route: Route,
    plan: &'a QueryPlan,
    ctx: &'a RouteCtx<'a>,
) -> BoxFuture<'a, RouteHits> {
    match route {
        Route::Chunk => crate::context_runtime::chunk_route::ChunkRoute.run(plan, ctx),
        Route::Vector => crate::context_runtime::vector_route::VectorRoute.run(plan, ctx),
        Route::Keyword => crate::context_runtime::keyword_route::KeywordRoute.run(plan, ctx),
        Route::Temporal => crate::context_runtime::temporal_route::TemporalRoute.run(plan, ctx),
        Route::Entity => crate::context_runtime::entity_route::EntityRoute.run(plan, ctx),
        Route::Graph => crate::context_runtime::graph_route::GraphRoute.run(plan, ctx),
    }
}

pub fn hit_from_search_result(
    _route: Route,
    branch: RouteBranch,
    result: SearchResult,
) -> RouteHit {
    RouteHit {
        memory_id: result.id.clone(),
        score: result.score,
        signals: RouteSignals {
            branch,
            confidence: result.score,
            search_result: Some(result),
        },
        graph_path: None,
    }
}

pub fn memory_record_to_search_result(record: &MemoryRecord, score: f32) -> SearchResult {
    SearchResult {
        id: record.id.clone(),
        timestamp: record.timestamp,
        app_name: record.app_name.clone(),
        bundle_id: record.bundle_id.clone(),
        window_title: record.window_title.clone(),
        session_id: record.session_id.clone(),
        text: record.text.clone(),
        clean_text: record.clean_text.clone(),
        ocr_confidence: record.ocr_confidence,
        ocr_block_count: record.ocr_block_count,
        snippet: record.snippet.clone(),
        display_summary: record.display_summary.clone(),
        internal_context: record.internal_context.clone(),
        summary_source: record.summary_source.clone(),
        noise_score: record.noise_score,
        session_key: record.session_key.clone(),
        lexical_shadow: record.lexical_shadow.clone(),
        memory_context: record.memory_context.clone(),
        reopen_kind: record.reopen_kind.clone(),
        reopen_url: record.reopen_url.clone(),
        reopen_file_path: record.reopen_file_path.clone(),
        reopen_app_bundle_id: record.reopen_app_bundle_id.clone(),
        reopen_app_name: record.reopen_app_name.clone(),
        reopen_app_deep_link: record.reopen_app_deep_link.clone(),
        reopen_captured_at_ms: record.reopen_captured_at_ms,
        reopen_confidence: record.reopen_confidence,
        reopen_validation_status: record.reopen_validation_status.clone(),
        user_intent: record.user_intent.clone(),
        topic: record.topic.clone(),
        workflow: record.workflow.clone(),
        search_aliases: record.search_aliases.clone(),
        related_memory_ids: record.related_memory_ids.clone(),
        evidence_confidence: record.evidence_confidence,
        confidence_score: record.confidence_score,
        importance_score: record.importance_score,
        specificity_score: record.specificity_score,
        intent_score: record.intent_score,
        entity_score: record.entity_score,
        agent_usefulness_score: record.agent_usefulness_score,
        ocr_noise_score: record.ocr_noise_score,
        score,
        screenshot_path: record.screenshot_path.clone(),
        url: record.url.clone(),
        decay_score: record.decay_score,
        schema_version: record.schema_version,
        activity_type: record.activity_type.clone(),
        files_touched: record.files_touched.clone(),
        session_duration_mins: record.session_duration_mins,
        project: record.project.clone(),
        tags: record.tags.clone(),
        outcome: record.outcome.clone(),
        extraction_confidence: record.extraction_confidence,
        anchor_coverage_score: record.anchor_coverage_score,
        extracted_entities: record.entities.clone(),
        content_hash: record.content_hash.clone(),
        dedup_fingerprint: record.dedup_fingerprint.clone(),
        is_consolidated: record.is_consolidated,
        is_soft_deleted: record.is_soft_deleted,
        insight_what_happened: record.insight_what_happened.clone(),
        insight_why_mattered: record.insight_why_mattered.clone(),
        insight_what_changed: record.insight_what_changed.clone(),
        insight_context_thread: record.insight_context_thread.clone(),
        insight_spans_json: record.insight_spans_json.clone(),
        insight_card_confidence: record.insight_card_confidence,
        synthesis_branch: record.synthesis_branch.clone(),
        topic_categories: record.topic_categories.clone(),
        matched_routes: Vec::new(),
        matched_chunk_ids: Vec::new(),
        chunk_evidence: Vec::new(),
        enrichment_status: record.enrichment_status.clone(),
        reviewed_at_ms: record.reviewed_at_ms,
        reviewer_generation: record.reviewer_generation,
        storage_outcome: record.storage_outcome.clone(),
    }
}

pub fn finish_route(route: Route, started: Instant, hits: Vec<RouteHit>) -> RouteHits {
    let elapsed_ms = started.elapsed().as_millis() as u64;
    let route_name = route_name(route);
    runtime_metrics::record_ms(route_metric_ms(route_name), elapsed_ms);
    for _ in &hits {
        runtime_metrics::bump(route_metric_hits(route_name));
    }
    RouteHits {
        route,
        hits,
        elapsed_ms,
    }
}

fn route_name(route: Route) -> &'static str {
    match route {
        Route::Chunk => "chunk",
        Route::Vector => "vector",
        Route::Keyword => "keyword",
        Route::Temporal => "temporal",
        Route::Entity => "entity",
        Route::Graph => "graph",
    }
}

fn route_metric_ms(name: &'static str) -> &'static str {
    match name {
        "vector" => "fndr.retrieval.route.vector.ms",
        "keyword" => "fndr.retrieval.route.keyword.ms",
        "temporal" => "fndr.retrieval.route.temporal.ms",
        "entity" => "fndr.retrieval.route.entity.ms",
        "graph" => "fndr.retrieval.route.graph.ms",
        "chunk" => "fndr.retrieval.route.chunk.ms",
        _ => "fndr.retrieval.route.unknown.ms",
    }
}

fn route_metric_hits(name: &'static str) -> &'static str {
    match name {
        "vector" => "fndr.retrieval.route.vector.hits",
        "keyword" => "fndr.retrieval.route.keyword.hits",
        "temporal" => "fndr.retrieval.route.temporal.hits",
        "entity" => "fndr.retrieval.route.entity.hits",
        "graph" => "fndr.retrieval.route.graph.hits",
        "chunk" => "fndr.retrieval.route.chunk.hits",
        _ => "fndr.retrieval.route.unknown.hits",
    }
}
