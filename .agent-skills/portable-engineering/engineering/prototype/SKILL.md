# Prototype

Portable agent skill. Works in Cursor, Codex, ChatGPT, Gemini, Antigravity, Claude, or any coding assistant because it is plain Markdown instructions, not a tool-specific slash command.

## Trigger
Use this skill when the user says: `prototype, spike, explore design, throwaway version, compare UI options, test state logic`.

## Goal
Build a throwaway prototype to learn quickly without polluting production architecture.

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


## Prototype rule

A prototype is for learning, not shipping. Keep it isolated, reversible, and clearly marked as disposable.

## Choose prototype type

Use a terminal/state prototype when exploring:

- State machines.
- Business rules.
- Ranking/retrieval logic.
- Data transformations.
- Scheduling/allocation logic.

Use a UI prototype when exploring:

- Layout alternatives.
- Interaction models.
- Visual hierarchy.
- Onboarding flows.
- Dashboard density.

## Workflow

1. State the learning question.
2. Define what the prototype must prove or disprove.
3. Create the smallest isolated implementation.
4. Use fake data unless real data is required to answer the question.
5. Build multiple alternatives when comparing UX.
6. Do not wire prototype code into production paths.
7. Capture findings and recommendation.
8. Delete the prototype or leave it under `docs/prototypes/` with a clear expiration note.

## Anti-bloat constraints

- No new production dependencies without explicit approval.
- No prototype code in core modules.
- No permanent abstractions created from a prototype until the learning is documented.
- No hidden feature flags that keep dead prototype paths alive.

## Required output

```text
PROTOTYPE REPORT
Learning question:
- ...
Prototype location:
- ...
Alternatives tested:
- ...
Result:
- ...
Recommendation:
- ...
Production implementation guidance:
- Reuse: ...
- Delete: ...
- Avoid: ...
```
