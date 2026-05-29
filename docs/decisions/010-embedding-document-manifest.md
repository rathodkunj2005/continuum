# ADR 010: Canonical Embedding Documents And Manifests

## Status

Accepted

## Context

FNDR now writes several local retrieval vectors from the capture-to-storage pipeline:

- MiniLM 384-d text vectors for the live memory table.
- BGE 1024-d vectors for the explicit v5 parent/chunk reindex path.
- CLIP 512-d image vectors for image-to-image similarity.
- Insight graph node vectors for graph search.

These vector spaces must stay separate, but search needs one typed contract that explains which source text or visual signal produced each vector.

## Decision

FNDR composes retrieval source text through `src-tauri/src/memory_embedding_document.rs`.

The canonical document owns the primary/search text, snippet text, support texts, chunk source text, visual semantic text, and graph-node text. Capture, import, merge, storage normalization, graph commit, and explicit BGE reindex use this same composer instead of rebuilding embedding text independently.

Embedding provenance is stored additively in `raw_evidence.embedding_manifest`. The manifest records role-specific source hashes, vector contracts, per-role status, and visual semantic source. No destructive migration is required, and `memory_context` remains the only persisted narrative field.

## Consequences

- Existing memory rows remain readable.
- Normalization no longer silently rewrites non-empty `embedding_text` after a vector exists; stale legacy rows are flagged in the manifest.
- MiniLM, BGE, CLIP, and graph vectors remain dimension-checked and non-interchangeable.
- Future search and MCP context-pack code can inspect provenance instead of guessing why a vector exists.
