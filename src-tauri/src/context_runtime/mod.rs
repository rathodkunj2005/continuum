use crate::embedding::Embedder;
use crate::mcp;
use crate::search::HybridSearcher;
use crate::storage::{
    ActivityEvent, CodeContext, CommandEvent, CommitRef, ContextDelta, ContextPack,
    ContextPackItemReason, ContextRuntimeStatus, ContextTask, DecisionLedgerEntry, DecisionSummary,
    EdgeType, EntityAliasRecord, EntityRef, ErrorEvent, EvidenceRef, ExcludedContextItem,
    FailureSummary, GraphEdge, GraphNode, HealthStatus, IssueSummary, KnowledgePage,
    KnowledgePageType, KnowledgeStability, MemoryRecord, NodeType, PrivacyClass, ProjectContext,
    RelevantFile, SearchResult, WorkingState,
};
use crate::AppState;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use tauri::Emitter;

pub mod chunk_route;
pub mod composer;
pub mod context_pack;
pub mod entity_route;
pub mod evidence_pack;
pub mod fusion;
pub mod graph_plan;
pub mod graph_route;
pub mod keyword_route;
pub mod query_plan;
pub mod retrieval_routes;
pub mod temporal_route;
pub mod vector_route;
pub mod verifier;
mod wiki_policy;

static URL_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"https?://[^\s)>"]+"#).expect("valid URL regex"));
static FILE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?x)
        (?:
            [A-Za-z0-9._-]+/
        )+
        [A-Za-z0-9._-]+
        (?:\.[A-Za-z0-9._-]+)?
    ",
    )
    .expect("valid file regex")
});
static ISSUE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\B#(?P<num>\d{1,6})\b").expect("valid issue regex"));
static COMMAND_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?m)\b(cargo|git|npm|pnpm|yarn|make|python3?|uv|pytest|bash|zsh)\b[^\n\r]*")
        .expect("valid command regex")
});
static ERROR_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?im)(error|failed|exception|panic|traceback)[^\n\r]*").expect("valid error regex")
});

const DEFAULT_CONTEXT_BUDGET: u32 = 600;
const DEFAULT_SEARCH_LIMIT: usize = 12;
const PROJECT_PATH_MARKERS: &[&str] = &[
    "src",
    "src-tauri",
    "app",
    "apps",
    "lib",
    "libs",
    "crates",
    "packages",
    "pkg",
    "client",
    "server",
    "backend",
    "frontend",
    "components",
    "tests",
    "test",
    "docs",
];
const GENERIC_PATH_SEGMENTS: &[&str] = &[
    "users",
    "user",
    "home",
    "workspace",
    "workspaces",
    "repo",
    "repos",
    "repository",
    "repositories",
    "project",
    "projects",
    "desktop",
    "documents",
    "downloads",
    "code",
    "dev",
    "tmp",
    "private",
    "var",
    "opt",
    "applications",
    "library",
    "volumes",
];

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContextRequest {
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub agent_type: String,
    #[serde(default)]
    pub budget_tokens: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default)]
    pub active_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CodeContextRequest {
    #[serde(default)]
    pub query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub budget_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DecisionProposal {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub proposed_by: String,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SessionContextState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_pack_id: Option<String>,
    #[serde(default)]
    last_generated_at: i64,
}

pub async fn sync_memory_record(
    state: &AppState,
    record: &MemoryRecord,
    source_hint: Option<&str>,
) -> Result<ActivityEvent, String> {
    let _ = state.graph.ingest_memory(record).await;

    let event = build_activity_event(state, record, source_hint).await?;
    state
        .store
        .upsert_activity_events(std::slice::from_ref(&event))
        .await
        .map_err(|e| e.to_string())?;

    let aliases = build_alias_records(&event);
    if !aliases.is_empty() {
        state
            .store
            .upsert_entity_aliases(&aliases)
            .await
            .map_err(|e| e.to_string())?;
    }

    upsert_runtime_graph(state, record, &event)
        .await
        .map_err(|e| e.to_string())?;

    if let Some(project) = event.project.as_deref() {
        let project_context = rebuild_project_context(state, project).await?;
        state
            .store
            .upsert_project_contexts(std::slice::from_ref(&project_context))
            .await
            .map_err(|e| e.to_string())?;
        if let Ok(pages) = compile_knowledge_pages(state, Some(project)).await {
            let _ = state.store.upsert_knowledge_pages(&pages).await;
        }
    } else if let Ok(pages) = compile_knowledge_pages(state, None).await {
        let _ = state.store.upsert_knowledge_pages(&pages).await;
    }

    // Proactive delta push for active subscriptions
    let subs = state.runtime_subscriptions.read().clone();
    if !subs.is_empty() {
        let handle_opt = state.app_handle.read().as_ref().cloned();
        if let Some(handle) = handle_opt {
            for session_id in subs {
                if let Ok(delta) = build_context_delta(state, &session_id, None).await {
                    let _ = handle.emit("context://delta", delta);
                }
            }
        }
    }

    Ok(event)
}

pub async fn sync_memory_records(
    state: &AppState,
    records: &[MemoryRecord],
    source_hint: Option<&str>,
) -> Result<Vec<ActivityEvent>, String> {
    let mut events = Vec::new();
    for record in records {
        match sync_memory_record(state, record, source_hint).await {
            Ok(event) => events.push(event),
            Err(err) => tracing::warn!("Context runtime sync failed for {}: {}", record.id, err),
        }
    }
    Ok(events)
}

/// Bounded JSON snapshot of the Lance insight graph for MCP and [`ContextPack::graph_context`].
pub async fn insight_graph_context_mcp(
    state: &AppState,
    project: Option<&str>,
) -> Option<serde_json::Value> {
    use crate::graph::graph_store::GraphStore;
    use crate::graph::schema::{GraphEdgeType, GraphNodeType};

    let gs = GraphStore::new(state.store.clone());
    let nodes = gs.all_nodes().await.ok()?;
    let edges = gs.all_edges().await.ok()?;
    let mut pr_nodes: Vec<_> = nodes
        .iter()
        .filter(|n| n.node_type == GraphNodeType::Project)
        .cloned()
        .collect();
    if let Some(p) = project {
        if !p.trim().is_empty() {
            pr_nodes.retain(|n| {
                n.label.eq_ignore_ascii_case(p)
                    || n.label.contains(p)
                    || p.contains(n.label.as_str())
            });
        }
    }
    pr_nodes.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());
    pr_nodes.truncate(5);
    let mut e_sorted = edges.clone();
    e_sorted.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());
    let top_edges: Vec<serde_json::Value> = e_sorted
        .iter()
        .take(3)
        .map(|e| {
            json!({
                "source": e.source_id,
                "target": e.target_id,
                "edge_type": format!("{:?}", e.edge_type),
                "confidence": e.confidence,
                "conflict_flag": e.conflict_flag,
            })
        })
        .collect();
    let conflicts: Vec<serde_json::Value> = edges
        .iter()
        .filter(|e| e.conflict_flag || e.edge_type == GraphEdgeType::Contradicts)
        .take(8)
        .map(|e| {
            json!({
                "source": e.source_id,
                "target": e.target_id,
                "confidence": e.confidence,
            })
        })
        .collect();
    let mut wiki = crate::wiki::synthesize_wiki_stub(project);
    if wiki.len() > 1800 {
        wiki.truncate(1797);
        wiki.push_str("...");
    }
    let pr_json: Vec<serde_json::Value> = pr_nodes
        .iter()
        .map(|n| {
            json!({
                "id": n.id,
                "label": n.label,
                "confidence": n.confidence,
                "source_memory_ids": n.source_memory_ids,
            })
        })
        .collect();
    let out = json!({
        "top_project_nodes": pr_json,
        "top_edges": top_edges,
        "conflicts": conflicts,
        "wiki_summary": wiki,
    });
    let s = serde_json::to_string(&out).unwrap_or_default();
    if s.len() > 7500 {
        Some(json!({
            "truncated": true,
            "preview": s.chars().take(7400).collect::<String>(),
        }))
    } else {
        Some(out)
    }
}

pub async fn build_context_pack(
    state: &AppState,
    request: ContextRequest,
) -> Result<ContextPack, String> {
    let budget_tokens = normalize_budget(request.budget_tokens);
    let mut excluded = Vec::new();
    let mut candidates = if request.query.trim().is_empty() {
        state
            .store
            .list_recent_results(DEFAULT_SEARCH_LIMIT, None)
            .await
            .map_err(|e| e.to_string())?
    } else {
        let embedder = Embedder::new().map_err(|e| e.to_string())?;
        HybridSearcher::search(
            &state.store,
            &embedder,
            request.query.trim(),
            DEFAULT_SEARCH_LIMIT,
            None,
            None,
        )
        .await
        .map_err(|e| e.to_string())?
    };

    let mut events = Vec::new();
    let mut seen_event_ids = HashSet::new();
    for result in &candidates {
        let Some(event) = ensure_event_for_result(state, result).await? else {
            continue;
        };
        if let Some(request_project) = request.project.as_deref() {
            if event.project.as_deref() != Some(request_project) {
                excluded.push(ExcludedContextItem {
                    id: result.id.clone(),
                    reason: format!(
                        "project mismatch ({})",
                        event.project.clone().unwrap_or_else(|| "none".to_string())
                    ),
                });
                continue;
            }
        }
        if seen_event_ids.insert(event.id.clone()) {
            events.push(event);
        }
    }

    if events.is_empty() {
        events = state
            .store
            .list_activity_events(6, request.project.as_deref())
            .await
            .map_err(|e| e.to_string())?;
    }

    let fallback_project = latest_active_project(state).await;
    let active_project = request
        .project
        .clone()
        .or_else(|| infer_project_from_files(&request.active_files))
        .or_else(|| events.iter().find_map(|event| event.project.clone()))
        .or(fallback_project);

    if let Some(project) = active_project.as_deref() {
        let recent_project_events = state
            .store
            .list_activity_events(8, Some(project))
            .await
            .map_err(|e| e.to_string())?;
        for event in recent_project_events {
            if seen_event_ids.insert(event.id.clone()) {
                events.push(event);
            }
        }
    }

    events.sort_by_key(|event| std::cmp::Reverse(event.end_time));
    if events.len() > 12 {
        events.truncate(12);
    }

    let project_context = if let Some(project) = active_project.as_deref() {
        let stored_context = state
            .store
            .get_project_context(project)
            .await
            .map_err(|e| e.to_string())?;
        match stored_context {
            Some(context) => Some(context),
            None => Some(rebuild_project_context(state, project).await?),
        }
    } else {
        None
    };

    let relevant_files =
        collect_relevant_files(&request.active_files, &events, project_context.as_ref());
    let recent_decisions = collect_recent_decisions(&events, project_context.as_ref());
    let known_failures = collect_failures(&events, project_context.as_ref());
    let open_tasks = collect_open_tasks(state, active_project.as_deref()).await?;
    let open_issues = collect_open_issues(&events);
    let evidence = collect_evidence(&events);
    let active_goal = project_context
        .as_ref()
        .and_then(|context| {
            (!context.active_goal.trim().is_empty()).then(|| context.active_goal.clone())
        })
        .or_else(|| {
            events
                .iter()
                .find_map(|event| event.next_steps.first().cloned())
        })
        .or_else(|| events.iter().find_map(|event| event.intent.clone()));
    let summary = build_pack_summary(active_project.as_deref(), &events, &request.query);
    let recommended_next_action = active_goal
        .clone()
        .or_else(|| {
            events
                .iter()
                .find_map(|event| event.next_steps.first().cloned())
        })
        .or_else(|| {
            known_failures
                .first()
                .map(|failure| failure.summary.clone())
        });
    let do_not_do = vec![
        "Do not store or request raw screenshots by default.".to_string(),
        "Do not treat FNDR context as permission to mutate memory directly.".to_string(),
    ];
    let included = build_included_reasons(
        &events,
        &relevant_files,
        &recent_decisions,
        &known_failures,
        &request.active_files,
        &request.query,
        active_project.as_deref(),
    );

    for result in candidates.drain(..) {
        if events.iter().any(|event| event.memory_id == result.id) {
            continue;
        }
        if excluded.iter().any(|item| item.id == result.id) {
            continue;
        }
        excluded.push(ExcludedContextItem {
            id: result.id,
            reason: "zero graph/entity relevance after runtime filtering".to_string(),
        });
    }

    let graph_context = insight_graph_context_mcp(state, active_project.as_deref()).await;

    let mut pack = ContextPack {
        id: format!("ctx_{}", uuid::Uuid::new_v4().simple()),
        session_id: request.session_id.clone(),
        generated_at: chrono::Utc::now().timestamp_millis(),
        project: active_project.clone(),
        agent_type: normalize_agent_type(&request.agent_type),
        budget_tokens,
        tokens_used: 0,
        query: (!request.query.trim().is_empty()).then(|| request.query.trim().to_string()),
        active_goal,
        summary,
        relevant_files,
        recent_decisions,
        open_issues,
        known_failures,
        open_tasks,
        recommended_next_action,
        do_not_do,
        evidence,
        included,
        excluded,
        confidence: average_confidence(&events),
        graph_context,
        surfacing_reasons: Vec::new(),
        verify_outcome: None,
    };

    apply_section_budgets(&mut pack);
    trim_pack_to_budget(&mut pack);
    pack.tokens_used = estimate_pack_tokens(&pack);

    state
        .store
        .append_context_packs(std::slice::from_ref(&pack))
        .await
        .map_err(|e| e.to_string())?;

    if let Some(session_id) = request.session_id.as_deref() {
        save_session_state(
            state,
            session_id,
            SessionContextState {
                last_pack_id: Some(pack.id.clone()),
                last_generated_at: pack.generated_at,
            },
        )?;
    }

    Ok(pack)
}

pub async fn build_code_context(
    state: &AppState,
    request: CodeContextRequest,
) -> Result<CodeContext, String> {
    let pack = build_context_pack(
        state,
        ContextRequest {
            query: request.query.clone(),
            agent_type: "coding_agent".to_string(),
            budget_tokens: normalize_budget(request.budget_tokens),
            session_id: None,
            active_files: request.files.clone(),
            project: request.repo.clone(),
        },
    )
    .await?;

    let project = pack.project.clone().unwrap_or_else(|| {
        request
            .repo
            .clone()
            .unwrap_or_else(|| "workspace".to_string())
    });
    let events = state
        .store
        .list_activity_events(10, pack.project.as_deref())
        .await
        .map_err(|e| e.to_string())?;
    let recent_commands = collect_command_events(&events);
    let recent_errors = collect_error_events(&events);

    Ok(CodeContext {
        repo: project,
        branch: None,
        active_files: request.files,
        related_files: pack.relevant_files,
        recent_commands,
        recent_errors,
        recent_commits: Vec::<CommitRef>::new(),
        relevant_decisions: pack.recent_decisions,
        unresolved_tasks: pack.open_tasks,
        recommended_context: pack.recommended_next_action.unwrap_or_else(|| pack.summary),
    })
}

pub async fn build_context_delta(
    state: &AppState,
    session_id: &str,
    since_timestamp: Option<i64>,
) -> Result<ContextDelta, String> {
    let session_state = load_session_state(state, session_id).unwrap_or_default();
    let baseline = since_timestamp.unwrap_or(session_state.last_generated_at);
    let recent_events = state
        .store
        .list_activity_events(24, None)
        .await
        .map_err(|e| e.to_string())?;
    let new_events = recent_events
        .into_iter()
        .filter(|event| event.end_time > baseline)
        .collect::<Vec<_>>();
    let changed_entities = dedupe_entities(
        new_events
            .iter()
            .flat_map(|event| event.entities.clone())
            .collect(),
    );
    let new_failures = collect_failures(&new_events, None);
    let new_items = new_events
        .iter()
        .map(|event| format!("{}: {}", event.title, trim_chars(&event.summary, 96)))
        .collect::<Vec<_>>();

    let mut delta = ContextDelta {
        id: format!("ctxd_{}", uuid::Uuid::new_v4().simple()),
        session_id: session_id.to_string(),
        since: baseline,
        generated_at: chrono::Utc::now().timestamp_millis(),
        query: None,
        new_events,
        changed_entities,
        resolved_tasks: Vec::new(),
        new_failures,
        new_items,
        tokens_used: 0,
    };
    delta.tokens_used = estimate_delta_tokens(&delta);

    state
        .store
        .append_context_deltas(std::slice::from_ref(&delta))
        .await
        .map_err(|e| e.to_string())?;

    save_session_state(
        state,
        session_id,
        SessionContextState {
            last_pack_id: session_state.last_pack_id,
            last_generated_at: delta.generated_at,
        },
    )?;

    Ok(delta)
}

pub async fn get_recent_working_state(
    state: &AppState,
    project: Option<String>,
) -> Result<WorkingState, String> {
    let project = project.or(latest_active_project(state).await);
    let recent_events = state
        .store
        .list_activity_events(8, project.as_deref())
        .await
        .map_err(|e| e.to_string())?;
    let relevant_files = collect_relevant_files(&[], &recent_events, None);
    let known_failures = collect_failures(&recent_events, None);
    let open_tasks = collect_open_tasks(state, project.as_deref()).await?;
    let recent_commands = recent_events
        .iter()
        .flat_map(|event| event.commands.clone())
        .collect::<Vec<_>>();
    let recent_errors = recent_events
        .iter()
        .flat_map(|event| event.errors.clone())
        .collect::<Vec<_>>();

    Ok(WorkingState {
        project,
        summary: build_pack_summary(None, &recent_events, ""),
        active_goal: recent_events
            .iter()
            .find_map(|event| event.next_steps.first().cloned()),
        recent_events: recent_events.clone(),
        relevant_files,
        open_tasks,
        known_failures,
        recent_commands,
        recent_errors,
        confidence: average_confidence(&recent_events),
    })
}

pub async fn remember_decision(
    state: &AppState,
    proposal: DecisionProposal,
) -> Result<DecisionLedgerEntry, String> {
    if proposal.title.trim().is_empty() {
        return Err("Decision title is required.".to_string());
    }
    let mut evidence = Vec::new();
    for evidence_id in &proposal.evidence_ids {
        if let Some(memory) = state
            .store
            .get_memory_by_id(evidence_id)
            .await
            .map_err(|e| e.to_string())?
        {
            evidence.push(memory_to_evidence(
                &memory,
                classify_source_type(&memory, None),
            ));
        }
    }

    let entry = DecisionLedgerEntry {
        id: format!("decision_{}", uuid::Uuid::new_v4().simple()),
        project: proposal.project.clone(),
        title: proposal.title.trim().to_string(),
        summary: proposal.summary.trim().to_string(),
        status: "proposed".to_string(),
        proposed_by: if proposal.proposed_by.trim().is_empty() {
            "agent".to_string()
        } else {
            proposal.proposed_by.trim().to_string()
        },
        evidence,
        privacy_class: PrivacyClass::Project,
        created_at: chrono::Utc::now().timestamp_millis(),
    };

    state
        .store
        .append_decision_ledger_entries(std::slice::from_ref(&entry))
        .await
        .map_err(|e| e.to_string())?;

    if let Some(project) = proposal.project.as_deref() {
        let project_context = rebuild_project_context(state, project).await?;
        state
            .store
            .upsert_project_contexts(std::slice::from_ref(&project_context))
            .await
            .map_err(|e| e.to_string())?;
    }

    Ok(entry)
}

pub async fn health_check(state: &AppState) -> Result<HealthStatus, String> {
    let runtime_tables = vec![
        "activity_events".to_string(),
        "project_contexts".to_string(),
        "decision_ledger".to_string(),
        "context_packs".to_string(),
        "context_deltas".to_string(),
        "entity_aliases".to_string(),
        "knowledge_pages".to_string(),
    ];
    let last_pack = state
        .store
        .list_context_packs(1, None)
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .next();
    let mut degraded_reasons = Vec::new();
    if !state.ai_model_available() {
        degraded_reasons.push("Local inference model is not downloaded.".to_string());
    }
    let storage_usage_bytes = recursive_size(&state.store.data_dir());
    let status = if degraded_reasons.is_empty() {
        "healthy"
    } else {
        "degraded"
    };

    Ok(HealthStatus {
        status: status.to_string(),
        index_ready: true,
        embedding_model: state.config.read().embedding.model_name.clone(),
        embedding_dimension: state.config.read().embedding.dimension as u32,
        model_status: if state.ai_model_loaded() {
            "loaded".to_string()
        } else if state.ai_model_available() {
            "downloaded".to_string()
        } else {
            "missing".to_string()
        },
        failed_jobs: 0,
        queue_lag_ms: 0,
        storage_usage_bytes,
        runtime_tables,
        degraded_reasons,
        active_project: latest_active_project(state).await,
        last_context_pack_id: last_pack.map(|pack| pack.id),
    })
}

pub async fn get_context_runtime_status(state: &AppState) -> Result<ContextRuntimeStatus, String> {
    let latest_pack = state
        .store
        .list_context_packs(1, None)
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .next();
    let activity_event_count = state
        .store
        .count_activity_events()
        .await
        .map_err(|e| e.to_string())?;
    let decision_count = state
        .store
        .count_decision_entries()
        .await
        .map_err(|e| e.to_string())?;
    let recent_pack_count = state
        .store
        .count_context_packs()
        .await
        .map_err(|e| e.to_string())?;
    let mcp_status = mcp::status();

    Ok(ContextRuntimeStatus {
        status: if activity_event_count == 0 {
            "warming".to_string()
        } else if mcp_status.running {
            "healthy".to_string()
        } else {
            "degraded".to_string()
        },
        mcp_running: mcp_status.running,
        active_project: latest_pack
            .as_ref()
            .and_then(|pack| pack.project.clone())
            .or(latest_active_project(state).await),
        current_context_pack: latest_pack.as_ref().map(|pack| pack.id.clone()),
        recent_pack_count,
        activity_event_count,
        decision_count,
        failed_writes: 0,
        last_error: mcp_status.last_error,
        latest_pack_summary: latest_pack.as_ref().map(|pack| pack.summary.clone()),
        latest_pack_tokens_used: latest_pack.map(|pack| pack.tokens_used).unwrap_or(0),
    })
}

pub async fn list_recent_context_packs(
    state: &AppState,
    limit: usize,
) -> Result<Vec<ContextPack>, String> {
    state
        .store
        .list_context_packs(limit.max(1), None)
        .await
        .map_err(|e| e.to_string())
}

pub async fn get_context_pack_detail(
    state: &AppState,
    pack_id: &str,
) -> Result<Option<ContextPack>, String> {
    state
        .store
        .get_context_pack_by_id(pack_id)
        .await
        .map_err(|e| e.to_string())
}

pub fn render_pack_markdown(pack: &ContextPack) -> String {
    let files = if pack.relevant_files.is_empty() {
        "- No files highlighted.".to_string()
    } else {
        pack.relevant_files
            .iter()
            .map(|file| format!("- {}: {}", file.path, file.why))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let failures = if pack.known_failures.is_empty() {
        "- No current failures captured.".to_string()
    } else {
        pack.known_failures
            .iter()
            .map(|failure| format!("- {}: {}", failure.title, failure.summary))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let decisions = if pack.recent_decisions.is_empty() {
        "- No recent decisions recorded.".to_string()
    } else {
        pack.recent_decisions
            .iter()
            .map(|decision| format!("- {}: {}", decision.title, decision.summary))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let tasks = if pack.open_tasks.is_empty() {
        "- No open tasks linked.".to_string()
    } else {
        pack.open_tasks
            .iter()
            .map(|task| format!("- {} [{}]", task.title, task.status))
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        "# FNDR Context Pack\n\n\
Project: {}\n\
Agent type: {}\n\
Tokens used: {}/{}\n\n\
## Summary\n\n\
{}\n\n\
## Active goal\n\n\
{}\n\n\
## Relevant files\n\n\
{}\n\n\
## Known failures\n\n\
{}\n\n\
## Recent decisions\n\n\
{}\n\n\
## Open tasks\n\n\
{}\n",
        pack.project
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        pack.agent_type,
        pack.tokens_used,
        pack.budget_tokens,
        pack.summary,
        pack.active_goal
            .clone()
            .unwrap_or_else(|| "No active goal inferred.".to_string()),
        files,
        failures,
        decisions,
        tasks
    )
}

async fn build_activity_event(
    state: &AppState,
    record: &MemoryRecord,
    source_hint: Option<&str>,
) -> Result<ActivityEvent, String> {
    let project = detect_project_for_record(record);
    let entities = extract_entities(state, record, project.as_deref()).await?;
    let source_type = classify_source_type(record, source_hint);
    let privacy_class = classify_privacy(record);

    Ok(ActivityEvent {
        id: format!("event:{}", record.id),
        memory_id: record.id.clone(),
        start_time: record.timestamp,
        end_time: record.timestamp,
        project,
        repo: detect_repo_for_record(record),
        branch: detect_branch_for_record(record),
        activity_type: infer_activity_type(record),
        title: build_event_title(record),
        summary: if !record.memory_context.trim().is_empty() {
            trim_chars(&record.memory_context, 280)
        } else if !record.display_summary.trim().is_empty() {
            record.display_summary.clone()
        } else {
            record.snippet.clone()
        },
        intent: if !record.user_intent.trim().is_empty() {
            Some(record.user_intent.clone())
        } else {
            record.next_steps.first().cloned()
        },
        outcome: normalize_outcome(&record.outcome),
        entities,
        source_memory_ids: vec![record.id.clone()],
        evidence: vec![memory_to_evidence(record, source_type)],
        confidence: event_confidence(record),
        memory_value: estimate_memory_value(record),
        privacy_class,
        active_files: dedupe_strings(record.files_touched.clone()),
        errors: dedupe_strings(record.errors.clone()),
        commands: extract_commands(record),
        decisions: dedupe_strings(record.decisions.clone()),
        next_steps: dedupe_strings(record.next_steps.clone()),
        tags: dedupe_strings(record.tags.clone()),
        created_at: chrono::Utc::now().timestamp_millis(),
        updated_at: chrono::Utc::now().timestamp_millis(),
    })
}

async fn ensure_event_for_result(
    state: &AppState,
    result: &SearchResult,
) -> Result<Option<ActivityEvent>, String> {
    if let Some(event) = state
        .store
        .get_activity_event_by_memory_id(&result.id)
        .await
        .map_err(|e| e.to_string())?
    {
        return Ok(Some(event));
    }
    let Some(record) = state
        .store
        .get_memory_by_id(&result.id)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };
    sync_memory_record(state, &record, None).await.map(Some)
}

async fn rebuild_project_context(
    state: &AppState,
    project: &str,
) -> Result<ProjectContext, String> {
    let events = state
        .store
        .list_activity_events(18, Some(project))
        .await
        .map_err(|e| e.to_string())?;
    let decisions = collect_recent_decisions(&events, None);
    let failures = collect_failures(&events, None);
    let open_tasks = collect_open_tasks(state, Some(project)).await?;
    let relevant_files = collect_relevant_files(&[], &events, None);

    Ok(ProjectContext {
        id: format!("project:{}", normalize_alias_key(project)),
        project: project.to_string(),
        active_goal: events
            .iter()
            .find_map(|event| event.next_steps.first().cloned())
            .unwrap_or_else(|| format!("Continue recent {} work.", project)),
        summary: build_pack_summary(Some(project), &events, ""),
        relevant_files,
        recent_decisions: decisions,
        open_issues: collect_open_issues(&events),
        known_failures: failures,
        open_tasks,
        constraints: Vec::new(),
        confidence: average_confidence(&events),
        privacy_class: PrivacyClass::Project,
        updated_at: chrono::Utc::now().timestamp_millis(),
    })
}

fn compact_claim_title(prefix: &str, value: &str) -> String {
    let trimmed = value.trim().trim_end_matches('.');
    if trimmed.is_empty() {
        return prefix.to_string();
    }
    let normalized = if trimmed.len() > 120 {
        format!("{}...", &trimmed[..120])
    } else {
        trimmed.to_string()
    };
    format!("{prefix}: {normalized}")
}

fn summarize_page_context(events: &[ActivityEvent], fallback: &str) -> String {
    let mut parts = Vec::new();
    for event in events.iter().take(3) {
        if !event.summary.trim().is_empty() {
            parts.push(event.summary.trim().to_string());
        }
    }
    if parts.is_empty() {
        fallback.to_string()
    } else {
        parts.join(" ")
    }
}

fn gather_page_entities(events: &[ActivityEvent]) -> Vec<String> {
    let mut entities = Vec::new();
    let mut seen = HashSet::new();
    for event in events {
        for entity in &event.entities {
            let normalized = normalize_alias_key(&entity.canonical_name);
            if normalized.is_empty() || !seen.insert(normalized) {
                continue;
            }
            entities.push(entity.canonical_name.clone());
            if entities.len() >= 12 {
                return entities;
            }
        }
    }
    entities
}

fn collect_supporting_memory_ids(events: &[ActivityEvent]) -> Vec<String> {
    let mut ids = Vec::new();
    let mut seen = HashSet::new();
    for event in events {
        if seen.insert(event.memory_id.clone()) {
            ids.push(event.memory_id.clone());
        }
        for memory_id in &event.source_memory_ids {
            if seen.insert(memory_id.clone()) {
                ids.push(memory_id.clone());
            }
        }
    }
    ids
}

fn collect_supporting_evidence_ids(events: &[ActivityEvent]) -> Vec<String> {
    let mut ids = Vec::new();
    let mut seen = HashSet::new();
    for event in events {
        for evidence in &event.evidence {
            if seen.insert(evidence.id.clone()) {
                ids.push(evidence.id.clone());
            }
        }
    }
    ids
}

fn average_event_confidence(events: &[ActivityEvent]) -> f32 {
    if events.is_empty() {
        return 0.0;
    }
    let sum: f32 = events.iter().map(|event| event.confidence).sum();
    (sum / events.len() as f32).clamp(0.0, 1.0)
}

fn page_stability_from_support(support_count: usize) -> KnowledgeStability {
    if support_count >= 4 {
        KnowledgeStability::Stable
    } else {
        KnowledgeStability::Emerging
    }
}

pub async fn compile_knowledge_pages(
    state: &AppState,
    project_filter: Option<&str>,
) -> Result<Vec<KnowledgePage>, String> {
    let events = state
        .store
        .list_activity_events(260, project_filter)
        .await
        .map_err(|e| e.to_string())?;
    if events.is_empty() {
        return Ok(Vec::new());
    }

    let mut pages: Vec<KnowledgePage> = Vec::new();

    let mut by_project: HashMap<String, Vec<ActivityEvent>> = HashMap::new();
    for event in &events {
        if let Some(project) = event.project.as_deref() {
            by_project
                .entry(project.to_string())
                .or_default()
                .push(event.clone());
        }
    }
    for (project, project_events) in by_project {
        let first_seen = project_events
            .iter()
            .map(|event| event.start_time)
            .min()
            .unwrap_or_default();
        let last_updated = project_events
            .iter()
            .map(|event| event.end_time)
            .max()
            .unwrap_or(first_seen);
        let context = summarize_page_context(
            &project_events,
            &format!("Recent activity for project {}.", project),
        );
        let page = KnowledgePage {
            page_id: format!("kp:project:{}", normalize_alias_key(&project)),
            page_type: KnowledgePageType::ProjectPage,
            title: compact_claim_title("Project Focus", &project),
            page_context: context,
            canonical_entities: gather_page_entities(&project_events),
            supporting_memory_ids: collect_supporting_memory_ids(&project_events),
            supporting_evidence_ids: collect_supporting_evidence_ids(&project_events),
            related_page_ids: Vec::new(),
            confidence_score: average_event_confidence(&project_events),
            stability: page_stability_from_support(project_events.len()),
            first_seen,
            last_updated,
            project: Some(project),
            topic: None,
            workflow: None,
        };
        pages.push(page);
    }

    let mut by_decision: HashMap<String, Vec<ActivityEvent>> = HashMap::new();
    for event in &events {
        for decision in &event.decisions {
            let key = normalize_alias_key(decision);
            if key.is_empty() {
                continue;
            }
            by_decision.entry(key).or_default().push(event.clone());
        }
    }
    for (decision_key, decision_events) in by_decision {
        let sample = decision_events
            .first()
            .and_then(|event| event.decisions.first())
            .cloned()
            .unwrap_or_else(|| decision_key.clone());
        let first_seen = decision_events
            .iter()
            .map(|event| event.start_time)
            .min()
            .unwrap_or_default();
        let last_updated = decision_events
            .iter()
            .map(|event| event.end_time)
            .max()
            .unwrap_or(first_seen);
        pages.push(KnowledgePage {
            page_id: format!("kp:decision:{}", decision_key),
            page_type: KnowledgePageType::DecisionPage,
            title: compact_claim_title("Decision", &sample),
            page_context: summarize_page_context(&decision_events, &sample),
            canonical_entities: gather_page_entities(&decision_events),
            supporting_memory_ids: collect_supporting_memory_ids(&decision_events),
            supporting_evidence_ids: collect_supporting_evidence_ids(&decision_events),
            related_page_ids: Vec::new(),
            confidence_score: average_event_confidence(&decision_events),
            stability: page_stability_from_support(decision_events.len()),
            first_seen,
            last_updated,
            project: decision_events
                .first()
                .and_then(|event| event.project.clone()),
            topic: None,
            workflow: None,
        });
    }

    let mut by_breakthrough: HashMap<String, Vec<ActivityEvent>> = HashMap::new();
    for event in &events {
        let summary_lower = event.summary.to_ascii_lowercase();
        let looks_breakthrough = summary_lower.contains("fixed")
            || summary_lower.contains("resolved")
            || summary_lower.contains("solved")
            || summary_lower.contains("working")
            || event.outcome.eq_ignore_ascii_case("succeeded");
        if !looks_breakthrough {
            continue;
        }
        let key = normalize_alias_key(&event.title);
        if key.is_empty() {
            continue;
        }
        by_breakthrough.entry(key).or_default().push(event.clone());
    }
    for (breakthrough_key, breakthrough_events) in by_breakthrough {
        let sample = breakthrough_events
            .first()
            .map(|event| event.title.clone())
            .unwrap_or_else(|| breakthrough_key.clone());
        let first_seen = breakthrough_events
            .iter()
            .map(|event| event.start_time)
            .min()
            .unwrap_or_default();
        let last_updated = breakthrough_events
            .iter()
            .map(|event| event.end_time)
            .max()
            .unwrap_or(first_seen);
        pages.push(KnowledgePage {
            page_id: format!("kp:breakthrough:{}", breakthrough_key),
            page_type: KnowledgePageType::BreakthroughPage,
            title: compact_claim_title("Breakthrough", &sample),
            page_context: summarize_page_context(&breakthrough_events, &sample),
            canonical_entities: gather_page_entities(&breakthrough_events),
            supporting_memory_ids: collect_supporting_memory_ids(&breakthrough_events),
            supporting_evidence_ids: collect_supporting_evidence_ids(&breakthrough_events),
            related_page_ids: Vec::new(),
            confidence_score: average_event_confidence(&breakthrough_events),
            stability: page_stability_from_support(breakthrough_events.len()),
            first_seen,
            last_updated,
            project: breakthrough_events
                .first()
                .and_then(|event| event.project.clone()),
            topic: None,
            workflow: None,
        });
    }

    let mut by_topic: HashMap<String, Vec<ActivityEvent>> = HashMap::new();
    for event in &events {
        for tag in &event.tags {
            let key = normalize_alias_key(tag);
            if key.is_empty() {
                continue;
            }
            by_topic.entry(key).or_default().push(event.clone());
        }
    }
    for (topic_key, topic_events) in by_topic {
        if topic_events.len() < 2 {
            continue;
        }
        let sample = topic_events
            .first()
            .and_then(|event| event.tags.first())
            .cloned()
            .unwrap_or_else(|| topic_key.clone());
        let first_seen = topic_events
            .iter()
            .map(|event| event.start_time)
            .min()
            .unwrap_or_default();
        let last_updated = topic_events
            .iter()
            .map(|event| event.end_time)
            .max()
            .unwrap_or(first_seen);
        pages.push(KnowledgePage {
            page_id: format!("kp:topic:{}", topic_key),
            page_type: KnowledgePageType::TopicPage,
            title: compact_claim_title("Topic Pattern", &sample),
            page_context: summarize_page_context(&topic_events, &sample),
            canonical_entities: gather_page_entities(&topic_events),
            supporting_memory_ids: collect_supporting_memory_ids(&topic_events),
            supporting_evidence_ids: collect_supporting_evidence_ids(&topic_events),
            related_page_ids: Vec::new(),
            confidence_score: average_event_confidence(&topic_events),
            stability: page_stability_from_support(topic_events.len()),
            first_seen,
            last_updated,
            project: topic_events.first().and_then(|event| event.project.clone()),
            topic: Some(sample),
            workflow: None,
        });
    }

    let mut claim_groups: HashMap<String, Vec<ActivityEvent>> = HashMap::new();
    for event in &events {
        let claim = event
            .summary
            .split('.')
            .next()
            .unwrap_or_default()
            .trim()
            .to_string();
        let key = normalize_alias_key(&claim);
        if key.split('_').count() < 4 {
            continue;
        }
        claim_groups.entry(key).or_default().push(event.clone());
    }
    for (claim_key, claim_events) in claim_groups {
        if claim_events.len() < 2 {
            continue;
        }
        let sample = claim_events
            .first()
            .map(|event| event.summary.clone())
            .unwrap_or_else(|| claim_key.clone());
        let first_seen = claim_events
            .iter()
            .map(|event| event.start_time)
            .min()
            .unwrap_or_default();
        let last_updated = claim_events
            .iter()
            .map(|event| event.end_time)
            .max()
            .unwrap_or(first_seen);
        pages.push(KnowledgePage {
            page_id: format!("kp:claim:{}", claim_key),
            page_type: KnowledgePageType::ClaimPage,
            title: compact_claim_title("Claim", &sample),
            page_context: summarize_page_context(&claim_events, &sample),
            canonical_entities: gather_page_entities(&claim_events),
            supporting_memory_ids: collect_supporting_memory_ids(&claim_events),
            supporting_evidence_ids: collect_supporting_evidence_ids(&claim_events),
            related_page_ids: Vec::new(),
            confidence_score: average_event_confidence(&claim_events),
            stability: page_stability_from_support(claim_events.len()),
            first_seen,
            last_updated,
            project: claim_events.first().and_then(|event| event.project.clone()),
            topic: None,
            workflow: None,
        });
    }

    // Lightweight contradiction detection: opposite recommendation language for same project.
    let mut contradiction_pages: Vec<KnowledgePage> = Vec::new();
    let decisions = pages
        .iter()
        .filter(|page| page.page_type == KnowledgePageType::DecisionPage)
        .cloned()
        .collect::<Vec<_>>();
    for left in &decisions {
        for right in &decisions {
            if left.page_id >= right.page_id {
                continue;
            }
            if left.project != right.project {
                continue;
            }
            let l = left.title.to_ascii_lowercase();
            let r = right.title.to_ascii_lowercase();
            let opposite = (l.contains("avoid") && !r.contains("avoid"))
                || (r.contains("avoid") && !l.contains("avoid"))
                || (l.contains("not ") && !r.contains("not "))
                || (r.contains("not ") && !l.contains("not "));
            if !opposite {
                continue;
            }
            contradiction_pages.push(KnowledgePage {
                page_id: format!(
                    "kp:contradiction:{}:{}",
                    normalize_alias_key(&left.page_id),
                    normalize_alias_key(&right.page_id)
                ),
                page_type: KnowledgePageType::ContradictionPage,
                title: compact_claim_title(
                    "Contradiction",
                    &format!("{} vs {}", left.title, right.title),
                ),
                page_context: format!(
                    "Newer evidence suggests a contradiction between '{}' and '{}'.",
                    left.title, right.title
                ),
                canonical_entities: dedupe_strings(
                    left.canonical_entities
                        .iter()
                        .cloned()
                        .chain(right.canonical_entities.iter().cloned())
                        .collect(),
                ),
                supporting_memory_ids: dedupe_strings(
                    left.supporting_memory_ids
                        .iter()
                        .cloned()
                        .chain(right.supporting_memory_ids.iter().cloned())
                        .collect(),
                ),
                supporting_evidence_ids: dedupe_strings(
                    left.supporting_evidence_ids
                        .iter()
                        .cloned()
                        .chain(right.supporting_evidence_ids.iter().cloned())
                        .collect(),
                ),
                related_page_ids: vec![left.page_id.clone(), right.page_id.clone()],
                confidence_score: ((left.confidence_score + right.confidence_score) / 2.0)
                    .clamp(0.0, 1.0),
                stability: KnowledgeStability::Contradicted,
                first_seen: left.first_seen.min(right.first_seen),
                last_updated: left.last_updated.max(right.last_updated),
                project: left.project.clone(),
                topic: None,
                workflow: None,
            });
        }
    }
    pages.extend(contradiction_pages);

    // Link related pages by entity overlap.
    let snapshot = pages.clone();
    let mut by_id: HashMap<String, Vec<String>> = HashMap::new();
    for left in &snapshot {
        let left_entities = left
            .canonical_entities
            .iter()
            .map(|entity| normalize_alias_key(entity))
            .collect::<HashSet<_>>();
        for right in &snapshot {
            if left.page_id == right.page_id {
                continue;
            }
            if left.project != right.project && left.topic != right.topic {
                continue;
            }
            let overlap = right
                .canonical_entities
                .iter()
                .map(|entity| normalize_alias_key(entity))
                .filter(|entity| left_entities.contains(entity))
                .count();
            if overlap > 0 {
                by_id
                    .entry(left.page_id.clone())
                    .or_default()
                    .push(right.page_id.clone());
            }
        }
    }
    for page in &mut pages {
        page.related_page_ids = dedupe_strings(by_id.remove(&page.page_id).unwrap_or_default());
    }

    pages.sort_by(|left, right| {
        right
            .last_updated
            .cmp(&left.last_updated)
            .then_with(|| left.page_id.cmp(&right.page_id))
    });
    pages.truncate(240);
    Ok(pages)
}

async fn collect_open_tasks(
    state: &AppState,
    project: Option<&str>,
) -> Result<Vec<ContextTask>, String> {
    let tasks = state.store.list_tasks().await.map_err(|e| e.to_string())?;
    let mut open = Vec::new();
    for task in tasks
        .into_iter()
        .filter(|task| !task.is_completed && !task.is_dismissed)
    {
        if let Some(project) = project {
            let mut matches_project = false;
            if let Some(source_memory_id) = task.source_memory_id.as_deref() {
                if let Some(event) = state
                    .store
                    .get_activity_event_by_memory_id(source_memory_id)
                    .await
                    .map_err(|e| e.to_string())?
                {
                    matches_project = event.project.as_deref() == Some(project);
                }
            }
            if !matches_project && !task.linked_memory_ids.is_empty() {
                for memory_id in &task.linked_memory_ids {
                    if let Some(event) = state
                        .store
                        .get_activity_event_by_memory_id(memory_id)
                        .await
                        .map_err(|e| e.to_string())?
                    {
                        if event.project.as_deref() == Some(project) {
                            matches_project = true;
                            break;
                        }
                    }
                }
            }
            if !matches_project {
                continue;
            }
        }

        open.push(ContextTask {
            id: task.id,
            title: task.title,
            status: match task.task_type {
                crate::storage::TaskType::Todo => "todo".to_string(),
                crate::storage::TaskType::Reminder => "reminder".to_string(),
                crate::storage::TaskType::Followup => "followup".to_string(),
            },
            source: task.source_app,
            due_at: task.due_date,
        });
    }
    open.sort_by(|left, right| left.title.cmp(&right.title));
    if open.len() > 8 {
        open.truncate(8);
    }
    Ok(open)
}

async fn extract_entities(
    state: &AppState,
    record: &MemoryRecord,
    project: Option<&str>,
) -> Result<Vec<EntityRef>, String> {
    let mut entities = Vec::new();
    let mut seen = HashSet::new();

    if let Some(project_name) = project {
        push_entity(
            state,
            &mut entities,
            &mut seen,
            "project",
            project_name,
            vec![project_name.to_string()],
            project,
        )
        .await?;
    }

    for file in record
        .files_touched
        .iter()
        .filter(|value| !value.trim().is_empty())
    {
        push_entity(
            state,
            &mut entities,
            &mut seen,
            "file",
            file,
            vec![file.clone(), basename(file)],
            project,
        )
        .await?;
    }

    for capture in FILE_RE
        .find_iter(&entity_source_text(record))
        .map(|m| m.as_str().to_string())
    {
        push_entity(
            state,
            &mut entities,
            &mut seen,
            "file",
            &capture,
            vec![capture.clone(), basename(&capture)],
            project,
        )
        .await?;
    }

    for capture in URL_RE
        .find_iter(&entity_source_text(record))
        .map(|m| m.as_str().to_string())
    {
        push_entity(
            state,
            &mut entities,
            &mut seen,
            "url",
            &capture,
            vec![capture.clone()],
            project,
        )
        .await?;
    }

    for issue in ISSUE_RE
        .captures_iter(&entity_source_text(record))
        .filter_map(|capture| capture.name("num").map(|m| format!("#{}", m.as_str())))
    {
        push_entity(
            state,
            &mut entities,
            &mut seen,
            "issue",
            &issue,
            vec![issue.clone()],
            project,
        )
        .await?;
    }

    for command in extract_commands(record) {
        push_entity(
            state,
            &mut entities,
            &mut seen,
            "command",
            &command,
            vec![command.clone()],
            project,
        )
        .await?;
    }

    for error in record.errors.iter().cloned().chain(
        ERROR_RE
            .find_iter(&entity_source_text(record))
            .map(|m| m.as_str().trim().to_string()),
    ) {
        if error.trim().is_empty() {
            continue;
        }
        push_entity(
            state,
            &mut entities,
            &mut seen,
            "error",
            &error,
            vec![error.clone()],
            project,
        )
        .await?;
    }

    for decision in record
        .decisions
        .iter()
        .filter(|value| !value.trim().is_empty())
    {
        push_entity(
            state,
            &mut entities,
            &mut seen,
            "decision",
            decision,
            vec![decision.clone()],
            project,
        )
        .await?;
    }

    for tag in record.tags.iter().filter(|value| !value.trim().is_empty()) {
        push_entity(
            state,
            &mut entities,
            &mut seen,
            "concept",
            tag,
            vec![tag.clone()],
            project,
        )
        .await?;
    }

    Ok(entities)
}

async fn push_entity(
    state: &AppState,
    entities: &mut Vec<EntityRef>,
    seen: &mut HashSet<String>,
    entity_type: &str,
    candidate: &str,
    aliases: Vec<String>,
    project: Option<&str>,
) -> Result<(), String> {
    let trimmed = candidate.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    let alias_key = format!("{}:{}", entity_type, normalize_alias_key(trimmed));
    if !seen.insert(alias_key.clone()) {
        return Ok(());
    }

    if let Some(alias) = state
        .store
        .resolve_entity_alias(&alias_key, project)
        .await
        .map_err(|e| e.to_string())?
    {
        entities.push(EntityRef {
            canonical_id: alias.canonical_id,
            canonical_name: alias.canonical_name,
            entity_type: alias.entity_type,
            confidence: alias.confidence,
            aliases,
        });
        return Ok(());
    }

    entities.push(EntityRef {
        canonical_id: canonical_entity_id(entity_type, trimmed),
        canonical_name: trimmed.to_string(),
        entity_type: entity_type.to_string(),
        confidence: 0.82,
        aliases,
    });
    Ok(())
}

async fn upsert_runtime_graph(
    state: &AppState,
    record: &MemoryRecord,
    event: &ActivityEvent,
) -> Result<(), Box<dyn std::error::Error>> {
    let now = chrono::Utc::now().timestamp_millis();
    let event_node_id = format!("activity:{}", event.id);
    let mut nodes = vec![GraphNode {
        id: event_node_id.clone(),
        node_type: NodeType::ActivityEvent,
        label: event.title.clone(),
        created_at: event.start_time,
        metadata: json!({
            "project": event.project,
            "memory_id": event.memory_id,
            "activity_type": event.activity_type,
            "summary": event.summary,
            "outcome": event.outcome,
        }),
    }];
    let mut edges = vec![GraphEdge {
        id: format!("edge:{}:memory", event.id),
        source: event_node_id.clone(),
        target: format!("memory:{}", record.id),
        edge_type: EdgeType::ResultedIn,
        timestamp: now,
        metadata: json!({"reason": "source_memory"}),
    }];

    if let Some(project) = event.project.as_deref() {
        let project_node_id = format!("project:{}", normalize_alias_key(project));
        nodes.push(GraphNode {
            id: project_node_id.clone(),
            node_type: NodeType::Project,
            label: project.to_string(),
            created_at: event.start_time,
            metadata: json!({"project": project}),
        });
        edges.push(GraphEdge {
            id: format!("edge:{}:project", event.id),
            source: event_node_id.clone(),
            target: project_node_id,
            edge_type: EdgeType::BelongsTo,
            timestamp: now,
            metadata: json!({}),
        });
    }

    for entity in &event.entities {
        let generic_entity = matches!(
            normalize_alias_key(&entity.canonical_name).as_str(),
            "ui" | "window" | "chrome" | "browser" | "tab" | "page"
        );
        if generic_entity && entity.confidence < 0.70 {
            continue;
        }
        let node_type = match entity.entity_type.as_str() {
            "file" => NodeType::File,
            "url" => NodeType::Url,
            "error" => NodeType::Error,
            "command" => NodeType::Command,
            "decision" => NodeType::Decision,
            "issue" => NodeType::Issue,
            "concept" => NodeType::Concept,
            "project" => NodeType::Project,
            _ => NodeType::Entity,
        };
        nodes.push(GraphNode {
            id: entity.canonical_id.clone(),
            node_type,
            label: entity.canonical_name.clone(),
            created_at: event.start_time,
            metadata: json!({
                "entity_type": entity.entity_type,
                "aliases": entity.aliases,
                "confidence": entity.confidence,
                "evidence_backed": !generic_entity,
                "uncertain": generic_entity,
            }),
        });
        let edge_type = match entity.entity_type.as_str() {
            "file" => EdgeType::EditedFile,
            "error" => EdgeType::BlockedBy,
            "command" => EdgeType::InformedBy,
            "decision" => EdgeType::ResultedIn,
            _ => EdgeType::MentionedIn,
        };
        edges.push(GraphEdge {
            id: format!(
                "edge:{}:{}",
                event.id,
                normalize_alias_key(&entity.canonical_id)
            ),
            source: event_node_id.clone(),
            target: entity.canonical_id.clone(),
            edge_type,
            timestamp: now,
            metadata: json!({
                "entity_type": entity.entity_type,
                "evidence_backed": !generic_entity,
                "uncertain": generic_entity
            }),
        });
    }

    for evidence in &event.evidence {
        let evidence_node_id = if evidence.id.trim().is_empty() {
            format!(
                "evidence:{}:{}",
                event.id,
                normalize_alias_key(&evidence.source_id)
            )
        } else {
            evidence.id.clone()
        };
        nodes.push(GraphNode {
            id: evidence_node_id.clone(),
            node_type: NodeType::Entity,
            label: if !evidence.summary.trim().is_empty() {
                trim_chars(&evidence.summary, 120)
            } else {
                trim_chars(&evidence.snippet, 120)
            },
            created_at: evidence.timestamp,
            metadata: json!({
                "entity_type": "evidence",
                "source_type": evidence.source_type,
                "source_id": evidence.source_id,
                "privacy_class": evidence.privacy_class,
                "evidence_backed": true,
            }),
        });
        edges.push(GraphEdge {
            id: format!(
                "edge:{}:evidence:{}",
                event.id,
                normalize_alias_key(&evidence_node_id)
            ),
            source: event_node_id.clone(),
            target: evidence_node_id,
            edge_type: EdgeType::InformedBy,
            timestamp: now,
            metadata: json!({"entity_type": "evidence", "evidence_backed": true}),
        });
    }

    for blocker in &event.errors {
        if blocker.trim().is_empty() {
            continue;
        }
        let blocker_id = format!("blocker:{}", normalize_alias_key(blocker));
        nodes.push(GraphNode {
            id: blocker_id.clone(),
            node_type: NodeType::Error,
            label: trim_chars(blocker, 120),
            created_at: event.start_time,
            metadata: json!({"entity_type": "blocker", "evidence_backed": true}),
        });
        edges.push(GraphEdge {
            id: format!("edge:{}:blocker:{}", event.id, normalize_alias_key(blocker)),
            source: event_node_id.clone(),
            target: blocker_id,
            edge_type: EdgeType::BlockedBy,
            timestamp: now,
            metadata: json!({"entity_type": "blocker", "evidence_backed": true}),
        });
    }

    for todo in &event.next_steps {
        if todo.trim().is_empty() {
            continue;
        }
        let todo_id = format!("todo:{}", normalize_alias_key(todo));
        nodes.push(GraphNode {
            id: todo_id.clone(),
            node_type: NodeType::Task,
            label: trim_chars(todo, 120),
            created_at: event.start_time,
            metadata: json!({"entity_type": "todo", "evidence_backed": true}),
        });
        edges.push(GraphEdge {
            id: format!("edge:{}:todo:{}", event.id, normalize_alias_key(todo)),
            source: event_node_id.clone(),
            target: todo_id,
            edge_type: EdgeType::ResultedIn,
            timestamp: now,
            metadata: json!({"entity_type": "todo", "evidence_backed": true}),
        });
    }

    state.store.upsert_nodes(&nodes).await?;
    state.store.upsert_edges(&edges).await?;
    Ok(())
}

fn build_alias_records(event: &ActivityEvent) -> Vec<EntityAliasRecord> {
    let mut aliases = Vec::new();
    for entity in &event.entities {
        let mut seen = HashSet::new();
        for alias in entity
            .aliases
            .iter()
            .cloned()
            .chain(std::iter::once(entity.canonical_name.clone()))
        {
            let key = format!("{}:{}", entity.entity_type, normalize_alias_key(&alias));
            if !seen.insert(key.clone()) {
                continue;
            }
            aliases.push(EntityAliasRecord {
                alias_key: key,
                canonical_id: entity.canonical_id.clone(),
                canonical_name: entity.canonical_name.clone(),
                entity_type: entity.entity_type.clone(),
                project: event.project.clone(),
                confidence: entity.confidence,
                updated_at: chrono::Utc::now().timestamp_millis(),
            });
        }
    }
    aliases
}

fn detect_project_for_record(record: &MemoryRecord) -> Option<String> {
    if !record.project.trim().is_empty() {
        return Some(record.project.trim().to_string());
    }

    if let Some(project) = infer_project_from_files(&record.files_touched) {
        return Some(project);
    }

    if let Some(project) = FILE_RE
        .find_iter(&entity_source_text(record))
        .find_map(|m| infer_project_from_path(m.as_str()))
    {
        return Some(project);
    }

    if let Some(project) = infer_project_from_window(record) {
        return Some(project);
    }

    infer_project_from_url(record.url.as_deref())
}

fn infer_project_from_files(files: &[String]) -> Option<String> {
    for file in files {
        if let Some(project) = infer_project_from_path(file) {
            return Some(project);
        }
    }
    None
}

fn detect_repo_for_record(record: &MemoryRecord) -> Option<String> {
    let explicit_project = record.project.trim();
    if !explicit_project.is_empty() {
        return Some(normalize_alias_key(explicit_project));
    }

    infer_repo_slug_from_files(&record.files_touched)
        .or_else(|| {
            FILE_RE
                .find_iter(&entity_source_text(record))
                .find_map(|m| infer_repo_slug_from_path(m.as_str()))
        })
        .or_else(|| infer_repo_slug_from_window(record))
        .or_else(|| infer_repo_slug_from_url(record.url.as_deref()))
}

fn infer_project_from_path(path: &str) -> Option<String> {
    infer_repo_slug_from_path(path).and_then(|slug| normalize_project_name(&slug))
}

fn infer_repo_slug_from_files(files: &[String]) -> Option<String> {
    files
        .iter()
        .find_map(|file| infer_repo_slug_from_path(file))
}

fn infer_repo_slug_from_path(path: &str) -> Option<String> {
    let segments = path_segments(path);
    if segments.len() < 2 {
        return None;
    }

    for (index, segment) in segments.iter().enumerate() {
        if PROJECT_PATH_MARKERS.contains(&segment.as_str()) && index >= 1 {
            let candidate = &segments[index - 1];
            if is_meaningful_project_segment(candidate) {
                return Some(candidate.trim_end_matches(".git").to_string());
            }
        }
    }

    None
}

fn infer_project_from_window(record: &MemoryRecord) -> Option<String> {
    FILE_RE
        .find_iter(&record.window_title)
        .find_map(|m| infer_project_from_path(m.as_str()))
}

fn infer_repo_slug_from_window(record: &MemoryRecord) -> Option<String> {
    FILE_RE
        .find_iter(&record.window_title)
        .find_map(|m| infer_repo_slug_from_path(m.as_str()))
}

fn infer_project_from_url(url: Option<&str>) -> Option<String> {
    infer_repo_slug_from_url(url).and_then(|slug| normalize_project_name(&slug))
}

fn infer_repo_slug_from_url(url: Option<&str>) -> Option<String> {
    let raw = url?.trim();
    if raw.is_empty() {
        return None;
    }

    let without_scheme = raw
        .split("://")
        .nth(1)
        .unwrap_or(raw)
        .trim_start_matches('/');
    let mut parts = without_scheme.split('/');
    parts.next()?;
    let path_segments = parts
        .filter_map(|segment| {
            let trimmed = segment.trim();
            (!trimmed.is_empty()).then(|| trimmed.trim_end_matches(".git").to_string())
        })
        .collect::<Vec<_>>();

    for marker in [
        "project",
        "projects",
        "repo",
        "repos",
        "repository",
        "repositories",
    ] {
        if let Some(index) = path_segments
            .iter()
            .position(|segment| segment.eq_ignore_ascii_case(marker))
        {
            if let Some(candidate) = path_segments.get(index + 1) {
                if is_meaningful_project_segment(candidate) {
                    return Some(candidate.clone());
                }
            }
        }
    }

    // Generic `owner/repo/<resource>/...` paths (GitHub, GitLab, Gitea, etc.) — no host allowlist.
    const REPO_CHILD_SEGMENTS: &[&str] = &[
        "pull",
        "pulls",
        "issues",
        "merge_requests",
        "mr",
        "commit",
        "commits",
        "tree",
        "blob",
        "compare",
        "actions",
        "wiki",
        "security",
        "releases",
        "tags",
        "branches",
        "graphs",
        "pulse",
        "network",
        "stargazers",
        "forks",
        "watchers",
    ];
    if path_segments.len() >= 3 {
        let third = path_segments[2].to_lowercase();
        if REPO_CHILD_SEGMENTS
            .iter()
            .copied()
            .any(|marker| marker == third.as_str())
        {
            if let Some(candidate) = path_segments.get(1) {
                if is_meaningful_project_segment(candidate) {
                    return Some(candidate.clone());
                }
            }
        }
    }

    None
}

fn path_segments(path: &str) -> Vec<String> {
    path.replace('\\', "/")
        .split('/')
        .filter_map(|segment| {
            let trimmed = segment.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
        .collect()
}

fn is_meaningful_project_segment(segment: &str) -> bool {
    let trimmed = segment
        .trim()
        .trim_matches(|ch: char| !ch.is_alphanumeric() && ch != '-' && ch != '_');
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_lowercase();
    if GENERIC_PATH_SEGMENTS.contains(&lower.as_str())
        || PROJECT_PATH_MARKERS.contains(&lower.as_str())
    {
        return false;
    }
    !trimmed.contains('.')
}

fn normalize_project_name(value: &str) -> Option<String> {
    let trimmed = value
        .trim()
        .trim_end_matches(".git")
        .trim_matches(|ch: char| !ch.is_alphanumeric() && ch != '-' && ch != '_');
    if !is_meaningful_project_segment(trimmed) {
        return None;
    }

    let parts = trimmed
        .split(['-', '_'])
        .filter(|part| !part.trim().is_empty())
        .map(|part| part.trim())
        .collect::<Vec<_>>();
    if parts.is_empty() {
        return None;
    }

    if parts.len() == 1 {
        let part = parts[0];
        if part.len() <= 5 && part.chars().all(|ch| ch.is_ascii_alphanumeric()) {
            return Some(part.to_uppercase());
        }
        return Some(capitalize_token(part));
    }

    Some(
        parts
            .into_iter()
            .map(capitalize_token)
            .collect::<Vec<_>>()
            .join(" "),
    )
}

fn detect_branch_for_record(record: &MemoryRecord) -> Option<String> {
    let lower = record.window_title.to_lowercase();
    for marker in ["branch:", " on "] {
        if let Some(index) = lower.find(marker) {
            let rest = &record.window_title[index + marker.len()..];
            let branch = rest
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .trim_matches(|ch: char| {
                    !ch.is_alphanumeric() && ch != '/' && ch != '-' && ch != '_'
                });
            if !branch.is_empty() {
                return Some(branch.to_string());
            }
        }
    }
    None
}

fn infer_activity_type(record: &MemoryRecord) -> String {
    if !record.activity_type.trim().is_empty() {
        return record.activity_type.to_lowercase();
    }

    // All branches below derive labels from already-extracted structured
    // fields on `MemoryRecord`. No app-name or URL-host allowlists.
    let has_errors = !record.errors.is_empty();
    let has_commands = !record.commands.is_empty();
    let has_decisions_count = record.decisions.len();
    let has_next_steps_count = record.next_steps.len();
    let has_files_touched_count = record.files_touched.len();
    let memory_context_chars = record.memory_context.chars().count();

    if has_errors && has_commands {
        return "debugging".to_string();
    }
    if has_decisions_count >= 1 && has_next_steps_count >= 2 {
        return "implementation planning".to_string();
    }
    if has_files_touched_count >= 2 && has_decisions_count == 0 {
        return "refactor review".to_string();
    }
    if record
        .decisions
        .iter()
        .any(|d| decision_verb_stem_design(d))
    {
        return "system design".to_string();
    }
    if has_next_steps_count >= 1 && record.url.is_some() {
        return "architecture analysis".to_string();
    }
    if memory_context_chars > 800 && !has_errors && !has_commands {
        return "instruction writing".to_string();
    }

    // Lightweight content-derived fallback when no strong structural signal
    // is present. Uses only generic morphological cues, never app names.
    let text = format!(
        "{} {} {}",
        record.clean_text.to_ascii_lowercase(),
        record.internal_context.to_ascii_lowercase(),
        record.memory_context.to_ascii_lowercase()
    );
    let mut scores: Vec<(&str, f32)> = vec![("unknown", 0.05)];
    if has_files_touched_count > 0 {
        scores.push(("coding", 0.32));
    }
    if has_errors || text.contains("error") || text.contains("failed") {
        scores.push(("debugging", 0.40));
    }
    if has_decisions_count > 0 || text.contains("decision") || text.contains("tradeoff") {
        scores.push(("planning", 0.30));
    }
    if record.url.is_some() {
        scores.push(("researching", 0.20));
    }
    if has_next_steps_count > 0 || text.contains("todo") || text.contains("next step") {
        scores.push(("organizing_information", 0.24));
    }
    if text.contains("test") || text.contains("assert") || text.contains("validation") {
        scores.push(("testing_workflow", 0.26));
    }
    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scores
        .first()
        .map(|(label, _)| (*label).to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

/// True when a decision string starts with a generic design/proposal verb.
/// Stems-only — independent of any product or library naming.
fn decision_verb_stem_design(decision: &str) -> bool {
    let lower = decision.trim().to_ascii_lowercase();
    let first = lower.split_whitespace().next().unwrap_or("");
    matches!(
        first,
        "design"
            | "designed"
            | "propose"
            | "proposed"
            | "plan"
            | "planned"
            | "architect"
            | "architected"
    )
}

fn build_event_title(record: &MemoryRecord) -> String {
    let compressed = crate::graph::compress_node_label(record);
    if !compressed.trim().is_empty() {
        return trim_chars(&compressed, 96);
    }
    if !record.display_summary.trim().is_empty() {
        return trim_chars(&record.display_summary, 96);
    }
    if !record.snippet.trim().is_empty() {
        return trim_chars(&record.snippet, 96);
    }
    trim_chars(&record.window_title, 96)
}

fn normalize_outcome(outcome: &str) -> String {
    let lowered = outcome.trim().to_lowercase();
    match lowered.as_str() {
        "succeeded" | "failed" | "abandoned" | "unresolved" | "unknown" => lowered,
        "" => "unknown".to_string(),
        other => other.to_string(),
    }
}

fn classify_source_type(record: &MemoryRecord, source_hint: Option<&str>) -> String {
    if let Some(source_hint) = source_hint.filter(|value| !value.trim().is_empty()) {
        return source_hint.to_string();
    }
    if record.session_key.starts_with("meeting:") {
        "audio".to_string()
    } else if record.app_name.eq_ignore_ascii_case("finder") {
        "file".to_string()
    } else if record.url.is_some() {
        "browser".to_string()
    } else if record.app_name.to_lowercase().contains("terminal") {
        "terminal".to_string()
    } else {
        "screen".to_string()
    }
}

fn classify_privacy(record: &MemoryRecord) -> PrivacyClass {
    let lower = format!(
        "{} {} {}",
        record.app_name.to_lowercase(),
        record.window_title.to_lowercase(),
        record.clean_text.to_lowercase()
    );
    if lower.contains("password") || lower.contains("secret") || lower.contains("token") {
        PrivacyClass::Secret
    } else if lower.contains("bank") || lower.contains("finance") || lower.contains("health") {
        PrivacyClass::Sensitive
    } else if lower.contains("message") || lower.contains("mail") {
        PrivacyClass::Personal
    } else {
        PrivacyClass::Project
    }
}

fn event_confidence(record: &MemoryRecord) -> f32 {
    record
        .extraction_confidence
        .max(record.ocr_confidence)
        .max(0.45)
        .min(0.99)
}

fn estimate_memory_value(record: &MemoryRecord) -> f32 {
    let mut value: f32 = 0.45;
    if !record.files_touched.is_empty() {
        value += 0.18;
    }
    if !record.errors.is_empty() {
        value += 0.16;
    }
    if !record.decisions.is_empty() {
        value += 0.12;
    }
    if !record.project.trim().is_empty() {
        value += 0.08;
    }
    value.min(0.95)
}

fn entity_source_text(record: &MemoryRecord) -> String {
    format!(
        "{}\n{}\n{}\n{}\n{}",
        record.window_title,
        record.clean_text,
        record.internal_context,
        record.errors.join("\n"),
        record.decisions.join("\n")
    )
}

fn extract_commands(record: &MemoryRecord) -> Vec<String> {
    let mut commands = COMMAND_RE
        .find_iter(&entity_source_text(record))
        .map(|m| trim_chars(m.as_str().trim(), 120))
        .collect::<Vec<_>>();
    if commands.is_empty() && record.app_name.to_lowercase().contains("terminal") {
        let snippet = trim_chars(&record.snippet, 120);
        if !snippet.is_empty() {
            commands.push(snippet);
        }
    }
    dedupe_strings(commands)
}

fn memory_to_evidence(record: &MemoryRecord, source_type: String) -> EvidenceRef {
    EvidenceRef {
        id: format!("evidence:{}", record.id),
        source_type,
        source_id: record.id.clone(),
        summary: trim_chars(
            if !record.memory_context.trim().is_empty() {
                &record.memory_context
            } else if !record.display_summary.trim().is_empty() {
                &record.display_summary
            } else {
                &record.snippet
            },
            120,
        ),
        snippet: trim_chars(&record.clean_text, 220),
        timestamp: record.timestamp,
        privacy_class: classify_privacy(record),
    }
}

fn collect_relevant_files(
    active_files: &[String],
    events: &[ActivityEvent],
    project_context: Option<&ProjectContext>,
) -> Vec<RelevantFile> {
    let mut files = Vec::new();
    let mut seen = HashSet::new();
    for path in active_files.iter().filter(|value| !value.trim().is_empty()) {
        if seen.insert(path.clone()) {
            files.push(RelevantFile {
                path: path.clone(),
                why: "Active file matched the current request.".to_string(),
            });
        }
    }

    let mut frequency: HashMap<String, usize> = HashMap::new();
    for event in events {
        for path in &event.active_files {
            *frequency.entry(path.clone()).or_insert(0) += 1;
        }
    }
    let mut ranked = frequency.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    for (path, count) in ranked {
        if seen.insert(path.clone()) {
            files.push(RelevantFile {
                path,
                why: format!("Touched in {} recent activity event(s).", count),
            });
        }
    }

    if let Some(project_context) = project_context {
        for file in &project_context.relevant_files {
            if seen.insert(file.path.clone()) {
                files.push(file.clone());
            }
        }
    }

    if files.len() > 8 {
        files.truncate(8);
    }
    files
}

fn collect_recent_decisions(
    events: &[ActivityEvent],
    project_context: Option<&ProjectContext>,
) -> Vec<DecisionSummary> {
    let mut decisions = Vec::new();
    let mut seen = HashSet::new();
    for event in events {
        for decision in &event.decisions {
            let key = normalize_alias_key(decision);
            if seen.insert(key.clone()) {
                decisions.push(DecisionSummary {
                    id: format!("decision:{}", key),
                    title: trim_chars(decision, 96),
                    summary: trim_chars(&event.summary, 140),
                    timestamp: event.end_time,
                    evidence: event.evidence.clone(),
                });
            }
        }
    }
    if let Some(project_context) = project_context {
        for decision in &project_context.recent_decisions {
            if seen.insert(decision.id.clone()) {
                decisions.push(decision.clone());
            }
        }
    }
    decisions.sort_by_key(|decision| std::cmp::Reverse(decision.timestamp));
    if decisions.len() > 6 {
        decisions.truncate(6);
    }
    decisions
}

fn collect_failures(
    events: &[ActivityEvent],
    project_context: Option<&ProjectContext>,
) -> Vec<FailureSummary> {
    let mut failures = Vec::new();
    let mut seen = HashSet::new();
    for event in events {
        for error in &event.errors {
            let key = normalize_alias_key(error);
            if seen.insert(key.clone()) {
                failures.push(FailureSummary {
                    id: format!("failure:{}", key),
                    title: trim_chars(error, 96),
                    summary: trim_chars(&event.summary, 140),
                    error: trim_chars(error, 140),
                    related_files: event.active_files.clone(),
                    last_seen_at: event.end_time,
                    evidence: event.evidence.clone(),
                });
            }
        }
        if event.outcome == "failed" || event.outcome == "unresolved" {
            let key = normalize_alias_key(&event.title);
            if seen.insert(key.clone()) {
                failures.push(FailureSummary {
                    id: format!("failure:{}", key),
                    title: trim_chars(&event.title, 96),
                    summary: trim_chars(&event.summary, 140),
                    error: event
                        .errors
                        .first()
                        .cloned()
                        .unwrap_or_else(|| event.outcome.clone()),
                    related_files: event.active_files.clone(),
                    last_seen_at: event.end_time,
                    evidence: event.evidence.clone(),
                });
            }
        }
    }
    if let Some(project_context) = project_context {
        for failure in &project_context.known_failures {
            if seen.insert(failure.id.clone()) {
                failures.push(failure.clone());
            }
        }
    }
    failures.sort_by_key(|failure| std::cmp::Reverse(failure.last_seen_at));
    if failures.len() > 6 {
        failures.truncate(6);
    }
    failures
}

fn collect_open_issues(events: &[ActivityEvent]) -> Vec<IssueSummary> {
    let mut issues = Vec::new();
    let mut seen = HashSet::new();
    for event in events {
        for entity in &event.entities {
            if entity.entity_type != "issue" {
                continue;
            }
            if seen.insert(entity.canonical_id.clone()) {
                issues.push(IssueSummary {
                    id: entity.canonical_id.clone(),
                    title: entity.canonical_name.clone(),
                    summary: trim_chars(&event.summary, 140),
                    status: if event.outcome == "succeeded" {
                        "resolved".to_string()
                    } else {
                        "open".to_string()
                    },
                });
            }
        }
    }
    if issues.len() > 5 {
        issues.truncate(5);
    }
    issues
}

fn collect_evidence(events: &[ActivityEvent]) -> Vec<EvidenceRef> {
    let mut evidence = Vec::new();
    let mut seen = HashSet::new();
    for event in events {
        for item in &event.evidence {
            if item.privacy_class == PrivacyClass::Secret
                || item.privacy_class == PrivacyClass::Blocked
            {
                continue;
            }
            if seen.insert(item.id.clone()) {
                evidence.push(item.clone());
            }
        }
    }
    if evidence.len() > 8 {
        evidence.truncate(8);
    }
    evidence
}

fn build_included_reasons(
    events: &[ActivityEvent],
    relevant_files: &[RelevantFile],
    recent_decisions: &[DecisionSummary],
    failures: &[FailureSummary],
    active_files: &[String],
    query: &str,
    active_project: Option<&str>,
) -> Vec<ContextPackItemReason> {
    let mut reasons = Vec::new();
    for event in events.iter().take(4) {
        reasons.push(ContextPackItemReason {
            id: event.id.clone(),
            label: event.title.clone(),
            kind: "event".to_string(),
            reason: if !query.trim().is_empty() {
                "Query recall matched this activity event.".to_string()
            } else if let Some(project) = active_project {
                format!("Recent activity in active project {}.", project)
            } else {
                "Recent working-state activity.".to_string()
            },
        });
    }
    for file in relevant_files.iter().take(4) {
        reasons.push(ContextPackItemReason {
            id: file.path.clone(),
            label: file.path.clone(),
            kind: "file".to_string(),
            reason: if active_files.iter().any(|active| active == &file.path) {
                "Active file matched the request.".to_string()
            } else {
                file.why.clone()
            },
        });
    }
    for failure in failures.iter().take(3) {
        reasons.push(ContextPackItemReason {
            id: failure.id.clone(),
            label: failure.title.clone(),
            kind: "failure".to_string(),
            reason: "Recent unresolved or failed work.".to_string(),
        });
    }
    for decision in recent_decisions.iter().take(3) {
        reasons.push(ContextPackItemReason {
            id: decision.id.clone(),
            label: decision.title.clone(),
            kind: "decision".to_string(),
            reason: "Recent project decision connected to the retrieved context.".to_string(),
        });
    }
    reasons
}

fn collect_command_events(events: &[ActivityEvent]) -> Vec<CommandEvent> {
    let mut commands = Vec::new();
    let mut seen = HashSet::new();
    for event in events {
        for command in &event.commands {
            if seen.insert(command.clone()) {
                commands.push(CommandEvent {
                    command: command.clone(),
                    timestamp: event.end_time,
                    summary: trim_chars(&event.summary, 120),
                });
            }
        }
    }
    if commands.len() > 8 {
        commands.truncate(8);
    }
    commands
}

fn collect_error_events(events: &[ActivityEvent]) -> Vec<ErrorEvent> {
    let mut errors = Vec::new();
    let mut seen = HashSet::new();
    for event in events {
        for error in &event.errors {
            if seen.insert(error.clone()) {
                errors.push(ErrorEvent {
                    error: trim_chars(error, 140),
                    timestamp: event.end_time,
                    summary: trim_chars(&event.summary, 120),
                });
            }
        }
    }
    if errors.len() > 8 {
        errors.truncate(8);
    }
    errors
}

fn build_pack_summary(project: Option<&str>, events: &[ActivityEvent], query: &str) -> String {
    if let Some(project) = project {
        if !query.trim().is_empty() {
            return format!(
                "Context pack for {} focused on '{}' across {} recent activity event(s).",
                project,
                trim_chars(query.trim(), 64),
                events.len()
            );
        }
        return format!(
            "Recent {} working state compiled from {} activity event(s).",
            project,
            events.len()
        );
    }
    if let Some(event) = events.first() {
        return format!(
            "Recent context centered on '{}' from {} activity event(s).",
            trim_chars(&event.title, 64),
            events.len()
        );
    }
    "FNDR does not have enough runtime context yet.".to_string()
}

fn apply_section_budgets(pack: &mut ContextPack) {
    let budget_chars = pack.budget_tokens.saturating_mul(4) as usize;
    let active_goal_budget = ((budget_chars as f32) * 0.15) as usize;
    let failure_budget = ((budget_chars as f32) * 0.20) as usize;
    let file_budget = ((budget_chars as f32) * 0.20) as usize;
    let decision_budget = ((budget_chars as f32) * 0.20) as usize;
    let task_budget = ((budget_chars as f32) * 0.10) as usize;
    let evidence_budget = ((budget_chars as f32) * 0.15) as usize;

    if let Some(active_goal) = pack.active_goal.as_mut() {
        *active_goal = trim_chars(active_goal, active_goal_budget.max(48));
    }
    trim_failure_section(&mut pack.known_failures, failure_budget.max(120));
    trim_file_section(&mut pack.relevant_files, file_budget.max(120));
    trim_decision_section(&mut pack.recent_decisions, decision_budget.max(120));
    trim_task_section(&mut pack.open_tasks, task_budget.max(80));
    trim_evidence_section(&mut pack.evidence, evidence_budget.max(120));
}

fn trim_pack_to_budget(pack: &mut ContextPack) {
    pack.tokens_used = estimate_pack_tokens(pack);
    while pack.tokens_used > pack.budget_tokens && !pack.evidence.is_empty() {
        pack.evidence.pop();
        pack.tokens_used = estimate_pack_tokens(pack);
    }
    while pack.tokens_used > pack.budget_tokens && !pack.recent_decisions.is_empty() {
        pack.recent_decisions.pop();
        pack.tokens_used = estimate_pack_tokens(pack);
    }
    while pack.tokens_used > pack.budget_tokens && !pack.open_issues.is_empty() {
        pack.open_issues.pop();
        pack.tokens_used = estimate_pack_tokens(pack);
    }
    while pack.tokens_used > pack.budget_tokens && !pack.open_tasks.is_empty() {
        pack.open_tasks.pop();
        pack.tokens_used = estimate_pack_tokens(pack);
    }
}

fn trim_failure_section(failures: &mut Vec<FailureSummary>, budget: usize) {
    let mut remaining = budget;
    failures.retain_mut(|failure| {
        if remaining == 0 {
            return false;
        }
        failure.title = trim_chars(&failure.title, 72);
        failure.summary = trim_chars(&failure.summary, remaining.min(120));
        let used = failure.title.len() + failure.summary.len() + failure.error.len();
        remaining = remaining.saturating_sub(used);
        true
    });
}

fn trim_file_section(files: &mut Vec<RelevantFile>, budget: usize) {
    let mut remaining = budget;
    files.retain_mut(|file| {
        if remaining == 0 {
            return false;
        }
        file.why = trim_chars(&file.why, remaining.min(96));
        let used = file.path.len() + file.why.len();
        remaining = remaining.saturating_sub(used);
        true
    });
}

fn trim_decision_section(decisions: &mut Vec<DecisionSummary>, budget: usize) {
    let mut remaining = budget;
    decisions.retain_mut(|decision| {
        if remaining == 0 {
            return false;
        }
        decision.title = trim_chars(&decision.title, 80);
        decision.summary = trim_chars(&decision.summary, remaining.min(120));
        let used = decision.title.len() + decision.summary.len();
        remaining = remaining.saturating_sub(used);
        true
    });
}

fn trim_task_section(tasks: &mut Vec<ContextTask>, budget: usize) {
    let mut remaining = budget;
    tasks.retain_mut(|task| {
        if remaining == 0 {
            return false;
        }
        task.title = trim_chars(&task.title, remaining.min(96));
        remaining = remaining.saturating_sub(task.title.len());
        true
    });
}

fn trim_evidence_section(evidence: &mut Vec<EvidenceRef>, budget: usize) {
    let mut remaining = budget;
    evidence.retain_mut(|item| {
        if remaining == 0 {
            return false;
        }
        item.summary = trim_chars(&item.summary, remaining.min(80));
        item.snippet = trim_chars(&item.snippet, remaining.min(120));
        let used = item.summary.len() + item.snippet.len();
        remaining = remaining.saturating_sub(used);
        true
    });
}

fn estimate_pack_tokens(pack: &ContextPack) -> u32 {
    approximate_tokens(&serde_json::to_string(pack).unwrap_or_default())
}

fn estimate_delta_tokens(delta: &ContextDelta) -> u32 {
    approximate_tokens(&serde_json::to_string(delta).unwrap_or_default())
}

fn average_confidence(events: &[ActivityEvent]) -> f32 {
    if events.is_empty() {
        return 0.0;
    }
    let total = events.iter().map(|event| event.confidence).sum::<f32>();
    total / events.len() as f32
}

fn approximate_tokens(value: &str) -> u32 {
    ((value.chars().count() as f32) / 4.0).ceil() as u32
}

fn canonical_entity_id(entity_type: &str, candidate: &str) -> String {
    format!("{}:{}", entity_type, normalize_alias_key(candidate))
}

fn normalize_alias_key(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || ch == '/' || ch == '.' || ch == '_' || ch == '-' {
                ch
            } else {
                ':'
            }
        })
        .collect::<String>()
        .split(':')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join(":")
}

fn capitalize_token(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str().to_lowercase()),
        None => String::new(),
    }
}

fn basename(value: &str) -> String {
    value.rsplit('/').next().unwrap_or(value).trim().to_string()
}

fn trim_chars(value: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let normalized = value.replace('\n', " ").replace('\r', " ");
    let normalized = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= max_chars {
        normalized
    } else {
        format!(
            "{}...",
            normalized
                .chars()
                .take(max_chars.saturating_sub(3))
                .collect::<String>()
        )
    }
}

fn dedupe_strings(values: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        let key = trimmed.to_lowercase();
        if seen.insert(key) {
            out.push(trimmed.to_string());
        }
    }
    out
}

fn dedupe_entities(values: Vec<EntityRef>) -> Vec<EntityRef> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for value in values {
        if seen.insert(value.canonical_id.clone()) {
            out.push(value);
        }
    }
    out
}

fn normalize_agent_type(value: &str) -> String {
    let trimmed = value.trim().to_lowercase();
    if trimmed.is_empty() {
        "chat_agent".to_string()
    } else {
        trimmed
    }
}

fn normalize_budget(value: u32) -> u32 {
    value.clamp(200, 12_000).max(DEFAULT_CONTEXT_BUDGET)
}

async fn latest_active_project(state: &AppState) -> Option<String> {
    state
        .store
        .list_activity_events(1, None)
        .await
        .ok()
        .and_then(|events| events.into_iter().find_map(|event| event.project))
}

fn save_session_state(
    state: &AppState,
    session_id: &str,
    next: SessionContextState,
) -> Result<(), String> {
    state
        .state_store
        .save_json(&format!("context_session:{session_id}"), &next)
}

fn load_session_state(state: &AppState, session_id: &str) -> Option<SessionContextState> {
    state
        .state_store
        .load_json(&format!("context_session:{session_id}"))
        .ok()
        .flatten()
}

fn recursive_size(path: &std::path::Path) -> u64 {
    if !path.exists() {
        return 0;
    }
    if path.is_file() {
        return std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    }

    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            total = total.saturating_add(recursive_size(&entry.path()));
        }
    }
    total
}

// ---------------------------------------------------------------------------
// Phase 3 — agentic graph rag entry point
// ---------------------------------------------------------------------------

/// Compose mode for [`run_query`] — caller picks deterministic cards vs. a
/// grounded LLM answer (still bundled with cards + evidence + verifier outcome).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposeMode {
    Cards,
    Answer,
}

/// Single-call entry point that drives the full Phase 3 pipeline:
/// plan → RouteRunner::dispatch (5 routes) → fuse → collect_evidence → verify
/// → compose. Returns the bundled [`ComposedAnswer`] (always carrying cards +
/// evidence + verify_outcome regardless of mode).
pub async fn run_query(
    state: &AppState,
    query: &str,
    limit: usize,
    mode: ComposeMode,
) -> Result<context_pack::ComposedAnswer, String> {
    let plan = query_plan::plan(query, &query_plan::PlanHints::default());
    let weights = context_pack::FusionWeights::for_intent(plan.intent);

    let embedder = Embedder::new().ok();
    // The typed insight graph (`graph::schema`) is not yet persisted; until the
    // typed-graph storage table lands, the graph route runs against an empty
    // in-memory index built fresh per query. The other four routes still hit
    // real data, so the pipeline gracefully degrades without graph hops.
    let nodes: Vec<crate::graph::schema::GraphNode> = Vec::new();
    let edges: Vec<crate::graph::schema::GraphEdge> = Vec::new();
    let graph_index = crate::graph::graph_index::GraphIndex::build(&nodes, &edges);
    let inference = {
        let guard = state.inference.read();
        guard.as_ref().map(std::sync::Arc::clone)
    };

    let search_config = state.config.read().search.clone().normalized();
    let mut ctx = retrieval_routes::RouteCtx::new(&state.store, &search_config)
        .with_graph(&graph_index, &nodes, &edges)
        .with_limits(limit.max(1), None, None, &[])
        .with_now_ms(chrono::Utc::now().timestamp_millis());
    if let Some(emb) = embedder.as_ref() {
        ctx = ctx.with_embedder(emb);
    }
    if let Some(eng) = inference.as_deref() {
        ctx = ctx.with_inference(Some(eng));
    }

    let route_hits = retrieval_routes::RouteRunner::dispatch(&plan, &ctx).await;
    let fused = fusion::fuse(&plan, route_hits.clone(), &weights);
    let debug_trace = search_debug_trace(&plan, &route_hits, &fused, &weights);
    let evidence = evidence_pack::collect_evidence(&fused, &state.store).await;
    let outcome = verifier::verify(&plan, &fused, &evidence);

    let answer = match mode {
        ComposeMode::Cards => {
            let cards = composer::compose_cards(&plan, &fused, &evidence, &state.store).await;
            context_pack::ComposedAnswer {
                query: plan.raw.clone(),
                answer: String::new(),
                evidence,
                cards,
                verify_outcome: outcome,
                surfacing_reasons: fused.iter().map(|h| h.surfacing_reason.clone()).collect(),
                debug_trace: Some(debug_trace),
            }
        }
        ComposeMode::Answer => {
            let mut answer = composer::compose_answer(
                &plan,
                &fused,
                &evidence,
                outcome,
                inference.as_deref(),
                &state.store,
            )
            .await;
            answer.debug_trace = Some(debug_trace);
            answer
        }
    };

    Ok(answer)
}

fn search_debug_trace(
    plan: &query_plan::QueryPlan,
    route_hits: &[retrieval_routes::RouteHits],
    fused: &[context_pack::FusedHit],
    weights: &context_pack::FusionWeights,
) -> serde_json::Value {
    serde_json::json!({
        "plan": {
            "intent": plan.intent,
            "retrieval_routes": plan.retrieval_routes,
            "target_project": plan.target_project,
            "target_topics_count": plan.target_topics.len(),
            "target_entities_count": plan.target_entities.len(),
            "time_window": plan.time_window,
        },
        "weights": weights,
        "routes": route_hits.iter().map(route_trace).collect::<Vec<_>>(),
        "fused_count": fused.len(),
        "fused": fused.iter().take(12).map(fused_trace).collect::<Vec<_>>(),
    })
}

fn route_trace(route_hits: &retrieval_routes::RouteHits) -> serde_json::Value {
    serde_json::json!({
        "route": route_hits.route,
        "candidate_count": route_hits.hits.len(),
        "elapsed_ms": route_hits.elapsed_ms,
        "top_candidates": route_hits.hits.iter().take(5).map(|hit| {
            let embedding_reason_labels = hit.signals.search_result.as_ref()
                .map(|result| result.embedding_reason_labels.clone())
                .unwrap_or_default();
            serde_json::json!({
                "memory_id": hit.memory_id,
                "score": hit.score,
                "branch": hit.signals.branch,
                "embedding_reason_labels": embedding_reason_labels,
                "has_graph_path": hit.graph_path.as_ref().map(|path| !path.is_empty()).unwrap_or(false),
            })
        }).collect::<Vec<_>>(),
    })
}

fn fused_trace(hit: &context_pack::FusedHit) -> serde_json::Value {
    let embedding_reason_labels = hit
        .surfacing_reason
        .routes
        .iter()
        .filter(|route| route.starts_with("embedding:"))
        .cloned()
        .collect::<Vec<_>>();
    serde_json::json!({
        "memory_id": hit.memory_id,
        "score": hit.score,
        "contributing_routes": hit.contributing_routes,
        "signals": hit.signals,
        "embedding_reason_labels": embedding_reason_labels,
        "included_with_embedding_warnings": !embedding_reason_labels.is_empty(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> MemoryRecord {
        MemoryRecord {
            app_name: "Cursor".to_string(),
            window_title: "src-tauri/src/search/hybrid.rs - Cursor".to_string(),
            clean_text: String::new(),
            snippet: "Updated search pipeline".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn explicit_project_wins_over_other_heuristics() {
        let mut memory = record();
        memory.project = "Atlas".to_string();
        memory.files_touched =
            vec!["/Users/anurupkumar/fndr/src-tauri/src/search/hybrid.rs".to_string()];
        memory.url = Some("https://github.com/openai/fndr/issues/52".to_string());

        assert_eq!(detect_project_for_record(&memory).as_deref(), Some("Atlas"));
        assert_eq!(detect_repo_for_record(&memory).as_deref(), Some("atlas"));
    }

    #[test]
    fn file_paths_infer_project_before_window_or_url() {
        let mut memory = record();
        memory.files_touched = vec![
            "/Users/anurupkumar/fndr/src-tauri/src/search/hybrid.rs".to_string(),
            "/Users/anurupkumar/fndr/src/domains/workspace/AgentPanel.tsx".to_string(),
        ];
        memory.url = Some("https://github.com/example/other-repo/issues/1".to_string());

        assert_eq!(detect_project_for_record(&memory).as_deref(), Some("FNDR"));
        assert_eq!(detect_repo_for_record(&memory).as_deref(), Some("fndr"));
    }

    #[test]
    fn url_repo_slug_is_used_when_file_evidence_is_missing() {
        let mut memory = record();
        memory.files_touched.clear();
        memory.window_title = "Issue 52 - browser".to_string();
        memory.app_name = "Google Chrome".to_string();
        memory.url = Some("https://github.com/openai/fndr/pull/52".to_string());

        assert_eq!(detect_project_for_record(&memory).as_deref(), Some("FNDR"));
        assert_eq!(detect_repo_for_record(&memory).as_deref(), Some("fndr"));
    }

    #[test]
    fn weak_relative_paths_do_not_false_positive() {
        assert_eq!(
            infer_project_from_files(&["src-tauri/src/search/hybrid.rs".to_string()]),
            None
        );
        assert_eq!(infer_project_from_url(Some("https://github.com")), None);
    }

    #[test]
    fn infer_activity_type_respects_explicit_non_empty_value() {
        let mut memory = record();
        memory.activity_type = "Researching".to_string();
        assert_eq!(infer_activity_type(&memory), "researching");
    }

    #[test]
    fn infer_activity_type_errors_and_commands_is_debugging() {
        let mut memory = record();
        memory.errors = vec!["timeout".to_string()];
        memory.commands = vec!["cargo test".to_string()];
        assert_eq!(infer_activity_type(&memory), "debugging");
    }

    #[test]
    fn infer_activity_type_implementation_planning_signal() {
        let mut memory = record();
        memory.decisions = vec!["Ship incremental rollout".to_string()];
        memory.next_steps = vec!["Add metrics".to_string(), "Cut feature flag".to_string()];
        assert_eq!(infer_activity_type(&memory), "implementation planning");
    }

    #[test]
    fn infer_activity_type_refactor_review_two_files_no_decisions() {
        let mut memory = record();
        memory.files_touched = vec!["a.rs".to_string(), "b.rs".to_string()];
        assert_eq!(infer_activity_type(&memory), "refactor review");
    }

    #[test]
    fn infer_activity_type_system_design_from_decision_stem() {
        let mut memory = record();
        memory.decisions = vec!["Design modular boundaries".to_string()];
        memory.files_touched = vec!["a.rs".to_string(), "b.rs".to_string()];
        assert_eq!(infer_activity_type(&memory), "system design");
    }

    #[test]
    fn infer_activity_type_architecture_analysis_url_and_next_steps() {
        let mut memory = record();
        memory.url = Some("https://example.com/arch".to_string());
        memory.next_steps = vec!["Document ADR".to_string()];
        assert_eq!(infer_activity_type(&memory), "architecture analysis");
    }

    #[test]
    fn infer_activity_type_instruction_writing_long_context() {
        let mut memory = record();
        memory.memory_context = "y".repeat(801);
        assert_eq!(infer_activity_type(&memory), "instruction writing");
    }

    #[test]
    fn infer_activity_type_fallback_scoring_from_text_signals() {
        let mut memory = record();
        memory.clean_text = "Investigated the compilation error on line 12".to_string();
        assert_eq!(infer_activity_type(&memory), "debugging");

        memory.clean_text = "Added unit tests and assertions for the parser".to_string();
        memory.internal_context = String::new();
        memory.memory_context = String::new();
        assert_eq!(infer_activity_type(&memory), "testing_workflow");
    }

    #[test]
    fn search_debug_trace_reports_embedding_warnings_without_source_text() {
        let mut plan = query_plan::plan("debug embeddings", &query_plan::PlanHints::default());
        plan.retrieval_routes = vec![query_plan::Route::Vector];
        let mut result = crate::storage::SearchResult {
            id: "memory-1".to_string(),
            score: 0.74,
            ..Default::default()
        };
        result.embedding_reason_labels = vec!["embedding:primary:stale_source_text".to_string()];
        let route_hits = vec![retrieval_routes::RouteHits {
            route: query_plan::Route::Vector,
            hits: vec![retrieval_routes::RouteHit {
                memory_id: "memory-1".to_string(),
                score: 0.74,
                signals: retrieval_routes::RouteSignals {
                    branch: retrieval_routes::RouteBranch::Semantic,
                    confidence: 0.74,
                    search_result: Some(result),
                },
                graph_path: None,
            }],
            elapsed_ms: 3,
        }];
        let fused = vec![context_pack::FusedHit {
            memory_id: "memory-1".to_string(),
            score: 0.333,
            signals: context_pack::FusionSignals {
                vector: 0.74,
                ..Default::default()
            },
            surfacing_reason: context_pack::SurfacingReason {
                headline: "Matched in 1 routes".to_string(),
                routes: vec![
                    "vector".to_string(),
                    "embedding:primary:stale_source_text".to_string(),
                ],
                graph_path: None,
                anchor_terms_hit: Vec::new(),
                recency_boost: 0.0,
            },
            contributing_routes: vec![query_plan::Route::Vector],
        }];

        let trace = search_debug_trace(
            &plan,
            &route_hits,
            &fused,
            &context_pack::FusionWeights::default(),
        );

        assert_eq!(trace["routes"][0]["candidate_count"], 1);
        assert_eq!(
            trace["routes"][0]["top_candidates"][0]["embedding_reason_labels"][0],
            "embedding:primary:stale_source_text"
        );
        assert_eq!(trace["fused"][0]["included_with_embedding_warnings"], true);
        assert!(!trace.to_string().contains("RAW_OCR"));
    }
}
