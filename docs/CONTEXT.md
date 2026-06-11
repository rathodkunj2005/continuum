# FNDR — shared context for agents

Use this file with **`AGENTS.md`** and the portable skills under `.agent-skills/portable-engineering/`.

## What FNDR is

A macOS Tauri application that builds a **searchable local memory** from screen context, meetings, tasks, downloads, and related signals. See `README.md` for product areas and user-facing capabilities.

## Where truth lives

| Topic | Location |
| --- | --- |
| Product + setup + dev commands | `README.md` |
| Shared vocabulary (this file) | `docs/CONTEXT.md` |
| Pipeline and components | `docs/architecture/ARCHITECTURE.md` |
| Architecture decisions | `docs/decisions/` |
| UX / visual direction | `docs/product/DESIGN_DIRECTION.md` |
| Intelligence engine notes | `docs/product/intelligence-engine.md` |
| Insight graph (Lance schema + policies) | `docs/architecture/graph-schema.md` |
| Agent defaults + skill map | `AGENTS.md` |

## Engineering vocabulary

- **Memory record** (`MemoryRecord`): persisted unit of captured context stored and indexed for search. This is the **parent** in the parent-child RAG model — the authoritative record for card synthesis, holding full OCR, insight fields, and metadata.
- **Memory chunk** (`MemoryChunkRecord`, to be added by Subagent 7): an overlapping text window derived from a parent `MemoryRecord`, carrying its own embedding and a `parent_id` foreign key. Used by the chunk-first retrieval path for higher-precision vector search. See ADR 008.
- **Embedding document** (`MemoryEmbeddingDocument`): the canonical in-memory retrieval document used to derive primary/search text, snippet text, support text, chunk text, visual semantic text, and graph-node text before vectors are written. Its provenance is stored additively under `raw_evidence.embedding_manifest`. See ADR 010.
- **Memory card**: UI-facing presentation of a search hit / browse item.
- **Memory Vault**: full-screen browse surface for all memories, the global insight graph (Louvain-clustered layout), and per-project graph scopes (`src/domains/memory-vault/MemoryCardsPanel` + sidebar entry).
- **Capture pipeline**: screen → OCR / text extraction → chunking → embedding → storage.
- **Hybrid search**: vector + keyword retrieval with reranking as implemented in Rust.
- **Parent-child RAG**: retrieval pattern where child chunks are searched first for precision, then matched chunks' parent records are fetched for full-context card synthesis. Governed by ADR 008.
- **Sidecar**: Python helpers under `src-tauri/sidecars/` for transcription, agent, graph, TTS, etc.
- **Status events**: backend → renderer push channels (`capture://status`, `privacy://alerts`, `meeting://status`, `proactive_suggestion`, model-download events). Always-on UI state subscribes via `useTauriEvent` after one initial fetch instead of polling. See ADR 011.

## Default quality bar

Prefer small diffs, tests at stable boundaries, and evidence-backed debugging — see `AGENTS.md` and the `diagnose` / `tdd` skills.
