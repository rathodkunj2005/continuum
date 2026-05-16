# Agent Types Reference

## AgentMode

An enum that defines the operational mode of the agent. Controls the level of autonomy and safety gates.

- **Ask** (default): Read-only mode for answering questions about context. Cannot write files, run commands, or send messages.
- **Plan**: Read-only mode for planning actions and suggesting edits. Can read memories and project context but cannot write files or run commands.
- **Act**: Interactive mode where the agent can open files and run commands with approval. Requires explicit approval for write_file, open_file, and run_readonly_command.
- **Learn**: Pattern recognition mode for creating or updating skills. Can draft skill/eval candidates that require user review before activation.

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum AgentMode {
    #[default]
    Ask,
    Plan,
    Act,
    Learn,
}
```

## RiskLevel

An enum that classifies the risk associated with a tool or action.

- **Low** (default): Safe operations, typically read-only actions (e.g., reading memory, building context pack).
- **Medium**: Moderate risk operations that require user awareness (e.g., opening files, running read-only commands, creating skills).
- **High**: High-risk operations that should typically be blocked in certain modes (e.g., writing files, running mutating commands, sending external messages).
- **Blocked**: Permanently blocked operations regardless of mode (e.g., credential access in Act mode).

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum RiskLevel {
    #[default]
    Low,
    Medium,
    High,
    Blocked,
}
```

## PermissionScope

An enum that categorizes the types of operations the agent can perform.

Variants:
- ReadMemory
- ReadProjectMemory
- ReadRecentContext
- BuildContextPack
- WriteAgentNote
- OpenFile
- ReadFile
- WriteFile
- RunReadonlyCommand
- RunMutatingCommand
- NetworkAccess
- SendExternalMessage
- CreateSkill
- UpdateSkill

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
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
```

## ToolPolicy

A struct that defines the policy for a single tool or operation. Controls what tools are allowed, what approval is required, and why.

Fields:
- **tool** (String): The name of the tool being governed (e.g., "write_file", "run_mutating_command").
- **scope** (PermissionScope): The category of operation this tool performs.
- **risk** (RiskLevel): The risk level associated with this tool.
- **allowed** (bool): Whether the tool is allowed in the current mode.
- **requires_approval** (bool): Whether the tool requires explicit user approval before execution.
- **reason** (String): Explanation of why this policy is in place (e.g., "Ask mode is read-only").

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolPolicy {
    pub tool: String,
    pub scope: PermissionScope,
    pub risk: RiskLevel,
    pub allowed: bool,
    pub requires_approval: bool,
    pub reason: String,
}
```

## AgentContextRequest

A struct that specifies what context the agent should retrieve and how to build the context pack.

Fields:
- **user_goal** (String): The user's question or request for the agent.
- **mode** (AgentMode): The operational mode (Ask, Plan, Act, or Learn). Defaults to Ask.
- **project** (Option<String>): Optional project name to filter context to a specific project.
- **app** (Option<String>): Optional app name (accepted but only applied through retrieval ranking).
- **domain** (Option<String>): Optional domain name (accepted but only applied through retrieval ranking).
- **window_minutes** (Option<u32>): Optional time window in minutes to limit context to recent activity.
- **selected_memory_ids** (Vec<String>): List of specific memory IDs to include (empty means all).
- **include_raw_evidence** (bool): Whether to include full evidence snippets (false truncates to 36 words).
- **budget_tokens** (u32): Token budget for the context pack (normalized to 900–4000 tokens).

## AgentContextPack

The complete context provided to the agent for decision-making. Contains memories, policies, and evidence.

Fields:
- **task_id** (String): Unique identifier for this context pack.
- **user_goal** (String): The user's question/request being answered.
- **mode** (AgentMode): The agent's operational mode.
- **relevant_memories** (Vec<AgentMemoryCard>): The selected memories that matched the query.
- **current_project** (Option<ProjectContext>): The project context (goals, files, constraints).
- **recent_workflow_trace** (Vec<WorkflowStep>): A timeline of recent actions relevant to the goal.
- **entities** (Vec<EntityRef>): Named entities (topics, names) extracted from memories.
- **files** (Vec<FileRef>): Relevant files mentioned in the context.
- **urls** (Vec<UrlRef>): URLs referenced in the memories.
- **commands** (Vec<CommandEvidence>): Shell/build commands found in recent activity.
- **errors** (Vec<ErrorEvidence>): Known failures and error messages.
- **decisions** (Vec<DecisionEvidence>): Recent architectural or project decisions.
- **todos** (Vec<TodoEvidence>): Open tasks and issues.
- **privacy_scope** (PrivacyScope): Privacy constraints and settings.
- **allowed_tools** (Vec<ToolPolicy>): Tools available in this mode with their policies.
- **disallowed_context** (Vec<RedactionNote>): Context items excluded or redacted with reasons.
- **token_budget** (TokenBudget): Requested, used, and remaining token budget information.
- **confidence** (f32): Confidence score for the retrieved context (0.0–1.0).
- **evidence_summary** (String): Human-readable summary of what memories were selected and why.
- **source_context_pack_id** (String): The ID of the underlying ContextPack this was built from.

## AgentRunResponse

The final response returned from an agent run. Contains the decision, context, and audit information.

Fields:
- **run_id** (String): Unique identifier for this agent run.
- **mode** (AgentMode): The mode that was used for this run.
- **answer** (String): The agent's response/decision.
- **context_pack** (AgentContextPack): The context used to formulate the answer.
- **proposed_actions** (Vec<ProposedAction>): Actions the agent suggests the user take.
- **blocked_actions** (Vec<ToolPolicy>): Tools that were requested but blocked.
- **audit_warning** (Option<String>): Optional warning if the audit record failed to write.

## AgentAuditRecord

A record of an agent run persisted to disk in JSONL format for transparency and feedback.

Fields:
- **run_id** (String): Unique identifier for this run.
- **created_at** (i64): Timestamp (milliseconds since epoch) when the run occurred.
- **user_goal** (String): The user's question/request.
- **mode** (AgentMode): The operational mode used.
- **context_pack_id** (Option<String>): ID of the context pack used.
- **memories_used** (Vec<String>): IDs of the memories selected.
- **tools_requested** (Vec<String>): Tools the agent attempted to use.
- **tools_allowed** (Vec<ToolPolicy>): Tools that were allowed.
- **tools_blocked** (Vec<ToolPolicy>): Tools that were blocked or denied.
- **approvals_required** (Vec<ToolPolicy>): Tools requiring approval that were allowed.
- **redactions_applied** (Vec<RedactionNote>): Context redacted for privacy.
- **dropped_context** (Vec<RedactionNote>): Context dropped due to limits or filters.
- **confidence** (f32): Confidence in the retrieved context.
- **output_summary** (String): Summary of the agent's response (truncated to 1200 chars).
- **result_status** (AgentRunStatus): Success, Partial, Blocked, or Failed.
- **error_message** (Option<String>): Error message if the run failed.
- **selected_memories** (Vec<MemoryRetrievalExplanation>): Detailed explanation of why each memory was selected.
- **feedback** (Vec<AgentRetrievalFeedback>): User feedback on the quality of retrieved memories.

## AgentRunStatus

An enum tracking the outcome of an agent run.

- **Success** (default): The agent ran successfully and produced a valid response.
- **Partial**: The agent ran but some tools or context were blocked.
- **Blocked**: The agent run was blocked, typically due to high-risk operations.
- **Failed**: The agent run encountered an error and could not complete.

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum AgentRunStatus {
    #[default]
    Success,
    Partial,
    Blocked,
    Failed,
}
```

## Supporting Structures

### AgentMemoryCard

A single memory with title, summary, and evidence links.

Fields:
- **memory_id** (String): ID of the memory.
- **title** (String): Display title for the memory.
- **summary** (String): Longer summary text.
- **timestamp** (i64): When the memory was recorded.
- **app_name** (String): The application associated with the memory.
- **window_title** (String): Browser or window title if applicable.
- **url** (Option<String>): URL if the memory came from a browser.
- **confidence** (f32): Confidence score for this memory.
- **match_reason** (String): Explanation of why this memory was selected.
- **evidence** (Vec<EvidenceRef>): Supporting evidence snippets.

### PrivacyScope

Controls what context can be exposed to the agent.

Fields:
- **local_only** (bool): Data is local, not sent to external services.
- **read_only** (bool): Agent cannot write files or mutate system state.
- **include_raw_evidence** (bool): Whether to include untruncated evidence.
- **include_sensitive_context** (bool): Whether sensitive/private context can be shown.
- **exclude_private_apps** (bool): Blocklist private applications.
- **excluded_apps_or_domains** (Vec<String>): List of apps/domains to exclude.
- **project** (Option<String>): Scoped to a specific project if set.
- **window_minutes** (Option<u32>): Time window for recent context.
- **incognito_active** (bool): Whether incognito mode is active.

### TokenBudget

Tracks context token usage.

Fields:
- **requested** (u32): Tokens requested by the user.
- **max** (u32): Maximum tokens allowed (4000).
- **used** (u32): Tokens actually used.
- **dropped_items** (u32): Number of context items dropped due to budget.

### RedactionNote

Tracks why a piece of context was excluded or redacted.

Fields:
- **id** (String): ID of the excluded item.
- **reason** (String): Why it was excluded (e.g., "privacy", "budget limit").

## Module Exports

The agent module (mod.rs) exports these public items:

- `AgentMode`
- `RiskLevel`
- `PermissionScope`
- `ToolPolicy`
- `AgentContextRequest`
- `AgentContextPack`
- `AgentRunResponse`
- `AgentAuditRecord`
- `AgentEvalCase`
- `AgentPrompt`
- `AgentSkillCandidate`
- `policy_for_mode()` function
- `build_agent_context_pack()` function
- `get_agent_prompt()` function
- `list_agent_prompts()` function
