use crate::agent::policy::{AgentMode, RiskLevel};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentActionKind {
    OpenUrl,
    OpenFile,
    RevealInFinder,
    RunReadOnlyCommand,
    CreateDraftNote,
    CreateChecklist,
    RunProjectTest,
    DelegateToHermes,
    DelegateToClaudeCode,
    CreateSkillCandidate,
    CreateEvalCandidate,
    ScheduleAgentJob,
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum AgentActionStatus {
    #[default]
    Proposed,
    NeedsApproval,
    Approved,
    Running,
    Succeeded,
    Failed,
    Blocked,
    Cancelled,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionResult {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAction {
    pub id: String,
    pub run_id: String,
    pub profile_id: Option<String>,
    pub title: String,
    pub description: String,
    pub kind: AgentActionKind,
    pub input: BTreeMap<String, serde_json::Value>,
    pub risk_level: RiskLevel,
    pub required_capabilities: Vec<String>,
    pub status: AgentActionStatus,
    pub created_at: i64,
    pub approved_at: Option<i64>,
    pub executed_at: Option<i64>,
    pub result: Option<ActionResult>,
}

impl Default for AgentAction {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            run_id: String::new(),
            profile_id: None,
            title: String::new(),
            description: String::new(),
            kind: AgentActionKind::Unsupported,
            input: BTreeMap::new(),
            risk_level: RiskLevel::High,
            required_capabilities: Vec::new(),
            status: AgentActionStatus::Proposed,
            created_at: chrono::Utc::now().timestamp(),
            approved_at: None,
            executed_at: None,
            result: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionPolicyDecision {
    pub action_id: String,
    pub kind: AgentActionKind,
    pub allowed: bool,
    pub requires_approval: bool,
    pub reason: String,
    pub blocked_because: Option<String>,
}

pub fn policy_for_action(
    kind: &AgentActionKind,
    risk_level: &RiskLevel,
    mode: &AgentMode,
) -> ActionPolicyDecision {
    let action_id = String::new();

    // Ask mode: block all actions
    if matches!(mode, AgentMode::Ask) {
        return ActionPolicyDecision {
            action_id,
            kind: kind.clone(),
            allowed: false,
            requires_approval: false,
            reason: "Ask mode is read-only".to_string(),
            blocked_because: Some("Ask mode does not allow any actions".to_string()),
        };
    }

    // Plan mode: propose but require approval before execution
    if matches!(mode, AgentMode::Plan) {
        return ActionPolicyDecision {
            action_id,
            kind: kind.clone(),
            allowed: true,
            requires_approval: true,
            reason: "Plan mode proposes actions for user review".to_string(),
            blocked_because: None,
        };
    }

    // Act/Learn mode: check kind and risk
    let (allowed, requires_approval, reason, blocked) = match kind {
        AgentActionKind::OpenUrl => (true, false, "Safe to open URLs".to_string(), None),
        AgentActionKind::OpenFile => (
            true,
            false,
            "Safe to open known project files".to_string(),
            None,
        ),
        AgentActionKind::RevealInFinder => (true, false, "Safe reveal on macOS".to_string(), None),
        AgentActionKind::CreateDraftNote => (true, false, "Local-only note".to_string(), None),
        AgentActionKind::CreateChecklist => (true, false, "Local-only checklist".to_string(), None),
        AgentActionKind::RunReadOnlyCommand => match risk_level {
            RiskLevel::Low => (true, false, "Safe read-only command".to_string(), None),
            RiskLevel::Medium => (
                true,
                true,
                "Medium risk command requires approval".to_string(),
                None,
            ),
            _ => (
                false,
                false,
                "High-risk command blocked".to_string(),
                Some("Blocked by policy: high risk".to_string()),
            ),
        },
        AgentActionKind::RunProjectTest => (
            true,
            true,
            "Test execution requires approval".to_string(),
            None,
        ),
        AgentActionKind::DelegateToHermes => (
            true,
            true,
            "Hermes delegation requires approval".to_string(),
            None,
        ),
        AgentActionKind::DelegateToClaudeCode => (
            true,
            true,
            "Claude Code delegation requires approval".to_string(),
            None,
        ),
        AgentActionKind::CreateSkillCandidate => (
            true,
            false,
            "Skill candidate (no auto-activation)".to_string(),
            None,
        ),
        AgentActionKind::CreateEvalCandidate => (
            true,
            false,
            "Eval candidate (no auto-activation)".to_string(),
            None,
        ),
        AgentActionKind::ScheduleAgentJob => (
            true,
            true,
            "Scheduled job requires approval".to_string(),
            None,
        ),
        AgentActionKind::Unsupported => (
            false,
            false,
            "Action kind not supported".to_string(),
            Some("Unknown action type".to_string()),
        ),
    };

    ActionPolicyDecision {
        action_id,
        kind: kind.clone(),
        allowed,
        requires_approval,
        reason,
        blocked_because: blocked,
    }
}
