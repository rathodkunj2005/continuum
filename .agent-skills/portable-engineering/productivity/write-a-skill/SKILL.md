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
