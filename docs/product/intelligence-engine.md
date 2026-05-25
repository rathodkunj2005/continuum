# FNDR Intelligence Engine (Current Architecture)

This document reflects the live local-first implementation in this repository.

## 1. Ingestion Pipeline

- Capture loop: `src-tauri/src/capture/mod.rs`
- macOS adapter: `src-tauri/src/capture/macos.rs`
- OCR: `src-tauri/src/ocr/vision.rs`
- OCR cleanup/noise filtering: `src-tauri/src/capture/text_cleanup.rs`
- Text chunking and embedding: `src-tauri/src/embedding/chunking.rs`, `src-tauri/src/embedding/onnx.rs`

Per memory event, FNDR stores:
- app/window/session metadata
- cleaned OCR text
- snippet summary
- URL/session key
- text embeddings for memory, snippet, and support text
- screenshot/image fields retained for schema compatibility, but current capture does not persist raw screenshots

Schema: `src-tauri/src/storage/schema.rs`

## 2. Local Storage and Graph

- Primary persistence: LanceDB via `src-tauri/src/storage/lance_store.rs`
- Memories table plus tasks/meetings/segments/nodes/edges tables
- Graph ingest path: `src-tauri/src/graph/mod.rs`

Graph node types:
- `MemoryChunk`
- `Entity` (session-oriented today)
- `Task`
- `Url`

Graph edge types:
- `PART_OF_SESSION`
- `REFERENCE_FOR_TASK`
- `OCCURRED_AT`

## 3. Query-Time Intelligence

- UI entrypoint: `search_memory_cards` in `src-tauri/src/ipc/commands/search.rs`
- Retrieval core: `src-tauri/src/search/hybrid.rs`
- Card synthesis/grouping: `src-tauri/src/search/memory_cards.rs`

Current behavior:
- semantic branch (query embedding + vector search) when embedder backend is real
- keyword branch always available
- fusion/rerank + relevance gating
- deterministic card fallback when LLM synthesis is unavailable or times out
- confidence + evidence IDs on memory cards

## 4. Meeting Memory Ingestion

- Meeting runtime/transcription: `src-tauri/src/meeting/mod.rs`
- Meeting transcript is ingested into FNDR memory after recording stops
- Transcript memories now attempt text embeddings before falling back to zero vectors

## 5. Notes on Reliability

Recent hardening includes:
- embedder startup smoke-embed (not ping-only)
- runtime degrade-to-mock behavior when real embedding fails
- embedder status surfaced in runtime status payload
- fallback summaries remain visible in browse surfaces

Known work in progress:
- fully activity-adaptive capture sampling
- automatic task extraction from memories (task panel exists, extraction wiring is partial)

## 6. Planned: Chunk-first Retrieval and Reviewed-memory Enrichment

These flows are not yet wired but are planned as part of the parent-child RAG upgrade (ADR 008):

**Chunk-first retrieval** (Subagent 8):
- At query time, the child-chunk table (`memory_chunks_v1_bge_1024`) is searched first using the BGE query prefix and 1024-d vectors.
- Matched chunks are grouped by `parent_id`; the top-ranked parent records are fetched from the v5 parent table for full context.
- During the v4→v5 transition period, search fans out to both the v4 parent table and the v5 chunk table, with results merged and deduplicated by parent `id` before card synthesis.

**Reviewed-memory enrichment** (Subagents 9–11):
- A background review worker will periodically assess memory quality and relevance, adding or updating enrichment fields on parent records without creating new captures.
- Enriched fields surface in Vault card presentation (Subagent 10) and feed the daily review and backfill cycle (Subagent 11).
- No dates are committed for these flows.
