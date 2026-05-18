use super::audit::{agent_dir, AgentAuditRecord};
use super::policy::RiskLevel;
use serde::{Deserialize, Serialize};
use std::fs::{create_dir_all, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

const SKILL_DRAFTS_FILE: &str = "skill_drafts.jsonl";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentSkillCategory {
    Development,
    Writing,
    Research,
    Admin,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentSkillCandidate {
    #[serde(default)]
    pub draft_id: String,
    pub name: String,
    pub category: AgentSkillCategory,
    pub source: String,
    pub created_from_memories: Vec<String>,
    pub risk_level: RiskLevel,
    pub requires_approval: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_verified: Option<i64>,
    pub when_to_use: String,
    pub required_context: Vec<String>,
    pub procedure: Vec<String>,
    pub verification: Vec<String>,
    pub failure_cases: Vec<String>,
    pub privacy_notes: Vec<String>,
}

impl Default for AgentSkillCategory {
    fn default() -> Self {
        Self::Other
    }
}

pub fn propose_skill_from_audit(record: &AgentAuditRecord) -> Result<AgentSkillCandidate, String> {
    if record.memories_used.is_empty() {
        return Err("Not enough cited memory context to propose a reusable skill.".to_string());
    }
    if record.user_goal.split_whitespace().count() < 3 {
        return Err("The run goal is too short to identify a repeatable workflow.".to_string());
    }

    let category = infer_category(&record.user_goal, &record.output_summary);
    let risk_level = if record
        .tools_blocked
        .iter()
        .any(|tool| matches!(tool.risk, RiskLevel::High | RiskLevel::Blocked))
    {
        RiskLevel::High
    } else if record.approvals_required.is_empty() {
        RiskLevel::Low
    } else {
        RiskLevel::Medium
    };
    let name = format!("fndr-{}", slugify(&record.user_goal));
    let evidence = record
        .selected_memories
        .iter()
        .take(5)
        .map(|memory| format!("{} ({})", memory.title, memory.memory_id))
        .collect::<Vec<_>>();

    Ok(AgentSkillCandidate {
        draft_id: format!("skill_draft_{}", uuid::Uuid::new_v4().simple()),
        name,
        category,
        source: "fndr-observed-workflow".to_string(),
        created_from_memories: record.memories_used.clone(),
        risk_level,
        requires_approval: true,
        last_verified: None,
        when_to_use: format!(
            "Use when the user asks for a similar workflow to: {}",
            record.user_goal
        ),
        required_context: vec![
            "Recent FNDR AgentContextPack for the task".to_string(),
            "Cited memories and retrieval explanation".to_string(),
            "Current privacy status before using context".to_string(),
        ],
        procedure: vec![
            "Build an AgentContextPack in Ask or Plan mode.".to_string(),
            "Review the memories used and dropped/redacted context.".to_string(),
            "Summarize the relevant evidence before suggesting actions.".to_string(),
            "Ask for approval before any Act-mode tool use.".to_string(),
        ],
        verification: if evidence.is_empty() {
            vec!["Verify the answer cites at least one FNDR memory.".to_string()]
        } else {
            evidence
        },
        failure_cases: vec![
            "Insufficient memory evidence for the current task.".to_string(),
            "Sensitive or blocklisted context is required but not approved.".to_string(),
            "The workflow needs mutating actions that have not been approved.".to_string(),
        ],
        privacy_notes: vec![
            "Do not include raw screenshots.".to_string(),
            "Use redacted evidence by default.".to_string(),
            "Do not activate this skill without user review.".to_string(),
        ],
    })
}

pub fn append_skill_draft(app_data_dir: &Path, draft: &AgentSkillCandidate) -> Result<(), String> {
    let dir = agent_dir(app_data_dir);
    create_dir_all(&dir).map_err(|err| err.to_string())?;
    let path = dir.join(SKILL_DRAFTS_FILE);
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| err.to_string())?;
    let line = serde_json::to_string(draft).map_err(|err| err.to_string())?;
    file.write_all(line.as_bytes())
        .map_err(|err| err.to_string())?;
    file.write_all(b"\n").map_err(|err| err.to_string())?;
    Ok(())
}

pub fn list_skill_drafts(
    app_data_dir: &Path,
    limit: usize,
) -> Result<Vec<AgentSkillCandidate>, String> {
    let path = agent_dir(app_data_dir).join(SKILL_DRAFTS_FILE);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = File::open(path).map_err(|err| err.to_string())?;
    let reader = BufReader::new(file);
    let mut rows = Vec::new();
    for line in reader.lines() {
        let line = line.map_err(|err| err.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        rows.push(
            serde_json::from_str::<AgentSkillCandidate>(&line).map_err(|err| err.to_string())?,
        );
    }
    rows.reverse();
    if limit > 0 && rows.len() > limit {
        rows.truncate(limit);
    }
    Ok(rows)
}

fn infer_category(goal: &str, output: &str) -> AgentSkillCategory {
    let text = format!("{goal} {output}").to_lowercase();
    if ["debug", "code", "tauri", "cargo", "npm", "test", "repo"]
        .iter()
        .any(|term| text.contains(term))
    {
        AgentSkillCategory::Development
    } else if ["write", "draft", "status", "update"]
        .iter()
        .any(|term| text.contains(term))
    {
        AgentSkillCategory::Writing
    } else if ["research", "find", "compare"]
        .iter()
        .any(|term| text.contains(term))
    {
        AgentSkillCategory::Research
    } else {
        AgentSkillCategory::Other
    }
}

fn slugify(value: &str) -> String {
    let slug = value
        .to_lowercase()
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .take(8)
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        "agent-workflow".to_string()
    } else {
        slug
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::audit::{AgentRunStatus, MemoryRetrievalExplanation};
    use crate::agent::policy::AgentMode;

    #[test]
    fn proposes_skill_from_repeatable_audit_run() {
        let record = AgentAuditRecord {
            run_id: "run-1".to_string(),
            user_goal: "Find the last Tauri dev port fix".to_string(),
            mode: AgentMode::Plan,
            memories_used: vec!["mem-1".to_string()],
            selected_memories: vec![MemoryRetrievalExplanation {
                memory_id: "mem-1".to_string(),
                title: "Fixed Tauri dev port issue".to_string(),
                ..Default::default()
            }],
            result_status: AgentRunStatus::Success,
            output_summary: "Plan checked cargo and npm commands.".to_string(),
            ..Default::default()
        };

        let draft = propose_skill_from_audit(&record).expect("skill draft");

        assert!(draft.name.contains("tauri"));
        assert_eq!(draft.source, "fndr-observed-workflow");
        assert!(draft.requires_approval);
    }
}
