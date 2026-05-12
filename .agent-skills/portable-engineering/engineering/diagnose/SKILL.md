# Diagnose

Portable agent skill. Works in Cursor, Codex, ChatGPT, Gemini, Antigravity, Claude, or any coding assistant because it is plain Markdown instructions, not a tool-specific slash command.

## Trigger
Use this skill when the user says: `diagnose, debug this, find root cause, fix hard bug, performance regression, flaky test`.

## Goal
Debug through evidence instead of guesses: reproduce, minimize, hypothesize, instrument, fix, and regression-test.

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


## Debugging principle

Do not patch symptoms. A fix is not complete until the root cause is named and a regression test or repeatable verification exists.

## Workflow

1. Restate the observed failure precisely.
2. Reproduce it with the smallest command, input, browser action, or test.
3. Minimize the failure:
   - Narrow to one route, function, component, query, data shape, or commit range.
   - Remove unrelated variables.
4. List hypotheses ranked by likelihood.
5. Add temporary instrumentation only where it can distinguish between hypotheses.
6. Run the repro and record evidence.
7. Eliminate hypotheses until one root cause remains.
8. Apply the smallest targeted fix.
9. Add or update a regression test.
10. Remove temporary logs/instrumentation unless they are useful permanent observability.
11. Run focused verification, then broader checks.

## Performance regression variant

For performance problems, capture:

- Baseline latency, memory, CPU, query count, render count, bundle size, or relevant metric.
- Input size and environment.
- Hot path evidence from profiler, traces, logs, or timings.
- Before/after measurements.

Never claim a performance fix without numbers.

## Flaky test variant

For flaky tests:

- Run the test repeatedly.
- Identify nondeterminism source: time, randomness, ordering, async race, network, shared state, database residue, environment.
- Fix determinism rather than increasing timeouts unless a timeout is clearly too low.

## Required output

```text
DIAGNOSIS REPORT
Observed failure:
- ...
Smallest repro:
- Command/steps: ...
Root cause:
- ...
Evidence:
- ...
Fix:
- ...
Regression test / verification:
- ...
Commands run:
- ...
Temporary instrumentation removed:
- yes/no
Remaining risk:
- ...
```
