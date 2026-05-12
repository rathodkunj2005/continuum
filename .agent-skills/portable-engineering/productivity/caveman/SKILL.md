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
