# 002: Embedding Dimension

> **Status (2026-05-20): amended for staged v5.**

Continuum treats text embedding dimension as an application contract, not a runtime preference. Model identity, ONNX filename, tokenizer filename, vector dimension, and LanceDB table name must move together.

## Current And Target Contracts

| Contract | Model | Dimensions | Table | Status |
|---|---|---|---|---|
| v4 MiniLM | `sentence-transformers/all-MiniLM-L6-v2` (`all-MiniLM-L6-v2.onnx`) | 384 | `memories_v4_minilm_384` | Current live durable path and v5 migration source |
| v5 BGE | `BAAI/bge-large-en-v1.5` (`bge-large-en-v1.5-quantized.onnx`) | 1024 | `memories_v5_bge_1024` | Additive target table and explicit reindex path |

Source of truth lives in `src-tauri/src/inference/model_config.rs`. The default app config and `Embedder::new()` remain v4 MiniLM so startup and live search do not require BGE assets. The v5 path is loaded only by explicit maintenance/reindex code.

## Validation Rules

- v4 config validation rejects non-384 defaults for the live path.
- v5 reindex validation requires BGE model assets and a 1024-d ONNX output.
- v4 and v5 Lance tables have separate fixed-size vector schemas.
- Continuum never silently falls back across dimensions. A 384-d MiniLM vector is refused by the v5 writer, and a 1024-d BGE vector is not written into the v4 schema.
- Existing v4 rows are not deleted or reset during v5 reindexing.

## Migration Policy

The explicit `reindex_memories_v5` maintenance IPC command reads v4 parent rows, embeds parent fields with the BGE document prefix, and writes idempotent v5 parent rows. Rows already present in v5 by `content_hash`, `dedup_fingerprint`, or stable id are skipped. Missing BGE files produce a clear command error and do not affect startup.
