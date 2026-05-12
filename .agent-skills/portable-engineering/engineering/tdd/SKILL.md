# TDD

Portable agent skill. Works in Cursor, Codex, ChatGPT, Gemini, Antigravity, Claude, or any coding assistant because it is plain Markdown instructions, not a tool-specific slash command.

## Trigger
Use this skill when the user says: `tdd, use test-driven development, implement this safely, one vertical slice, red green refactor`.

## Goal
Build a feature or fix a bug through a strict red-green-refactor loop so the agent never outruns feedback.

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


## Red-green-refactor contract

Never start by writing implementation code unless there is already a failing test that precisely captures the behavior. If the repo has no test framework, create the smallest focused test harness first.

## Workflow

1. Understand the requested behavior.
2. Identify the smallest externally observable boundary to test:
   - Public function.
   - API route.
   - UI behavior.
   - CLI command.
   - Database interaction behind a repository/service boundary.
3. Inspect existing tests and match their style.
4. Write one failing test for one behavior.
5. Run the focused test and confirm it fails for the expected reason.
6. Implement the smallest code change that can pass the test.
7. Run the focused test again.
8. Refactor names, duplication, boundaries, and error handling without changing behavior.
9. Run the focused test again.
10. Repeat for the next vertical slice.
11. Run the broader relevant suite only after focused tests pass.

## Test design rules

Good tests:

- Test behavior, not private implementation details.
- Use real domain terms from `CONTEXT.md` when available.
- Fail for one clear reason.
- Are deterministic and do not depend on time, network, or global state unless controlled.
- Prefer stable module boundaries over tiny internal helper functions.

Bad tests:

- Snapshot huge outputs without intent.
- Mock every dependency until no real behavior remains.
- Recreate implementation logic inside assertions.
- Only test that functions were called instead of checking outcomes.
- Cover broad workflows without a focused failure signal.

## Anti-bloat checks before each implementation step

Before adding a new file, class, service, hook, abstraction, or dependency, answer:

```text
Why can this not live in an existing module?
What interface will test it?
What code can be deleted or simplified after this change?
```

If those answers are weak, redesign the slice.

## Required output

```text
TDD SUMMARY
Behavior implemented:
- ...
Tests added/updated:
- ...
Red-green-refactor evidence:
- Failing test observed: yes/no + command/output summary
- Passing test observed: yes/no + command/output summary
Files changed:
- ...
Design notes:
- Existing code reused: ...
- New code added because: ...
Remaining risk:
- ...
```
