# Grill Me

Portable agent skill. Works in Cursor, Codex, ChatGPT, Gemini, Antigravity, Claude, or any coding assistant because it is plain Markdown instructions, not a tool-specific slash command.

## Trigger
Use this skill when the user says: `grill me, interview me, challenge my plan, ask me questions until clear`.

## Goal
Create shared understanding before execution by aggressively resolving assumptions, tradeoffs, and decision dependencies.

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


## Operating mode

Interview the user until the plan is specific enough to execute. Be direct. Do not rush into writing a plan if core decisions are unresolved.

## Workflow

1. Restate the rough goal in one sentence.
2. Build a decision tree:
   - Goal and success criteria.
   - Users/stakeholders.
   - Scope and non-goals.
   - Constraints.
   - Data/input/output.
   - Workflow and edge cases.
   - Risks and tradeoffs.
   - Test/evaluation method.
3. Ask the highest-leverage unresolved questions first.
4. Group questions into small batches.
5. After each answer, update the working plan.
6. Stop when the remaining unknowns do not block execution.
7. Produce an execution-ready brief.

## Question quality bar

Good questions force decisions:

```text
Which is more important for v1: correctness under edge cases or speed of demo? This changes whether we build validation now or fake the narrow path.
```

Bad questions invite vague answers:

```text
Can you tell me more about the project?
```

## Required output

```text
GRILLED BRIEF
Goal:
- ...
Decisions made:
- ...
Non-goals:
- ...
Open questions that do not block v1:
- ...
Recommended next step:
- ...
```
