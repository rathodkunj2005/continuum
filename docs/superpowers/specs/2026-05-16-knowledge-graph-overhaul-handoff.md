# Knowledge Graph Overhaul — Handoff Log

Companion to [`2026-05-16-knowledge-graph-overhaul-design.md`](2026-05-16-knowledge-graph-overhaul-design.md). Append one block per session.

---

## Session 1 — 2026-05-16

**Last commit on main (after Task 16):** `4952a01`

**Shipped:**

- `film` palette (Old Film dark / Archival Paper light) registered in [`src/shared/theme/cinematic-palettes.ts`](../../../src/shared/theme/cinematic-palettes.ts); now the default for fresh users.
- Brand-only tokens (halation, eases, fonts: Cormorant Garamond / EB Garamond / Cutive Mono) in [`src/shared/theme/film-paper.css`](../../../src/shared/theme/film-paper.css), imported from [`src/app/styles/index.css`](../../../src/app/styles/index.css).
- New typed graph data layer under [`src/domains/memory-vault/graph/`](../../../src/domains/memory-vault/graph/):
  `types`, `graphPalette`, `graphRelationshipResolver`, `graphDataBuilder`,
  `graphLegendBuilder`, `graphLayoutEngine`, `graphFilters` (scaffold).
- [`KnowledgeGraphCanvas.tsx`](../../../src/domains/memory-vault/KnowledgeGraphCanvas.tsx) (presentational SVG render).
- [`KnowledgeGraphSidePanel.tsx`](../../../src/domains/memory-vault/KnowledgeGraphSidePanel.tsx) (vertical right-side memory card; lazy-loads `getNodeDetail`).
- Rewritten composer [`KnowledgeGraph.tsx`](../../../src/domains/memory-vault/KnowledgeGraph.tsx); 357 → 110 lines; public prop API preserved verbatim.
- Hover dim + halation + edge-kind styling tied to `--cp-*` / `--film-*` tokens in [`KnowledgeGraph.css`](../../../src/domains/memory-vault/KnowledgeGraph.css).
- Vitest coverage:
  - `graph/__tests__/graphPalette.test.ts` (5 tests)
  - `graph/__tests__/graphRelationshipResolver.test.ts` (28 tests)
  - `graph/__tests__/graphDataBuilder.test.ts` (6 tests)
  - `graph/__tests__/graphLegendBuilder.test.ts` (4 tests)
  - `__tests__/KnowledgeGraph.hover.test.tsx` (3 tests)
  - **+46 new passing tests, 0 new regressions.**

**Verification:**

- `npm run typecheck` — PASS.
- `npm test` — 10 files / 56 tests pass. One pre-existing failure remains in `src/domains/memory-vault/MemoryCardsPanel.test.tsx:76` ("All Memories" tab); same as the S1 baseline, unrelated to graph work.
- `cargo test` — pre-existing build error in `src-tauri/tests/agent_regression.rs:180` (`fndr_lib::agent::validate_command` not yet wired up by the in-flight agent epic). Unchanged by S1; this session touched zero Rust files.

**Deliberately NOT done (S2):**

- Top filter bar UI (filter state exists in `graphFilters.ts`; controls + project/topic/entity/app pickers TODO).
- Right-side compact legend rendering (builder exists; presentational component TODO).
- Bottom-right zoom / reset / fit-to-graph / focus-selected controls.
- Keyboard shortcuts.
- Replacing the legacy `memory-graph-detail` aside in `MemoryCardsPanel.tsx` with `KnowledgeGraphSidePanel`. Both existing KG callsites currently pass `showSidePanel={false}` so the new card doesn't double up. Pick the full graph-stage callsite (line ~1216) first.

**Deliberately NOT done (S3):**

- Graph cache + hourly *active-use* refresh.
- LOD / virtualization / incremental layout.
- Loading / error / skeleton states (basic empty state shipped).
- App-wide theme migration (only main.tsx default changed; existing panel CSS still references the previous palette tokens).
- Accessibility audit + keyboard reachability sweep.
- Cursor amber trail; node drift animation (sine ±1px / 6s) from the design bundle.

**Where to resume:**

S2 plan: `docs/superpowers/plans/2026-05-16-knowledge-graph-overhaul-s2.md` (to be written when S2 starts). The natural starting move is replacing the legacy aside in `MemoryCardsPanel.tsx:~1216` with `<KnowledgeGraphSidePanel>` and wiring its `onOpenContext` / `onFilterRelated` props to the existing state in that file.

**Open concerns:**

- Node drift animation (sine ±1px / 6s phase per node) is documented in the design bundle but not yet wired — would need a separate `requestAnimationFrame` loop or per-node CSS keyframes; deferred to S3 perf pass.
- `getNodeDetail` currently returns the same `InsightGraphNode` shape that's already in the view. The side panel pulls `preview`/`summary` from `metadata`; confirm this is the intended source or extend IPC to surface a richer preview field.
- The skip-worktree bit on `src/app/styles/index.css` was cleared during Task 2 in order to push the `@import`. 32 other CSS / TSX files in `src/` still have skip-worktree set — if those need similar updates in S2/S3, the same `git update-index --no-skip-worktree <path>` step will be needed.

---

## Session 2 — 2026-05-16

**Last commit on main (after Task 11):** `2e9176f`

**Shipped:**

- `graphFilterOptions.ts` — `deriveFilterOptions(view)` returns the distinct `nodeTypes / projects / topics / edgeKinds` actually present in the view plus the `[minConf, maxConf]` confidence range. Pure, fully tested.
- Backfilled `graphFilters.test.ts` covering `applyFilters` for nodeType, project, confidence, and edgeKind paths plus the no-active-filters identity case.
- Canvas refactored to a `forwardRef` component with an imperative `KnowledgeGraphCanvasHandle` exposing `zoomIn / zoomOut / reset / fit`. The d3-zoom instance is captured in a ref so external chrome can drive it without reimplementing behavior.
- New presentational components:
  - [`KnowledgeGraphZoomControls.tsx`](../../../src/domains/memory-vault/KnowledgeGraphZoomControls.tsx) — bottom-right `+ / − / ⊕ / ⌂` group calling the canvas handle.
  - [`KnowledgeGraphLegend.tsx`](../../../src/domains/memory-vault/KnowledgeGraphLegend.tsx) — collapsible top-right panel, one row per community / edge-kind / encoding, swatch shapes for dot / ring / dash / dot-dot / arrow.
  - [`KnowledgeGraphTopBar.tsx`](../../../src/domains/memory-vault/KnowledgeGraphTopBar.tsx) — `type / project / edge` multi-select pills with checkbox menus, `min conf` slider, `clear · N` reset button. All option lists derived from real data.
- Composer wired:
  - Holds `filterState` (defaults `EMPTY_FILTERS`), derives `filterOptions` from the full view, computes a `filteredView` for the canvas + legend.
  - New props `showFilters`, `showLegend`, `showZoomControls` (all default `true`).
  - Keyboard shortcuts scoped to the shell element: `+`/`=` zoom in, `-`/`_` zoom out, `0` reset, `f` fit, `Escape` clear selection. Skipped when an input / textarea / contentEditable target has focus.
- All new chrome styled through `--cp-*` and `--film-*` tokens so palette and light/dark mode swaps continue to drive the look.
- `MemoryCardsPanel.tsx`:
  - Strip caller (height=220) now passes all four `show*={false}` props for a clean overview.
  - Graph-stage caller (height=420) now defaults the new side panel + top bar + legend + zoom controls *on* while the legacy `memory-graph-detail` aside remains below the canvas for its "Build Path" / semantic-search affordances.

**Verification:**

- `npm run typecheck` — PASS.
- `npm test` — 12 files / 65 tests pass. Same single pre-existing failure in `MemoryCardsPanel.test.tsx:76` ("All Memories" tab); unrelated to graph work. **+9 new passing tests** since S1.
- `cargo test` — same pre-existing build error in `tests/agent_regression.rs:180` from the in-flight agent epic; S2 touched zero Rust files.

**Deliberately NOT done (S3):**

- Graph cache + hourly *active-use* refresh (track app-used time, not wall-clock; rebuild in background, preserve hover/selection/zoom state).
- LOD / virtualization / incremental layout for thousand-node graphs.
- Skeleton / loading / explicit error states for the canvas.
- App-wide theme migration (currently only main.tsx default + new graph chrome use film tokens; other panels still reference the previous palette tokens).
- Accessibility audit + tab-order sweep.
- Folding the legacy `memory-graph-detail` aside (Build Path + semantic search inputs) into the new `KnowledgeGraphSidePanel` actions.
- Node drift animation (sine ±1px / 6s phase per node) and cursor amber trail from the design bundle.
- Topic + date pickers in the top bar (data shape exists in `deriveFilterOptions`; UI deferred to S3).

**Where to resume:**

S3 plan: `docs/superpowers/plans/2026-05-16-knowledge-graph-overhaul-s3.md` (to be written when S3 starts). The natural opener is the cache + hourly active-use refresh — it concentrates the architectural risk and is the only S3 item that touches data flow rather than chrome.

**Open concerns:**

- The d3 mouseenter pathway through jsdom is unreliable in tests; we drive neighborhood-highlight assertions through the `selectedNodeId` prop instead. Real-browser hover works fine (the same `data-state` effect runs on either input). If S3 adds further interactive tests, prefer prop-driven scenarios over `fireEvent.mouseEnter`.
- The legacy `memory-graph-detail` aside duplicates some content with the new side panel (label, timestamp). Users in the graph-stage view now see two panels for the selected node. Decide in S3 whether to (a) port "Build Path" and semantic-search into the new side panel's actions and delete the legacy aside, or (b) hide the legacy aside when the new side panel is visible.
