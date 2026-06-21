# Continuum — mandatory agent defaults

These rules apply to **every** AI-assisted change in this repository (Cursor, Claude Code, OpenAI Codex, Google Antigravity, and other agents that read this file). **The user should not have to name a skill.** Pick the matching workflow from `.agent-skills/portable-engineering/` automatically and follow it end-to-end for the current task.

## Product context

Continuum is a macOS desktop app: local screen-context memory, search, meetings, tasks, and MCP integrations. Stack: **React + TypeScript** (`src/`), **Tauri 2 + Rust** (`src-tauri/`), LanceDB, local embeddings, optional local GGUF. Authoritative overview: `README.md` and `docs/architecture/ARCHITECTURE.md`.

## Repo map for agents

- Shared vocabulary and pointers: `docs/CONTEXT.md` (stub at repo root: `CONTEXT.md`)
- Documentation index: `docs/README.md`
- Architecture decisions: `docs/decisions/`
- Design direction: `docs/product/DESIGN_DIRECTION.md`
- UI domains: `src/domains/` (see `src/domains/README.md`)
- Insight graph schema (Lance): `docs/architecture/graph-schema.md`
- Product technical notes: `docs/product/`

## Verification (after meaningful edits)

Run the **cheapest relevant** checks and say what you ran. Default full sweep from repo root: `make test` (runs `npm run typecheck`, `npm test`, and `cargo test` under `src-tauri/`). For small isolated edits, a subset is fine if you state why.

## Non-negotiable engineering rules

- Read existing code, tests, and docs that touch the task before editing.
- Prefer reusing, moving, simplifying, or deleting existing code over adding new layers.
- One vertical slice at a time; avoid drive-by refactors unrelated to the task.
- Preserve behavior unless the task explicitly changes it.
- Add or extend tests at stable boundaries where behavior is observable.
- Debug with evidence (repro, narrowing, hypotheses), not guesses.
- If something is unclear after inspection, ask targeted questions instead of assuming.

## Anti-bloat gate (before adding code)

Answer honestly:

> Can this be solved by deleting code, reusing an existing module, tightening an interface, adding a test, or improving a name instead of adding a new layer?

If yes, do that first.

## Portable skills (always on)

All workflows live under **`.agent-skills/portable-engineering/`** (plain Markdown `SKILL.md` files). **Open the file for the situation you are in** and comply with its workflow and required outputs, even when the user does not mention it.

| Situation | Skill path (from repo root) |
| --- | --- |
| Goal unclear or need a system-level frame | `.agent-skills/portable-engineering/engineering/zoom-out/SKILL.md` |
| Significant feature or architecture work (start here) | `.agent-skills/portable-engineering/engineering/grill-with-docs/SKILL.md` |
| Turn discovery into a PRD-shaped plan | `.agent-skills/portable-engineering/engineering/to-prd/SKILL.md` |
| Turn a plan into issues / vertical slices | `.agent-skills/portable-engineering/engineering/to-issues/SKILL.md` |
| Implement behavior with tight feedback loops | `.agent-skills/portable-engineering/engineering/tdd/SKILL.md` |
| Bug, regression, performance, or flaky test | `.agent-skills/portable-engineering/engineering/diagnose/SKILL.md` |
| Incoming work needs prioritization / routing | `.agent-skills/portable-engineering/engineering/triage/SKILL.md` |
| Time-boxed exploration or comparison | `.agent-skills/portable-engineering/engineering/prototype/SKILL.md` |
| Reduce entropy; readability and boundaries | `.agent-skills/portable-engineering/engineering/improve-codebase-architecture/SKILL.md` |
| Review for unnecessary complexity / bloat | `.agent-skills/portable-engineering/engineering/anti-bloat-review/SKILL.md` |
| Bootstrap or extend skill/doc conventions in-repo | `.agent-skills/portable-engineering/engineering/setup-portable-engineering-skills/SKILL.md` |
| End of session or switching tools / agents | `.agent-skills/portable-engineering/productivity/handoff/SKILL.md` |
| Ruthless prioritization / sequencing | `.agent-skills/portable-engineering/productivity/caveman/SKILL.md` |
| Challenge your own plan | `.agent-skills/portable-engineering/productivity/grill-me/SKILL.md` |
| Write a new portable skill | `.agent-skills/portable-engineering/productivity/write-a-skill/SKILL.md` |

When multiple rows apply, order matters: **zoom-out → grill-with-docs → to-prd / to-issues → tdd** for new work; **diagnose** supersedes generic implementation patterns for defects; **handoff** when stopping mid-flight.

### Offline / single-file reference

If the environment cannot open the tree, use `.agent-skills/portable-engineering/ALL_SKILLS_COMBINED.md` (same content as the individual `SKILL.md` files).

## Privacy and data safety

Do not commit secrets, real user captures, database blobs, or contents of ignored data directories. Follow `.gitignore` and `README.md` privacy guidance when suggesting commands or tests.
