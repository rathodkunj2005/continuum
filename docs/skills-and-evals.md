# Continuum Skills And Evals

Continuum's learning loop is intentionally user-reviewed. The current implementation adds typed shapes, deterministic draft generation, and local draft persistence; it does not silently activate skills or mutate high-risk behavior.

## Skill Lifecycle

```text
observed workflow
  -> skill candidate
  -> user-reviewed draft
  -> verified skill
  -> active skill
  -> monitored skill
  -> retired or improved skill
```

Backend type: `src-tauri/src/agent/skills.rs`.

Command:

- `propose_skill_from_run(run_id)`

The generator requires enough audit context and at least one cited memory. It creates a draft only; there is no activation path in this slice.

Skill candidates include:

- `name`
- `category`
- `source`
- `created_from_memories`
- `risk_level`
- `requires_approval`
- `when_to_use`
- `required_context`
- `procedure`
- `verification`
- `failure_cases`
- `privacy_notes`

## Eval Cases

Backend type: `src-tauri/src/agent/evals.rs`.

Command:

- `propose_eval_from_run(run_id)`

The generator creates a local eval draft from a successful or inspectable run. It includes forbidden actions and grading rules, but no eval runner is implemented yet.

Eval cases include:

- `workflow_name`
- `input_context_pack_id`
- `expected_outcome`
- `forbidden_actions`
- `required_evidence`
- `grading_rules`
- `privacy_scope`

## Approval Requirements

Skill creation and updates are medium-risk policy scopes. Learn mode can propose them, but user review is required before activation. High-risk or mutating actions remain outside this first slice.

Drafts are appended under the app support `agent/` directory. They are local artifacts for review, not active automation.
