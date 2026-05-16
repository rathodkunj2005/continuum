use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AgentMode {
    #[default]
    Ask,
    Plan,
    Act,
    Learn,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PermissionScope {
    ReadMemory,
    ReadProjectMemory,
    ReadRecentContext,
    BuildContextPack,
    WriteAgentNote,
    OpenFile,
    ReadFile,
    WriteFile,
    RunReadonlyCommand,
    RunMutatingCommand,
    NetworkAccess,
    SendExternalMessage,
    CreateSkill,
    UpdateSkill,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    #[default]
    Low,
    Medium,
    High,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolPolicy {
    pub tool: String,
    pub scope: PermissionScope,
    pub risk: RiskLevel,
    pub allowed: bool,
    pub requires_approval: bool,
    pub reason: String,
}

pub fn policy_for_mode(mode: AgentMode) -> Vec<ToolPolicy> {
    match mode {
        AgentMode::Ask => vec![
            allow("search_memories", PermissionScope::ReadMemory, RiskLevel::Low),
            allow(
                "build_agent_context_pack",
                PermissionScope::BuildContextPack,
                RiskLevel::Low,
            ),
            deny(
                "write_file",
                PermissionScope::WriteFile,
                RiskLevel::High,
                "Ask mode is read-only.",
            ),
            deny(
                "run_mutating_command",
                PermissionScope::RunMutatingCommand,
                RiskLevel::High,
                "Ask mode cannot execute mutating commands.",
            ),
            deny(
                "send_external_message",
                PermissionScope::SendExternalMessage,
                RiskLevel::High,
                "Ask mode cannot send external messages.",
            ),
        ],
        AgentMode::Plan => vec![
            allow("search_memories", PermissionScope::ReadMemory, RiskLevel::Low),
            allow(
                "read_project_context",
                PermissionScope::ReadProjectMemory,
                RiskLevel::Low,
            ),
            allow(
                "build_agent_context_pack",
                PermissionScope::BuildContextPack,
                RiskLevel::Low,
            ),
            deny(
                "write_file",
                PermissionScope::WriteFile,
                RiskLevel::High,
                "Plan mode can suggest edits but cannot write files.",
            ),
            deny(
                "run_mutating_command",
                PermissionScope::RunMutatingCommand,
                RiskLevel::High,
                "Plan mode can suggest commands but cannot run them.",
            ),
        ],
        AgentMode::Act => vec![
            allow("search_memories", PermissionScope::ReadMemory, RiskLevel::Low),
            allow(
                "build_agent_context_pack",
                PermissionScope::BuildContextPack,
                RiskLevel::Low,
            ),
            approve(
                "open_file",
                PermissionScope::OpenFile,
                RiskLevel::Medium,
                "Opening files requires a user-visible action request.",
            ),
            approve(
                "run_readonly_command",
                PermissionScope::RunReadonlyCommand,
                RiskLevel::Medium,
                "Read-only shell commands require approval before execution.",
            ),
            approve(
                "write_file",
                PermissionScope::WriteFile,
                RiskLevel::High,
                "Writing files requires explicit approval and a precise diff.",
            ),
            deny(
                "credential_access",
                PermissionScope::ReadFile,
                RiskLevel::Blocked,
                "Credential access is blocked unless a future scoped approval flow allows it.",
            ),
        ],
        AgentMode::Learn => vec![
            allow("search_memories", PermissionScope::ReadMemory, RiskLevel::Low),
            allow(
                "build_agent_context_pack",
                PermissionScope::BuildContextPack,
                RiskLevel::Low,
            ),
            approve(
                "create_skill",
                PermissionScope::CreateSkill,
                RiskLevel::Medium,
                "Skill candidates require user review before becoming active.",
            ),
            approve(
                "update_skill",
                PermissionScope::UpdateSkill,
                RiskLevel::Medium,
                "Skill updates require user review before activation.",
            ),
            deny(
                "run_mutating_command",
                PermissionScope::RunMutatingCommand,
                RiskLevel::High,
                "Learn mode records patterns; it does not execute actions.",
            ),
        ],
    }
}

fn allow(tool: &str, scope: PermissionScope, risk: RiskLevel) -> ToolPolicy {
    ToolPolicy {
        tool: tool.to_string(),
        scope,
        risk,
        allowed: true,
        requires_approval: false,
        reason: "Allowed by this mode's default read-only policy.".to_string(),
    }
}

fn approve(tool: &str, scope: PermissionScope, risk: RiskLevel, reason: &str) -> ToolPolicy {
    ToolPolicy {
        tool: tool.to_string(),
        scope,
        risk,
        allowed: true,
        requires_approval: true,
        reason: reason.to_string(),
    }
}

fn deny(tool: &str, scope: PermissionScope, risk: RiskLevel, reason: &str) -> ToolPolicy {
    ToolPolicy {
        tool: tool.to_string(),
        scope,
        risk,
        allowed: false,
        requires_approval: true,
        reason: reason.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ask_mode_is_read_only_by_default() {
        let policies = policy_for_mode(AgentMode::Ask);

        assert!(policies.iter().any(|policy| {
            policy.scope == PermissionScope::ReadMemory
                && policy.allowed
                && !policy.requires_approval
        }));
        assert!(policies.iter().any(|policy| {
            policy.scope == PermissionScope::WriteFile
                && !policy.allowed
                && policy.risk == RiskLevel::High
        }));
    }
}
