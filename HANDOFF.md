# Handoff: rag-memory-hero-upgrade

**Date:** 2026-05-21
**Branch:** `rag-memory-hero-upgrade`
**Status:** Subagents 1–13 complete; branch buildable; ready for PR review.

## Scope of this branch

Parent-child chunk RAG, ingest-first hardening, post-capture memory review (per-record + daily), and vault lifecycle presentation. All work is local-only and additive — no cloud dependencies introduced and no destructive migration of existing data.

## Completed work

### Subagents 6–8 — Parent-child chunk RAG
- Additive `memory_chunks_v1_bge_1024` table with BGE 1024-d child embeddings.
- v4 MiniLM 384-d parent table remains the durable write path; v5 chunk writes refuse wrong-dimension fallback.
- Chunk-first retrieval route fans out to the chunk table at query time and resolves matches back to parent records for card synthesis.

### Ingest-first hardening (pre-Subagent 9)
- Failed visual semantics never become `enriched_memory_card`.
- Empty OCR + blocked VLM + empty semantic fields → `visual_semantics_failed` / metadata-only.
- Score caps prevent low-evidence visual failures from polluting retrieval.
- VLM capability separated from runtime pressure (`host_total_ram_bytes` now sysctl-first).
- Low RAM means deferred runtime status, not `host_supports_vlm=false`.
- `activity_type` enum dumps normalise to `unknown`.
- Meta narration ("The OCR text indicates…", "The user is viewing…", "Visual-only frame", "No visible content", "Screen capture shows…") is filtered at write time and on display.
- Parent graph nodes use `NodeType::Memory`, not `MemoryChunk`.

### Subagent 9 — Memory Review Worker
- `enrichment_status` / `reviewed_at_ms` / `reviewer_generation` schema fields wired through schema, Arrow encode/decode, migration, and capture enqueue.
- Async per-memory review under the `model_pipeline_lock`; pressure-gated by pause / inference-loaded / battery / CPU.
- Grounding + narration validation; `review_failed` preserves the original record content.
- `get_memory_review_status` Tauri command exposes queue depth / last error / gating to the UI.

### Subagent 10 — Vault reviewed-memory presentation
- Lifecycle fields added to `SearchResult` and `MemoryCard` IPC; threaded through every constructor (Arrow reader, mcp::memory_to_search_result, context_runtime::retrieval_routes, ipc::search::memory_card_from_result, memory_cards builders).
- React `MemoryCard.tsx` derives a 5-state lifecycle chip (`DEVELOPED` / `PENDING` / `RAW` / `REVIEW_FAILED` / `VISUAL_FAILED`) and rewires preview-text priority: insight → reviewed display_summary → memory_context excerpt → safe fallback. Meta-OCR narration is stripped from the preview and from `InsightLayers` slots.
- Small review-status indicator added to `EngineMetricsCard`.
- New vault tests cover all five lifecycle states, reviewed-vs-OCR priority, and meta-narration cleanup.

### Subagent 11 — Daily review + backfill
- `run_daily_memory_review_cmd { date, dry_run }` and `backfill_memory_review { start_ms, end_ms, dry_run }` Tauri commands.
- Daily scheduler (`spawn_daily_scheduler`) wakes hourly, runs the previous calendar day once per day under the full pressure gate, releases the model pipeline lock between batches, and resumes on subsequent ticks if mid-batch pressure forced a defer.
- `ReviewWriteMode` enum (`ReviewedLocal` / `ReviewedDaily` / `DryRun`) lets a single pipeline serve the per-capture worker, the daily driver, and dry-run inspections without code duplication.
- Backfill enqueues per-memory `MemoryReviewJob`s for the existing worker to drain; dry-run reports `would_queue` without mutating the queue.

### Subagent 12 — Chunk-retrieval quality harness
- Deterministic chunk-first retrieval tests already on the branch (see `storage::lance_store::tests` and `context_runtime::chunk_route`).

### Subagent 13 — Integration cleanup (this pass)
- Committed three load-bearing edits that earlier subagents referenced but left in the working tree (`get_memory_review_status` command, narration banned-pattern additions, sysctl-first host RAM read).
- Removed `docs/superpowers/audits/2026-05-20-embedding-contract-drift.txt` (one-off debugging dump).
- Updated `docs/architecture/ARCHITECTURE.md` with a new "Memory review lifecycle" section and fixed the stale "not searched yet" claim on the v5 chunk table.

## Architecture invariants (verified)

- v4 MiniLM 384 remains the safe fallback / live durable source path.
- v5 BGE 1024 stays additive and refuses cross-dimension fallback (see `maintenance.rs:181`).
- `memory_chunks_v1_bge_1024` table exists and is searched.
- Chunk-first retrieval has deterministic tests (`storage::lance_store::tests`, `context_runtime::chunk_route`).
- Failed visual semantics surface as `visual_semantics_failed` and are not stored as enriched memories.
- `activity_type` enum-dumps normalize through `normalize_activity_type`.
- Parent graph nodes use `NodeType::Memory`.
- `memory_review` worker is async, pressure-gated, and uses `model_pipeline_lock`.
- Vault exposes lifecycle fields end-to-end (Rust → TS → React chip).
- Daily / backfill commands are local-only (no `reqwest` / `openai` / `anthropic` references introduced) and have `dry_run` paths that mutate nothing.
- `raw_screenshot_stored` defaults to false; no unexpected screenshot retention surface added.

## Test status

| Surface | Result |
| --- | --- |
| `cargo test --lib memory_review` | 27 / 27 pass |
| `cargo test --lib storage` | 29 / 29 pass |
| `cargo test --lib search` | 41 / 41 pass |
| `cargo test --lib maintenance` | 2 / 2 pass |
| `cargo build` (binary) | clean |
| `npm run typecheck` | clean |
| `npm test memory-vault` | 98 / 98 pass |

A full `cargo test --lib` can be run before PR open; cold runs take ~3 min on this machine.

## Manual dev-verification steps

1. `make test` (or `cargo test --lib` + `npm run typecheck` + `npm test memory-vault`).
2. `cargo run` (binary) and confirm the app boots without `__cmd__get_memory_review_status` resolution errors.
3. Open the Engine Metrics panel; confirm the new "Memory review" row reports `running / deferred / off` + queue depth.
4. Trigger `run_daily_memory_review_cmd` from the Tauri devtools with `{ date: <today>, dryRun: true }` and confirm a `DailyReviewSummary` returns with `dry_run: true`, `changed: 0`, `would_change >= 0`.
5. Verify the vault renders the lifecycle chip for at least one row (insert a synthetic row with `enrichment_status = "reviewed_local"` if the local capture stream hasn't produced one yet).

## Known remaining risks / non-blockers

- `LAST_RUN_DAY` in `daily.rs` is a process-static; if the user expects a way to force a re-run within the same process (e.g. for QA), add an IPC hook or expose a reset.
- `day_thread` is mentioned as an optional field in the Subagent 11 spec; it is not yet computed. Adding it is a schema change and deliberately deferred.
- ADR-008 still mentions Subagent 11 as a "migration worker" — left as-is since it captures design intent at the time of writing; the actual subagent layout is documented here in HANDOFF.md.

## Branch state

- Three integration-cleanup commits added on top of the prior subagent chain.
- No uncommitted files (`git status` clean).
- No `docs/superpowers/audits/` artefacts left behind.
- Buildable, fully-typed, all targeted tests green.
