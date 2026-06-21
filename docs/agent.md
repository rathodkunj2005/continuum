# Continuum Agent

Continuum Agent is the local control-plane layer over Continuum memory. It is not a generic chatbot: every run starts by building an `AgentContextPack`, applying privacy scope, and checking a mode-specific tool policy.

## Architecture

```text
capture pipeline
  -> memory records / graph / search
  -> context_runtime::ContextPack
  -> agent::AgentContextPack
  -> Ask / Plan / Act / Learn response
  -> policy / persisted audit / feedback / skill + eval drafts
```

Implemented code paths:

- `src-tauri/src/agent/context.rs`: typed context pack and deterministic local Ask/Plan/Act/Learn runner.
- `src-tauri/src/agent/policy.rs`: mode-based permission and risk policy.
- `src-tauri/src/agent/audit.rs`: append-only JSONL audit and retrieval feedback ledger.
- `src-tauri/src/agent/skills.rs`: user-reviewed skill candidate shape and deterministic draft generator.
- `src-tauri/src/agent/evals.rs`: local eval case shape and deterministic draft generator.
- `src-tauri/src/agent/prompts.rs`: Continuum-specific MCP prompt registry.
- `src-tauri/src/ipc/commands/agent.rs`: Tauri commands for UI access.

## Modes

- Ask: read-only memory/context. No file writes, commands, or external messages.
- Plan: read memory and project context, suggest next steps, no execution.
- Act: context building works, but actions are approval-gated. This slice does not execute actions.
- Learn: can describe skill/eval candidates later; activation requires user review.

## Local Provider Strategy

The existing Hermes/Ollama UI remains the model runtime surface. The new Agent command box works without API keys because it returns deterministic context-grounded output even when Hermes/Ollama is offline.

8GB-safe defaults:

- no LLM is loaded to build an AgentContextPack beyond the existing retrieval path
- raw evidence is off
- context budget defaults to `900` tokens for Ask mode
- visual models are not used by this Agent path

## Safety

Tool policy is external to the model. Dangerous scopes such as file writes, mutating commands, external messages, and credential access are denied or approval-gated in code before any runtime is involved.

Future Act mode must append `AgentAuditRecord` entries before and after approved actions.

## Audit Flow

Every `agent.run` call attempts to append a local audit row under the app support `agent/` directory. Audit rows include run id, mode, goal, context pack id, memories used, policy decisions, approvals required, dropped/redacted context, confidence, output summary, status, and error message if available.

Audit write failures do not crash successful agent responses; the response includes an audit warning. Failed context-pack builds still attempt to record a failed audit row before returning an error.

## Inspectability

The Agent page lists recent runs and can show:

- selected memories
- qualitative retrieval reasons
- dropped/redacted context
- allowed, blocked, and approval-required tools
- retrieval feedback attached to the run
- draft skill/eval proposals

Exact semantic/vector fusion scores are not exposed yet by the current retrieval layer, so explanations are honest and qualitative where necessary.

## Feedback Loop

Users and MCP clients can rate retrieval results as `useful`, `irrelevant`, `wrong`, `stale`, or `missing_context`. Feedback is persisted locally and attached to run detail. It does not mutate ranking automatically yet.
