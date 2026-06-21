# Continuum Agent Context Pack

`AgentContextPack` is the typed boundary between Continuum memory and any agent runtime. It is built from the existing `context_runtime::build_context_pack` path, then narrowed for agent use.

## Schema

Backend types live in `src-tauri/src/agent/context.rs`.

Key fields:

- `task_id`, `user_goal`, `mode`: identify the request and Ask / Plan / Act / Learn mode.
- `relevant_memories`: bounded memory cards with timestamp, app/window/URL, summary, match reason, confidence, and evidence refs.
- `current_project`, `recent_workflow_trace`, `files`, `urls`, `entities`, `commands`, `errors`, `decisions`, `todos`: typed context sections for agent planning.
- `privacy_scope`: local-only/read-only defaults, raw-evidence setting, blocklist scope, incognito state, and project/time bounds.
- `allowed_tools`: mode-derived policy entries from `src-tauri/src/agent/policy.rs`.
- `disallowed_context`: reasons context was dropped or redacted.
- `token_budget`: requested/max/used budget and dropped item count.
- `confidence`, `evidence_summary`, `source_context_pack_id`: provenance back to the underlying Continuum context pack.

## Ranking And Budgeting

The first slice reuses Continuum's existing context runtime ranking: semantic search when a goal is present, recent context fallback when it is not, project enrichment, graph context, decisions, failures, tasks, and section budgets.

The agent wrapper then applies an 8GB-safe default budget:

- default: `900` tokens
- minimum: `300`
- maximum: `4000`
- memory cards exposed to the agent: bounded to a small list by default

## Redaction

Raw evidence is off by default. Evidence snippets are truncated unless `include_raw_evidence` is explicitly requested. Blocklisted apps/domains, Continuum's own UI, and sensitive banking-style contexts are excluded before context reaches the agent.

Dropped context is recorded in `disallowed_context`; it is not silently hidden from the user.

## Provenance

Every selected memory carries:

- `memory_id`
- timestamp
- app/window/URL metadata when available
- evidence refs
- match reason from the underlying context pack

The UI and MCP responses expose the memory count, confidence, source context pack id, and redaction count.

## Explain Retrieval

`agent.explain_retrieval` can explain a persisted run or build a fresh Ask-mode run from a query. It returns:

- selected memories and match reasons
- qualitative semantic/keyword/recency/project/app/workflow signals
- dropped context
- redacted context
- privacy and policy reasons
- limitations

The current context runtime does not expose exact semantic, keyword, graph, or fusion scores. When scores are unavailable, Continuum returns qualitative evidence rather than fabricating numbers.

## Audit Relationship

`agent.run` persists the selected memory explanations in the audit ledger. This makes the context inspectable later without storing raw screenshots or raw sensitive evidence.
