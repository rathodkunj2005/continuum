# Agentic Graph RAG handoff

## Session: Phase 0 graph module restructure

Branch/worktree:
- Branch: `codex/agentic-graph-rag-phase0`
- Worktree: `~/.config/superpowers/worktrees/continuum/codex-agentic-graph-rag-phase0`
- Plan: `docs/superpowers/plans/2026-05-17-agentic-graph-rag.md`

What changed:
- Moved the insight graph module from `src-tauri/src/memory/graph/` to top-level `src-tauri/src/graph/`.
- Moved Lance insight graph persistence from `src-tauri/src/storage/graph_store.rs` to `src-tauri/src/graph/graph_store.rs`.
- Split graph schema ownership:
  - `src-tauri/src/graph/entities.rs`: `GraphNodeType` and node literal helpers.
  - `src-tauri/src/graph/edges.rs`: `GraphEdgeType`, edge literal helpers, and `edge_aliases::canonical`.
  - `src-tauri/src/graph/schema.rs`: `GraphNode`, `GraphEdge`, `GraphSubgraph`, and field-name constants.
- Renamed graph clustering module from `clusters.rs` to `community.rs`.
- Extracted `find_path` into `src-tauri/src/graph/pathfinding.rs`.
- Added `src-tauri/src/graph/graph_index.rs` and `src-tauri/src/graph/graph_rerank.rs` skeletons for later retrieval phases.
- Added node types: `Window`, `App`, `Command`.
- Added edge types: `OccurredInSession`, `BelongsToProject`, `UsedApp`, `SameTaskAs`, `EvidencedBy`.
- Extended `capture/entity_extractor.rs` so each memory emits:
  - `Memory --BelongsToProject--> Project` when project is populated.
  - `Memory --OccurredInSession--> Session` when session id is populated.
- Updated graph import sites from `crate::memory::graph` / `crate::storage::graph_store` to `crate::graph`.
- Updated `docs/architecture/graph-schema.md` for the new module map.

Verification run so far:
- `npm install`
- `npm run build` (needed because fresh worktree lacked `dist/` for Tauri macro)
- Baseline before edits: `cargo test -p continuum memory::graph::` passed 11 graph tests after building `dist/`.
- After edits: `cargo test -p continuum graph::` passed 20 tests, 1 ignored skeleton test.
- After edits: `cargo test -p continuum capture::entity_extractor` passed 8 tests.
- `rg "memory::graph|storage::graph_store" src-tauri/src src-tauri/tests` returned no hits.
- `npm test -- src/domains/memory-vault/MemoryCardsPanel.test.tsx` passed after updating a stale vault-only UI test that expected a removed "All memories" tab.
- `make test` passed: TypeScript typecheck, 74 Vitest tests, full Rust `cargo test`, and doc tests.

Where to look next:
- Phase 1 starts in `docs/superpowers/plans/2026-05-17-agentic-graph-rag.md` under "Phase 1 - Query planner".
- Primary files for Phase 1:
  - `src-tauri/src/context_runtime/query_plan.rs`
  - `src-tauri/src/context_runtime/graph_plan.rs`
  - `src-tauri/src/context_runtime/mod.rs`
  - `src-tauri/src/inference/mod.rs`
  - `src-tauri/src/search/query_processor.rs`
- Reuse graph primitives from:
  - `src-tauri/src/graph/entities.rs`
  - `src-tauri/src/graph/edges.rs`
  - `src-tauri/src/graph/graph_index.rs`
  - `src-tauri/src/graph/graph_rerank.rs`

Remaining plan phases:
- Phase 1: Query planner.
- Phase 2: Retrieval routes and fusion.
- Phase 3: Evidence pack, verifier, composer, and explainability.
- Phase 4: Backward-compatible IPC and MCP shims.
- Phase 5: UI "why surfaced", query-scoped graph, evidence/timeline expansion, Copy for Agent.

Notes for future agents:
- Keep using an isolated worktree unless the user explicitly asks to merge/push.
- Run Rust commands from `src-tauri/`, not the repo root.
- If a fresh worktree fails Rust tests with `frontendDist = "../dist" but this path doesn't exist`, run `npm run build` from the repo root first.
- `graph_rerank.rs` intentionally contains a compile-safe skeleton with an ignored test; the real implementation belongs to the later retrieval/fusion phases.

## Session: Phase 1 query planner

Branch/worktree:
- Branch: `codex/agentic-graph-rag-phase1`
- Worktree: `~/.config/superpowers/worktrees/continuum/codex-agentic-graph-rag-phase1`
- Plan: `docs/superpowers/plans/2026-05-17-agentic-graph-rag.md`

What changed:
- Added `src-tauri/src/context_runtime/query_plan.rs` with typed `QueryPlan`, `PlannerIntent`, `Route`, `TimeWindow`, `EntityHint`, `NeededContext`, `GraphExpansion`, `PlanHints`, and deterministic `plan(...)` rules.
- Added `src-tauri/src/context_runtime/graph_plan.rs` with per-intent graph seed kinds, allowed-edge whitelists, and max-hop rules.
- Exposed the planner modules from `src-tauri/src/context_runtime/mod.rs`.
- Promoted the existing hybrid-search `QueryProfile` / `QueryIntent` API for planner reuse and re-exported it through `src-tauri/src/search/query_processor.rs` and `src-tauri/src/search/mod.rs`.
- Added `InferenceEngine::refine_query_plan(...)` with a 400ms-compatible timeout, strict JSON-object validation, and `continuum.retrieval.planner.llm.{success,timeout,fail}` counters.
- Added `refine_plan_with_llm(...)` and `apply_refinement_json(...)` so optional LLM output can update only present fields in-place.
- Added `continuum.retrieval.planner.ms` latency recording for the rule planner.
- Added `src-tauri/tests/query_plan_rules.rs` with 10 planner-rule tests plus a model-skipping LLM smoke test.

Verification run:
- `cargo test -p continuum --test query_plan_rules` passed: 11 tests.
- `cargo test -p continuum context_runtime::query_plan:: -- --nocapture` passed and printed 6 representative `QueryPlan` JSON fixtures.
- `cargo test -p continuum context_runtime::graph_plan::` passed: 2 tests.

Where to look next:
- Phase 2 starts in `docs/superpowers/plans/2026-05-17-agentic-graph-rag.md` under "Phase 2 - Modular retrieval routes".
- Primary Phase 2 handoff files:
  - `src-tauri/src/context_runtime/query_plan.rs`
  - `src-tauri/src/context_runtime/graph_plan.rs`
  - `src-tauri/src/graph/graph_index.rs`
  - `src-tauri/src/graph/graph_rerank.rs`
  - existing retrieval entry points under `src-tauri/src/context_runtime/mod.rs` and `src-tauri/src/search/`

Notes for future agents:
- The Phase 1 planner is deterministic by default; LLM refinement is optional and additive.
- `PlanHints` is the adapter boundary for entity aliases and clock/budget inputs. Phase 2 should populate those hints from real store/runtime state rather than adding storage calls inside the pure planner.
- The plan has a small internal inconsistency on Definition max hops. The implementation follows the concrete example and route task: Definition uses `max_hops = 2`.
