# REFACTOR_REPORT

## Source LOC Gate
- LOC baseline (`loc-before.txt`, original campaign): **74,850**
- LOC after this pass (same counting rules: `src/`, `src-tauri/src/`, `docs/`, `README.md`, `CONTEXT.md`; `.ts` `.tsx` `.css` `.rs` `.md`; excludes lockfiles, `target/`, `node_modules/`, `dist/`, etc.): **72,911** (`loc-after.txt` regenerated)
- Net change **this pass** vs baseline: **−1,939**
- Cumulative narrative: earlier cleanup passes removed ~2k; this pass adds another ~1.9k tracked-line reduction on the same counter.
- **20,000 LOC target achieved: No**
- Remaining vs 20k goal (from 74,850 baseline): **~18,939**

## Phase A — `api/commands` (this pass)

### New / expanded modules (real moved code)
| File | Approx LOC | Role |
| --- | ---: | --- |
| `commands/mod.rs` | ~3,456 | Types + quality/memory debug + MCP + meetings + export PDF + capture/privacy/stats + autofill + tests; module declarations at top |
| `commands/maintenance.rs` | ~1,589 | Memory repair backfill, storage reclaim (incl. `reclaim_memory_storage_silent`), progress + `StorageHealth`, dev cache cleanup, `merge_bucket_for_anchor` |
| `commands/hermes_agent.rs` | ~1,824 | Hermes bridge/gateway, Codex/Ollama probes, agent task process, daily briefing, `get_fun_greeting`, `quick_setup_ollama` |
| `commands/todos.rs` | ~668 | Task scan limits + todo CRUD/backfill helpers |
| `commands/search.rs` | (existing) | Hybrid search + memory cards surface |
| `commands/common.rs` | (existing) | Shared embedder + truncation + autofill phrase helpers |

### Not done yet in Phase A (still in `mod.rs`, high LOC)
- Dedicated `quality.rs`, `memory.rs`, `privacy.rs`, `export.rs`, `meeting.rs`, `stats.rs`, `autofill.rs` peels (and thin `mod.rs` ~800 LOC target).

### Behavior / constraints
- **Tauri command names unchanged** (`main.rs` `generate_handler!` list unchanged aside from prior `search::` paths).
- **No embedding dimension / LanceDB schema migration** in this pass.
- **No privacy/blocklist/retention regression** by intent; `delete_memory` screenshot artifact cleanup unchanged.

## Phases B–E (this pass)
- **Not executed** in this session: `lance_store.rs` split, `mcp/mod.rs` split, further `capture/mod.rs` peels, frontend/CSS aggressive shrink. They remain the next highest-yield items after finishing Phase A extractions.

## Verification (this pass)
- `npm run typecheck` — pass  
- `npm test -- --run` — pass (5 files, 9 tests)  
- `cd src-tauri && cargo check` — pass  
- `cargo test --lib` — pass (**156** tests)  
- `cargo test --lib commands` — pass (filtered **9** `api::commands` tests)

## Remaining high-impact work (ordered)
1. **Finish Phase A**: extract `quality`, `privacy`, `export`, `meeting`, `stats`, `autofill`, small `memory` from `commands/mod.rs`; collapse duplicate imports; target **thin `mod.rs`**.
2. **`store/lance_store.rs`** split per plan (`normalization`, `embedding_text`, `aliasing`, …).
3. **`mcp/mod.rs`** split (`server`, `auth`, `tools`, `context_packets`, `transport`, `schema`).
4. **`capture/mod.rs`** continue extractions beside `admission.rs`.
5. **Frontend/CSS** targets from audit.
6. **Next large Rust modules** if still short of 20k: `context_runtime`, `meeting`, `search/hybrid`, `search/memory_cards`, `inference`, `embed/onnx`, `ocr/vision`.

## Why the 20k goal is still far
Most line movement this pass was **re-homing** commands code into cohesive files; net LOC drop (~1.9k on the repo-wide counter) comes from import cleanup and the overall shrink of the largest single file footprint, not yet from deleting whole subsystems. The remaining ~18.9k to the campaign goal requires **continued extractions + dead-branch removal** across `lance_store`, `mcp`, `capture`, and UI/CSS per the plan.
