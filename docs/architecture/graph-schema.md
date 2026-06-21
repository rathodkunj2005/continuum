# Continuum insight knowledge graph (LanceDB)

This document describes the **insight** graph persisted in LanceDB alongside memories. It is distinct from the **legacy** graph (`nodes` / `edges` tables with string IDs) used for timeline linking.

## Tables

| Table | Role |
| --- | --- |
| `graph_nodes` | Typed entities extracted from finalized memory / insight fields |
| `graph_edges` | Directed, typed relationships between nodes |

## Node types (`GraphNodeType`)

`Project`, `Memory`, `Concept`, `Decision`, `File`, `Error`, `Tool`, `Person`, `Url`, `Session`, `Task`, `Window`, `App`, `Command`

## Edge types (`GraphEdgeType`)

`DependsOn`, `Contains`, `Imports`, `Extends`, `Implements`, `PartOf`, `Supports`, `Contradicts`, `Supersedes`, `Refines`, `Questions`, `Resolves`, `Causes`, `Prevents`, `TriggeredBy`, `FixedBy`, `BrokeBy`, `PrecededBy`, `FollowedBy`, `SimilarTo`, `MentionedIn`, `UsedIn`, `CreatedBy`, `AppliesTo`, `OccurredInSession`, `BelongsToProject`, `UsedApp`, `SameTaskAs`, `EvidencedBy`

Spec-name aliases are normalized by `src-tauri/src/graph/edges.rs::edge_aliases::canonical`. Persisted Lance rows keep canonical PascalCase edge literals.

## `graph_nodes` columns

| Column | Type | Notes |
| --- | --- | --- |
| `id` | UUID (string in SQL filters) | Stable identity |
| `node_type` | string | One of `GraphNodeType` (PascalCase) |
| `label` | string | Human-readable label |
| `confidence` | float32 | 0..1 extraction salience |
| `source_memory_ids` | JSON array string | Memory row IDs supporting this node |
| `embedding` | fixed-size float list | Optional semantic vector (dim = app text embed dim) |
| `created_at_ms` / `created_at` | int64 / RFC3339 | UTC (schema uses `DateTime<Utc>` in Rust) |
| `updated_at_ms` / `updated_at` | int64 / RFC3339 | UTC |
| `stale` | bool | Marked by idle maintenance |
| `metadata` | JSON string | Arbitrary structured fields |

## `graph_edges` columns

| Column | Type | Notes |
| --- | --- | --- |
| `id` | UUID | Edge identity |
| `source_id` | UUID | Tail |
| `target_id` | UUID | Head |
| `edge_type` | string | One of `GraphEdgeType` (PascalCase) |
| `confidence` | float32 | 0..1 |
| `conflict_flag` | bool | Contradiction / paired dispute edges |
| `created_at_ms` / `created_at` | int64 / RFC3339 | UTC |
| `metadata` | JSON string | |

## Node record (`GraphNode` in Rust)

- `id`: UUID (stable v5 from memory id + type + label key in extractor)
- `node_type`, `label`, `confidence` (0..1)
- `source_memory_ids`: backing memory row ids (no raw OCR in graph payloads)
- `embedding`: optional vector for cosine search
- `created_at`, `updated_at`, `stale`, `metadata`

## Edge record (`GraphEdge`)

- `id`, `source_id`, `target_id`, `edge_type`, `confidence`
- `conflict_flag`: when true, edge participates in a tension pair; **Contradicts**/**Supports** pairs are both retained (no auto-resolution).

## Merge & dedup policy

- **Nodes**: upsert is keyed by exact `id` only (no fuzzy label merge into a different id).
- **Edges**: duplicate `(source_id, target_id, edge_type)` is suppressed on write.
- **Staleness**: idle job calls `mark_stale(30)` (days since `updated_at`); stale rows are flagged, not deleted.

## Extraction thresholds

- Card queued to `pending_graph_updates` when extraction `overall_confidence >= 0.5`.
- Lower bucket: `low_confidence_graph_candidates` (never auto-committed).
- Edge heuristics drop relationships below confidence **0.4** at extraction time.

## Louvain metadata on API subgraphs

`get_full_graph` and `get_graph_for_project` attach `louvain` (node id → community id) and `cluster_0_name` via `graph::community::attach_louvain_metadata` for UI clustering (Memory Vault graph).

## Write path

- **Capture / flush:** `entity_extractor::extract` runs on normalized memory rows; results are queued on `AppState` (`pending_graph_updates` / `low_confidence_graph_candidates`). No Lance writes on the capture hot path.
- **Idle commit:** `commit_graph_updates` drains the queue into Lance via `graph::graph_store::GraphStore`, gated by `system_resources::allows_graph_idle_commit` (pause, battery saver, power, CPU load).

## MCP

- `memory.graph_query` — keyword search over **legacy** string-id nodes/edges.
- `memory.graph_context` — bounded JSON over **insight** `graph_*` tables (summary + optional UUID neighborhood).

## MCP `ContextPack.graph_context`

Bounded JSON (~7500 serialized chars) with top project nodes, top edges, conflicts, and wiki stub summary. No OCR text is embedded in graph nodes.

## Idle commit gating

`commit_graph_updates` runs only when `system_resources::allows_graph_idle_commit` passes (not paused, not battery-saver flag, pmset charging or estimated battery > 40%, load heuristic).

## Code map

- Schema & pure algorithms: `src-tauri/src/graph/entities.rs`, `edges.rs`, `schema.rs`, `traversal.rs`, `pathfinding.rs`, `community.rs`
- Lance I/O: `src-tauri/src/graph/graph_store.rs` (uses `Store`’s opened `graph_nodes_table` / `graph_edges_table`)
- Retrieval scaffolding: `src-tauri/src/graph/graph_index.rs`, `graph_rerank.rs`
- Extraction: `src-tauri/src/capture/entity_extractor.rs`
- Tauri commands: `src-tauri/src/ipc/commands/graph.rs`
