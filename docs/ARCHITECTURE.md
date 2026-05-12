# FNDR Architecture

FNDR is a local-first macOS memory pipeline. The stable product path is text-first:

```text
capture -> OCR -> chunking -> embedding -> LanceDB storage -> hybrid search -> MemoryCards -> UI
```

## Pipeline

1. Capture samples the foreground screen, skips private contexts, deduplicates frames, and keeps raw pixels off the persisted memory path.
2. OCR extracts screen text with Apple Vision and applies app-aware cleanup for browser and desktop noise.
3. Chunking turns cleaned OCR text into high-signal memory chunks with overlap and repeated-line suppression.
4. Embedding generates 1024-dimensional local text vectors for the full memory text, snippet text, and representative support text.
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
| `frontend/` | React UI for capture status, search, MemoryCards, timeline, and controls |

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

- `EmbeddingConfig`: model contract, 1024-dimensional vector size, sequence length, cache, batch size.
- `ChunkingConfig`: OCR chunk length, overlap, and target text windows.
- `SearchConfig`: branch limits, timeouts, fusion weights, relevance floors, and rerank pool size.
- `CapturePipelineConfig`: batching, semantic dedupe, idle behavior, and focus-drift thresholds.
- `MemoryCardConfig`: grouping, synthesis limits, and timeout behavior.
- `StoreConfig`: LanceDB retrieval expansion and keyword scan limits.
- `ProactiveConfig`: background similarity suggestion cadence, lookback, result limit, seen cache, and threshold.

## Stable vs Experimental

The stable search path is OCR text plus local text embeddings. Visual semantic search, meeting diarization, external graph services, and autonomous agent surfaces are treated as adjacent or experimental features unless wired through the core path above.
