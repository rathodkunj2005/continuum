# Triage

Portable agent skill. Works in Cursor, Codex, ChatGPT, Gemini, Antigravity, Claude, or any coding assistant because it is plain Markdown instructions, not a tool-specific slash command.

## Trigger
Use this skill when the user says: `triage issues, sort bugs, prioritize backlog, classify tickets, issue state machine`.

## Goal
Turn a messy issue list into clear states with severity, ownership, next action, and enough context for implementation.

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


## Issue state machine

Use these states unless the repo already defines its own labels:

```text
new → needs-repro → needs-design → ready → in-progress → blocked → review → done
```

Additional classification labels:

```text
bug | feature | chore | refactor | docs | test | perf | security | ux | data | infra
p0 | p1 | p2 | p3
small | medium | large
needs-adr | needs-prd | needs-test | duplicate | wontfix
```

## Workflow

1. Read each issue, bug report, or TODO.
2. Determine whether it is actionable.
3. If not actionable, identify the missing information.
4. Classify type, severity, risk, and likely module ownership.
5. Convert vague requests into one or more vertical slices.
6. Mark dependencies explicitly.
7. Do not solve the issue during triage unless it is trivial and requested.
8. Prefer closing duplicates over creating new parallel tasks.

## Severity guide

- `p0`: data loss, security issue, production outage, broken core workflow.
- `p1`: major user-facing failure with workaround or high-priority release blocker.
- `p2`: important bug/feature with limited blast radius.
- `p3`: polish, cleanup, internal improvement, low urgency.

## Ready criteria

An issue is `ready` only when it has:

- Observable expected behavior.
- Affected module or workflow.
- Acceptance criteria.
- Test or verification plan.
- Dependencies identified.

## Required output

```text
TRIAGE RESULT
Summary:
- Total issues reviewed: ...
- Ready: ...
- Blocked/needs info: ...
- Duplicates/wontfix: ...

Issues:
1. Title: ...
   State: ...
   Labels: ...
   Owner/module: ...
   Acceptance criteria:
   - ...
   Verification:
   - ...
   Next action:
   - ...
```
