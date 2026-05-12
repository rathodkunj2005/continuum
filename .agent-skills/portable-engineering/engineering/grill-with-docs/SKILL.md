# Grill With Docs

Portable agent skill. Works in Cursor, Codex, ChatGPT, Gemini, Antigravity, Claude, or any coding assistant because it is plain Markdown instructions, not a tool-specific slash command.

## Trigger
Use this skill when the user says: `grill with docs, challenge this plan, align with repo docs, update CONTEXT, update ADR`.

## Goal
Interrogate a proposed change against the existing domain language, module map, and architecture decisions before implementation.

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

Be adversarial but useful. The goal is not to produce code quickly; the goal is to prevent misalignment, vague requirements, and architecture drift.

## Workflow

1. Read available docs first:
   - `CONTEXT.md`
   - `docs/adr/*`
   - `README.md`
   - relevant PRDs/issues
   - relevant source files and tests
2. Summarize the current domain language and affected modules.
3. Identify mismatches between the user's words and the repo's established terms.
4. Ask targeted questions only where decisions block correct implementation.
5. Walk the design tree:
   - User workflow.
   - Data model.
   - Module ownership.
   - Public interfaces.
   - Failure modes.
   - Test strategy.
   - Migration/backward compatibility.
   - Privacy/security/performance implications.
6. Resolve dependencies between decisions. Do not let later decisions depend on unanswered earlier ones.
7. Propose a minimal design that fits the existing system.
8. Update or draft updates for `CONTEXT.md` and ADRs when the plan introduces new domain terms or architectural choices.

## Question rules

Ask questions in batches grouped by decision area. Do not ask vague questions like “anything else?” Ask questions that force useful tradeoffs.

Good:

```text
Should this behavior live at the retrieval boundary or the synthesis boundary? The answer changes which module owns the test and whether existing callers need to change.
```

Bad:

```text
Can you clarify more?
```

## ADR trigger

Create or update an ADR when the plan changes:

- Module boundaries.
- Persistence format.
- Public APIs.
- Security/privacy assumptions.
- Background jobs or external integrations.
- Testing strategy for a critical path.
- A meaningful tradeoff between speed, complexity, accuracy, cost, or UX.

## Required output

```text
GRILL WITH DOCS RESULT
Shared understanding:
- ...
Domain terms to use:
- ...
Affected modules/interfaces:
- ...
Decisions resolved:
- ...
Open blockers:
- ...
CONTEXT.md updates needed:
- ...
ADR updates needed:
- title: ...
Recommended next step:
- to-prd | to-issues | prototype | tdd | diagnose
```
