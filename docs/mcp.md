# Continuum MCP

Continuum MCP exposes local memory to external agents over MCP while preserving local-first privacy defaults.

## Modes

- `local`: default, loopback use.
- `tunnel`: intended for Cloudflare/ngrok/Tailscale with bearer auth.
- `public`: controlled non-loopback deployment.

Configuration is documented in `README.md`:

- `CONTINUUM_MCP_MODE`
- `CONTINUUM_MCP_REQUIRE_AUTH`
- `CONTINUUM_MCP_ALLOW_LOOPBACK_AUTH_BYPASS`
- `CONTINUUM_MCP_ALLOWED_ORIGINS`
- `CONTINUUM_MCP_PUBLIC_BASE_URL`

## Agent Tools Added In This Slice

- `agent.build_context_pack`: returns the typed `AgentContextPack` for Ask / Plan / Act / Learn.
- `agent.run`: builds a pack, persists an audit record, and returns deterministic local output with policy and blocked action details. It does not execute dangerous actions.
- `agent.privacy_status`: reports MCP/Agent privacy posture, auth mode, raw-evidence defaults, blocklist count, redaction setting, and dangerous-action policy.
- `agent.explain_retrieval`: explains selected memories, qualitative ranking signals, dropped context, redactions, policy reasons, and limitations.
- `agent.rate_result`: logs retrieval feedback (`useful`, `irrelevant`, `wrong`, `stale`, `missing_context`) without mutating ranking.
- `agent.list_prompts`: lists Continuum-specific prompt templates.
- `agent.get_prompt`: returns one prompt template.

Existing memory tools remain available, including `memory.search_full_context`, `memory.get_context_pack`, `memory.agent_brief`, `memory.timeline`, `memory.project_context`, `memory.decisions`, `memory.todos`, and graph/context tools.

## Resources

The MCP server now supports basic read-only resources:

- `continuum://privacy/settings`
- `continuum://todo/open`
- `continuum://decision/recent`

They are discoverable through `resources/list` and readable through `resources/read`.

## Prompts

The MCP server now supports `prompts/list` and `prompts/get` for:

- `resume_work`
- `debug_with_history`
- `write_status_update`
- `prepare_for_meeting`
- `handoff_to_coding_agent`
- `explain_my_thinking`
- `turn_workflow_into_skill`

Each prompt tells the external agent to call Continuum tools first, cite evidence, respect `agent.privacy_status`, explain uncertainty, and avoid actions without approval.

## Security Model

Defaults:

- local-first
- read-only Agent mode
- raw evidence excluded
- sensitive contexts redacted or excluded
- blocklist enforced before agent context exposure
- dangerous actions approval-gated or blocked

Remote/tunnel mode must use bearer auth and strict origin rules. Do not expose MCP publicly without auth.

## Example Tool Calls

```json
{
  "name": "agent.build_context_pack",
  "arguments": {
    "user_goal": "Summarize what I was debugging before lunch",
    "mode": "ask",
    "window_minutes": 240,
    "include_raw_evidence": false
  }
}
```

```json
{
  "name": "agent.run",
  "arguments": {
    "user_goal": "Draft a coding-agent handoff from my recent MCP work",
    "mode": "plan",
    "budget_tokens": 1400
  }
}
```

```json
{
  "name": "agent.explain_retrieval",
  "arguments": {
    "run_id": "agent_run_..."
  }
}
```

```json
{
  "name": "agent.rate_result",
  "arguments": {
    "run_id": "agent_run_...",
    "memory_id": "memory-id",
    "rating": "useful"
  }
}
```
