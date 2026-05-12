# FNDR Cleanup Report

Behavior-preserving mass cleanup. No drive-by features, no Screenpipe sidecar reintroduction, no LanceDB / embedding / search-ranking changes. Public Tauri command names are unchanged.

## 1. Summary

| Area | Delta |
| --- | --- |
| Files deleted (frontend) | 6 (`ChatPanel.tsx`, `ChatPanel.css`, `TodoModal.tsx`, `TodoModal.css`, `AskFndr.css`, `MemoryReconstructionPanel.css`) |
| Files deleted (lib pass-through) | 2 (`src/lib/memorySearch/nativeSearch.ts`, `types.ts`) and the now-empty `memorySearch/` directory |
| Files added (frontend) | 1 (`src/lib/config.ts` — typed central config) |
| Net lines removed (frontend) | **≈ 1,051 deleted vs ≈ 200 added → ≈ -850 net** |
| Net lines removed (Rust) | **≈ -45 net** (PDF font helper consolidation, magic-number cleanup in `main.rs` is roughly line-neutral) |
| Hardcoded values centralized | 25+ |
| Tauri command surface | **unchanged** (all 83 commands still registered from `api::commands::*`) |
| Verification | `npm run typecheck` ✅ · `npm test -- --run` 9/9 ✅ · `cargo check` ✅ · targeted `cargo test config:: / http_util` 5/5 ✅ |

## 2. Inventory before cleanup

### 2a. Top frontend files (lines)

| Rank | File | Lines |
| --- | --- | --- |
| 1 | `src/api/tauri.ts` | 1260 |
| 2 | `src/components/AgentPanel.css` | 1250 |
| 3 | `src/components/AutofillOverlay.tsx` | 1237 |
| 4 | `src/components/ControlPanel.tsx` | 1236 |
| 5 | `src/components/AgentPanel.tsx` | 1128 |
| 6 | `src/components/ControlPanel.css` | 1127 |
| 7 | `src/styles/App.css` | 1013 |
| 8 | `src/components/StatsPanel.css` | 787 |
| 9 | `src/components/SearchBar.tsx` | 668 |
| 10 | `src/components/Onboarding.tsx` | 633 |
| 11 | `src/App.tsx` | 605 |
| 12 | `src/components/StatsPanel.tsx` | 579 |
| 13 | `src/components/PipelineInspectorPanel.tsx` | 578 |
| 14 | `src/components/MemoryCardsPanel.tsx` | 571 |
| 15 | `src/components/CommandPalette.tsx` | 561 |

### 2b. Top Rust files (lines)

| Rank | File | Lines |
| --- | --- | --- |
| 1 | `src-tauri/src/api/commands.rs` | 8626 |
| 2 | `src-tauri/src/store/lance_store.rs` | 6172 |
| 3 | `src-tauri/src/mcp/mod.rs` | 4335 |
| 4 | `src-tauri/src/capture/mod.rs` | 3725 |
| 5 | `src-tauri/src/context_runtime/mod.rs` | 2887 |
| 6 | `src-tauri/src/meeting/mod.rs` | 2526 |
| 7 | `src-tauri/src/search/hybrid.rs` | 2345 |
| 8 | `src-tauri/src/search/memory_cards.rs` | 1573 |
| 9 | `src-tauri/src/store/schema.rs` | 1285 |
| 10 | `src-tauri/src/inference/mod.rs` | 1278 |

### 2c. Hardcode hotspots

- `src-tauri/src/api/commands.rs`: literal Ollama URLs `http://127.0.0.1:11434/...` scattered, `127.0.0.1` host in `HERMES_API_HOST`, PDF font paths repeated, 50+ lines of identical `genpdf::fonts::FontData::load(...)` boilerplate across `export_meeting_pdf` and `export_daily_summary_pdf`.
- `src-tauri/src/mcp/mod.rs`: `"127.0.0.1".to_string()` literal in `McpRuntime::default` while a `LOOPBACK_HOST` constant already existed in the same file.
- `src-tauri/src/main.rs`: 12+ inline `Duration::from_secs(...)` / `* 86_400_000` / `0.5` / `8` magic numbers in the background task wiring (decay job, briefing job, stale-task job, context-switch detector, storage reclaim, tokio stack size).
- `src/App.tsx`: `30_000`, `2000`, `60_000`, `8_000`, `4` toast/poll timings.
- `src/hooks/useSearch.ts`: `BASE_SEARCH_TIMEOUT_MS`, `SEARCH_RESULT_LIMIT`, `40`, `6000`, `20`, `450`, `4000` adaptive-timeout knobs.
- `src/components/SearchBar.tsx`: `PLACEHOLDER_DISPLAY_DURATION`/`FADE_DURATION`, `320`, `5`, `600`, `0.3`, `10`, `2`, `350`, `250`, `48_000`, `128_000`, `2000` recorder + summary knobs.
- `src/components/SearchHistoryPanel.tsx`, `AutomationPanel.tsx`, `ControlPanel.tsx`, `main.tsx`: four duplicated `localStorage` key string literals (`"fndr-theme"`, `"fndr-palette"`, `"fndr-automations"`, `"fndr-search-history"`).

### 2d. Duplication hotspots

- `truncate_chars` defined three times: `inference/mod.rs:77`, `accessibility/mod.rs:196`, `api/commands.rs:368`. The accessibility and commands versions are byte-identical; the inference version differs slightly (uses `saturating_sub(3)` to reserve room for the trailing `...`).
- The two PDF exports in `api/commands.rs` shared ~50 lines of duplicate font-loading boilerplate.
- `src/lib/memorySearch/nativeSearch.ts` was a pass-through wrapper for `searchMemoryCards` with no behavior of its own — used by exactly two callers.
- `chatWithGemma()` in `src/api/tauri.ts` was a 5-line wrapper around `summarize_search` referencing a no-longer-supported model name, called only by the unused `ChatPanel`.

## 3. Changes by area

### 3a. Frontend — dead-code removal (verified unused via ripgrep)

| Path | Why safe to delete |
| --- | --- |
| `src/components/ChatPanel.tsx` (152 lines) | `rg "ChatPanel"` showed only its self-import. No `<ChatPanel>` JSX anywhere; `AppPanels.tsx` does not mount it. |
| `src/components/ChatPanel.css` (284 lines) | Imported only by the deleted `ChatPanel.tsx`. |
| `src/components/TodoModal.tsx` (77 lines) | `rg "\bTodoModal\b"` showed only its self-definition. No call site; only `TodoPanel` (a different component) is actually used. |
| `src/components/TodoModal.css` (145 lines) | Imported only by the deleted `TodoModal.tsx`. |
| `src/components/AskFndr.css` (107 lines) | `rg "AskFndr"` outside the file only matched a Rust type name (`AskFndrArgs` in `mcp/mod.rs`). No corresponding `.tsx` exists, no class references. |
| `src/components/MemoryReconstructionPanel.css` (194 lines) | `rg "MemoryReconstruction"` outside the file matched only a Rust internal type (`MemoryReconstruction` in `graph/mod.rs`). No `.tsx` corresponds. |
| `src/lib/memorySearch/nativeSearch.ts` (16 lines) | Pass-through wrapper for `searchMemoryCards`. Both callers (`useSearch.ts`, `SearchBar.tsx`) now call `searchMemoryCards` directly. |
| `src/lib/memorySearch/types.ts` (4 lines) | Only consumed by `nativeSearch.ts`. Inline `timeFilter` / `appFilter` already exist on the underlying API. |
| `chatWithGemma` + `ChatMessage` interface (13 lines in `src/api/tauri.ts`) | Sole caller was the deleted `ChatPanel`. The underlying Tauri command `summarize_search` is still exposed via the named `summarizeSearch` export. |
| `export` keyword removed from `RAW_PLACEHOLDERS` (`placeholders.ts`) | Only consumer is the same file's `PLACEHOLDERS = shuffle(RAW_PLACEHOLDERS)`. |
| `export` keyword removed from `loadSearchHistory` (`SearchHistoryPanel.tsx`) | Only callers are inside the same file. The actually-public `appendToSearchHistory` and `SearchHistoryPanel` stay exported. |

### 3b. Frontend — splits / moves (one new module)

- **Created `src/lib/config.ts`** — single typed home for storage keys, polling cadences, toast and search-bar timings. Consolidates 25+ scattered magic numbers and 4 duplicated localStorage key strings. Imported by `App.tsx`, `main.tsx`, `SearchBar.tsx`, `SearchHistoryPanel.tsx`, `AutomationPanel.tsx`, `ControlPanel.tsx`, `useSearch.ts`.

Big TSX files (`SearchBar.tsx` 668, `App.tsx` 605, `ControlPanel.tsx` 1236, `AutofillOverlay.tsx` 1237, `AgentPanel.tsx` 1128) were **not split this pass**. They are flagged in §7 with a recommended cut.

### 3c. Frontend — hardcodes moved to `src/lib/config.ts`

| Old value (location) | New constant |
| --- | --- |
| `"fndr-theme"` in `ControlPanel.tsx`, `main.tsx` | `STORAGE_KEYS.theme` |
| `"fndr-palette"` in `ControlPanel.tsx`, `main.tsx` | `STORAGE_KEYS.palette` |
| `"fndr-automations"` in `AutomationPanel.tsx` | `STORAGE_KEYS.automations` |
| `"fndr-search-history"` in `SearchHistoryPanel.tsx` | `STORAGE_KEYS.searchHistory` |
| `BASE_SEARCH_TIMEOUT_MS=6_000`, `SEARCH_RESULT_LIMIT=12`, debounce `40`, char/word bonuses `20`/`450`/`6000` cap, retry `4000` in `useSearch.ts` | `SEARCH_LIMITS.{baseTimeoutMs, resultLimit, typingDebounceMs, perCharBonusMs, perWordBonusMs, timeoutBonusCapMs, retryBonusMs}` |
| `30` history cap in `SearchHistoryPanel.tsx` | `SEARCH_HISTORY.maxEntries` |
| Toast `8_000` duration + `4`-stack limit in `App.tsx` | `TOAST.defaultDurationMs`, `TOAST.stackLimit` |
| Polling `30_000`/`2000`/`60_000` in `App.tsx` | `POLL_INTERVALS.{appNamesMs, captureStatusMs, clockTickMs}` |
| Automation tick `60_000` in `AutomationPanel.tsx` | `POLL_INTERVALS.automationsMs` |
| `@memory` popover: trigger `2`, debounce `320`, limit `5` in `SearchBar.tsx` | `MEMORY_MENTIONS.{minQueryLength, debounceMs, limit}` |
| Placeholder cycling `3000`/`400` in `SearchBar.tsx` | `SEARCH_PLACEHOLDER.{displayDurationMs, fadeDurationMs}` |
| Summary delay `600`, coverage `0.3`, card cap `5`, snippets `10`/`2` in `SearchBar.tsx` | `SEARCH_SUMMARY.{delayMs, coverageFloor, maxCards, maxSnippets, snippetsPerCard}` |
| Voice recorder: sample `48000`, ch `1`, slice `250`, min `350`, bitrate `128_000`, status `2000` in `SearchBar.tsx` | `VOICE_RECORDING.{sampleRate, channelCount, timesliceMs, minDurationMs, audioBitsPerSecond, statusClearMs}` |

### 3d. Rust — `api/commands.rs`

- **Added** named constants for local-service endpoints at the top of the Hermes/agent section (one place — was three scattered string literals):
  - `HERMES_API_HOST`, `HERMES_API_PORT` (already existed)
  - `OLLAMA_HOME_URL = "http://127.0.0.1:11434"` (was an inline error-message literal)
  - `OLLAMA_BASE_URL` (already existed)
  - `OLLAMA_API_TAGS_URL = "http://127.0.0.1:11434/api/tags"` (was an inline `.get(...)` literal)
- **Replaced inline literals** in `detect_ollama_state()` and the "FNDR could not reach Ollama at …" error in `validate_hermes_gateway_prerequisites` to read from the constants above. Error message text remains identical via `format!`.
- **De-duplicated** the PDF font-loading boilerplate. `export_meeting_pdf` and `export_daily_summary_pdf` each had ~25 lines of identical `genpdf::fonts::FontData::load(...)` calls. Both now call a single `load_pdf_font_family() -> Result<FontFamily, String>` helper. Net delete: ~34 lines.
- Added `MACOS_SUPPLEMENTAL_FONT_DIR` (`/System/Library/Fonts/Supplemental`) and `PDF_PAGE_MARGIN: u8 = 18` constants so the font path and page margin are now named in one place.

### 3e. Rust — `main.rs`

Twelve previously inline magic numbers / Durations promoted to named module-level constants. The behavior (intervals, thresholds, stack size) is byte-identical; only readability and configuration locality changed.

| Old inline value | New constant | What it controls |
| --- | --- | --- |
| `8 * 1024 * 1024` | `TOKIO_WORKER_STACK_BYTES` | Tokio worker stack |
| `Duration::from_secs(60)` (startup) | `MAINTENANCE_FIRST_DELAY` | Storage-reclaim warm-up wait |
| `Duration::from_secs(6 * 3600)` (×2) | `STORAGE_RECLAIM_INTERVAL`, `DECAY_INTERVAL` | Background job cadence |
| `24 * 3600 * 1000` | `DECAY_LOOKBACK_MS` | Decay query window |
| `0.15` | `DECAY_FLOOR` | Ebbinghaus floor |
| `86_400_000.0` | `MS_PER_DAY` | day-conversion divisor |
| `Duration::from_secs(60)` (proactive notif) | `PROACTIVE_NOTIFICATION_STARTUP_DELAY` | Proactive subsystem warm-up |
| `Duration::from_secs(30)` | `PROACTIVE_NOTIFICATION_TICK` | Proactive tick |
| `Duration::from_secs(7200)` | `STALE_TASK_CHECK_INTERVAL` | Stale-task scan cadence |
| `3 * 86_400_000` | `STALE_TASK_THRESHOLD_MS` | Stale-task age cutoff |
| `3` | `STALE_TASK_TITLES_SHOWN` | Titles shown in notification |
| `Duration::from_secs(10)` | `APP_SWITCH_SAMPLE_INTERVAL` | App-switch sampling cadence |
| `15` / `20` / `8` / `6` | `APP_SWITCH_WINDOW` / `APP_SWITCH_RECENT_CAPACITY` / `APP_SWITCH_THRESHOLD` / `APP_SWITCH_UNIQUE_THRESHOLD` | Context-switch heuristic knobs |
| `3` / `20` | `BRIEFING_MIN_MEMORIES` / `BRIEFING_MAX_CARD_LINES` | Daily-briefing thresholds |

### 3f. Rust — `mcp/mod.rs`

- `McpRuntime::default { host: "127.0.0.1".to_string(), … }` now reads `host: LOOPBACK_HOST.to_string()`. The `LOOPBACK_HOST = "127.0.0.1"` constant was already declared further down in the same file — this just removes the duplicate string literal.

### 3g. Docs

- No `docs/ARCHITECTURE.md` table change is necessary: no module boundaries actually moved, and the public Tauri command list is unchanged.
- The pre-existing `docs/ARCHITECTURE.md` modification (`http_util` row) is unrelated to this cleanup and is left alone.

## 3a. Naming changes

Intentionally minimal this pass — public Tauri command names, exported React component names, and exported hooks all keep their existing names.

| Old → New | Reason |
| --- | --- |
| `export const RAW_PLACEHOLDERS` → `const RAW_PLACEHOLDERS` (`placeholders.ts`) | Same-file only; no external consumer. |
| `export function loadSearchHistory` → `function loadSearchHistory` (`SearchHistoryPanel.tsx`) | Same-file only; reduces accidental surface. |
| Inline string literal `"http://127.0.0.1:11434/api/tags"` → `OLLAMA_API_TAGS_URL` | One named source for the Ollama probe URL. |
| Inline `"http://127.0.0.1:11434"` in error text → `OLLAMA_HOME_URL` via `format!` | Same user-visible error text, derived from the same constant. |
| `"127.0.0.1".to_string()` in `McpRuntime::default` → `LOOPBACK_HOST.to_string()` | Reuses an existing constant in the same module. |

## 4. Behavior-preservation notes

Risks considered and how each was preserved:

1. **Privacy / blocklist / retention.** No touch. `Blocklist`, `dismiss_privacy_alert`, `set_blocklist`, `delete_older_than`, `set_retention_days` paths are untouched; the privacy-related defaults in `Config::default()` (1Password, Keychain Access, etc.) are unchanged.
2. **Biometric lock.** Lock screen + `request_biometric_auth` + `OnboardingState.biometric_enabled` flow unchanged. The `disable biometric lock` path in `App.tsx` still calls `getOnboardingState` / `saveOnboardingState` exactly as before.
3. **Public Tauri command names.** All 83 `#[tauri::command]` entries in `main.rs::generate_handler!` are still resolvable to the same `api::commands::*` paths. The cleanup did not move any command into a submodule (deferred — see §7).
4. **Search ranking knobs.** `SearchConfig`, `MemoryCardConfig`, `StoreConfig`, `ProactiveConfig`, `MemoryQualityConfig` in `src-tauri/src/config.rs` are untouched. No hybrid weights, candidate multipliers, or rerank thresholds changed.
5. **Embedding contract.** `EmbeddingConfig`, the 1024-dim invariant, and the validate-at-startup check are all unchanged.
6. **LanceDB schema.** `src-tauri/src/store/` was not modified.
7. **No Screenpipe reintroduction.** Grep confirms remaining "Screenpipe" occurrences are inside `#[cfg(test)]` test fixtures in `capture/mod.rs` and `capture/macos.rs` (asserting OCR parsing for a sample title — not a sidecar). They are left untouched.
8. **Capture rate / dedup / VLM knobs.** `CapturePipelineConfig` is unchanged. Background-task cadence in `main.rs` is constant-renamed but numerically identical.
9. **Internal-result filtering.** `strip_internal_fndr_results` / `is_internal_fndr_result` paths are untouched.
10. **`useSearch` hook.** The previous timeout formula (`base + len*20 + words*450 + retry*4000`, each capped at 6000) is preserved exactly via `SEARCH_LIMITS.*` — only the literals moved.
11. **Voice recorder behavior.** `getUserMedia` constraints, `MediaRecorder` timeslice, and minimum recording duration are constant-renamed, numerically identical.
12. **Toast queue.** `enqueueToast(toast, durationMs?)` default duration was `8_000`, queue cap `4`. Now `TOAST.defaultDurationMs = 8_000` and `TOAST.stackLimit = 4`.
13. **Hermes / Ollama / Codex agent flows.** No behavior change. The `format!("FNDR could not reach Ollama at {OLLAMA_HOME_URL}. …")` produces the same exact string as the previous literal.

## 5. Verification

Commands run, in this order:

| # | Command | Result |
| --- | --- | --- |
| 1 | `cd /Users/anurupkumar/fndr && npm run typecheck` | ✅ tsc clean (no errors) |
| 2 | `cd /Users/anurupkumar/fndr && npm test -- --run` | ✅ 9/9 tests pass across 5 files (`useSearch.test.tsx`, `Timeline.test.tsx`, `SearchBar.test.tsx`, `ControlPanel.test.tsx`, `MemoryCardsPanel.test.tsx`) |
| 3 | `cd /Users/anurupkumar/fndr/src-tauri && cargo check` | ✅ clean compile, no warnings |
| 4 | `cd /Users/anurupkumar/fndr/src-tauri && cargo test --lib config::` (touched config-adjacent code) | ✅ 4/4 pass (`default_config_validates`, `rejects_embedding_dimension_mismatch`, `rejects_zero_search_weights`, `memory_card_defaults_enable_llm_group_synthesis`) |
| 5 | `cd /Users/anurupkumar/fndr/src-tauri && cargo test --lib http_util` (touched http_util.rs siblings) | ✅ 1/1 pass (`clients_build`) |

Why **not** a full `cargo test`: the task brief explicitly recommends targeted module tests to avoid locking the artifact directory for several minutes. The modules I actually touched (`config`, `http_util`, `main`, `api/commands` — which compiles via `cargo check`) are covered by 1, 3, 4, 5.

## 6. Residual risks / unmeasured assumptions

- **`commands.rs` is still 8,592 lines / 261 functions in one file.** I deliberately did not split it this pass; the autofill section alone is ~1,100 lines tangled with `AppState`, `AppHandle<R>`, `AutofillConfig`, and `accessibility::FieldContext`. A split was scoped (see §7) but moving 9 Tauri commands with their private helpers in one slice carries a real risk of breaking the `api::commands::<name>` paths registered in `main.rs`. The user explicitly authorized documenting the proposed split rather than forcing it.
- **`truncate_chars` is still defined three times** (`inference/mod.rs`, `accessibility/mod.rs`, `api/commands.rs`). Two are byte-identical; the third has a 3-char reservation difference. Consolidating into a new `text_util` module would create a new layer — flagged as deferred per anti-bloat skill, not done as a drive-by.
- **`AutofillOverlay.tsx` (1237 lines)**, **`AgentPanel.tsx` (1128 lines)**, **`Onboarding.tsx` (633 lines)**, **`PipelineInspectorPanel.tsx` (578 lines)** all still exceed the 600-line guideline — left as follow-ups.
- **The new `src/lib/config.ts` is _not_ exhaustively wired.** Many components (e.g. `ControlPanel.tsx` polling intervals `15000` / `3000`, `AutomationPanel.tsx`'s `55 * 60_000` "hourly" floor and 24h/week intervals, `SearchHistoryPanel.tsx` panel-internal magic numbers, `useModelDownloadStatus`/auto-fill numeric thresholds) still hold literals scoped to their own component. I only centralized the values that appeared in more than one place or in core capture/search-affecting paths.
- The remaining "Screenpipe" strings in `capture/mod.rs` and `capture/macos.rs` are inside `#[cfg(test)]` blocks and just test that the OCR parser handles a title called "Screenpipe …" — they are not a Screenpipe integration. Renaming them is cosmetic and was left for a follow-up because it requires regenerating the test fixture's expected output.
- I did not delete dependencies in `package.json` / `Cargo.toml`. A scan for actually-unused packages should follow the bigger module splits, since splitting may free additional crates from `commands.rs`.
- Git is reporting some of the deleted CSS files (`AskFndr.css`, `ChatPanel.css`, `TodoModal.css`, `MemoryReconstructionPanel.css`) as still-tracked because they currently have a `skip-worktree` flag set in this checkout. The on-disk files are gone (verified via `ls`). If you want git to see them as deleted you'll need `git update-index --no-skip-worktree <path>` before committing.

## 7. What next (deliberately deferred)

Ordered by impact ÷ risk:

1. **Split `api/commands.rs` autofill section into `api/commands/autofill.rs`** (~1,100 lines moved, no behavior change). Keep `api/commands.rs` as a parent module with `mod autofill; pub use autofill::*;` so all 9 autofill Tauri commands stay resolvable at `api::commands::<name>` and `main.rs` does not need to change. _Risk:_ private helpers (`extract_candidates_from_result`, `rank_autofill_candidates`, etc.) need to come over together. Recommend in a dedicated slice with `cargo build --release` smoke-tested.
2. **Split the Hermes/Agent section of `api/commands.rs`** (~1,300 lines starting at the `HERMES_GATEWAY_PROCESS` constant) into `api/commands/hermes_agent.rs` with the same re-export pattern.
3. **Split the storage-reclaim + memory-repair section** (~700 lines around `reclaim_memory_storage_for_state` / `run_memory_repair_backfill_for_state`) into `api/commands/maintenance.rs`. These two functions are referenced from `main.rs::setup` and need explicit re-exports.
4. **Consolidate `truncate_chars`.** Create `src-tauri/src/text_util.rs` (alongside `http_util.rs`) with one `pub fn truncate_chars(input: &str, max_chars: usize) -> String` matching the byte-identical accessibility/commands implementation. Migrate inference's version separately with a careful eye on its `saturating_sub(3)` behavior.
5. **Split `SearchBar.tsx` (668 lines) into a `useVoiceRecorder` hook and `useSearchSummary` hook.** Component shell stays under ~250 lines. Test against `SearchBar.test.tsx`.
6. **Split `AutofillOverlay.tsx` (1237 lines)** into phase-state-machine + presentational pieces.
7. **Audit `package.json` and `src-tauri/Cargo.toml`** for unused crates once §1–§6 are merged.
8. **Rename the test-fixture "Screenpipe" strings** in `capture/mod.rs` / `capture/macos.rs` to a neutral product name (e.g. "FNDR Docs"). Test-only, behavior-preserving, but requires re-deriving the expected `parsed.title` and key strings.
9. **`getCaptureQualityDashboard` is exported in `src/api/tauri.ts` but never imported** in TypeScript. The Tauri command itself is registered in `main.rs` and may be invoked externally (MCP, tests) — confirm with the team before deleting the TS wrapper.
10. **Frontend `src/components/AgentPanel.css` (1250 lines) and `ControlPanel.css` (1127 lines)** likely contain unused selectors. A `purgecss`-style audit against the live JSX class lists would compress these substantially; left for a focused styling slice.
