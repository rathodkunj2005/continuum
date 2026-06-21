use continuum_lib::config::{SearchConfig, DEFAULT_IMAGE_EMBEDDING_DIM};
use continuum_lib::context_runtime::query_plan::{
    plan, EntityHint, EntityHintKind, GraphExpansion, PlanHints, Route, TimeWindow,
};
use continuum_lib::context_runtime::retrieval_routes::{RouteCtx, RouteRunner};
use continuum_lib::embedding::{Embedder, EMBEDDING_DIM};
use continuum_lib::graph::graph_index::GraphIndex;
use continuum_lib::graph::schema::{GraphEdge, GraphEdgeType, GraphNode, GraphNodeType};
use continuum_lib::storage::{MemoryRecord, Store};
use uuid::Uuid;

fn record(id: &str, text: &str, timestamp: i64, embedding: Vec<f32>) -> MemoryRecord {
    MemoryRecord {
        id: id.to_string(),
        timestamp,
        app_name: "Terminal".to_string(),
        window_title: "Agentic Graph RAG".to_string(),
        session_id: "phase-2-session".to_string(),
        text: text.to_string(),
        clean_text: text.to_string(),
        snippet: text.to_string(),
        summary_source: "llm".to_string(),
        embedding: embedding.clone(),
        snippet_embedding: embedding,
        support_embedding: vec![0.0; EMBEDDING_DIM],
        image_embedding: vec![0.0; DEFAULT_IMAGE_EMBEDDING_DIM],
        decay_score: 1.0,
        project: "Continuum".to_string(),
        entities: vec!["Continuum".to_string()],
        ..Default::default()
    }
}

fn node(id: u128, label: &str, node_type: GraphNodeType, memory_id: &str) -> GraphNode {
    GraphNode {
        id: Uuid::from_u128(id),
        node_type,
        label: label.to_string(),
        confidence: 0.92,
        source_memory_ids: vec![memory_id.to_string()],
        embedding: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        stale: false,
        metadata: serde_json::json!({}),
    }
}

fn edge(source: u128, target: u128, edge_type: GraphEdgeType) -> GraphEdge {
    GraphEdge {
        id: Uuid::new_v4(),
        source_id: Uuid::from_u128(source),
        target_id: Uuid::from_u128(target),
        edge_type,
        confidence: 0.9,
        conflict_flag: false,
        created_at: chrono::Utc::now(),
        metadata: serde_json::json!({}),
    }
}

#[test]
fn dispatch_runs_all_five_routes_and_graph_returns_path() {
    std::env::set_var("CONTINUUM_ALLOW_MOCK_EMBEDDER", "1");
    let now = chrono::Utc::now().timestamp_millis();
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::new(dir.path()).expect("store");
    let embedder = Embedder::new().expect("embedder");

    let texts = [
        "Continuum planner alpha vector keyword seed memory",
        "Connected graph evidence reached from the planner seed",
        "Temporal route note from today for Continuum",
        "Continuum project entity route anchor",
    ];
    let embeddings = embedder
        .embed_batch(
            &texts
                .iter()
                .map(|text| text.to_string())
                .collect::<Vec<_>>(),
        )
        .expect("embeddings");
    let records = vec![
        record("m-seed", texts[0], now - 1_000, embeddings[0].clone()),
        record("m-graph", texts[1], now - 900, embeddings[1].clone()),
        record("m-temporal", texts[2], now - 800, embeddings[2].clone()),
        record("m-entity", texts[3], now - 700, embeddings[3].clone()),
    ];

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        store.add_batch(&records).await.expect("add records");
    });

    let graph_nodes = vec![
        node(1, "planner seed", GraphNodeType::Concept, "m-seed"),
        node(
            2,
            "connected graph evidence",
            GraphNodeType::Concept,
            "m-graph",
        ),
        node(3, "Continuum", GraphNodeType::Project, "m-entity"),
    ];
    let graph_edges = vec![edge(1, 2, GraphEdgeType::SameTaskAs)];
    let graph_index = GraphIndex::build(&graph_nodes, &graph_edges);

    let mut query_plan = plan(
        "Continuum planner alpha today",
        &PlanHints {
            now_ms: Some(now),
            ..Default::default()
        },
    );
    query_plan.target_project = Some("Continuum".to_string());
    query_plan.target_entities = vec![EntityHint {
        label: "Continuum".to_string(),
        kind: EntityHintKind::Concept,
    }];
    query_plan.time_window = Some(TimeWindow {
        from_ms: now - 60_000,
        to_ms: now + 60_000,
    });
    query_plan.retrieval_routes = vec![
        Route::Vector,
        Route::Keyword,
        Route::Temporal,
        Route::Entity,
        Route::Graph,
    ];
    query_plan.graph_expansion = GraphExpansion {
        max_hops: 1,
        seed_kinds: vec![GraphNodeType::Concept],
        allowed_edges: vec![GraphEdgeType::SameTaskAs],
    };

    let config = SearchConfig::default().normalized();
    let ctx = RouteCtx::new(&store, &config)
        .with_embedder(&embedder)
        .with_graph(&graph_index, &graph_nodes, &graph_edges)
        .with_limits(8, None, None, &[])
        .with_now_ms(now)
        .allowing_mock_vectors();

    let route_hits = rt.block_on(async { RouteRunner::dispatch(&query_plan, &ctx).await });
    for route in [
        Route::Vector,
        Route::Keyword,
        Route::Temporal,
        Route::Entity,
        Route::Graph,
    ] {
        let hits = route_hits
            .iter()
            .find(|group| group.route == route)
            .unwrap_or_else(|| panic!("missing {route:?} route"));
        assert!(!hits.hits.is_empty(), "expected {route:?} hits");
    }

    let graph_hits = route_hits
        .iter()
        .find(|group| group.route == Route::Graph)
        .expect("graph route");
    assert!(
        graph_hits
            .hits
            .iter()
            .any(|hit| hit.graph_path.as_ref().is_some_and(|path| !path.is_empty())),
        "graph route should return at least one hit with a graph path"
    );
}
