# CLEANUP_AUDIT.md

## Scope and method
This audit was run on the existing FNDR codebase (`/Users/anurupkumar/fndr`) before structural edits, using repo scans, compiler/test feedback, and import/dependency tracing.

## Current total LOC (baseline)
- Baseline LOC before this slice (`rg --files` excluding `node_modules/.git/src-tauri/target/dist/build/coverage`): **89,246**
- LOC after this slice: **89,215**
- Requested target removal: **20,000 LOC**
- Current net removal in this slice: **31 LOC** (plus a large reduction in hardcoded policy branches and API surface)

## Top 30 largest files (baseline)
1. `src-tauri/Cargo.lock` — 10,893
2. `src-tauri/src/api/commands.rs` — 8,609
3. `src-tauri/src/store/lance_store.rs` — 6,433
4. `src-tauri/src/mcp/mod.rs` — 4,335
5. `src-tauri/src/capture/mod.rs` — 4,227 (now 4,103 after split)
6. `src-tauri/src/context_runtime/mod.rs` — 2,998
7. `src-tauri/src/meeting/mod.rs` — 2,526
8. `src-tauri/src/search/hybrid.rs` — 2,345
9. `src-tauri/src/search/memory_cards.rs` — 1,641
10. `src-tauri/src/store/schema.rs` — 1,285
11. `src-tauri/src/inference/mod.rs` — 1,278
12. `src/api/tauri.ts` — 1,259 (now 1,202)
13. `src/components/AgentPanel.css` — 1,250
14. `src/components/ControlPanel.tsx` — 1,237
15. `src/components/AutofillOverlay.tsx` — 1,237
16. `src/components/AgentPanel.tsx` — 1,128
17. `src/components/ControlPanel.css` — 1,127
18. `src-tauri/src/config.rs` — 1,092
19. `src/styles/App.css` — 1,013
20. `src-tauri/src/embed/onnx.rs` — 919
21. `src-tauri/src/capture/text_cleanup.rs` — 910
22. `src-tauri/src/api/onboarding.rs` — 854
23. `src-tauri/src/graph/mod.rs` — 793
24. `src/components/StatsPanel.css` — 787
25. `src-tauri/src/speech.rs` — 786
26. `src-tauri/src/accessibility/mod.rs` — 716
27. `src-tauri/src/ocr/vision.rs` — 713
28. `src/components/SearchBar.tsx` — 675
29. `src-tauri/src/capture/macos.rs` — 634
30. `src/App.tsx` — 605

## Dead/unused inventory

### Dead exports / wrappers
- Unused TypeScript API wrappers (no import sites in `src/`):
  - `getCaptureQualityDashboard`
  - `getContextPackDetail`
  - `exportMeetingPdf`
  - `speakText`
  - `executeTodo`
  - `showAutofillOverlayWindow`
- All six were removed in this slice.

### Unused components / CSS / hooks
- Compiler import graph (`npx tsc --noEmit --listFiles`) includes all TS/TSX files under `src/`.
- CSS import graph audit found no fully unimported CSS files.
- Result: no safe full-file frontend deletions discovered in this pass.

### Unused/extra command surface signals
- Tauri command registration count: 97
- Frontend invoked command names found across `src/api/*.ts`: 92
- 5 registered-but-not-invoked names are internal helpers / intentionally non-frontend entrypoints:
  - `create_autofill_overlay_window`
  - `register_autofill_shortcut`
  - `reclaim_memory_storage_silent`
  - `get_stats` (frontend wrapper uses it indirectly)
  - `link_audio_to_memories`

## Duplicate helper inventory

### Frontend duplicates (confirmed)
- `tokenOverlap` + tokenization logic duplicated between:
  - `src/components/Timeline.tsx`
  - `src/lib/cardCleanup.ts`
- Consolidated in this slice by reusing `tokenOverlap` from `cardCleanup.ts`.

### Rust duplicates (high-value candidates)
- `extract_domain`-like logic repeated across capture/search/store modules.
- `truncate_chars`-style helpers repeated in multiple modules.
- `normalize_text` variants repeated in OCR/search query processing.
- These are still candidates for next slices; not all can be merged safely without boundary work.

## Hardcoding / overfitting inventory

### Capture overfit hotspots (pre-slice)
- `src-tauri/src/capture/mod.rs` had site/domain-specific branching in admission policy:
  - `youtube.com`, `google.`, `bing.com`, `duckduckgo.com`, `x.com/twitter.com`, `linkedin.com`, `reddit.com`
  - title hardcoding like `videos - youtube`
- This was replaced in this slice with generalized URL/title heuristics in `src-tauri/src/capture/admission.rs`.

### Remaining hotspots (not yet removed)
- `src/components/DailyBriefing.tsx` app-name keyword category mapping.
- `src/components/MemoryCardsPanel.tsx` app-name keyword clustering buckets.
- Some search/story-label heuristics in `Timeline.tsx` and `search/memory_cards.rs` still keyword-heavy.

## Repeated constants/config inventory
- `src-tauri/src/config.rs` is the intended config center and is already dense with defaults.
- Remaining repetition still appears in:
  - ad hoc UI keyword lists in component files
  - module-local thresholds not yet normalized under capture/search config surfaces

## Brittle if/else replacing model/generalized decisions
- Capture admission had hardcoded domain branching (now reduced).
- Autofill alias group logic is still static and domain-linguistic (not app-domain specific, but still rules-heavy).
- Search rerank + story quality still combine many deterministic branches; no model-assisted collapse yet in this slice.

## Capture/privacy/storage/search bottleneck inventory
- Major maintenance bottlenecks remain in giant files:
  - `api/commands.rs` (8.6k lines)
  - `capture/mod.rs` (still 4.1k after split)
  - `store/lance_store.rs` (6.4k)
- Command/file boundaries are still too broad for localized refactors and isolated testing.
- Resource governance exists but is distributed; not yet a single resource governor mode controller.

## Current tests covering touched areas
- Frontend:
  - `src/components/Timeline.test.tsx`
  - `src/components/SearchBar.test.tsx`
  - `src/components/ControlPanel.test.tsx`
  - `src/components/MemoryCardsPanel.test.tsx`
  - `src/hooks/useSearch.test.tsx`
- Rust capture/search/privacy/store/graph/mcp filters executed.
- Known environment-sensitive failures:
  - `mcp::tests::localhost_initialize_tools_list_and_call_work_without_auth` (port probe permission)
  - `http_util::tests::clients_build` (system-configuration NULL object in this environment)

## Safe deletion candidates (next slices)
1. Additional unused TS exports/interfaces in `src/api/tauri.ts` after import-graph confirmation.
2. Redundant utility duplicates where one canonical implementation already exists.
3. Stale test fixtures/keywords tied to old naming where behavior is unchanged.
4. Obsolete docs superseded by architecture map + refactor reports.

## Risky deletion candidates (needs focused characterization tests)
1. Large sections of `api/commands.rs` (public Tauri surface, many side effects).
2. Legacy paths in `store/lance_store.rs` around migration/repair/compaction.
3. Search ranking branches in `search/hybrid.rs` and `search/memory_cards.rs`.
4. Meeting and MCP integrations where environment assumptions are fragile.

## Large-file split plan (behavior-preserving)
1. `src-tauri/src/api/commands.rs`
   - `api/commands/mod.rs`
   - `api/commands/autofill.rs`
   - `api/commands/hermes_agent.rs`
   - `api/commands/maintenance.rs`
   - `api/commands/search.rs`
   - `api/commands/memory.rs`
   - re-export public commands to preserve `main.rs` registration names.
2. `src-tauri/src/capture/mod.rs`
   - split started: `capture/admission.rs` added.
   - next: `capture/salience.rs`, `capture/dedup.rs`, `capture/resource_governor.rs` from existing logic blocks.
3. Frontend giant components
   - `Timeline.tsx` cleanup started (dedupe extraction).
   - next: `SearchBar.tsx`, `AutofillOverlay.tsx`, `ControlPanel.tsx` split by state/presenter/helpers.

## Pipeline inefficiency inventory
- Admission still runs mostly in one broad loop file with many responsibilities.
- Backpressure/resource policy not fully centralized into one mode contract.
- Search/card heuristics still include duplicated lexical logic in separate modules.

## Verification plan
For each vertical slice:
1. `npm run typecheck`
2. `npm test -- --run`
3. `cd src-tauri && cargo check`
4. Targeted tests for touched modules:
   - `cargo test --lib capture`
   - `cargo test --lib store`
   - `cargo test --lib search`
   - `cargo test --lib privacy`
   - `cargo test --lib graph`
   - `cargo test --lib mcp`
5. Periodically run full `cargo test --lib` and compare failure set.

