use super::audit::{
    append_agent_audit_record, audit_record_from_failure, audit_record_from_success,
};
use super::policy::{policy_for_mode, AgentMode, PermissionScope, RiskLevel, ToolPolicy};
use crate::context_runtime::{self, ContextRequest};
use crate::privacy::Blocklist;
use crate::storage::{
    ContextPack, ContextTask, DecisionSummary, EntityRef, EvidenceRef, FailureSummary,
    ProjectContext, RelevantFile,
};
use crate::AppState;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::atomic::Ordering;

const DEFAULT_AGENT_TOKEN_BUDGET: u32 = 900;
const MAX_AGENT_TOKEN_BUDGET: u32 = 4_000;
const DEFAULT_AGENT_MEMORY_LIMIT: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentContextRequest {
    pub user_goal: String,
    #[serde(default)]
    pub mode: AgentMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_minutes: Option<u32>,
    #[serde(default)]
    pub selected_memory_ids: Vec<String>,
    #[serde(default)]
    pub include_raw_evidence: bool,
    #[serde(default)]
    pub budget_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentContextPack {
    pub task_id: String,
    pub user_goal: String,
    pub mode: AgentMode,
    pub relevant_memories: Vec<AgentMemoryCard>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_project: Option<ProjectContext>,
    #[serde(default)]
    pub recent_workflow_trace: Vec<WorkflowStep>,
    #[serde(default)]
    pub entities: Vec<EntityRef>,
    #[serde(default)]
    pub files: Vec<FileRef>,
    #[serde(default)]
    pub urls: Vec<UrlRef>,
    #[serde(default)]
    pub commands: Vec<CommandEvidence>,
    #[serde(default)]
    pub errors: Vec<ErrorEvidence>,
    #[serde(default)]
    pub decisions: Vec<DecisionEvidence>,
    #[serde(default)]
    pub todos: Vec<TodoEvidence>,
    pub privacy_scope: PrivacyScope,
    pub allowed_tools: Vec<ToolPolicy>,
    #[serde(default)]
    pub disallowed_context: Vec<RedactionNote>,
    pub token_budget: TokenBudget,
    pub confidence: f32,
    pub evidence_summary: String,
    pub source_context_pack_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentRunResponse {
    pub run_id: String,
    pub mode: AgentMode,
    pub answer: String,
    pub context_pack: AgentContextPack,
    pub proposed_actions: Vec<ProposedAction>,
    pub blocked_actions: Vec<ToolPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audit_warning: Option<String>,
}

pub async fn build_agent_context_pack(
    state: &AppState,
    request: AgentContextRequest,
) -> Result<AgentContextPack, String> {
    let budget_tokens = normalize_budget(request.budget_tokens);
    let context_request = ContextRequest {
        query: request.user_goal.clone(),
        agent_type: format!("{:?}_agent", request.mode).to_lowercase(),
        budget_tokens,
        session_id: None,
        active_files: Vec::new(),
        project: request.project.clone(),
    };
    let pack = context_runtime::build_context_pack(state, context_request).await?;
    let filtered = build_from_context_pack(state, request, pack).await?;

    Ok(filtered)
}

pub async fn run_agent_request(
    state: &AppState,
    request: AgentContextRequest,
) -> Result<AgentRunResponse, String> {
    let run_id = format!("agent_run_{}", uuid::Uuid::new_v4().simple());
    let context_pack = match build_agent_context_pack(state, request.clone()).await {
        Ok(pack) => pack,
        Err(err) => {
            let record = audit_record_from_failure(&run_id, &request, &err);
            let audit_message = append_agent_audit_record(state.app_data_dir.as_path(), &record)
                .err()
                .map(|audit_err| format!(" Audit write failed: {audit_err}"))
                .unwrap_or_default();
            return Err(format!(
                "Agent run failed: {err}.{audit_message} run_id={run_id}"
            ));
        }
    };
    let answer = deterministic_response(&context_pack);
    let blocked_actions = context_pack
        .allowed_tools
        .iter()
        .filter(|policy| !policy.allowed || policy.risk == RiskLevel::Blocked)
        .cloned()
        .collect::<Vec<_>>();
    let proposed_actions = proposed_actions_for_mode(&context_pack);
    let audit_record =
        audit_record_from_success(&run_id, &request, &context_pack, &blocked_actions, &answer);
    let audit_warning =
        append_agent_audit_record(state.app_data_dir.as_path(), &audit_record).err();

    Ok(AgentRunResponse {
        run_id,
        mode: context_pack.mode,
        answer,
        context_pack,
        proposed_actions,
        blocked_actions,
        audit_warning,
    })
}

async fn build_from_context_pack(
    state: &AppState,
    request: AgentContextRequest,
    pack: ContextPack,
) -> Result<AgentContextPack, String> {
    let budget_tokens = normalize_budget(request.budget_tokens);
    let privacy_scope = privacy_scope_for_request(state, &request);
    let mut disallowed_context = pack
        .excluded
        .iter()
        .map(|item| RedactionNote {
            id: item.id.clone(),
            reason: item.reason.clone(),
        })
        .collect::<Vec<_>>();
    note_unsupported_filters(&request, &mut disallowed_context);

    let blocklist = state.config.read().blocklist.clone();
    let selected: HashSet<&str> = request
        .selected_memory_ids
        .iter()
        .map(String::as_str)
        .collect();
    let mut memory_cards = Vec::new();
    let mut seen = HashSet::new();

    for evidence in &pack.evidence {
        if !selected.is_empty() && !selected.contains(evidence.source_id.as_str()) {
            disallowed_context.push(RedactionNote {
                id: evidence.source_id.clone(),
                reason: "not selected for this agent context request".to_string(),
            });
            continue;
        }
        if !seen.insert(evidence.source_id.clone()) {
            continue;
        }

        let memory = state
            .store
            .get_memory_by_id(&evidence.source_id)
            .await
            .map_err(|err| err.to_string())?;
        if let Some(memory) = memory {
            if is_private_memory(
                &memory.app_name,
                memory.url.as_deref(),
                &memory.window_title,
                &blocklist,
            ) {
                disallowed_context.push(RedactionNote {
                    id: memory.id.clone(),
                    reason: "excluded by Continuum privacy blocklist or sensitive-context policy"
                        .to_string(),
                });
                continue;
            }
            if memory_cards.len() >= DEFAULT_AGENT_MEMORY_LIMIT {
                disallowed_context.push(RedactionNote {
                    id: memory.id.clone(),
                    reason: "dropped after agent memory limit was reached".to_string(),
                });
                continue;
            }
            memory_cards.push(AgentMemoryCard {
                memory_id: memory.id.clone(),
                title: first_non_empty(&[
                    memory.display_summary.as_str(),
                    memory.insight_what_happened.as_str(),
                    memory.window_title.as_str(),
                    evidence.summary.as_str(),
                ]),
                summary: first_non_empty(&[
                    memory.memory_context.as_str(),
                    memory.display_summary.as_str(),
                    evidence.summary.as_str(),
                    evidence.snippet.as_str(),
                ]),
                timestamp: memory.timestamp,
                app_name: memory.app_name.clone(),
                window_title: memory.window_title.clone(),
                url: memory.url.clone(),
                confidence: memory
                    .confidence_score
                    .max(memory.insight_card_confidence)
                    .max(evidence_confidence_floor(pack.confidence)),
                match_reason: included_reason_for(&pack, &memory.id),
                evidence: vec![redact_evidence(evidence, request.include_raw_evidence)],
            });
        } else {
            memory_cards.push(AgentMemoryCard {
                memory_id: evidence.source_id.clone(),
                title: evidence.summary.clone(),
                summary: evidence.snippet.clone(),
                timestamp: evidence.timestamp,
                app_name: evidence.source_type.clone(),
                window_title: String::new(),
                url: None,
                confidence: evidence_confidence_floor(pack.confidence),
                match_reason: included_reason_for(&pack, &evidence.source_id),
                evidence: vec![redact_evidence(evidence, request.include_raw_evidence)],
            });
        }
    }

    let files = pack
        .relevant_files
        .iter()
        .map(|file| FileRef {
            path: file.path.clone(),
            reason: file.why.clone(),
        })
        .collect();
    let urls = collect_urls(&memory_cards, &pack.evidence);
    let entities = collect_entities_from_cards(&memory_cards);
    let recent_workflow_trace = workflow_trace_from_cards(&memory_cards);
    let commands = collect_command_evidence(&memory_cards);
    let errors = pack
        .known_failures
        .iter()
        .map(ErrorEvidence::from_failure)
        .collect();
    let decisions = pack
        .recent_decisions
        .iter()
        .map(DecisionEvidence::from_decision)
        .collect();
    let todos = pack
        .open_tasks
        .iter()
        .map(TodoEvidence::from_task)
        .collect();
    let evidence_summary = evidence_summary(&pack, memory_cards.len(), disallowed_context.len());
    let tokens_used = pack.tokens_used;

    Ok(AgentContextPack {
        task_id: format!("agent_ctx_{}", uuid::Uuid::new_v4().simple()),
        user_goal: request.user_goal,
        mode: request.mode,
        relevant_memories: memory_cards,
        current_project: pack.project.as_ref().map(|project| ProjectContext {
            id: project.clone(),
            project: project.clone(),
            active_goal: pack.active_goal.clone().unwrap_or_default(),
            summary: pack.summary.clone(),
            relevant_files: pack
                .relevant_files
                .iter()
                .map(|file| RelevantFile {
                    path: file.path.clone(),
                    why: file.why.clone(),
                })
                .collect(),
            recent_decisions: pack.recent_decisions.clone(),
            open_issues: pack.open_issues.clone(),
            known_failures: pack.known_failures.clone(),
            open_tasks: pack.open_tasks.clone(),
            constraints: pack.do_not_do.clone(),
            confidence: pack.confidence,
            privacy_class: Default::default(),
            updated_at: pack.generated_at,
        }),
        recent_workflow_trace,
        entities,
        files,
        urls,
        commands,
        errors,
        decisions,
        todos,
        privacy_scope,
        allowed_tools: policy_for_mode(request.mode),
        disallowed_context,
        token_budget: TokenBudget {
            requested: budget_tokens,
            max: MAX_AGENT_TOKEN_BUDGET,
            used: tokens_used,
            dropped_items: pack.excluded.len() as u32,
        },
        confidence: pack.confidence,
        evidence_summary,
        source_context_pack_id: pack.id,
    })
}

fn normalize_budget(value: u32) -> u32 {
    if value == 0 {
        DEFAULT_AGENT_TOKEN_BUDGET
    } else {
        value.clamp(300, MAX_AGENT_TOKEN_BUDGET)
    }
}

fn privacy_scope_for_request(state: &AppState, request: &AgentContextRequest) -> PrivacyScope {
    PrivacyScope {
        local_only: true,
        read_only: !matches!(request.mode, AgentMode::Act),
        include_raw_evidence: request.include_raw_evidence,
        include_sensitive_context: false,
        exclude_private_apps: true,
        excluded_apps_or_domains: state.config.read().blocklist.clone(),
        project: request.project.clone(),
        window_minutes: request.window_minutes,
        incognito_active: state.is_incognito.load(Ordering::SeqCst),
    }
}

fn note_unsupported_filters(request: &AgentContextRequest, notes: &mut Vec<RedactionNote>) {
    if request.app.is_some() {
        notes.push(RedactionNote {
            id: "filter:app".to_string(),
            reason: "app filter is accepted by the agent API but currently only applied through existing retrieval ranking".to_string(),
        });
    }
    if request.domain.is_some() {
        notes.push(RedactionNote {
            id: "filter:domain".to_string(),
            reason: "domain filter is accepted by the agent API but currently only applied through existing retrieval ranking".to_string(),
        });
    }
    if request.window_minutes.is_some() {
        notes.push(RedactionNote {
            id: "filter:window_minutes".to_string(),
            reason: "time-window filter is recorded in privacy scope; existing context runtime chooses bounded recent context".to_string(),
        });
    }
}

fn is_private_memory(
    app_name: &str,
    url: Option<&str>,
    window_title: &str,
    blocklist: &[String],
) -> bool {
    Blocklist::is_internal_app(app_name, None)
        || Blocklist::is_blocked(app_name, blocklist)
        || Blocklist::is_context_blocked(url, Some(window_title), blocklist)
        || Blocklist::is_sensitive_context(url, Some(window_title))
}

fn redact_evidence(evidence: &EvidenceRef, include_raw: bool) -> EvidenceRef {
    if include_raw {
        return evidence.clone();
    }
    let mut redacted = evidence.clone();
    redacted.snippet = truncate_words(&redacted.snippet, 36);
    redacted
}

fn included_reason_for(pack: &ContextPack, id: &str) -> String {
    pack.included
        .iter()
        .find(|reason| reason.id == id)
        .map(|reason| reason.reason.clone())
        .unwrap_or_else(|| "selected by Continuum retrieval context pack".to_string())
}

fn collect_urls(cards: &[AgentMemoryCard], evidence: &[EvidenceRef]) -> Vec<UrlRef> {
    let mut urls = BTreeMap::new();
    for card in cards {
        if let Some(url) = card.url.as_deref().filter(|url| !url.trim().is_empty()) {
            urls.entry(url.to_string()).or_insert_with(|| UrlRef {
                url: url.to_string(),
                source_memory_id: card.memory_id.clone(),
                reason: "captured browser URL".to_string(),
            });
        }
    }
    for item in evidence {
        for url in extract_urls(&item.snippet) {
            urls.entry(url.clone()).or_insert_with(|| UrlRef {
                url,
                source_memory_id: item.source_id.clone(),
                reason: "URL mentioned in evidence snippet".to_string(),
            });
        }
    }
    urls.into_values().take(12).collect()
}

fn collect_entities_from_cards(cards: &[AgentMemoryCard]) -> Vec<EntityRef> {
    let mut names = BTreeSet::new();
    for card in cards {
        for token in card
            .title
            .split(|ch: char| !ch.is_alphanumeric() && ch != '-' && ch != '_')
            .filter(|token| token.len() > 3 && token.chars().next().is_some_and(char::is_uppercase))
        {
            names.insert(token.to_string());
        }
    }
    names
        .into_iter()
        .take(12)
        .map(|name| EntityRef {
            canonical_id: name.to_lowercase(),
            canonical_name: name,
            entity_type: "topic".to_string(),
            confidence: 0.45,
            aliases: Vec::new(),
        })
        .collect()
}

fn workflow_trace_from_cards(cards: &[AgentMemoryCard]) -> Vec<WorkflowStep> {
    cards
        .iter()
        .map(|card| WorkflowStep {
            timestamp: card.timestamp,
            title: card.title.clone(),
            app_name: card.app_name.clone(),
            summary: truncate_words(&card.summary, 28),
            source_memory_id: card.memory_id.clone(),
        })
        .collect()
}

fn collect_command_evidence(cards: &[AgentMemoryCard]) -> Vec<CommandEvidence> {
    let mut commands = Vec::new();
    for card in cards {
        for line in card.summary.lines() {
            let trimmed = line.trim();
            if starts_like_command(trimmed) {
                commands.push(CommandEvidence {
                    command: truncate_words(trimmed, 20),
                    source_memory_id: card.memory_id.clone(),
                    timestamp: card.timestamp,
                });
            }
        }
    }
    commands.truncate(10);
    commands
}

fn starts_like_command(value: &str) -> bool {
    [
        "cargo ", "npm ", "pnpm ", "yarn ", "git ", "make ", "python ", "python3 ", "uv ",
    ]
    .iter()
    .any(|prefix| value.starts_with(prefix))
}

fn evidence_summary(pack: &ContextPack, memory_count: usize, redaction_count: usize) -> String {
    let mut parts = vec![format!(
        "{} memories selected from Continuum context pack {}",
        memory_count, pack.id
    )];
    if !pack.summary.trim().is_empty() {
        parts.push(truncate_words(&pack.summary, 34));
    }
    if redaction_count > 0 {
        parts.push(format!(
            "{redaction_count} context items were dropped or redacted"
        ));
    }
    parts.join(". ")
}

fn deterministic_response(pack: &AgentContextPack) -> String {
    match pack.mode {
        AgentMode::Ask => format!(
            "{}\n\nEvidence used: {} memory item(s). Confidence {:.2}.",
            pack.evidence_summary,
            pack.relevant_memories.len(),
            pack.confidence
        ),
        AgentMode::Plan => {
            let mut lines = vec![format!(
                "Plan grounded in {} memory item(s):",
                pack.relevant_memories.len()
            )];
            if let Some(project) = pack.current_project.as_ref() {
                lines.push(format!("- Project: {}", project.project));
            }
            for file in pack.files.iter().take(5) {
                lines.push(format!("- Relevant file: {}", file.path));
            }
            for error in pack.errors.iter().take(3) {
                lines.push(format!("- Check blocker: {}", error.summary));
            }
            lines.push("- No actions were executed.".to_string());
            lines.join("\n")
        }
        AgentMode::Act => {
            "Act mode is policy-gated. Continuum built context, but no action runs without approval."
                .to_string()
        }
        AgentMode::Learn => {
            "Learn mode can draft skill/eval candidates from this context, but activation requires user review."
                .to_string()
        }
    }
}

fn proposed_actions_for_mode(pack: &AgentContextPack) -> Vec<ProposedAction> {
    match pack.mode {
        AgentMode::Ask => Vec::new(),
        AgentMode::Plan => vec![ProposedAction {
            label: "Review cited memories before acting".to_string(),
            scope: PermissionScope::ReadMemory,
            risk: RiskLevel::Low,
            requires_approval: false,
        }],
        AgentMode::Act => vec![ProposedAction {
            label: "Request approval before opening files or running commands".to_string(),
            scope: PermissionScope::OpenFile,
            risk: RiskLevel::Medium,
            requires_approval: true,
        }],
        AgentMode::Learn => vec![ProposedAction {
            label: "Draft a skill candidate from this workflow".to_string(),
            scope: PermissionScope::CreateSkill,
            risk: RiskLevel::Medium,
            requires_approval: true,
        }],
    }
}

fn first_non_empty(values: &[&str]) -> String {
    values
        .iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
        .unwrap_or("Continuum memory")
        .to_string()
}

fn evidence_confidence_floor(value: f32) -> f32 {
    if value > 0.0 {
        value
    } else {
        0.35
    }
}

fn truncate_words(value: &str, max_words: usize) -> String {
    let words = value.split_whitespace().take(max_words).collect::<Vec<_>>();
    let truncated = words.join(" ");
    if value.split_whitespace().count() > max_words {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn extract_urls(value: &str) -> Vec<String> {
    value
        .split_whitespace()
        .filter_map(|part| {
            let trimmed = part
                .trim_matches(|ch: char| matches!(ch, '"' | '\'' | ')' | '(' | ',' | '.' | ';'));
            (trimmed.starts_with("http://") || trimmed.starts_with("https://"))
                .then(|| trimmed.to_string())
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentMemoryCard {
    pub memory_id: String,
    pub title: String,
    pub summary: String,
    pub timestamp: i64,
    pub app_name: String,
    pub window_title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub confidence: f32,
    pub match_reason: String,
    pub evidence: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkflowStep {
    pub timestamp: i64,
    pub title: String,
    pub app_name: String,
    pub summary: String,
    pub source_memory_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileRef {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UrlRef {
    pub url: String,
    pub source_memory_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CommandEvidence {
    pub command: String,
    pub source_memory_id: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ErrorEvidence {
    pub summary: String,
    pub error: String,
    pub related_files: Vec<String>,
    pub source_memory_ids: Vec<String>,
}

impl ErrorEvidence {
    fn from_failure(failure: &FailureSummary) -> Self {
        Self {
            summary: failure.summary.clone(),
            error: failure.error.clone(),
            related_files: failure.related_files.clone(),
            source_memory_ids: failure
                .evidence
                .iter()
                .map(|evidence| evidence.source_id.clone())
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DecisionEvidence {
    pub title: String,
    pub summary: String,
    pub source_memory_ids: Vec<String>,
}

impl DecisionEvidence {
    fn from_decision(decision: &DecisionSummary) -> Self {
        Self {
            title: decision.title.clone(),
            summary: decision.summary.clone(),
            source_memory_ids: decision
                .evidence
                .iter()
                .map(|evidence| evidence.source_id.clone())
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TodoEvidence {
    pub title: String,
    pub status: String,
    pub source: String,
}

impl TodoEvidence {
    fn from_task(task: &ContextTask) -> Self {
        Self {
            title: task.title.clone(),
            status: task.status.clone(),
            source: task.source.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PrivacyScope {
    pub local_only: bool,
    pub read_only: bool,
    pub include_raw_evidence: bool,
    pub include_sensitive_context: bool,
    pub exclude_private_apps: bool,
    pub excluded_apps_or_domains: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_minutes: Option<u32>,
    pub incognito_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RedactionNote {
    pub id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenBudget {
    pub requested: u32,
    pub max: u32,
    pub used: u32,
    pub dropped_items: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposedAction {
    pub label: String,
    pub scope: PermissionScope,
    pub risk: RiskLevel,
    pub requires_approval: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_evidence_snippets_unless_raw_is_requested() {
        let evidence = EvidenceRef {
            snippet: (0..80)
                .map(|idx| format!("word{idx}"))
                .collect::<Vec<_>>()
                .join(" "),
            ..Default::default()
        };

        let redacted = redact_evidence(&evidence, false);
        let raw = redact_evidence(&evidence, true);

        assert!(redacted.snippet.split_whitespace().count() <= 37);
        assert_eq!(raw.snippet, evidence.snippet);
    }

    #[test]
    fn normalizes_agent_budget_for_8gb_safe_defaults() {
        assert_eq!(normalize_budget(0), DEFAULT_AGENT_TOKEN_BUDGET);
        assert_eq!(normalize_budget(10_000), MAX_AGENT_TOKEN_BUDGET);
        assert_eq!(normalize_budget(100), 300);
    }
}
