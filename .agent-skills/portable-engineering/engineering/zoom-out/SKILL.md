# Zoom Out

Portable agent skill. Works in Cursor, Codex, ChatGPT, Gemini, Antigravity, Claude, or any coding assistant because it is plain Markdown instructions, not a tool-specific slash command.

## Trigger
Use this skill when the user says: `zoom out, explain this codebase, where does this fit, broader context, unfamiliar code section`.

## Goal
Explain an unfamiliar code area in the context of the whole system so the next change does not damage architecture.

## Non-negotiable engineering rules
- Inspect the existing code, tests, docs, and naming before proposing or editing anything.
- Prefer using, moving, simplifying, or deleting existing code over adding new code.
- Do not stack abstractions on top of abstractions. If a direct change solves the problem cleanly, use the direct change.
- Keep changes small enough to review. One vertical slice beats a broad rewrite.
- Preserve working behavior unless the requested change explicitly replaces it.
- Add or update tests at the boundary where behavior is observable.
- Run the cheapest relevant feedback loop after each meaningful change: typecheck, lint, unit test, focused integration test, browser check, or targeted script.
- If the codebase lacks a feedback loop, create the smallest useful one before changing behavior.
- Surface uncertainty instead of guessing. If a dependency, convention, or requirement is unclear, inspect more or ask targeted questions.

## Inputs to look for
- Current task or bug report.
- Existing tests, failing output, logs, stack traces, screenshots, or repro steps.
- Repository docs: `README`, `CONTEXT.md`, `docs/adr/*`, design docs, issue tracker notes, PRDs.
- Module boundaries, public interfaces, API routes, database schema, typed models, and domain terminology.


## Workflow

1. Inspect nearby files, imports, tests, routes, types, and docs.
2. Identify the local responsibility of the code.
3. Trace inbound callers and outbound dependencies.
4. Connect the code to user workflows and domain language.
5. Identify the public interface and what must remain stable.
6. Identify hidden coupling, risky assumptions, and likely test points.
7. Provide a small map before recommending changes.

## Output style

Use concise, high-signal sections. Avoid explaining every line. Prioritize what matters for making safe changes.

## Required output

```text
ZOOMED-OUT MAP
What this area does:
- ...
Where it sits in the system:
- Upstream callers: ...
- Downstream dependencies: ...
- Domain concepts: ...
Public interface / stable boundary:
- ...
Important files:
- ...
How to test it:
- ...
Risks before editing:
- ...
Recommended safe next step:
- ...
```
