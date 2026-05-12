# Handoff

Portable agent skill. Works in Cursor, Codex, ChatGPT, Gemini, Antigravity, Claude, or any coding assistant because it is plain Markdown instructions, not a tool-specific slash command.

## Trigger
Use this skill when the user says: `handoff, compact this session, summarize for another agent, continue later, create handoff doc`.

## Goal
Create a compact but complete transfer document so another agent or future session can continue without re-discovery.

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

1. Identify the goal of the session.
2. Summarize decisions made and why.
3. List files changed, files inspected, and commands run.
4. Capture current state: passing/failing tests, known bugs, unfinished work.
5. Preserve exact next steps.
6. Include risk areas and things not to touch.
7. Include user preferences or constraints relevant to the work.
8. Avoid long narrative. Make it actionable.

## Required output

```md
# Handoff: <Project / Task>

## Goal

## Current state

## Decisions made
| Decision | Reason | Files/Docs |
|---|---|---|

## Files changed
| File | Change | Notes |
|---|---|---|

## Files inspected but not changed

## Commands run
| Command | Result |
|---|---|

## Tests / verification

## Known issues

## Next steps
1. ...
2. ...
3. ...

## Risks / do not do

## Useful context for next agent
```
