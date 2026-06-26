# Continuum documentation

Use this index to find the right document quickly. **Authoritative agent vocabulary** lives in [`CONTEXT.md`](../CONTEXT.md) together with [`AGENTS.md`](../AGENTS.md) at the repository root.

## Start here

| Document | Purpose |
| --- | --- |
| [`CONTEXT.md`](../CONTEXT.md) | Product terms, where truth lives, default quality bar |
| [`architecture/ARCHITECTURE.md`](architecture/ARCHITECTURE.md) | Capture → search → UI pipeline and core Rust modules |
| [`product/DESIGN_DIRECTION.md`](product/DESIGN_DIRECTION.md) | UX and visual direction |
| [`mcp.md`](mcp.md) | MCP tools, modes, privacy model, and agent-facing additions |
| [`agent.md`](agent.md) | Continuum Agent architecture, modes, provider strategy, and safety |
| [`agent-context-pack.md`](agent-context-pack.md) | Typed context pack schema, ranking, redaction, and provenance |
| [`skills-and-evals.md`](skills-and-evals.md) | Skill lifecycle, eval case shape, and approval requirements |
| [`INSTALL.md`](INSTALL.md) | Teammate install guide for the packaged `.dmg` (quarantine, permissions, onboarding) |
| [`RELEASE.md`](RELEASE.md) | Maintainer guide: required GitHub secrets and how to cut a packaged release |

## By topic

| Folder | Contents |
| --- | --- |
| [`decisions/`](decisions/) | Architecture decision records (ADRs), numbered filenames |
| [`architecture/`](architecture/) | Long-form architecture + insight graph schema (`graph-schema.md`) |
| [`setup/engineering/`](setup/engineering/) | Implementation guides (timeline rules, repo layout, refactoring notes, agent tooling) |
| [`product/`](product/) | Product-level technical notes (e.g. intelligence engine) |
| [`agents/`](agents/) | Reserved for agent/MCP-oriented runbooks (add as needed) |
| Frontend source layout | [`../src/domains/README.md`](../src/domains/README.md) |

## Root files (not under `docs/`)

`README.md`, `AGENTS.md`, `CLAUDE.md`, and a short root `CONTEXT.md` pointer exist for tooling and first-time orientation. All substantive prose should live under **`docs/`** as above.
