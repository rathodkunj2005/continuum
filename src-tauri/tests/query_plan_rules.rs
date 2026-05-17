use fndr_lib::context_runtime::query_plan::{
    apply_refinement_json, plan, EntityAliasHint, EntityHintKind, PlanHints, PlannerIntent, Route,
};
use fndr_lib::graph::schema::{GraphEdgeType, GraphNodeType};

fn route_names(plan_routes: &[Route]) -> Vec<Route> {
    plan_routes.to_vec()
}

#[test]
fn definition_query_uses_graph_evidence() {
    let plan = plan("why is the planner debounce 250ms", &PlanHints::default());

    assert_eq!(plan.intent, PlannerIntent::Definition);
    assert_eq!(
        route_names(&plan.retrieval_routes),
        vec![Route::Vector, Route::Keyword, Route::Graph]
    );
    assert_eq!(plan.graph_expansion.max_hops, 2);
    assert!(plan
        .graph_expansion
        .allowed_edges
        .contains(&GraphEdgeType::EvidencedBy));
}

#[test]
fn debug_query_requests_error_context() {
    let plan = plan(
        "debug the LanceDB schema error from yesterday",
        &PlanHints::default(),
    );

    assert_eq!(plan.intent, PlannerIntent::Debug);
    assert!(plan.needed_context.errors);
    assert!(plan.retrieval_routes.contains(&Route::Temporal));
    assert!(plan
        .graph_expansion
        .allowed_edges
        .contains(&GraphEdgeType::Causes));
}

#[test]
fn resume_work_query_uses_session_and_project_edges() {
    let plan = plan(
        "what was I working on yesterday in FNDR",
        &PlanHints::default(),
    );

    assert_eq!(plan.intent, PlannerIntent::ResumeWork);
    assert!(plan.needed_context.recent_changes);
    assert!(plan.time_window.is_some());
    assert!(plan
        .graph_expansion
        .allowed_edges
        .contains(&GraphEdgeType::OccurredInSession));
}

#[test]
fn lookup_query_detects_entity_route() {
    let plan = plan(
        "where did I mention ScreenCaptureKit",
        &PlanHints::default(),
    );

    assert_eq!(plan.intent, PlannerIntent::Lookup);
    assert!(plan.retrieval_routes.contains(&Route::Entity));
    assert!(plan
        .target_entities
        .iter()
        .any(|entity| entity.label == "ScreenCaptureKit"));
}

#[test]
fn how_to_query_detects_file_and_command_entities() {
    let plan = plan(
        "how do I run cargo test for src-tauri/src/graph/graph_index.rs",
        &PlanHints::default(),
    );

    assert_eq!(plan.intent, PlannerIntent::HowTo);
    assert!(plan.needed_context.commands);
    assert!(plan
        .target_entities
        .iter()
        .any(|entity| entity.kind == EntityHintKind::Command && entity.label == "cargo test"));
    assert!(plan
        .target_entities
        .iter()
        .any(|entity| entity.kind == EntityHintKind::File));
}

#[test]
fn timeline_query_disables_graph_hops() {
    let plan = plan("timeline of planner changes today", &PlanHints::default());

    assert_eq!(plan.intent, PlannerIntent::Timeline);
    assert!(plan.retrieval_routes.contains(&Route::Temporal));
    assert_eq!(plan.graph_expansion.max_hops, 0);
}

#[test]
fn related_query_uses_similarity_edges() {
    let plan = plan(
        "what is related to MCP retrieval feedback",
        &PlanHints::default(),
    );

    assert_eq!(plan.intent, PlannerIntent::RelatedTo);
    assert_eq!(plan.graph_expansion.max_hops, 2);
    assert!(plan
        .graph_expansion
        .allowed_edges
        .contains(&GraphEdgeType::SimilarTo));
}

#[test]
fn project_alias_hints_select_target_project() {
    let hints = PlanHints {
        entity_aliases: vec![EntityAliasHint {
            alias: "fndr".to_string(),
            canonical_name: "FNDR".to_string(),
            entity_type: "project".to_string(),
            project: Some("FNDR".to_string()),
        }],
        ..PlanHints::default()
    };

    let plan = plan("resume work on FNDR", &hints);

    assert_eq!(plan.target_project.as_deref(), Some("FNDR"));
    assert!(plan.retrieval_routes.contains(&Route::Entity));
    assert!(plan
        .graph_expansion
        .seed_kinds
        .contains(&GraphNodeType::Project));
}

#[test]
fn url_and_dotted_identifiers_become_entity_hints() {
    let plan = plan(
        "lookup https://example.com and context_runtime.query_plan",
        &PlanHints::default(),
    );

    assert!(plan
        .target_entities
        .iter()
        .any(|entity| entity.kind == EntityHintKind::Url));
    assert!(plan
        .target_entities
        .iter()
        .any(|entity| entity.kind == EntityHintKind::Tool));
}

#[test]
fn refinement_json_merges_only_present_fields() {
    let mut query_plan = plan("lookup graph rerank", &PlanHints::default());

    let changed = apply_refinement_json(
        &mut query_plan,
        r#"{"target_project":"FNDR","target_topics":["graph rerank"],"graph_max_hops":2}"#,
    );

    assert!(changed);
    assert_eq!(query_plan.target_project.as_deref(), Some("FNDR"));
    assert_eq!(query_plan.target_topics, vec!["graph rerank"]);
    assert_eq!(query_plan.graph_expansion.max_hops, 2);
}

#[tokio::test]
async fn llm_refinement_smoke_skips_when_model_missing() {
    let Ok(engine) = fndr_lib::inference::InferenceEngine::new(None, None).await else {
        return;
    };

    let mut successes = 0;
    for query in [
        "resume work on FNDR",
        "why is the graph rerank timeout 400ms",
        "related memories for LanceDB schema migration",
    ] {
        if engine.refine_query_plan(query, "{}", 400).await.is_some() {
            successes += 1;
        }
    }

    assert!(successes >= 2);
}
