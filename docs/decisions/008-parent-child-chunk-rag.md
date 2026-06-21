# 008: Parent-child chunk RAG architecture

## Status

Accepted — design documented; implementation split across Subagents 6, 7, 8.

## Decision

Continuum will evolve its retrieval pipeline to a **parent-child chunk RAG** model: each screen-capture memory is the **parent** (`MemoryRecord`), and the text it contains is split into overlapping **child chunks** (`MemoryChunkRecord`, added by Subagent 7). At query time the chunk index is searched first for precision; the matched chunk's parent record is then fetched for full-context card synthesis. This replaces the current single-record vector search, which embeds the entire memory text as one vector and loses fine-grained signal on long or dense OCR captures.

---

## Vocabulary

| Term | Rust type | LanceDB table | Role |
|---|---|---|---|
| **Parent** | `MemoryRecord` | `memories_v4_minilm_384` (current) / `memories_v5_bge_1024` (forward target) | Full memory unit — app context, insight fields, summary, full OCR, metadata. The authoritative record for card synthesis. |
| **Child chunk** | `MemoryChunkRecord` *(to be added by Subagent 7)* | `memory_chunks_v1_bge_1024` *(forward target)* | A single overlapping text window derived from the parent's `clean_text`. Carries its own embedding and a `parent_id` foreign key. Never surfaces raw screenshots. |

A `parent_id` on every child chunk points back to the `MemoryRecord.id` in the parent table. The relationship is one-to-many: one parent can have zero or more chunks.

---

## Embedding contract status

| Contract | Model | Dimensions | Table | Status |
|---|---|---|---|---|
| v4 MiniLM | `all-MiniLM-L6-v2` (ONNX) | 384 | `memories_v4_minilm_384` | **Current durable write path.** All captures, search, and ingestion target this table today. Source of truth: `src-tauri/src/inference/model_config.rs` (`MEMORIES_V4_TABLE`). |
| v5 BGE | `BAAI/bge-large-en-v1.5` (ONNX) | 1024 | `memories_v5_bge_1024` | **Additive parent target.** Schema and explicit `reindex_memories_v5` path are wired; live capture/search still use v4 until the retrieval cutover lands. |

The child-chunk table (`memory_chunks_v1_bge_1024`) targets the v5 BGE model. Subagent 7 adds the additive table and explicit v5 reindex chunk writes; live search remains on v4 until the chunk-first retrieval cutover lands.

---

## BGE asymmetric prefixes

BAAI/bge-large-en-v1.5 is an asymmetric bi-encoder — the recommended prefixes differ between indexing and querying:

| Operation | Prefix to prepend |
|---|---|
| Indexing a document or chunk | `"Represent this sentence: "` |
| Search query embedding | `"Represent this question for searching relevant passages: "` |

Both parent records and child chunks use the **document prefix** at index time. Query embeddings use the **query prefix** at search time. Prefix mismatch silently degrades recall; the embedding pipeline must apply them consistently.

---

## Parent rollup rule

**Chosen rule: best salient chunk vector as parent rollup.**

When a parent record does not yet have a dedicated v5 embedding (pre-migration rows, or rows written before the v5 write path ships), the parent's representative vector is derived by selecting the single highest-salience child chunk and using its embedding as a proxy for the full record.

Rationale: the codebase already implements `rank_salient_spans` in `src-tauri/src/capture/text_cleanup.rs`, which scores and rank-orders text spans by content signal. This is a best-first selection strategy — not mean-pooling or max-pooling of all dimensions — and it aligns with how the existing insight pipeline picks representative evidence for card synthesis (see `src-tauri/src/memory/evidence.rs` and ADR 007). No mean-pooling or max-pooling variants exist in the current code (`rg -n "max_pool\|mean_pool" src-tauri/src/` returns zero results).

The rule in practice:

1. Chunk the parent's `clean_text` into overlapping windows.
2. Score each chunk with `rank_salient_spans` logic (or an equivalent salience scorer on the chunk text).
3. Embed the highest-scoring chunk using BGE v5 + document prefix.
4. Store that embedding in the parent row's v5 vector column.

This is idempotent: re-indexing a parent already populated produces the same rollup vector as long as the text and salience rules are unchanged.

---

## Future table names

```
memories_v5_bge_1024          — parent records (BGE 1024-d text vector)
memory_chunks_v1_bge_1024     — child chunks  (BGE 1024-d per-chunk vector)
```

Both tables share the same model and dimension contract so a single ONNX model session serves both index paths.

---

## Migration policy

1. **No destructive reset.** The v4 parent table (`memories_v4_minilm_384`) remains readable and searchable until a full v5 backfill is verified complete. Users are not asked to wipe their database.
2. **Idempotent reindex.** The migration worker (Subagent 11) iterates existing parent records and writes child chunks only for rows whose `content_hash` is not yet present in the chunk table. Re-running the worker is safe.
3. **`content_hash` skip.** Each child-chunk write checks whether a row with matching `parent_id` + `content_hash` already exists before inserting. Duplicate chunk rows are never written.
4. **Explicit parent reindex first.** Subagent 6 writes v5 parent rows only through `reindex_memories_v5`. New live captures continue to write v4 until the later cutover slice.
5. **Dual-table search later.** During the retrieval transition, search queries will fan out to both v4 parent and v5 chunk tables; results are merged and deduplicated by parent `id` before card synthesis.

---

## Privacy

Child chunks are derived exclusively from `clean_text` — the sanitized OCR output that has already passed through the app-aware noise filter, blocklist exclusion, and privacy gating defined in ADR 004. Specifically:

- Blocked apps, internal Continuum windows, blocked URLs, and sensitive-context screens are excluded **before** OCR runs; no chunk is ever written for a suppressed capture.
- Chunks contain text only. No pixel data, no screenshot path, no raw image bytes — consistent with the no-screenshot-persistence contract in ADR 004 and the CLIP-vector-only update in that ADR.
- Chunk text is shorter than the full parent `clean_text`; it does not introduce additional sensitive surface area beyond what the parent already stores.
- The chunk blocklist/exclusion rules are identical to the parent's: no separate privacy layer is needed for chunks.

---

## Test and eval guidance (for Subagent 12)

Minimum test set required before the chunk-first retrieval path is considered stable:

| Category | What to verify |
|---|---|
| Chunk boundary correctness | No chunk exceeds max token length; overlap windows are within configured bounds; no empty chunks written. |
| `parent_id` integrity | Every chunk row's `parent_id` resolves to an existing parent record; orphan chunks are rejected. |
| `content_hash` deduplication | Re-indexing the same parent twice produces the same number of chunk rows (idempotency). |
| BGE prefix correctness | Document-prefixed chunk embeddings and query-prefixed search vectors are verified to be different for identical text inputs; query prefix is applied at search time only. |
| Salience rollup determinism | The parent rollup vector produced from a given `clean_text` is identical across two independent runs. |
| Recall comparison | A held-out set of (query, expected parent id) pairs shows chunk-first retrieval recall ≥ single-record v4 retrieval recall. |
| Privacy gate | A memory from a blocklisted app has zero rows in the chunk table after a full reindex. |
| Dual-table merge | A query that matches both a v4 parent and a v5 chunk row for the same underlying memory returns exactly one deduplicated card. |
