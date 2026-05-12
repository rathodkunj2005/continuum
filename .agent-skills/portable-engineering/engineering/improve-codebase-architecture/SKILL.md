# Improve Codebase Architecture

Portable agent skill. Works in Cursor, Codex, ChatGPT, Gemini, Antigravity, Claude, or any coding assistant because it is plain Markdown instructions, not a tool-specific slash command.

## Trigger
Use this skill when the user says: `improve architecture, reduce codebase complexity, deep modules, refactor safely, codebase is messy, avoid ball of mud`.

## Goal
Find and execute small architecture improvements that make the codebase easier to understand, test, and modify without broad rewrites.

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


## Core idea

Prefer deep modules: a simple public interface hiding meaningful complexity. Avoid shallow modules: many tiny files with complex call chains, pass-through wrappers, and no stable behavior boundary.

## Workflow

1. Read `CONTEXT.md`, ADRs, module docs, and current tests.
2. Map the current area:
   - Modules/files involved.
   - Public interfaces.
   - Data flow.
   - Test boundaries.
   - Repeated concepts/names.
3. Detect architecture smells:
   - Pass-through services or hooks.
   - Circular dependencies.
   - Duplicate domain logic.
   - Feature logic scattered across UI, routes, database, and utility files.
   - Boolean/config parameter explosion.
   - Tests forced to mock many internals.
   - Files named by technical layer only, hiding domain purpose.
   - New code added because old code was hard to understand.
4. Identify deepening opportunities:
   - Move related behavior behind one small interface.
   - Collapse pass-through layers.
   - Extract a domain concept only when it hides real complexity.
   - Rename modules/functions to match shared language.
   - Strengthen typed boundaries.
   - Delete unused or duplicated code.
5. Pick one low-risk improvement.
6. Add characterization tests before refactoring if behavior is not already covered.
7. Refactor in small mechanical steps.
8. Run focused tests after each step.
9. Update `CONTEXT.md` or ADRs if the module map changed.

## What not to do

- Do not rewrite a whole subsystem for aesthetics.
- Do not introduce a framework, dependency injection container, event bus, global store, or generic abstraction unless the existing code proves it is needed.
- Do not split files merely because they are long. Split only around stable concepts and interfaces.
- Do not create `utils`, `helpers`, `common`, or `misc` dumping grounds.
- Do not change behavior and architecture in the same step unless the test makes the behavior change explicit.

## Deep module checklist

A good module has:

- A name from the domain language.
- A small public surface.
- Internal freedom to change implementation.
- Tests through the public surface.
- Clear ownership of one concept.
- Few reasons to change.

## Required output

```text
ARCHITECTURE REVIEW
Current pain:
- ...
Smells found:
- ...
Deepening opportunity selected:
- ...
Why this is better:
- Understandability: ...
- Testability: ...
- Change safety: ...
Files changed:
- ...
Tests/verification:
- ...
CONTEXT/ADR updates:
- ...
Deferred improvements:
- ...
```
