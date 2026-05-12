# Portable Engineering Skills

These are plain Markdown `SKILL.md` files for disciplined AI-assisted software engineering. They are designed to work anywhere: Cursor, Codex, ChatGPT, Gemini, Antigravity, Claude, GitHub Copilot Chat, or a human code review process.

They are inspired by the public `mattpocock/skills` workflow ideas, but adapted into tool-neutral instructions. They avoid Claude-only slash-command assumptions and focus on repeatable engineering loops: clarify first, use shared language, build vertical slices, test through stable boundaries, diagnose before fixing, and continuously reduce codebase entropy.

## How to use in any tool

Copy this folder into a repo, usually as one of:

```text
.agent-skills/
docs/skills/
.skills/
```

Then invoke a skill with a direct instruction:

```text
Use docs/skills/engineering/tdd/SKILL.md for this change.
Task: add password reset email support.
```

For tools that can index project files, keep the skill folder in the repo. For tools that cannot, paste the relevant `SKILL.md` into the chat before the task.

## Recommended daily workflow

1. Run `setup-portable-engineering-skills` once per repo to create `CONTEXT.md`, `docs/adr/`, and basic workflow conventions.
2. Use `grill-with-docs` before significant features or architecture changes.
3. Use `to-prd` or `to-issues` to convert the plan into vertical slices.
4. Use `tdd` for implementation.
5. Use `diagnose` for bugs and performance regressions.
6. Use `improve-codebase-architecture` every few days or whenever the code starts becoming difficult to understand.
7. Use `handoff` when switching agents, tools, or sessions.

## Core anti-bloat rule

Before adding code, the agent must answer:

```text
Can this be solved by deleting code, reusing an existing module, tightening an interface, adding a test, or improving a name instead of adding a new layer?
```

If yes, do that first.

## Skills included

Engineering:

- `diagnose`
- `grill-with-docs`
- `triage`
- `improve-codebase-architecture`
- `setup-portable-engineering-skills`
- `tdd`
- `to-issues`
- `to-prd`
- `zoom-out`
- `prototype`
- `anti-bloat-review`

Productivity:

- `caveman`
- `grill-me`
- `handoff`
- `write-a-skill`

## Suggested repo files these skills can maintain

```text
CONTEXT.md                 Shared domain language and system map
docs/adr/                  Architecture Decision Records
docs/prd/                  Product requirements and implementation plans
docs/handoffs/             Session handoff notes
docs/issues/               Local issue files when no GitHub/Linear tracker exists
docs/prototypes/           Throwaway prototypes and comparison notes
```

## Universal invocation template

```text
Use the skill at <path-to-SKILL.md>.
Stay inside the workflow.
Do not implement broad unrelated changes.
Before coding, summarize the smallest safe plan and the existing files you will touch.
After coding, report tests/checks run and any remaining risk.

Task:
<task here>
```
