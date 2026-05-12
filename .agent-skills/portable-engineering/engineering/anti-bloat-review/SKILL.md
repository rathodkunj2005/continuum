# Anti-Bloat Review

Portable agent skill. Works in Cursor, Codex, ChatGPT, Gemini, Antigravity, Claude, or any coding assistant because it is plain Markdown instructions, not a tool-specific slash command.

## Trigger
Use this skill when the user says: `review for bloat, avoid code on code, simplify this PR, reduce complexity, is this overengineered`.

## Goal
Review a change or plan for unnecessary code, duplicated abstractions, weak boundaries, and poor testability.

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


## Review stance

Assume every new abstraction is guilty until it proves it reduces total complexity. The goal is not fewer lines at all costs; the goal is fewer concepts, clearer boundaries, and safer change.

## Bloat signals

Flag:

- New services that only call another service.
- New hooks/components/classes with no independent responsibility.
- Duplicated validation, mapping, or formatting logic.
- Feature-specific code placed in generic utilities.
- Generic frameworks introduced for one use case.
- Unused configuration knobs.
- Large snapshots instead of meaningful tests.
- Tests that require mocking many internals.
- Multiple sources of truth.
- Data models that mirror each other without reason.
- Comments explaining confusing code instead of simpler code.
- Code paths that exist only because the agent guessed future needs.

## Review workflow

1. Identify user-visible behavior added or changed.
2. Identify code added, code modified, and code deleted.
3. Compare the size of the implementation to the behavior delivered.
4. Check whether existing modules could absorb the change cleanly.
5. Check whether public interfaces became simpler or more complex.
6. Check testability: can behavior be verified at a stable boundary?
7. Propose concrete deletions, moves, renames, or interface reductions.
8. Distinguish must-fix issues from optional cleanup.

## Required output

```text
ANTI-BLOAT REVIEW
Behavior delivered:
- ...
Complexity added:
- ...
Bloat risks:
- ...
Simplifications required:
- ...
Code to delete or merge:
- ...
Interface improvements:
- ...
Testability gaps:
- ...
Verdict:
- approve | approve with cleanup | request changes
```
