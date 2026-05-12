# To PRD

Portable agent skill. Works in Cursor, Codex, ChatGPT, Gemini, Antigravity, Claude, or any coding assistant because it is plain Markdown instructions, not a tool-specific slash command.

## Trigger
Use this skill when the user says: `turn this into a PRD, write PRD, product requirements, implementation spec, no interview`.

## Goal
Synthesize the current context into a concise PRD that preserves engineering constraints and avoids vague feature creep.

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

1. Use only the provided conversation context and repository evidence. Do not invent requirements.
2. Identify the problem, target user, desired outcome, non-goals, and constraints.
3. Map the feature to existing domain terms and modules.
4. Specify behavior as acceptance criteria.
5. Define data flow and interfaces at a high level.
6. Include testing and observability requirements.
7. Identify risks and open questions.
8. If the plan changes architecture, draft an ADR title and decision summary.

## PRD template

```md
# PRD: <Feature Name>

## Problem

## Goal

## Users / actors

## Current behavior

## Proposed behavior

## Non-goals

## User workflows

## Functional requirements
- FR1: ...

## Non-functional requirements
- Performance:
- Reliability:
- Security/privacy:
- Accessibility:
- Maintainability:

## Domain language
| Term | Meaning | Existing code/docs |
|---|---|---|

## Affected modules and interfaces
| Module | Change | Interface impact | Tests |
|---|---|---|---|

## Data flow

## Acceptance criteria
- [ ] ...

## Test plan

## Rollout / migration plan

## Risks

## Open questions

## Suggested issues
```

## Required output

Return the PRD. Do not add implementation code unless explicitly requested.
