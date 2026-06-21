# Upgrade Continuum into an Agentic Graph RAG System

## Context

Continuum today is a local-first macOS memory app with **hybrid (vector + keyword) search** over OCR-derived memories, an **insight knowledge graph** in LanceDB (`graph_nodes` / `graph_edges`), and a `context_runtime` that fuses search hits with activity events into a `ContextPack`. The graph is populated by `capture/entity_extractor.rs` from finalized memories. MCP exposes `memory.*` and `agent.*` tools.

The graph is currently a passive byproduct of capture. Retrieval is a monolithic `HybridSearcher` that blends only vector + snippet + keyword branches. There is no query planner, no graph route, no temporal route, no entity route, no fusion module, no evidence pack, no verifier, and no composer. The UI shows synthesis-time `insight_why_mattered` text but cannot answer *why* a memory surfaced for a specific query.

We are upgrading Continuum to be an **Agentic Graph RAG** system: NL query → typed `QueryPlan` → parallel retrieval routes (vector + keyword + temporal + entity + graph) → fusion → evidence pack → verification → grounded answer / cards / context pack. The graph becomes a **retrieval substrate**, not a visualization. The UI shows a per-card "Why this surfaced" reason, an expanded card with evidence + linked subgraph + before/after timeline, a query-scoped subgraph view (no all-memory hairball), and a "Copy for Agent" action that exposes the new pipeline through MCP.

## Locked-in architectural decisions

1. **File layout: strict spec.** Move existing `src-tauri/src/memory/graph/` → `src-tauri/src/graph/` and `src-tauri/src/storage/graph_store.rs` → `src-tauri/src/graph/graph_store.rs`. Add retrieval modules inside `src-tauri/src/context_runtime/`. Update all import sites.
2. **Rollout: additive.** New IPC commands `continuum_*` and new MCP tools `continuum.*`. Existing `searchMemoryCards`, `buildAgentContextPack`, `memory.*`, and `agent.*` keep working as thin shims over the new pipeline.
3. **Query planner: rule-based v1 + optional LLM refinement** (400ms timeout, merge-if-parses).
4. **Edge migration: additive aliases.** Add 4 genuinely new edge variants (`OccurredInSession`, `BelongsToProject`, `UsedApp`, `SameTaskAs`, `EvidencedBy`); alias the rest of the spec names (`HasTopic`, `CausedError`, `ResolvedError`, `MadeDecision`, `CreatedTodo`, `RelatedTo`, `Before`, `After`, `VisitedUrl`, `UsesFile`, `MentionsEntity`) to existing variants via a translation table. Persisted Lance data is untouched.
5. **Node-type mapping:** keep `Concept` (=Topic), `Task` (=Todo), `File` (=Artifact). Add `Window`, `App`, `Command`. Drop `Entity` (too generic — use `Concept` with `metadata.entity_class`).
6. **Backwards-compat invariant:** existing `cargo test`, `npm test`, and `make test` must remain green after every phase. Public IPC return shapes only gain optional fields.

## Critical files

- `src-tauri/src/memory/graph/schema.rs` (will move + split)
- `src-tauri/src/memory/graph/traversal.rs` (will move + split into `traversal.rs` + `pathfinding.rs`)
- `src-tauri/src/memory/graph/clusters.rs` (will move → `community.rs`)
- `src-tauri/src/memory/graph/legacy.rs` (will move)
- `src-tauri/src/storage/graph_store.rs` (will move → `graph/graph_store.rs`)
- `src-tauri/src/capture/entity_extractor.rs` (stays, but emits new edges)
- `src-tauri/src/search/hybrid.rs` (becomes a back-compat shim)
- `src-tauri/src/search/reranker.rs` (`anchor_coverage_score` exposed `pub`)
- `src-tauri/src/search/memory_cards.rs` (`MemoryCard` gains optional `surfacing_reason`)
- `src-tauri/src/search/query_processor.rs` (`QueryProfile::from_query` reused by planner)
- `src-tauri/src/context_runtime/mod.rs` (becomes public façade over new pipeline)
- `src-tauri/src/inference/mod.rs` (add `refine_query_plan` helper)
- `src-tauri/src/agent/context.rs` (`build_agent_context_pack` calls new pipeline)
- `src-tauri/src/mcp/mod.rs` (add `continuum.*` tools alongside existing)
- `src-tauri/src/ipc/commands/` (add `retrieval.rs`)
- `src-tauri/src/lib.rs` (register new module + IPC commands)
- `src/shared/ipc/tauri.ts` (new IPC bindings + `surfacing_reason` field)
- `src/domains/memory-vault/MemoryCardsPanel.tsx`
- `src/domains/memory-vault/KnowledgeGraph.tsx` + `graph/graphDataBuilder.ts`

---

## Phase 0 — Move and restructure graph module

**Goal:** Move the existing graph module to its spec-mandated location, split files per spec, and add new node/edge variants — all without changing persisted data.

### Files

Create (under new `src-tauri/src/graph/`):
- `mod.rs` — module root re-exporting public types from sibling files.
- `entities.rs` — `GraphNodeType` enum + node helpers (from current `schema.rs` lines covering nodes).
- `edges.rs` — `GraphEdgeType` enum + `edge_aliases::canonical(name) -> Option<GraphEdgeType>`.
- `schema.rs` — `GraphNode` / `GraphEdge` record structs + `Arrow` field constants. Imports from `entities.rs` / `edges.rs`.
- `graph_store.rs` — Lance I/O (moved verbatim from `src-tauri/src/storage/graph_store.rs`).
- `graph_index.rs` — stub: `pub struct GraphIndex { adjacency: HashMap<Uuid, Vec<(Uuid, GraphEdgeType, f32)>> }` with `build(nodes, edges) -> Self` + `pub fn neighbors(&self, id, kinds, max_hops) -> Vec<...>` (initially just calls into `traversal.rs`).
- `traversal.rs` — moved from `memory/graph/traversal.rs`, keeps `undirected_adjacency`, `bfs_neighborhood`. `find_path` extracted to `pathfinding.rs`.
- `community.rs` — moved from `clusters.rs` (rename only); `attach_louvain_metadata` keeps its signature.
- `pathfinding.rs` — `find_path` (moved from `traversal.rs`).
- `graph_rerank.rs` — `pub fn rerank_with_graph_signals(hits: &mut [FusedHit], index: &GraphIndex, plan: &GraphExpansion)` (skeleton + unit-test stub).

Move and delete:
- Delete `src-tauri/src/memory/graph/` after move; remove the `pub mod graph;` line from `memory/mod.rs`.
- Delete `src-tauri/src/storage/graph_store.rs` after move; remove `pub mod graph_store;` from `storage/mod.rs`.

Modify:
- `src-tauri/src/lib.rs` — add `pub mod graph;`.
- `src-tauri/src/capture/entity_extractor.rs` — emit `BelongsToProject` and `OccurredInSession` edges when `record.project` and `record.session_id` are non-empty.

Find-and-replace import sites (script + manual verify):
- `crate::memory::graph::*` → `crate::graph::*`
- `crate::storage::graph_store::*` → `crate::graph::graph_store::*`
- Expected ~20–30 sites; verify with `rg "memory::graph|storage::graph_store"` → 0 hits.

### Tasks

- [x] **0.1: Create the new directory skeleton.**
  - `mkdir src-tauri/src/graph` and add empty `mod.rs` re-exporting `pub mod entities; pub mod edges; pub mod schema; pub mod graph_store; pub mod graph_index; pub mod traversal; pub mod community; pub mod pathfinding; pub mod graph_rerank;`
- [x] **0.2: Copy files into new location** (do not delete originals yet).
  - `cp src-tauri/src/memory/graph/schema.rs src-tauri/src/graph/_schema_full.rs`
  - `cp src-tauri/src/memory/graph/traversal.rs src-tauri/src/graph/traversal.rs`
  - `cp src-tauri/src/memory/graph/clusters.rs src-tauri/src/graph/community.rs`
  - `cp src-tauri/src/memory/graph/legacy.rs src-tauri/src/graph/legacy.rs` (legacy stays as a sub-file)
  - `cp src-tauri/src/storage/graph_store.rs src-tauri/src/graph/graph_store.rs`
- [x] **0.3: Split `_schema_full.rs`** into `entities.rs` (the `GraphNodeType` enum + `from_str`/`to_str` for nodes), `edges.rs` (the `GraphEdgeType` enum + `from_str`/`to_str` for edges), and `schema.rs` (the `GraphNode`, `GraphEdge` record structs, Arrow constants, and field accessors).
  - Each enum gets a `#[derive(Serialize, Deserialize, specta::Type)]` matching the current derives.
  - Delete `_schema_full.rs`.
- [x] **0.4: Extract `find_path`** from `traversal.rs` into `pathfinding.rs`; update internal callers (just `traversal.rs` and `clusters.rs`).
- [x] **0.5: Add new node variants.** In `entities.rs`:
  ```rust
  pub enum GraphNodeType {
      Project, Memory, Concept, Decision, File, Error, Tool,
      Person, Url, Session, Task,
      Window, App, Command, // new
  }
  ```
  Update `to_str`/`from_str` to include the new variants.
- [x] **0.6: Add new edge variants.** In `edges.rs`:
  ```rust
  pub enum GraphEdgeType {
      // existing 25 variants...
      OccurredInSession, BelongsToProject, UsedApp, SameTaskAs, EvidencedBy,
  }
  ```
- [x] **0.7: Add `edge_aliases::canonical(name: &str) -> Option<GraphEdgeType>`** translating spec names → canonical Rust variants:
  ```rust
  "HasTopic"        => MentionedIn
  "CausedError"     => Causes
  "ResolvedError"   => Resolves
  "MadeDecision"    => CreatedBy
  "CreatedTodo"     => CreatedBy
  "RelatedTo"       => SimilarTo
  "Before"          => PrecededBy
  "After"           => FollowedBy
  "VisitedUrl"      => MentionedIn
  "UsesFile"        => UsedIn
  "MentionsEntity"  => MentionedIn
  ```
  Plus identity for the 5 new variants and the 25 existing variants. Unit test mapping every spec name.
- [x] **0.8: Update `graph_store.rs`** to extend `node_type_lit` / `node_type_from_lit` / `edge_type_lit` / `edge_type_from_lit` to cover the new variants.
- [x] **0.9: Stub `graph_index.rs`:**
  ```rust
  pub struct GraphIndex { /* adjacency: HashMap<Uuid, Vec<Adj>> */ }
  impl GraphIndex {
      pub fn build(nodes: &[GraphNode], edges: &[GraphEdge]) -> Self { /* call traversal::undirected_adjacency */ }
      pub fn neighbors(&self, id: Uuid, allowed: &[GraphEdgeType], max_hops: usize) -> Vec<NeighborHit> { /* call traversal::bfs_neighborhood */ }
  }
  ```
- [x] **0.10: Stub `graph_rerank.rs`:** empty function body + signature; unit test marked `#[ignore]`.
- [x] **0.11: Update `src-tauri/src/lib.rs`** — add `pub mod graph;`. Remove `pub mod graph;` from `src-tauri/src/memory/mod.rs` and `pub mod graph_store;` from `src-tauri/src/storage/mod.rs`.
- [x] **0.12: Replace imports.**
  ```bash
  rg -l "crate::memory::graph" src-tauri/src | xargs sd "crate::memory::graph" "crate::graph"
  rg -l "crate::storage::graph_store" src-tauri/src | xargs sd "crate::storage::graph_store" "crate::graph::graph_store"
  ```
  Spot-check `rg "memory::graph|storage::graph_store" src-tauri/src` → 0 hits.
- [x] **0.13: Delete originals.** `rm -r src-tauri/src/memory/graph` and `rm src-tauri/src/storage/graph_store.rs`.
- [x] **0.14: Extend entity extractor.** In `capture/entity_extractor.rs::extract`, after the existing Project/Session/Memory node creation, emit edges:
  - `Memory --BelongsToProject--> Project` when `record.project.is_some()`.
  - `Memory --OccurredInSession--> Session` when `record.session_id.is_some()`.
  Use the existing confidence formula. Cap edges per record (existing limit is 8 nodes; raise edge cap symmetrically if needed).
- [x] **0.15: Run `cargo test -p continuum graph::`** (formerly `memory::graph::`). Update test paths. All graph tests pass.
- [x] **0.16: Run `make test`.** Expected: green.
- [x] **0.17: Commit.**
  ```bash
  git add -A
  git commit -m "refactor(graph): hoist graph/ to top-level, split entities/edges, add Window/App/Command + 5 new edge variants"
  ```

### Verification

- `rg "memory::graph|storage::graph_store"` → 0 hits.
- `cargo test -p continuum graph::` passes.
- `cargo test -p continuum capture::entity_extractor` passes; new edges are produced for memories with project+session.
- A fixture test re-opens an existing LanceDB and reads old `graph_nodes` / `graph_edges` rows successfully (string literals unchanged).
- `make test` green.

### Smallest demoable outcome

A unit test in `capture/entity_extractor.rs` that constructs a `MemoryRecord` with `project="Continuum"` and `session_id="sess-1"` and asserts the resulting `ExtractionResult.edges` includes exactly one `BelongsToProject` and one `OccurredInSession` edge.

---

## Phase 1 — Query planner

**Goal:** Turn an NL query into a typed `QueryPlan` via deterministic rules, with optional async LLM refinement.

### Files

Create:
- `src-tauri/src/context_runtime/query_plan.rs` — `QueryPlan`, `PlannerIntent`, `TimeWindow`, `EntityHint`, `NeededContext`, `Route`, `GraphExpansion`. `pub fn plan(query: &str, hints: &PlanHints) -> QueryPlan` (rules) and `pub async fn refine_plan_with_llm(plan: &mut QueryPlan, engine: &InferenceEngine) -> bool` (optional, 400ms timeout).
- `src-tauri/src/context_runtime/graph_plan.rs` — `GraphPlan` struct: seed-node selection rules, allowed-edge whitelist per intent, max-hops table (Definition→1, Debug→2, ResumeWork→2, Lookup→1, HowTo→1, Timeline→0, RelatedTo→2).

Modify:
- `src-tauri/src/context_runtime/mod.rs` — `pub mod query_plan; pub mod graph_plan;`.
- `src-tauri/src/inference/mod.rs` — add `pub async fn refine_query_plan(&self, query: &str, current_plan_json: &str, timeout_ms: u64) -> Option<String>` using existing 400ms timeout pattern from `synthesize_memory_card`.
- `src-tauri/src/search/query_processor.rs` — promote `QueryProfile` to `pub` and add `pub fn anchor_terms(&self) -> &[String]` so planner can reuse it.

### Structs

```rust
// query_plan.rs
pub struct QueryPlan {
    pub raw: String,
    pub intent: PlannerIntent,
    pub target_project: Option<String>,
    pub target_topics: Vec<String>,
    pub target_entities: Vec<EntityHint>,
    pub time_window: Option<TimeWindow>,
    pub needed_context: NeededContext,
    pub retrieval_routes: Vec<Route>,
    pub graph_expansion: GraphExpansion,
    pub budget_tokens: u32,
}

pub enum PlannerIntent { ResumeWork, Debug, Lookup, HowTo, Definition, Timeline, RelatedTo }
pub enum Route { Vector, Keyword, Temporal, Entity, Graph }

pub struct EntityHint { pub label: String, pub kind: EntityHintKind }
pub enum EntityHintKind { Concept, Person, Tool, File, Url, App, Command }

pub struct TimeWindow { pub from_ms: i64, pub to_ms: i64 }

pub struct NeededContext {
    pub files: bool, pub decisions: bool, pub errors: bool,
    pub todos: bool, pub commands: bool, pub recent_changes: bool,
}

pub struct GraphExpansion {
    pub max_hops: u8,           // 0..=2
    pub seed_kinds: Vec<GraphNodeType>,
    pub allowed_edges: Vec<GraphEdgeType>,
}
```

### Tasks

- [x] **1.1: Define structs** in `query_plan.rs` with `#[derive(Serialize, Deserialize, specta::Type, Clone, Debug)]`.
- [x] **1.2: Write `plan_rules`.** Use `QueryProfile::from_query` for anchor terms, intent disambiguation, and recency heuristics. Map `QueryIntent::Definition` → `PlannerIntent::Definition` etc. Detect `target_project` by matching anchor terms against `entity_aliases` store. Detect `target_entities` from CamelCase / dotted identifiers / file extensions / URLs.
- [x] **1.3: Write `route_selection` rules.**
  - Always include `Vector` and `Keyword`.
  - If `target_entities` non-empty or `target_project.is_some()`: add `Entity`.
  - If `time_window.is_some()` or query contains "today" / "yesterday" / "last week" / "before" / "after": add `Temporal`.
  - If intent ∈ {ResumeWork, Debug, Definition, RelatedTo}: add `Graph` with `max_hops=2`. Otherwise `Graph` with `max_hops=1`.
- [x] **1.4: Write `GraphPlan::from(intent)`** in `graph_plan.rs`. Allowed-edge whitelist per intent:
  - Definition → `BelongsToProject, MentionedIn, EvidencedBy`
  - Debug → `Causes, Resolves, FixedBy, BrokeBy, TriggeredBy`
  - ResumeWork → `OccurredInSession, BelongsToProject, PrecededBy, FollowedBy, SameTaskAs`
  - RelatedTo → `SimilarTo, MentionedIn`
- [x] **1.5: Write 10 unit tests** in `tests/query_plan_rules.rs` covering one example per `PlannerIntent`, route set, and graph expansion.
  Example: `plan("why is the planner debounce 250ms")` → `intent=Definition`, `routes=[Vector,Keyword,Graph]`, `graph_expansion.max_hops=2`, `allowed_edges` includes `EvidencedBy`.
- [x] **1.6: Add `InferenceEngine::refine_query_plan`** with prompt:
  ```
  System: You output a tiny JSON object with optional fields only.
  Schema: {"target_project"?: string, "target_topics"?: string[], "graph_max_hops"?: 0|1|2}
  Query: <raw>
  Current plan: <plan_json>
  Output JSON only.
  ```
  400ms timeout via `tokio::time::timeout`. On parse-fail return `None`.
- [x] **1.7: `refine_plan_with_llm`** races the LLM call against the timeout, merges the parsed JSON into the plan in-place (only fields present override), and is safe to call concurrently.
- [x] **1.8: Integration test.** With a real `InferenceEngine` (skipped if model missing), confirm refinement parses on 3 fixture queries.
- [x] **1.9: Telemetry.** Add `continuum.retrieval.planner.ms` histogram and `continuum.retrieval.planner.llm.{success,timeout,fail}` counters.
- [x] **1.10: Commit.**
  ```bash
  git commit -m "feat(query_plan): rule-based planner + optional LLM refinement"
  ```

### Verification

- `cargo test -p continuum context_runtime::query_plan::` passes.
- Snapshot fixture: log `QueryPlan` JSON for 6 representative queries; eyeball plans look sensible.
- LLM refinement on a known model returns success on ≥2/3 fixtures and times out cleanly otherwise.

### Smallest demoable outcome

`plan("why is the planner debounce 250ms")` returns `PlannerIntent::Definition`, `routes=[Vector,Keyword,Graph]`, `graph_expansion.max_hops=2`.

---

## Phase 2 — Modular retrieval routes (inside `context_runtime/`)

**Goal:** Decompose `HybridSearcher` into 5 typed routes that run in parallel; preserve existing search behavior bit-for-bit when only Vector+Keyword routes are enabled.

### Files

Create:
- `src-tauri/src/context_runtime/retrieval_routes.rs` — `pub trait RetrievalRoute { async fn run(&self, plan: &QueryPlan, ctx: &RouteCtx) -> RouteHits; }`. `RouteRunner::dispatch(plan, ctx) -> Vec<RouteHits>` runs requested routes in parallel via `tokio::join!`. `RouteHits { route: Route, hits: Vec<RouteHit> }`. `RouteHit { memory_id, score, signals, optional graph_path }`.
- `src-tauri/src/context_runtime/vector_route.rs` — extracts the semantic + snippet branch logic currently in `search/hybrid.rs::semantic_branch`.
- `src-tauri/src/context_runtime/keyword_route.rs` — extracts `search/hybrid.rs::keyword_branch` (BM25-style lexical scan).
- `src-tauri/src/context_runtime/temporal_route.rs` — queries `Store::list_activity_events(time_window)` and `Store::list_recent_memories(time_window)`; scores by recency decay (exponential half-life 6h for "now", 24h for "today", 7d for "this week").
- `src-tauri/src/context_runtime/entity_route.rs` — for each `target_entity` in plan, look up matching `graph_nodes` by label (case-insensitive prefix + exact), gather `source_memory_ids`, score by entity confidence × node-type weight.
- `src-tauri/src/context_runtime/graph_route.rs` — seeds = union(vector top-k + keyword top-k + entity-route hits); for each seed memory, walk to its graph nodes, call `GraphIndex::neighbors(node, allowed_edges, max_hops)`, gather connected memories with their path as `Vec<(node_label, edge_type, node_label)>`.

Modify:
- `src-tauri/src/context_runtime/mod.rs` — register new pub mods.
- `src-tauri/src/search/hybrid.rs` — `HybridSearcher::search` builds a `QueryPlan{ routes: [Vector, Keyword] }` and delegates to `RouteRunner::dispatch`. Pulls private branch helpers out into the new `vector_route.rs` / `keyword_route.rs`. **No duplication** — old fns are *moved*, not copied.
- `src-tauri/src/search/reranker.rs` — `anchor_coverage_score` becomes `pub fn`.
- `src-tauri/src/search/mod.rs` — re-export the new route types under `crate::search::` for any external import.

### Tasks

- [ ] **2.1: Create `RouteCtx`** carrying `&Store`, `&GraphIndex`, `&InferenceEngine?`, `&EmbeddingService`, `&SearchConfig`.
- [ ] **2.2: Define `RetrievalRoute` trait** and `RouteRunner::dispatch` using `futures::future::join_all` over the requested routes.
- [ ] **2.3: Move semantic branch.** Cut `semantic_branch` + `snippet_branch` from `hybrid.rs` into `vector_route.rs::VectorRoute::run`. Update `hybrid.rs` to call them via the route trait.
- [ ] **2.4: Move keyword branch** similarly into `keyword_route.rs::KeywordRoute::run`.
- [ ] **2.5: Run existing search tests** — `cargo test -p continuum search::` must pass identically (regression gate).
- [ ] **2.6: Build `TemporalRoute`** with `apply_recency_decay(now_ms, event_ms) -> f32`. Test on a fixture DB.
- [ ] **2.7: Build `EntityRoute`** matching against `graph_nodes` and de-duping memory hits.
- [ ] **2.8: Build `GraphRoute`** with seed-node selection (top 5 vector hits + top 5 keyword hits + all entity hits). Path is built incrementally during BFS in `GraphIndex::neighbors`. Cap returned paths at 25.
- [ ] **2.9: `RouteHit::graph_path`** is `Option<Vec<PathStep>>` where `PathStep { from_label: String, edge: GraphEdgeType, to_label: String }`.
- [ ] **2.10: Wire `RouteRunner::dispatch`** to fan out routes in parallel, return `Vec<RouteHits>` keyed by `Route`.
- [ ] **2.11: Unit tests per route file** (one happy-path test, one empty-result test).
- [ ] **2.12: Integration test** that runs all 5 routes on a fixture DB and asserts each returns ≥1 hit and the graph route produces a non-empty `graph_path` on at least one hit.
- [ ] **2.13: Performance budget.** p50 per route ≤ 80ms on a 10k-memory DB; runner total p50 ≤ 200ms with `tokio::join!`.
- [ ] **2.14: Telemetry.** `continuum.retrieval.route.{name}.ms` histograms; `continuum.retrieval.route.{name}.hits` counters.
- [ ] **2.15: Commit.**

### Verification

- All existing `cargo test -p continuum search::` tests pass unchanged (regression gate).
- New route tests pass.
- `make test` green.

### Smallest demoable outcome

A test executing `RouteRunner::dispatch(plan_with_all_5_routes, ctx)` returns 5 `RouteHits` vectors; the `Graph` vector has at least one hit whose `graph_path` is non-empty.

---

## Phase 3 — Fusion, evidence pack, verifier, composer

**Goal:** Combine route hits, collect evidence per memory, verify groundedness, and compose either grounded answer or memory cards (with `surfacing_reason`) — all as explicit, pluggable stages.

### Files

Create:
- `src-tauri/src/context_runtime/fusion.rs` — `pub fn fuse(plan: &QueryPlan, hits: Vec<RouteHits>, weights: &FusionWeights) -> Vec<FusedHit>`. `FusedHit { memory_id, score, signals: FusionSignals, surfacing_reason: SurfacingReason }`.
- `src-tauri/src/context_runtime/evidence_pack.rs` — `EvidencePack { files: Vec<FileRef>, commands: Vec<CommandRef>, decisions: Vec<DecisionRef>, errors: Vec<ErrorRef>, todos: Vec<TaskRef>, urls: Vec<UrlRef> }`. `pub fn collect_evidence(hits: &[FusedHit], store: &Store) -> EvidencePack`. Reuses existing `MemoryRecord` columns (`active_files`, `commands`, `decisions`, `next_steps`, `errors`, `urls`).
- `src-tauri/src/context_runtime/verifier.rs` — `pub fn verify(plan: &QueryPlan, fused: &[FusedHit], evidence: &EvidencePack) -> VerifyOutcome` returning `Grounded { confidence } | PartialAnswer { missing } | NotEnoughEvidence { reason }`. Rules:
  - Reject hits where `confidence_score < 0.3` and not surfaced by `Graph` route.
  - Reject the whole answer if fewer than 2 distinct memory_ids back the top result.
  - Reject if `plan.needed_context.files` and `evidence.files.is_empty()`.
  - Demote hits where the only route is `Graph` *and* the path length is 2 (weak provenance).
- `src-tauri/src/context_runtime/composer.rs` — `pub async fn compose_answer(plan, fused, evidence, engine) -> ComposedAnswer` and `pub fn compose_cards(plan, fused, evidence) -> Vec<MemoryCard>`. `SurfacingReason` is filled deterministically per card from route + path data:
  - `headline`: `"Matched in {N} memories"` / `"Reached via {edge_type} from {seed_label}"` / `"Most recent of {N} this session"`.
  - `routes`: `["vector", "keyword", "graph(2-hop via Decision:planner-debounce)"]`.
  - `graph_path`: `["plan.ts", "UsedIn", "Decision: 250ms debounce"]`.
  - `anchor_terms_hit`: from `QueryProfile`.
  - `recency_boost`: from temporal route signal.
- `src-tauri/src/context_runtime/context_pack.rs` — moves the `ContextPack` struct out of the current `mod.rs` into its own file, adds `surfacing_reasons: Vec<SurfacingReason>` and `verify_outcome: VerifyOutcome` fields. Existing serde compatibility preserved (all new fields default).

Modify:
- `src-tauri/src/search/memory_cards.rs` — `MemoryCard` gains `pub surfacing_reason: Option<SurfacingReason>` (default `None` — frontend types unchanged unless they read it).
- `src-tauri/src/search/reranker.rs` — already pub from Phase 2; `fusion.rs` calls `anchor_coverage_score` directly.
- `src-tauri/src/context_runtime/mod.rs` — `build_context_pack` rewritten as `plan → RouteRunner::dispatch → fuse → collect_evidence → verify → compose`. **Same public signature.** All existing callers (e.g., `agent::context::build_agent_context_pack`) keep working.

### Fusion weights

```rust
pub struct FusionWeights {
    pub vector:   f32, // 0.45
    pub keyword:  f32, // 0.20
    pub temporal: f32, // 0.10
    pub entity:   f32, // 0.10
    pub graph:    f32, // 0.10
    pub recency:  f32, // 0.05 (multiplicative decay)
}
```

Per intent, `FusionWeights::for_intent(intent)` returns tuned weights (e.g., `Debug` boosts `graph` to 0.20 and drops `vector` to 0.35; `Lookup` boosts `keyword` to 0.30).

### Tasks

- [ ] **3.1: Add `surfacing_reason`** to `MemoryCard` (optional, default `None`). Regenerate Specta types; frontend types unchanged unless field is read.
- [ ] **3.2: Implement `fusion::fuse`** as a pure function. For each memory_id present in any route, sum weighted scores; carry through anchor-coverage from reranker; emit `FusionSignals { vector, keyword, temporal, entity, graph, recency, coverage, phrase }`.
- [ ] **3.3: Generate `SurfacingReason`** deterministically in `fusion::fuse`. Headline templates per route combo (see above).
- [ ] **3.4: Implement `evidence_pack::collect_evidence`** by joining fused hits to their `MemoryRecord` columns; dedupe by path / command string / decision text.
- [ ] **3.5: Implement `verifier::verify`** with the 4 rules above. Unit test: a fused result with all hits at `confidence < 0.3` returns `NotEnoughEvidence`.
- [ ] **3.6: Implement `composer::compose_cards`** — emits `Vec<MemoryCard>` with `surfacing_reason` populated; sorts by fused score; caps at 12.
- [ ] **3.7: Implement `composer::compose_answer`** — builds a context string from top fused hits + evidence pack; calls existing `InferenceEngine::answer(question, context_str)`; runs a post-hoc cite check (every file/command/decision mentioned in the answer must appear in `evidence`). On failure, fall back to `compose_partial_answer` which says what we have without inventing the rest.
- [ ] **3.8: Rewire `context_runtime::build_context_pack`** to the new pipeline. Keep the same public return shape; new optional fields `verify_outcome` and `surfacing_reasons` are appended.
- [ ] **3.9: Integration test** `tests/end_to_end_continuum_query.rs` with fixture DB: `run_query("how did we fix the planner bug")` → returns a `ComposedAnswer` where every cited file in `answer` also appears in `evidence.files` (verifier guarantee).
- [ ] **3.10: Telemetry.** `continuum.retrieval.fusion.ms`, `continuum.retrieval.evidence.{file,decision,command}.count`, `continuum.retrieval.verify.{grounded,partial,no_evidence}`.
- [ ] **3.11: Commit.**

### Verification

- `cargo test -p continuum context_runtime::` passes (existing + new tests).
- `make test` green.
- Fixture query produces a cited answer with verifier `Grounded` outcome.

### Smallest demoable outcome

`retrieval::run_query("how did we fix the planner bug")` returns `ComposedAnswer { answer, evidence, cards, verify_outcome: Grounded }` where every file path mentioned in `answer` appears in `evidence.files`.

---

## Phase 4 — MCP `continuum.*` namespace + new IPC

**Goal:** Expose the new pipeline through MCP and Tauri IPC without breaking existing tools.

### Files

Create:
- `src-tauri/src/ipc/commands/retrieval.rs` — Tauri commands `continuum_search`, `continuum_answer`, `continuum_build_context_pack`, `continuum_get_memory_subgraph`, `continuum_get_related_memories`, `continuum_quality_status`, `continuum_open_target`, `continuum_privacy_status`, `continuum_timeline`. Each is a 5–20 line wrapper.

Modify:
- `src-tauri/src/ipc/commands/mod.rs` — `pub mod retrieval;`.
- `src-tauri/src/lib.rs` — register new commands in `tauri::generate_handler!`.
- `src-tauri/src/mcp/mod.rs` — register `continuum.*` tools in the `tools/list` JSON payload and add dispatch arms in `tools/call`.
- `src-tauri/src/agent/context.rs` — `build_agent_context_pack` calls `retrieval::run_query` under the hood (delivers the same `AgentContextPack` shape).

### MCP tools to add

| Tool | Wraps |
|---|---|
| `continuum.search` | `retrieval::run_query` with `compose: "cards"` |
| `continuum.answer` | `retrieval::run_query` with `compose: "answer"` |
| `continuum.build_context_pack` | existing `context_runtime::build_context_pack` (upgraded under the hood) |
| `continuum.get_related_memories` | seed from `memory_id`, run only `Graph` route |
| `continuum.get_memory_subgraph` | `GraphIndex::neighbors(seed_ids, max_hops, allowed_edges)` |
| `continuum.timeline` | wraps `memory.timeline` |
| `continuum.open_target` | wraps existing `reopen_memory` |
| `continuum.privacy_status` | wraps existing `agent.privacy_status` |
| `continuum.quality_status` | aggregates `memory_quality::classify_storage_outcome` counts |

### Tasks

- [ ] **4.1: Add Tauri commands** in `retrieval.rs`. Use `#[tauri::command]` + `#[specta::specta]`. Define request/response types in the same file with serde + specta.
- [ ] **4.2: Wire `generate_handler!`** in `lib.rs`. Confirm `cargo build` passes.
- [ ] **4.3: Add MCP tool JSON schemas** in `mcp/mod.rs` `tools/list` static. Use copy-and-edit from existing `memory.search_full_context`.
- [ ] **4.4: Add dispatch arms** in `tools/call` (one per new tool).
- [ ] **4.5: Keep all existing `memory.*` and `agent.*` MCP tools registered.** Their handlers continue to work because the back-compat shim in `hybrid.rs` calls the new pipeline.
- [ ] **4.6: Refactor `agent::context::build_agent_context_pack`** to call `retrieval::run_query` and translate `ComposedAnswer` → `AgentContextPack`. Existing callers unchanged.
- [ ] **4.7: Integration test per new MCP tool** hitting the JSON-RPC endpoint via the test harness in `mcp/`.
- [ ] **4.8: Telemetry.** `continuum.mcp.{tool}.calls`, `continuum.mcp.{tool}.ms`.
- [ ] **4.9: Commit.**

### Verification

- `cargo test -p continuum mcp::` passes.
- Manual smoke: launch the app, `curl POST http://127.0.0.1:<port>/mcp -d '{"method":"tools/call","params":{"name":"continuum.answer","arguments":{"query":"..."}}}'` returns a JSON object with `answer`, `evidence`, `surfacing_reasons[]`.

### Smallest demoable outcome

A unit + integration test pair: `cargo test mcp::continuum_answer_smoke` runs the MCP endpoint locally and asserts a fixture query returns a grounded answer.

---

## Phase 5 — Frontend: "Why this surfaced", expanded cards, query-scoped graph, Copy for Agent

**Goal:** Expose the upgraded pipeline in the UI without breaking existing screens. Render a per-card "Why this surfaced" chip, an expanded card with evidence + subgraph + before/after timeline, a query-scoped subgraph view, and a "Copy for Agent" action.

### Files

Create:
- `src/domains/memory-vault/SurfacingReason.tsx` — single-line chip showing `surfacing_reason.headline`; tooltip on hover shows `routes` + `graph_path`.
- `src/domains/memory-vault/ExpandedMemoryCard.tsx` — modal/sheet on card click: evidence list (files, decisions, commands, errors, todos) + subgraph thumbnail + before/after timeline strip + actions (Open, Evidence, Related, Copy for Agent).
- `src/domains/memory-vault/QueryScopedGraph.tsx` — thin wrapper over `KnowledgeGraphCanvas` that takes `seedIds: string[]` + `maxHops: number` and renders the subgraph returned by `continuum_get_memory_subgraph`.
- `src/domains/memory-vault/CopyForAgentButton.tsx` — calls `continuum_build_context_pack({ query, mode: "agent" })`, renders the markdown into the clipboard, shows a toast.
- `src/shared/hooks/useContinuumAnswer.ts` — debounced wrapper calling `continuum_answer`.

Modify:
- `src/shared/ipc/tauri.ts` — add typed wrappers for each new IPC; add `MemoryCard.surfacing_reason?: SurfacingReason`; add `SurfacingReason`, `EvidencePack`, `VerifyOutcome` types matching specta-generated Rust types.
- `src/domains/memory-vault/MemoryCardsPanel.tsx` — render `<SurfacingReason>` chip under each card title when `card.surfacing_reason` is present; clicking the card opens `<ExpandedMemoryCard>`.
- `src/domains/memory-vault/KnowledgeGraphTopBar.tsx` — add `viewMode` selector: `all | project | session | decision | error | agent_context | query`.
- `src/domains/memory-vault/KnowledgeGraph.tsx` — when `viewMode === "query"`, source nodes/edges from `continuum_get_memory_subgraph({seed_ids: lastQueryMemoryIds, max_hops: 2})` instead of the all-memory builder. For other modes, filter the existing canvas data.
- `src/domains/memory-vault/graph/graphDataBuilder.ts` — accept an optional `seedIds` prefilter for query mode.
- `src/domains/search/SearchBar.tsx` — call `continuum_search` (new path) by default; gate behind a setting `useAgenticGraphRag` (default `true`).

### Tasks

- [ ] **5.1: Add specta-generated types** for `SurfacingReason`, `EvidencePack`, `VerifyOutcome`, `FusionSignals`, `RouteHit`, `GraphPathStep`. Confirm `npm run typecheck` passes.
- [ ] **5.2: Add IPC wrappers** in `tauri.ts` for each `continuum_*` command with proper TypeScript types.
- [ ] **5.3: Build `SurfacingReason`** with tailwind-equivalent styling matching the existing warm palette (`#FAF9F6` / `#3E2723` / `#E65100`).
- [ ] **5.4: Build `ExpandedMemoryCard`** by lifting evidence layout from `InsightLayers.tsx` and adding a graph subgraph thumbnail (16:9 mini-canvas).
- [ ] **5.5: Build `QueryScopedGraph`** reusing `graphLayoutEngine.ts` and `KnowledgeGraphCanvas`. Hairball test: ensure default node count ≤ 25 on a typical query.
- [ ] **5.6: Add `viewMode` state** to `KnowledgeGraphTopBar`; wire to `KnowledgeGraph` data-source switch.
- [ ] **5.7: Update `MemoryCardsPanel`** to render the chip + open expanded card on click. Preserve existing list/graph/project browse modes.
- [ ] **5.8: Add "Copy for Agent" button** in `ExpandedMemoryCard`. Render copied content as a markdown agent context pack.
- [ ] **5.9: Switch `SearchBar.onSubmit`** to call `continuum_search` by default. Keep `searchMemoryCards` callable behind the setting toggle.
- [ ] **5.10: Vitest coverage.**
  - `MemoryCardsPanel.test.tsx` renders `surfacing_reason` when present and not when absent.
  - `QueryScopedGraph.test.tsx` snapshots a fixture subgraph and asserts node count ≤ 25.
  - `CopyForAgentButton.test.tsx` calls the IPC and writes to clipboard mock.
- [ ] **5.11: Manual UX walkthrough** (`npm run tauri dev`).
  1. Type "planner debounce" → results show "Reached via Decision: 250ms debounce → File: plan.ts" headline.
  2. Click a card → expanded card shows the linked decision + file refs and a 6-node subgraph.
  3. Toggle graph view to `query` → hairball collapses to query-scoped subgraph.
  4. Click "Copy for Agent" → toast confirms; clipboard contains a markdown agent context pack.
- [ ] **5.12: Commit.**

### Verification

- `npm test` green.
- `npm run typecheck` green.
- Manual UX walkthrough passes the 4-step script above.

### Smallest demoable outcome

Query "planner debounce" surfaces 3 cards each with a one-line surfacing reason; clicking expands to evidence + subgraph; toggling graph view to `query` shows a small subgraph instead of the all-memory hairball.

---

## Phase 6 — Wiring, performance, full-system verification

**Goal:** Prove no regressions, hit latency budgets, document the new architecture.

### Files

Modify:
- `src-tauri/src/lib.rs` — confirm all new IPC commands and `pub mod graph;` are wired.
- `src-tauri/src/telemetry/` — confirm metrics from each phase land in the dashboard.
- `src-tauri/src/evals/` — add `retrieval_quality.rs` with 20 fixture queries and expected memory ids.
- `AGENTS.md` — add a 1-paragraph section pointing at `src-tauri/src/context_runtime/` + `src-tauri/src/graph/`.
- `docs/architecture/ARCHITECTURE.md` — update the pipeline diagram: capture → OCR → embedding → storage → **{query plan → routes → fusion → evidence → verify → compose}** → memory cards / answer / context pack.
- `docs/architecture/graph-schema.md` — append node/edge variant additions and the alias table.

### Tasks

- [ ] **6.1: Add tracing spans** at every retrieval stage. Confirm OpenTelemetry export still works.
- [ ] **6.2: Performance benchmark.** Add a `cargo bench` target `bench_retrieval_e2e` that runs 20 fixture queries on a 10k-memory DB. Targets: p50 ≤ 350 ms without LLM answer, p50 ≤ 800 ms with LLM answer.
- [ ] **6.3: Eval harness.** `retrieval_quality.rs` runs 20 fixtures and computes recall@5, MRR, and verifier-grounded rate. Run it manually after each phase ≥ 1.
- [ ] **6.4: Cleanup pass.** Delete dead branches in `hybrid.rs` now that fusion lives in `context_runtime/fusion.rs`. **Do NOT delete `hybrid.rs`** — it stays as the back-compat shim.
- [ ] **6.5: Regression smoke.**
  - Run `make test`.
  - Manually call the legacy IPC `searchMemoryCards` and `buildAgentContextPack` and confirm responses are identical or strictly better on a fixture DB.
  - Call legacy MCP tools `memory.search_full_context`, `agent.build_context_pack`, `memory.graph_context` and confirm responses validate against existing JSON schemas.
- [ ] **6.6: Update docs.**
- [ ] **6.7: Final commit + tag.**
  ```bash
  git commit -m "feat(retrieval): agentic graph rag — query plan, routes, fusion, evidence, verifier, composer"
  git tag agentic-graph-rag-v1
  ```

### Verification

- `make test` green.
- `cargo bench bench_retrieval_e2e` meets p50 targets.
- Recall@5 ≥ 0.7 and verifier-grounded rate ≥ 0.6 on the 20-fixture eval.
- All legacy IPC and MCP tools return identical or strictly better responses on the fixture DB.

### Smallest demoable outcome

A scripted demo:
1. A pre-existing query returns the same top result as before the refactor.
2. A new vague query "what was I doing with the Continuum knowledge graph?" returns a grounded answer citing a specific decision + file, plus 3 cards with "Why this surfaced".
3. The graph view in `query` mode shows the relevant 6-node subgraph instead of the all-memory hairball.
4. "Copy for Agent" produces a markdown context pack ready to paste into Claude Code.

---

## End-to-end verification checklist

- [ ] `make test` green.
- [ ] `cargo test -p continuum graph::` passes (Phase 0).
- [ ] `cargo test -p continuum context_runtime::` passes (Phases 1–3).
- [ ] `cargo test -p continuum mcp::continuum_` passes (Phase 4).
- [ ] `npm test` + `npm run typecheck` green (Phase 5).
- [ ] Legacy IPC: `searchMemoryCards`, `buildAgentContextPack` return identical-or-better results.
- [ ] Legacy MCP: `memory.search_full_context`, `agent.build_context_pack`, `memory.graph_context` validate against existing schemas.
- [ ] Manual UX walkthrough: 4-step demo passes.
- [ ] `cargo bench bench_retrieval_e2e` p50 ≤ 350 ms (no-LLM), ≤ 800 ms (LLM answer).
- [ ] Recall@5 ≥ 0.7, verifier-grounded ≥ 0.6 on 20-fixture eval.
- [ ] Telemetry visible in dashboard for planner / per-route / fusion / verifier.
- [ ] Documentation updated (`AGENTS.md`, `ARCHITECTURE.md`, `graph-schema.md`).
