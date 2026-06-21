# Decision 006: MCP Deployment Modes for Continuum Cognitive Infrastructure

## Status

Accepted (Phase 1, transport hardening)

## Context

Continuum is evolving from a local experimental memory tool into a durable cognitive infrastructure layer that must interoperate across ChatGPT web, Claude, IDEs, and future agents. The MCP gateway now needs a deployment posture that is explicit about security, remote access, and transport compatibility.

## Decision

Continuum MCP now supports explicit deployment modes:

- `local` (default): localhost-only workflow, relaxed local ergonomics.
- `tunnel`: localhost bind with strict auth defaults for secure tunneling.
- `public`: non-loopback bind for controlled network deployments.

The gateway now:

- supports `GET /mcp` + `POST /mcp` for streamable-HTTP style clients,
- keeps legacy `/mcp/sse` + `/mcp/messages` behavior for backward compatibility,
- requires bearer auth by default in `tunnel` and `public` modes,
- disables loopback auth bypass by default in `tunnel` and `public` modes,
- validates `Origin` in non-local modes (when present),
- publishes optional public tunnel metadata in status/discovery when `CONTINUUM_MCP_PUBLIC_BASE_URL` is set.

## Consequences

Positive:

- Remote MCP usage becomes a first-class supported deployment mode.
- Tunnel traffic no longer inherits unsafe localhost auth assumptions.
- Existing local developer ergonomics remain intact by default.
- Continuum is better aligned with modern MCP transport guidance.

Tradeoffs:

- Browsers in non-local modes must use an explicitly allowed `Origin`.
- Some old clients relying on implicit localhost auth bypass in remote scenarios will need token headers.

## Follow-up Work

- Add first-party scoped API keys (per-client permissions).
- Add rate limiting and per-tool quotas.
- Add audit logs for external MCP clients.
- Add stateful session and resumable SSE support for richer server-initiated updates.
