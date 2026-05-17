use crate::context_runtime::graph_plan::GraphPlan;
use crate::graph::schema::{GraphEdgeType, GraphNodeType};
use crate::inference::InferenceEngine;
use crate::search::{QueryIntent, QueryProfile};
use crate::telemetry::runtime_metrics;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_BUDGET_TOKENS: u32 = 1_200;
const DAY_MS: i64 = 86_400_000;

#[derive(Serialize, Deserialize, Type, Clone, Debug, PartialEq, Eq)]
pub struct QueryPlan {
    pub raw: String,
    pub intent: PlannerIntent,
    pub target_project: Option<String>,
    pub target_topics: Vec<String>,
    pub target_entities: Vec<EntityHint>,
    pub time_window: Option<TimeWindow>,
    pub needed_context: NeededContext,
    pub retrieval_routes: Vec<Route>,
    pub graph_expansion: GraphExpansion,
    pub budget_tokens: u32,
}

#[derive(Serialize, Deserialize, Type, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlannerIntent {
    ResumeWork,
    Debug,
    Lookup,
    HowTo,
    Definition,
    Timeline,
    RelatedTo,
}

#[derive(Serialize, Deserialize, Type, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Route {
    Vector,
    Keyword,
    Temporal,
    Entity,
    Graph,
}

#[derive(Serialize, Deserialize, Type, Clone, Debug, PartialEq, Eq)]
pub struct EntityHint {
    pub label: String,
    pub kind: EntityHintKind,
}

#[derive(Serialize, Deserialize, Type, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EntityHintKind {
    Concept,
    Person,
    Tool,
    File,
    Url,
    App,
    Command,
}

#[derive(Serialize, Deserialize, Type, Clone, Debug, PartialEq, Eq)]
pub struct TimeWindow {
    pub from_ms: i64,
    pub to_ms: i64,
}

#[derive(Default, Serialize, Deserialize, Type, Clone, Debug, PartialEq, Eq)]
pub struct NeededContext {
    pub files: bool,
    pub decisions: bool,
    pub errors: bool,
    pub todos: bool,
    pub commands: bool,
    pub recent_changes: bool,
}

#[derive(Serialize, Deserialize, Type, Clone, Debug, PartialEq, Eq)]
pub struct GraphExpansion {
    pub max_hops: u8,
    pub seed_kinds: Vec<GraphNodeType>,
    pub allowed_edges: Vec<GraphEdgeType>,
}

#[derive(Default, Serialize, Deserialize, Type, Clone, Debug, PartialEq, Eq)]
pub struct PlanHints {
    #[serde(default)]
    pub entity_aliases: Vec<EntityAliasHint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub now_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_tokens: Option<u32>,
}

#[derive(Serialize, Deserialize, Type, Clone, Debug, PartialEq, Eq)]
pub struct EntityAliasHint {
    pub alias: String,
    pub canonical_name: String,
    pub entity_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
}

#[derive(Default, Deserialize)]
struct PlanRefinement {
    target_project: Option<String>,
    target_topics: Option<Vec<String>>,
    graph_max_hops: Option<u8>,
}

static URL_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"https?://[^\s)>"]+"#).expect("valid planner URL regex"));
static FILE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"\b(?:(?:[A-Za-z0-9._-]+/)+)?[A-Za-z0-9._-]+\.(?:rs|ts|tsx|js|jsx|md|toml|json|yaml|yml|py|go|java|swift|sql)\b",
    )
    .expect("valid planner file regex")
});
static CAMEL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\b[A-Z][a-z0-9]+(?:[A-Z][A-Za-z0-9]+)+\b").expect("valid planner CamelCase regex")
});
static DOTTED_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\b[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)+\b")
        .expect("valid planner dotted identifier regex")
});
static COMMAND_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"\b(?:cargo|git|npm|pnpm|yarn|make|python3?|uv|pytest)(?:\s+[A-Za-z0-9_./:-]+){0,3}",
    )
    .expect("valid planner command regex")
});

pub fn plan(query: &str, hints: &PlanHints) -> QueryPlan {
    let started = std::time::Instant::now();
    let profile = QueryProfile::from_query(query);
    let normalized = profile.normalized();
    let intent = planner_intent(&profile);
    let mut target_entities = detect_entity_hints(query);
    let target_project = detect_target_project(&profile, hints);
    if target_project.is_some()
        && !target_entities
            .iter()
            .any(|entity| entity.kind == EntityHintKind::Concept)
    {
        if let Some(project) = target_project.as_ref() {
            push_entity(&mut target_entities, project, EntityHintKind::Concept);
        }
    }

    let time_window = detect_time_window(normalized, hints.now_ms.unwrap_or_else(now_ms));
    let target_topics = profile.anchor_terms().to_vec();
    let needed_context = needed_context(intent, normalized, &target_entities);
    let graph_plan = GraphPlan::from_intent(intent);
    let mut graph_expansion = GraphExpansion {
        max_hops: graph_plan.max_hops,
        seed_kinds: graph_plan.seed_kinds,
        allowed_edges: graph_plan.allowed_edges,
    };
    if target_project.is_some() && !graph_expansion.seed_kinds.contains(&GraphNodeType::Project) {
        graph_expansion.seed_kinds.push(GraphNodeType::Project);
    }

    let retrieval_routes = route_selection(
        intent,
        target_project.is_some(),
        !target_entities.is_empty(),
        time_window.is_some() || profile.wants_recency() || contains_temporal_word(normalized),
    );

    let plan = QueryPlan {
        raw: query.to_string(),
        intent,
        target_project,
        target_topics,
        target_entities,
        time_window,
        needed_context,
        retrieval_routes,
        graph_expansion,
        budget_tokens: hints.budget_tokens.unwrap_or(DEFAULT_BUDGET_TOKENS),
    };
    runtime_metrics::record_ms(
        "fndr.retrieval.planner.ms",
        started.elapsed().as_millis() as u64,
    );
    plan
}

pub async fn refine_plan_with_llm(plan: &mut QueryPlan, engine: &InferenceEngine) -> bool {
    let current_plan_json = match serde_json::to_string(plan) {
        Ok(json) => json,
        Err(_) => "{}".to_string(),
    };
    let Some(refinement_json) = engine
        .refine_query_plan(&plan.raw, &current_plan_json, 400)
        .await
    else {
        return false;
    };

    apply_refinement_json(plan, &refinement_json)
}

pub fn apply_refinement_json(plan: &mut QueryPlan, refinement_json: &str) -> bool {
    let Ok(refinement) = serde_json::from_str::<PlanRefinement>(refinement_json) else {
        return false;
    };

    let mut changed = false;
    if let Some(target_project) = refinement
        .target_project
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        plan.target_project = Some(target_project);
        changed = true;
    }
    if let Some(topics) = refinement.target_topics {
        let mut deduped = Vec::new();
        for topic in topics {
            push_unique(&mut deduped, topic.trim());
        }
        plan.target_topics = deduped;
        changed = true;
    }
    if let Some(max_hops) = refinement.graph_max_hops.filter(|value| *value <= 2) {
        plan.graph_expansion.max_hops = max_hops;
        changed = true;
    }

    changed
}

fn planner_intent(profile: &QueryProfile) -> PlannerIntent {
    let normalized = profile.normalized();
    if contains_any(normalized, &["timeline", "chronology", "sequence of"]) {
        return PlannerIntent::Timeline;
    }
    if contains_any(
        normalized,
        &["resume", "pick up", "continue", "working on", "left off"],
    ) {
        return PlannerIntent::ResumeWork;
    }
    if contains_any(
        normalized,
        &[
            "debug",
            "error",
            "failed",
            "failure",
            "panic",
            "exception",
            "broken",
        ],
    ) {
        return PlannerIntent::Debug;
    }
    if contains_any(normalized, &["related to", "similar to", "connected to"]) {
        return PlannerIntent::RelatedTo;
    }
    if normalized.starts_with("why is ")
        || normalized.starts_with("why does ")
        || normalized.starts_with("what is ")
        || normalized.starts_with("define ")
        || profile.intent() == QueryIntent::Definition
    {
        return PlannerIntent::Definition;
    }
    if profile.intent() == QueryIntent::HowTo {
        return PlannerIntent::HowTo;
    }
    PlannerIntent::Lookup
}

fn route_selection(
    intent: PlannerIntent,
    has_project: bool,
    has_entities: bool,
    needs_temporal: bool,
) -> Vec<Route> {
    let mut routes = vec![Route::Vector, Route::Keyword];
    if has_entities || has_project {
        routes.push(Route::Entity);
    }
    if needs_temporal {
        routes.push(Route::Temporal);
    }
    if matches!(
        intent,
        PlannerIntent::ResumeWork
            | PlannerIntent::Debug
            | PlannerIntent::Definition
            | PlannerIntent::RelatedTo
            | PlannerIntent::Lookup
            | PlannerIntent::HowTo
            | PlannerIntent::Timeline
    ) {
        routes.push(Route::Graph);
    }
    routes
}

fn detect_target_project(profile: &QueryProfile, hints: &PlanHints) -> Option<String> {
    if hints.entity_aliases.is_empty() {
        return None;
    }

    let anchors = profile
        .anchor_terms()
        .iter()
        .map(|term| normalize_key(term))
        .collect::<HashSet<_>>();
    let normalized_query = normalize_key(profile.normalized());

    hints
        .entity_aliases
        .iter()
        .filter(|alias| alias.entity_type.eq_ignore_ascii_case("project"))
        .find(|alias| {
            let alias_key = normalize_key(&alias.alias);
            let canonical_key = normalize_key(&alias.canonical_name);
            anchors.contains(&alias_key)
                || anchors.contains(&canonical_key)
                || normalized_query.contains(&alias_key)
                || normalized_query.contains(&canonical_key)
        })
        .map(|alias| {
            alias
                .project
                .clone()
                .unwrap_or_else(|| alias.canonical_name.clone())
        })
}

fn detect_entity_hints(query: &str) -> Vec<EntityHint> {
    let mut entities = Vec::new();
    for mat in URL_RE.find_iter(query) {
        push_entity(&mut entities, mat.as_str(), EntityHintKind::Url);
    }
    for mat in FILE_RE.find_iter(query) {
        push_entity(&mut entities, mat.as_str(), EntityHintKind::File);
    }
    for mat in COMMAND_RE.find_iter(query) {
        let label = canonical_command_label(mat.as_str());
        push_entity(&mut entities, &label, EntityHintKind::Command);
    }
    for mat in CAMEL_RE.find_iter(query) {
        push_entity(&mut entities, mat.as_str(), EntityHintKind::Concept);
    }
    for mat in DOTTED_RE.find_iter(query) {
        let value = mat.as_str();
        if !URL_RE.is_match(value) && !FILE_RE.is_match(value) {
            push_entity(&mut entities, value, EntityHintKind::Tool);
        }
    }
    entities
}

fn canonical_command_label(raw: &str) -> String {
    let words = raw.split_whitespace().collect::<Vec<_>>();
    if words.is_empty() {
        return String::new();
    }
    if words.first() == Some(&"cargo") && words.get(1) == Some(&"test") {
        return "cargo test".to_string();
    }
    if words.first() == Some(&"npm") && words.get(1) == Some(&"run") {
        if let Some(script) = words.get(2) {
            return format!("npm run {script}");
        }
        return "npm run".to_string();
    }
    words.into_iter().take(2).collect::<Vec<_>>().join(" ")
}

fn needed_context(
    intent: PlannerIntent,
    normalized: &str,
    entities: &[EntityHint],
) -> NeededContext {
    let files = entities
        .iter()
        .any(|entity| entity.kind == EntityHintKind::File)
        || contains_any(normalized, &["file", "files", "path"]);
    let commands = entities
        .iter()
        .any(|entity| entity.kind == EntityHintKind::Command)
        || contains_any(normalized, &["command", "terminal", "run"]);
    NeededContext {
        files,
        decisions: contains_any(normalized, &["decision", "decided", "why"]),
        errors: intent == PlannerIntent::Debug
            || contains_any(normalized, &["error", "failed", "failure", "panic"]),
        todos: contains_any(normalized, &["todo", "task", "next step"]),
        commands,
        recent_changes: intent == PlannerIntent::ResumeWork
            || contains_any(normalized, &["recent", "latest", "changes", "working on"]),
    }
}

fn detect_time_window(normalized: &str, now_ms: i64) -> Option<TimeWindow> {
    if normalized.contains("today") {
        return Some(TimeWindow {
            from_ms: now_ms.saturating_sub(DAY_MS),
            to_ms: now_ms,
        });
    }
    if normalized.contains("yesterday") {
        return Some(TimeWindow {
            from_ms: now_ms.saturating_sub(DAY_MS * 2),
            to_ms: now_ms.saturating_sub(DAY_MS),
        });
    }
    if normalized.contains("last week")
        || normalized.contains("recent")
        || normalized.contains("latest")
    {
        return Some(TimeWindow {
            from_ms: now_ms.saturating_sub(DAY_MS * 7),
            to_ms: now_ms,
        });
    }
    if normalized.contains("before") || normalized.contains("after") {
        return Some(TimeWindow {
            from_ms: now_ms.saturating_sub(DAY_MS * 30),
            to_ms: now_ms,
        });
    }
    None
}

fn contains_temporal_word(normalized: &str) -> bool {
    contains_any(
        normalized,
        &[
            "today",
            "yesterday",
            "last week",
            "before",
            "after",
            "recent",
            "latest",
        ],
    )
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn push_entity(target: &mut Vec<EntityHint>, label: &str, kind: EntityHintKind) {
    let label = label
        .trim()
        .trim_matches(|ch: char| ch == ',' || ch == '.' || ch == ';');
    if label.is_empty() {
        return;
    }
    if target
        .iter()
        .any(|entity| entity.kind == kind && entity.label.eq_ignore_ascii_case(label))
    {
        return;
    }
    target.push(EntityHint {
        label: label.to_string(),
        kind,
    });
}

fn push_unique(target: &mut Vec<String>, value: &str) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return;
    }
    if !target.iter().any(|existing| existing == trimmed) {
        target.push(trimmed.to_string());
    }
}

fn normalize_key(value: &str) -> String {
    value
        .to_ascii_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_fixture_logs_representative_plans() {
        for query in [
            "why is the planner debounce 250ms",
            "debug the LanceDB schema error",
            "resume work on FNDR today",
            "where did I mention ScreenCaptureKit",
            "how do I run cargo test for graph_index.rs",
            "what is related to MCP retrieval feedback",
        ] {
            let query_plan = plan(query, &PlanHints::default());
            println!(
                "{}",
                serde_json::to_string_pretty(&query_plan).expect("plan serializes")
            );
            assert!(!query_plan.retrieval_routes.is_empty());
        }
    }
}
