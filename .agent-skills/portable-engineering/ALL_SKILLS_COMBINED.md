# All Portable Engineering Skills

This file combines every `SKILL.md` for easy copy-paste into tools that cannot read a folder.


---

# Path: `engineering/anti-bloat-review/SKILL.md`

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


---

# Path: `engineering/diagnose/SKILL.md`

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


---

# Path: `engineering/grill-with-docs/SKILL.md`

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


---

# Path: `engineering/improve-codebase-architecture/SKILL.md`

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


---

# Path: `engineering/prototype/SKILL.md`

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


---

# Path: `engineering/setup-portable-engineering-skills/SKILL.md`

# Setup Portable Engineering Skills

Portable agent skill. Works in Cursor, Codex, ChatGPT, Gemini, Antigravity, Claude, or any coding assistant because it is plain Markdown instructions, not a tool-specific slash command.

## Trigger
Use this skill when the user says: `setup portable engineering skills, initialize skills, set up repo workflow, create CONTEXT and ADR structure`.

## Goal
Prepare a repository so any agent can work inside a clear domain model, documented decision trail, and consistent issue/test workflow.

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

1. Inspect the repository structure, package managers, frameworks, test commands, lint/typecheck commands, and existing docs.
2. Create or update `CONTEXT.md` with:
   - Product purpose.
   - Main user workflows.
   - Domain vocabulary.
   - Module map.
   - Testing strategy.
   - Known risky areas.
   - Commands for install, dev, build, test, lint, typecheck.
3. Create `docs/adr/` if missing.
4. Create `docs/issues/` if no issue tracker integration is available.
5. Create `docs/handoffs/` for session handoffs.
6. Create `docs/prd/` for PRDs and design notes.
7. Add a short `docs/skills-usage.md` describing how to invoke these skills in the repo.
8. Do not rewrite the project or reorganize folders during setup. This skill only creates workflow scaffolding and documents existing reality.

## Required output

Return:

```text
SETUP COMPLETE
Files created/updated:
- ...
Detected commands:
- install: ...
- dev: ...
- test: ...
- lint/typecheck: ...
Repo map:
- ...
Recommended next skill:
- grill-with-docs | tdd | diagnose | improve-codebase-architecture
Open questions:
- ...
```

## `CONTEXT.md` template

```md
# Context

## Product purpose

## Primary users

## Core workflows

## Domain language
| Term | Meaning | Code names / files | Notes |
|---|---|---|---|

## System map
| Area | Responsibility | Main files | Public interface | Tests |
|---|---|---|---|---|

## Architecture decisions
See `docs/adr/`.

## Feedback loops
| Command | Purpose | When to run |
|---|---|---|

## Risk map
| Area | Risk | How to test |
|---|---|---|

## Agent rules for this repo
- Prefer modifying existing modules over creating new ones.
- Keep public interfaces stable unless the task is explicitly about changing them.
- Add tests at module boundaries.
- Update this file when terminology, module ownership, or workflows change.
```


---

# Path: `engineering/tdd/SKILL.md`

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


---

# Path: `engineering/to-issues/SKILL.md`

# To Issues

Portable agent skill. Works in Cursor, Codex, ChatGPT, Gemini, Antigravity, Claude, or any coding assistant because it is plain Markdown instructions, not a tool-specific slash command.

## Trigger
Use this skill when the user says: `turn this into issues, break into tasks, create tickets, vertical slices, implementation plan to issues`.

## Goal
Convert a plan, PRD, or conversation into independently implementable issues that are small, testable, and ordered.

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


## Issue slicing principle

Each issue should deliver one observable behavior or one safe enabling change. Avoid horizontal slices like “build backend,” “build frontend,” or “refactor everything.”

## Workflow

1. Extract user value, technical constraints, and non-goals.
2. Identify vertical slices.
3. Order slices by dependency and risk.
4. Separate discovery/prototype tasks from production implementation tasks.
5. Include acceptance criteria and verification commands for every issue.
6. Mark issues that require ADRs, migrations, security review, or design review.
7. Keep each issue small enough for one focused agent session when possible.

## Good issue shape

```md
# Title

## User-visible outcome

## Scope

## Out of scope

## Affected modules

## Acceptance criteria
- [ ] ...

## Test / verification plan
- ...

## Implementation notes
- Reuse ...
- Avoid ...

## Dependencies
- ...
```

## Anti-bloat rule

Every issue must state what existing code should be reused or deleted. If the answer is unknown, add a discovery step before implementation.

## Required output

```text
ISSUE BREAKDOWN
Milestone / goal:
- ...
Recommended order:
1. ...
2. ...

Issue 1: ...
Type: ...
Size: small|medium|large
Affected modules: ...
Acceptance criteria:
- ...
Verification:
- ...
Reuse/delete guidance:
- ...
Dependencies:
- ...
```


---

# Path: `engineering/to-prd/SKILL.md`

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


---

# Path: `engineering/triage/SKILL.md`

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


---

# Path: `engineering/zoom-out/SKILL.md`

# Zoom Out

Portable agent skill. Works in Cursor, Codex, ChatGPT, Gemini, Antigravity, Claude, or any coding assistant because it is plain Markdown instructions, not a tool-specific slash command.

## Trigger
Use this skill when the user says: `zoom out, explain this codebase, where does this fit, broader context, unfamiliar code section`.

## Goal
Explain an unfamiliar code area in the context of the whole system so the next change does not damage architecture.

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

1. Inspect nearby files, imports, tests, routes, types, and docs.
2. Identify the local responsibility of the code.
3. Trace inbound callers and outbound dependencies.
4. Connect the code to user workflows and domain language.
5. Identify the public interface and what must remain stable.
6. Identify hidden coupling, risky assumptions, and likely test points.
7. Provide a small map before recommending changes.

## Output style

Use concise, high-signal sections. Avoid explaining every line. Prioritize what matters for making safe changes.

## Required output

```text
ZOOMED-OUT MAP
What this area does:
- ...
Where it sits in the system:
- Upstream callers: ...
- Downstream dependencies: ...
- Domain concepts: ...
Public interface / stable boundary:
- ...
Important files:
- ...
How to test it:
- ...
Risks before editing:
- ...
Recommended safe next step:
- ...
```


---

# Path: `productivity/caveman/SKILL.md`

# Caveman

Portable agent skill. Works in Cursor, Codex, ChatGPT, Gemini, Antigravity, Claude, or any coding assistant because it is plain Markdown instructions, not a tool-specific slash command.

## Trigger
Use this skill when the user says: `caveman, be terse, compress this, token-efficient mode, less words`.

## Goal
Communicate with maximum technical signal and minimum filler while preserving accuracy.

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


## Style rules

- Drop greetings, apologies, hedging, motivational phrasing, and repeated context.
- Use compact bullets or terse paragraphs.
- Keep exact technical terms.
- Do not omit constraints, risks, commands, or file names.
- Prefer `do X because Y` over long explanation.
- Preserve uncertainty where it matters.

## Output pattern

```text
Goal: ...
Do:
- ...
Avoid:
- ...
Commands:
- ...
Risk:
- ...
Next: ...
```

## Compression checks

Before final answer, remove:

- Polite filler.
- Restated prompt.
- Obvious background.
- Duplicated bullets.
- “It is important to note” style phrases.

Do not remove:

- Acceptance criteria.
- Edge cases.
- Safety warnings.
- Test commands.
- File paths.


---

# Path: `productivity/grill-me/SKILL.md`

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


---

# Path: `productivity/handoff/SKILL.md`

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


---

# Path: `productivity/write-a-skill/SKILL.md`

# Write A Skill

Portable agent skill. Works in Cursor, Codex, ChatGPT, Gemini, Antigravity, Claude, or any coding assistant because it is plain Markdown instructions, not a tool-specific slash command.

## Trigger
Use this skill when the user says: `write a skill, create skill.md, make reusable agent instructions, new portable skill`.

## Goal
Create a reusable `SKILL.md` that encodes a repeatable workflow with clear triggers, guardrails, and output contracts.

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


## Skill design principles

A good skill is:

- Specific enough to change agent behavior.
- Portable across tools.
- Written as instructions, not prose about instructions.
- Triggered by recognizable user language.
- Organized around a workflow and output contract.
- Small enough to compose with other skills.
- Opinionated about failure modes.

## Required structure

```md
# <Skill Name>

Portable agent skill. Works in any coding assistant because it is plain Markdown instructions.

## Trigger
Use this skill when the user says: `<phrases>`.

## Goal
<one sentence>

## Non-negotiable rules
- ...

## Inputs to look for
- ...

## Workflow
1. ...

## What not to do
- ...

## Required output
```text
...
```
```

## Workflow

1. Identify the repeated task or failure mode.
2. Define when the skill should trigger.
3. Define what success looks like.
4. Write the smallest workflow that reliably reaches success.
5. Add guardrails against common bad agent behavior.
6. Add an output contract that makes completion auditable.
7. Remove tool-specific assumptions unless the skill is intentionally tool-specific.
8. Add examples only if they clarify behavior.

## Required output

Return a complete `SKILL.md` ready to save to disk.
