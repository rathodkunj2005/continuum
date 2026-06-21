# Continuum Architecture

Continuum is a local-first macOS memory pipeline. The stable product path is text-first:

```text
capture -> OCR -> chunking -> embedding -> LanceDB storage -> hybrid search -> MemoryCards / Memory Vault -> UI
```

## Pipeline

1. Capture samples the foreground screen, skips private contexts, deduplicates frames, and keeps raw pixels off the persisted memory path.
2. OCR extracts screen text with Apple Vision and applies app-aware cleanup for browser and desktop noise.
3. Chunking turns cleaned OCR text into high-signal memory chunks with overlap and repeated-line suppression.
4. Embedding generates 384-dimensional local text vectors (all-MiniLM-L6-v2 via ONNX) for the live full-memory, snippet, and representative support vectors. The embedding contract — model name, file, tokenizer, dimension, and Lance table name — lives in `src-tauri/src/inference/model_config.rs`. The additive BGE 1024-d v5 parent target is available through explicit reindexing, not startup or live search.
5. LanceDB storage persists compact memory records, metadata, and vector columns for retrieval.
6. Hybrid search runs semantic vector retrieval and lexical keyword retrieval, then fuses, gates, and reranks candidates.
7. MemoryCards group related search hits into grounded cards with deterministic fallbacks.
8. The React UI presents capture status, search, cards, timeline views, privacy controls, and supporting workflows.

## Core Modules

| Module | Responsibility |
| --- | --- |
| `capture/` | Screen sampling, deduplication, privacy exclusions, OCR-to-memory assembly |
| `ocr/` | Apple Vision OCR and recognized-text metadata |
| `embed/` | OCR-aware chunking and local ONNX embedding generation |
| `store/` | LanceDB schema, migration checks, persistence, and vector normalization |
| `search/` | Hybrid vector/keyword retrieval, ranking, reranking, and MemoryCards |
| `http_util/` | Bounded `reqwest` clients for local probes (Ollama, Hermes) and agent LLM HTTP |
| `api/` | Tauri commands connecting the Rust pipeline to the frontend |
| `http_util` | Bounded `reqwest` client builders and JSON POST helper used by agent/provider HTTP from `api/` |
| `frontend/` | React UI under `src/` (`src/domains/` panels, `src/app/` shell) |

## Core Boundaries

The code keeps public Tauri command names stable, while internal names make the pipeline intent explicit:

- `extract_ocr_text`: app-aware OCR cleanup before any memory text enters the pipeline.
- `chunk_screen_text`: OCR-aware chunking for screen text.
- `embed_memory_chunk`: product-named embedding boundary for one memory chunk.
- `insert_memory_chunk`: product-named LanceDB write boundary for one memory chunk.
- `search_hybrid_memories`: semantic + keyword retrieval boundary.
- `build_memory_cards`: search-results to MemoryCards boundary.

## Configuration

Pipeline knobs live in `src-tauri/src/config.rs` rather than scattered literals:

- `EmbeddingConfig`: live v4 MiniLM model contract, 384-dimensional vector size, sequence length, cache, batch size. The v5 BGE 1024-d target is a separate code contract rather than the default runtime config.
- `ChunkingConfig`: OCR chunk length, overlap, and target text windows.
- `SearchConfig`: branch limits, timeouts, fusion weights, relevance floors, and rerank pool size.
- `CapturePipelineConfig`: batching, semantic dedupe, idle behavior, and focus-drift thresholds.
- `MemoryCardConfig`: grouping, synthesis limits, and timeout behavior.
- `StoreConfig`: LanceDB retrieval expansion and keyword scan limits.
- `ProactiveConfig`: background similarity suggestion cadence, lookback, result limit, seen cache, and threshold.

## Parent-child chunk RAG (forward architecture)

The current pipeline embeds each `MemoryRecord` as a single vector. The planned upgrade introduces a **parent-child RAG** model governed by ADR 008:

- **Parent**: `MemoryRecord` — the full capture unit, holds all metadata, insight fields, and OCR text. Current durable write path targets `memories_v4_minilm_384` (384-d MiniLM).
- **Child chunk**: `MemoryChunkRecord` (Subagent 7) — an overlapping text window derived from the parent's `clean_text`, carrying its own BGE 1024-d embedding and a `parent_id` foreign key. Target table: `memory_chunks_v1_bge_1024`.

At query time the chunk index is searched first for precision; matched chunks' parent records are fetched for card synthesis. The parent rollup vector is the embedding of the highest-salience child chunk, aligned with the existing `rank_salient_spans` strategy.

**Embedding contract timeline:**

| Contract | Table | Status |
|---|---|---|
| v4 MiniLM 384-d | `memories_v4_minilm_384` | Current durable write path |
| v5 BGE 1024-d | `memories_v5_bge_1024` | Additive parent target table and explicit reindex path |
| v1 BGE chunks 1024-d | `memory_chunks_v1_bge_1024` | Additive child chunk table; queried by the chunk-first retrieval route |

The v5 parent table is available as a migration target, but 1024-d is still **not** the current live durable write path. v4 remains readable, v5 writes refuse wrong-dimension fallback, and the chunk-first route fans out to the v5 chunk table at query time and resolves matches back to the v4 parent for card synthesis. See ADR 002 (amended) and ADR 008.

## Memory review lifecycle (post-capture)

Captured memories carry a `enrichment_status` lifecycle field that distinguishes raw captures from locally-reviewed ones:

| Status | Meaning |
| --- | --- |
| `""` / `pending` | Just captured; waiting for the post-capture review worker. |
| `reviewed_local` | The async per-memory worker upgraded the record under the local model and persisted a validated patch. |
| `reviewed_daily` | The daily batch driver re-reviewed yesterday's records and persisted a validated patch. |
| `review_failed` | The review surfaced an ungrounded or meta-narrated patch; the original content is preserved. |

The review pipeline is local-only and pressure-gated:

- The per-capture worker drains a small queue under the `model_pipeline_lock` so the LLM call never races capture.
- The daily scheduler wakes hourly, runs the previous calendar day once per day when inference is loaded and the pressure gate is open, and aborts gracefully if the gate closes mid-batch.
- Both paths run the same grounding + narration-filter validation. Failed patches preserve the original record content.
- IPC commands `run_daily_memory_review_cmd` and `backfill_memory_review` expose the same pipeline for manual or backfill use with a `dry_run` option that computes patches without mutating storage.

The vault surfaces lifecycle status through a chip on each card (`DEVELOPED` / `PENDING` / `RAW` / `REVIEW_FAILED` / `VISUAL_FAILED`) and prefers reviewed summaries over raw OCR for the preview text. See `src/domains/memory-vault/MemoryCard.tsx` for the derivation rules.

## Stable vs Experimental

The stable search path is OCR text plus local text embeddings. Screen captures and imported photos additionally write a 512-d CLIP `image_embedding`, exposed through `find_visually_similar_memories` for image-to-image retrieval over the same LanceDB column. Cross-modal text->image retrieval, meeting diarization, external graph services, and autonomous agent surfaces remain adjacent or experimental features unless wired through the core path above.
