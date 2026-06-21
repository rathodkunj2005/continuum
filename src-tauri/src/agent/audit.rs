use super::context::{AgentContextPack, AgentContextRequest, RedactionNote};
use super::policy::{AgentMode, RiskLevel, ToolPolicy};
use serde::{Deserialize, Serialize};
use std::fs::{create_dir_all, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const AGENT_DIR: &str = "agent";
const AUDIT_FILE: &str = "audit_runs.jsonl";
const FEEDBACK_FILE: &str = "retrieval_feedback.jsonl";

static AGENT_AUDIT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn audit_lock() -> &'static Mutex<()> {
    AGENT_AUDIT_LOCK.get_or_init(|| Mutex::new(()))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunStatus {
    #[default]
    Success,
    Partial,
    Blocked,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalFeedbackRating {
    Useful,
    Irrelevant,
    Wrong,
    Stale,
    MissingContext,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryRetrievalExplanation {
    pub memory_id: String,
    pub title: String,
    pub matched_reason: String,
    pub app_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub timestamp: i64,
    pub confidence: f32,
    pub semantic_relevance: String,
    pub keyword_match: String,
    pub recency: String,
    pub project_match: String,
    pub app_domain_match: String,
    pub workflow_continuity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentAuditRecord {
    pub run_id: String,
    pub created_at: i64,
    pub user_goal: String,
    pub mode: AgentMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_pack_id: Option<String>,
    pub memories_used: Vec<String>,
    pub tools_requested: Vec<String>,
    pub tools_allowed: Vec<ToolPolicy>,
    pub tools_blocked: Vec<ToolPolicy>,
    pub approvals_required: Vec<ToolPolicy>,
    pub redactions_applied: Vec<RedactionNote>,
    pub dropped_context: Vec<RedactionNote>,
    pub confidence: f32,
    pub output_summary: String,
    pub result_status: AgentRunStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    pub selected_memories: Vec<MemoryRetrievalExplanation>,
    #[serde(default)]
    pub feedback: Vec<AgentRetrievalFeedback>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRetrievalFeedback {
    pub feedback_id: String,
    pub run_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_id: Option<String>,
    pub rating: RetrievalFeedbackRating,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RetrievalExplanation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_pack_id: Option<String>,
    pub selected_memories: Vec<MemoryRetrievalExplanation>,
    pub dropped_context: Vec<RedactionNote>,
    pub redacted_context: Vec<RedactionNote>,
    pub privacy_policy_reasons: Vec<String>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExplainRetrievalRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_pack_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateResultRequest {
    pub run_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_id: Option<String>,
    pub rating: RetrievalFeedbackRating,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

pub fn agent_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(AGENT_DIR)
}

pub fn append_agent_audit_record(
    app_data_dir: &Path,
    record: &AgentAuditRecord,
) -> Result<(), String> {
    append_jsonl(app_data_dir, AUDIT_FILE, record)
}

pub fn list_agent_audit_runs(
    app_data_dir: &Path,
    limit: usize,
    mode: Option<AgentMode>,
    status: Option<AgentRunStatus>,
) -> Result<Vec<AgentAuditRecord>, String> {
    let feedback = list_feedback(app_data_dir, 0)?;
    let mut rows = read_jsonl::<AgentAuditRecord>(app_data_dir, AUDIT_FILE)?;
    rows.retain(|row| {
        mode.is_none_or(|mode| row.mode == mode)
            && status
                .as_ref()
                .is_none_or(|status| &row.result_status == status)
    });
    rows.sort_by_key(|row| std::cmp::Reverse(row.created_at));
    if limit > 0 && rows.len() > limit {
        rows.truncate(limit);
    }
    attach_feedback(&mut rows, &feedback);
    Ok(rows)
}

pub fn get_agent_audit_run(
    app_data_dir: &Path,
    run_id: &str,
) -> Result<Option<AgentAuditRecord>, String> {
    let feedback = list_feedback(app_data_dir, 0)?;
    let mut rows = read_jsonl::<AgentAuditRecord>(app_data_dir, AUDIT_FILE)?;
    attach_feedback(&mut rows, &feedback);
    Ok(rows.into_iter().rev().find(|row| row.run_id == run_id))
}

pub fn append_feedback(
    app_data_dir: &Path,
    request: RateResultRequest,
) -> Result<AgentRetrievalFeedback, String> {
    let feedback = AgentRetrievalFeedback {
        feedback_id: format!("agent_feedback_{}", uuid::Uuid::new_v4().simple()),
        run_id: request.run_id,
        memory_id: request.memory_id,
        rating: request.rating,
        note: request.note.map(|note| truncate_for_audit(&note, 800)),
        created_at: chrono::Utc::now().timestamp_millis(),
    };
    append_jsonl(app_data_dir, FEEDBACK_FILE, &feedback)?;
    Ok(feedback)
}

pub fn list_feedback(
    app_data_dir: &Path,
    limit: usize,
) -> Result<Vec<AgentRetrievalFeedback>, String> {
    let mut rows = read_jsonl::<AgentRetrievalFeedback>(app_data_dir, FEEDBACK_FILE)?;
    rows.sort_by_key(|row| std::cmp::Reverse(row.created_at));
    if limit > 0 && rows.len() > limit {
        rows.truncate(limit);
    }
    Ok(rows)
}

pub fn audit_record_from_success(
    run_id: &str,
    request: &AgentContextRequest,
    pack: &AgentContextPack,
    blocked_tools: &[ToolPolicy],
    output_summary: &str,
) -> AgentAuditRecord {
    let approvals_required = pack
        .allowed_tools
        .iter()
        .filter(|policy| policy.requires_approval && policy.allowed)
        .cloned()
        .collect::<Vec<_>>();
    let redactions = pack
        .disallowed_context
        .iter()
        .filter(|note| note.reason.contains("privacy") || note.reason.contains("redact"))
        .cloned()
        .collect::<Vec<_>>();
    let dropped = pack
        .disallowed_context
        .iter()
        .filter(|note| !note.reason.contains("privacy") && !note.reason.contains("redact"))
        .cloned()
        .collect::<Vec<_>>();

    AgentAuditRecord {
        run_id: run_id.to_string(),
        created_at: chrono::Utc::now().timestamp_millis(),
        user_goal: request.user_goal.clone(),
        mode: request.mode,
        context_pack_id: Some(pack.source_context_pack_id.clone()),
        memories_used: pack
            .relevant_memories
            .iter()
            .map(|memory| memory.memory_id.clone())
            .collect(),
        tools_requested: vec!["build_agent_context_pack".to_string()],
        tools_allowed: pack
            .allowed_tools
            .iter()
            .filter(|policy| policy.allowed)
            .cloned()
            .collect(),
        tools_blocked: blocked_tools.to_vec(),
        approvals_required,
        redactions_applied: redactions,
        dropped_context: dropped,
        confidence: pack.confidence,
        output_summary: truncate_for_audit(output_summary, 1200),
        result_status: if blocked_tools
            .iter()
            .any(|tool| tool.risk == RiskLevel::Blocked)
        {
            AgentRunStatus::Partial
        } else {
            AgentRunStatus::Success
        },
        error_message: None,
        selected_memories: pack
            .relevant_memories
            .iter()
            .map(|memory| MemoryRetrievalExplanation {
                memory_id: memory.memory_id.clone(),
                title: memory.title.clone(),
                matched_reason: memory.match_reason.clone(),
                app_name: memory.app_name.clone(),
                url: memory.url.clone(),
                timestamp: memory.timestamp,
                confidence: memory.confidence,
                semantic_relevance: "Used existing Continuum context-runtime retrieval; exact semantic score is not exposed yet.".to_string(),
                keyword_match: qualitative_keyword_signal(&request.user_goal, &memory.summary),
                recency: qualitative_recency(memory.timestamp),
                project_match: pack
                    .current_project
                    .as_ref()
                    .map(|project| format!("Project context available: {}", project.project))
                    .unwrap_or_else(|| "No explicit project match exposed.".to_string()),
                app_domain_match: memory
                    .url
                    .as_ref()
                    .map(|url| format!("URL evidence available: {}", truncate_for_audit(url, 120)))
                    .unwrap_or_else(|| format!("App evidence available: {}", memory.app_name)),
                workflow_continuity: if pack
                    .recent_workflow_trace
                    .iter()
                    .any(|step| step.source_memory_id == memory.memory_id)
                {
                    "Included in the recent workflow trace.".to_string()
                } else {
                    "No workflow-continuity score exposed.".to_string()
                },
            })
            .collect(),
        feedback: Vec::new(),
    }
}

pub fn audit_record_from_failure(
    run_id: &str,
    request: &AgentContextRequest,
    error: &str,
) -> AgentAuditRecord {
    AgentAuditRecord {
        run_id: run_id.to_string(),
        created_at: chrono::Utc::now().timestamp_millis(),
        user_goal: request.user_goal.clone(),
        mode: request.mode,
        result_status: AgentRunStatus::Failed,
        error_message: Some(truncate_for_audit(error, 1000)),
        output_summary: "Agent run failed before a context pack could be returned.".to_string(),
        ..Default::default()
    }
}

pub fn explanation_from_audit(record: &AgentAuditRecord) -> RetrievalExplanation {
    RetrievalExplanation {
        run_id: Some(record.run_id.clone()),
        context_pack_id: record.context_pack_id.clone(),
        selected_memories: record.selected_memories.clone(),
        dropped_context: record.dropped_context.clone(),
        redacted_context: record.redactions_applied.clone(),
        privacy_policy_reasons: vec![
            "Continuum Agent is local-first and read-only by default.".to_string(),
            "Raw screenshots are never written to agent audit logs.".to_string(),
            "Blocklisted and sensitive contexts are excluded before agent context exposure."
                .to_string(),
        ],
        limitations: vec![
            "Exact semantic, keyword, graph, and fusion scores are not exposed by the current context runtime.".to_string(),
            "Ranking-signal fields are qualitative unless Continuum storage exposes a concrete score.".to_string(),
        ],
    }
}

fn attach_feedback(records: &mut [AgentAuditRecord], feedback: &[AgentRetrievalFeedback]) {
    for record in records {
        record.feedback = feedback
            .iter()
            .filter(|item| item.run_id == record.run_id)
            .cloned()
            .collect();
    }
}

fn append_jsonl<T: Serialize>(app_data_dir: &Path, file_name: &str, row: &T) -> Result<(), String> {
    let _guard = audit_lock()
        .lock()
        .map_err(|_| "agent audit mutex poisoned".to_string())?;
    let dir = agent_dir(app_data_dir);
    create_dir_all(&dir).map_err(|err| err.to_string())?;
    let path = dir.join(file_name);
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|err| err.to_string())?;
    let line = serde_json::to_string(row).map_err(|err| err.to_string())?;
    file.write_all(line.as_bytes())
        .map_err(|err| err.to_string())?;
    file.write_all(b"\n").map_err(|err| err.to_string())?;
    file.flush().map_err(|err| err.to_string())?;
    Ok(())
}

fn read_jsonl<T: for<'de> Deserialize<'de>>(
    app_data_dir: &Path,
    file_name: &str,
) -> Result<Vec<T>, String> {
    let path = agent_dir(app_data_dir).join(file_name);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = File::open(path).map_err(|err| err.to_string())?;
    let reader = BufReader::new(file);
    let mut rows = Vec::new();
    for line in reader.lines() {
        let line = line.map_err(|err| err.to_string())?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        rows.push(serde_json::from_str::<T>(trimmed).map_err(|err| err.to_string())?);
    }
    Ok(rows)
}

// ── Action ledger ─────────────────────────────────────────────────────────────

const ACTIONS_FILE: &str = "actions.jsonl";

pub fn append_agent_action(
    app_data_dir: &Path,
    action: &crate::agent::actions::AgentAction,
) -> Result<(), String> {
    append_jsonl(app_data_dir, ACTIONS_FILE, action)
}

pub fn get_agent_action_by_id(
    app_data_dir: &Path,
    action_id: &str,
) -> Result<Option<crate::agent::actions::AgentAction>, String> {
    let records = read_jsonl::<crate::agent::actions::AgentAction>(app_data_dir, ACTIONS_FILE)?;
    // Return the last record matching action_id (in case of updates re-appended)
    Ok(records.into_iter().filter(|r| r.id == action_id).next_back())
}

pub fn list_actions_for_run(
    app_data_dir: &Path,
    run_id: &str,
) -> Result<Vec<crate::agent::actions::AgentAction>, String> {
    let records = read_jsonl::<crate::agent::actions::AgentAction>(app_data_dir, ACTIONS_FILE)?;
    // Deduplicate: keep last record per action id for this run
    let mut seen: std::collections::HashMap<String, crate::agent::actions::AgentAction> =
        std::collections::HashMap::new();
    for r in records.into_iter().filter(|r| r.run_id == run_id) {
        seen.insert(r.id.clone(), r);
    }
    let mut result: Vec<_> = seen.into_values().collect();
    result.sort_by_key(|a| a.created_at);
    Ok(result)
}

pub fn update_action_status(
    app_data_dir: &Path,
    action_id: &str,
    new_status: crate::agent::actions::AgentActionStatus,
    approved_at: Option<i64>,
    executed_at: Option<i64>,
    result: Option<crate::agent::actions::ActionResult>,
) -> Result<crate::agent::actions::AgentAction, String> {
    // Load existing action
    let mut action = get_agent_action_by_id(app_data_dir, action_id)?
        .ok_or_else(|| format!("Action not found: {}", action_id))?;

    // Apply updates
    action.status = new_status;
    if let Some(ts) = approved_at {
        action.approved_at = Some(ts);
    }
    if let Some(ts) = executed_at {
        action.executed_at = Some(ts);
    }
    if let Some(r) = result {
        action.result = Some(r);
    }

    // Append updated record (the reader uses `.last()` so this acts as an update)
    append_agent_action(app_data_dir, &action)?;
    Ok(action)
}

fn qualitative_keyword_signal(query: &str, summary: &str) -> String {
    let query_terms = query
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|term| term.len() > 2)
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    let summary = summary.to_lowercase();
    let matches = query_terms
        .iter()
        .filter(|term| summary.contains(term.as_str()))
        .take(8)
        .cloned()
        .collect::<Vec<_>>();
    if matches.is_empty() {
        "No exact query-term match exposed; selected by semantic/context signals.".to_string()
    } else {
        format!("Matched query terms: {}", matches.join(", "))
    }
}

fn qualitative_recency(timestamp: i64) -> String {
    let age_ms = chrono::Utc::now()
        .timestamp_millis()
        .saturating_sub(timestamp);
    let hours = age_ms / 3_600_000;
    if hours < 1 {
        "Seen within the last hour.".to_string()
    } else if hours < 24 {
        format!("Seen about {hours} hour(s) ago.")
    } else {
        format!("Seen about {} day(s) ago.", hours / 24)
    }
}

fn truncate_for_audit(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut truncated = value.chars().take(max_chars).collect::<String>();
    truncated.push_str("...");
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_agent_dir(label: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "continuum-agent-audit-{label}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        create_dir_all(&base).expect("create temp dir");
        base
    }

    #[test]
    fn audit_records_round_trip_as_jsonl() {
        let dir = temp_agent_dir("round-trip");
        let record = AgentAuditRecord {
            run_id: "run-1".to_string(),
            user_goal: "Explain recent context".to_string(),
            mode: AgentMode::Ask,
            result_status: AgentRunStatus::Success,
            ..Default::default()
        };

        append_agent_audit_record(&dir, &record).expect("append audit");
        let rows = list_agent_audit_runs(&dir, 10, Some(AgentMode::Ask), None).expect("list audit");

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].run_id, "run-1");
    }

    #[test]
    fn feedback_attaches_to_run_detail() {
        let dir = temp_agent_dir("feedback");
        append_agent_audit_record(
            &dir,
            &AgentAuditRecord {
                run_id: "run-feedback".to_string(),
                mode: AgentMode::Plan,
                result_status: AgentRunStatus::Success,
                ..Default::default()
            },
        )
        .expect("append audit");
        append_feedback(
            &dir,
            RateResultRequest {
                run_id: "run-feedback".to_string(),
                memory_id: Some("mem-1".to_string()),
                rating: RetrievalFeedbackRating::Useful,
                note: Some("good context".to_string()),
            },
        )
        .expect("append feedback");

        let record = get_agent_audit_run(&dir, "run-feedback")
            .expect("get audit")
            .expect("record exists");

        assert_eq!(record.feedback.len(), 1);
        assert_eq!(record.feedback[0].memory_id.as_deref(), Some("mem-1"));
    }
}
