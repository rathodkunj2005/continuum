# To Issues

Portable agent skill. Works in Cursor, Codex, ChatGPT, Gemini, Antigravity, Claude, or any coding assistant because it is plain Markdown instructions, not a tool-specific slash command.

## Trigger
Use this skill when the user says: `turn this into issues, break into tasks, create tickets, vertical slices, implementation plan to issues`.

## Goal
Convert a plan, PRD, or conversation into independently implementable issues that are small, testable, and ordered.

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


## Issue slicing principle

Each issue should deliver one observable behavior or one safe enabling change. Avoid horizontal slices like “build backend,” “build frontend,” or “refactor everything.”

## Workflow

1. Extract user value, technical constraints, and non-goals.
2. Identify vertical slices.
3. Order slices by dependency and risk.
4. Separate discovery/prototype tasks from production implementation tasks.
5. Include acceptance criteria and verification commands for every issue.
6. Mark issues that require ADRs, migrations, security review, or design review.
7. Keep each issue small enough for one focused agent session when possible.

## Good issue shape

```md
# Title

## User-visible outcome

## Scope

## Out of scope

## Affected modules

## Acceptance criteria
- [ ] ...

## Test / verification plan
- ...

## Implementation notes
- Reuse ...
- Avoid ...

## Dependencies
- ...
```

## Anti-bloat rule

Every issue must state what existing code should be reused or deleted. If the answer is unknown, add a discovery step before implementation.

## Required output

```text
ISSUE BREAKDOWN
Milestone / goal:
- ...
Recommended order:
1. ...
2. ...

Issue 1: ...
Type: ...
Size: small|medium|large
Affected modules: ...
Acceptance criteria:
- ...
Verification:
- ...
Reuse/delete guidance:
- ...
Dependencies:
- ...
```
