# Setup Portable Engineering Skills

Portable agent skill. Works in Cursor, Codex, ChatGPT, Gemini, Antigravity, Claude, or any coding assistant because it is plain Markdown instructions, not a tool-specific slash command.

## Trigger
Use this skill when the user says: `setup portable engineering skills, initialize skills, set up repo workflow, create CONTEXT and ADR structure`.

## Goal
Prepare a repository so any agent can work inside a clear domain model, documented decision trail, and consistent issue/test workflow.

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

1. Inspect the repository structure, package managers, frameworks, test commands, lint/typecheck commands, and existing docs.
2. Create or update `CONTEXT.md` with:
   - Product purpose.
   - Main user workflows.
   - Domain vocabulary.
   - Module map.
   - Testing strategy.
   - Known risky areas.
   - Commands for install, dev, build, test, lint, typecheck.
3. Create `docs/adr/` if missing.
4. Create `docs/issues/` if no issue tracker integration is available.
5. Create `docs/handoffs/` for session handoffs.
6. Create `docs/prd/` for PRDs and design notes.
7. Add a short `docs/skills-usage.md` describing how to invoke these skills in the repo.
8. Do not rewrite the project or reorganize folders during setup. This skill only creates workflow scaffolding and documents existing reality.

## Required output

Return:

```text
SETUP COMPLETE
Files created/updated:
- ...
Detected commands:
- install: ...
- dev: ...
- test: ...
- lint/typecheck: ...
Repo map:
- ...
Recommended next skill:
- grill-with-docs | tdd | diagnose | improve-codebase-architecture
Open questions:
- ...
```

## `CONTEXT.md` template

```md
# Context

## Product purpose

## Primary users

## Core workflows

## Domain language
| Term | Meaning | Code names / files | Notes |
|---|---|---|---|

## System map
| Area | Responsibility | Main files | Public interface | Tests |
|---|---|---|---|---|

## Architecture decisions
See `docs/adr/`.

## Feedback loops
| Command | Purpose | When to run |
|---|---|---|

## Risk map
| Area | Risk | How to test |
|---|---|---|

## Agent rules for this repo
- Prefer modifying existing modules over creating new ones.
- Keep public interfaces stable unless the task is explicitly about changing them.
- Add tests at module boundaries.
- Update this file when terminology, module ownership, or workflows change.
```
