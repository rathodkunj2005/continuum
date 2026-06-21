# Repository layout (high-signal map)

This is a **concise orientation** for the Continuum macOS app. Authoritative detail remains in `README.md` and `docs/architecture/ARCHITECTURE.md`. Start with **`docs/README.md`** for a doc index.

## Top level

| Path | Role |
| --- | --- |
| `src/` | React + TypeScript UI |
| `src-tauri/` | Tauri shell, capture, search, LanceDB, MCP, graph commands |
| `docs/` | Architecture, ADRs, engineering guides, graph schema, product notes (see `docs/README.md`) |
| `docs/CONTEXT.md` | Shared agent vocabulary (root `CONTEXT.md` points here) |
| `AGENTS.md` | Cross-tool agent contract |

## Frontend (`src/`)

| Path | Role |
| --- | --- |
| `src/app/` | Shell: `App.tsx`, `main.tsx`, autofill entry, `AppPanels`, sidebar, global `styles/`. |
| `src/domains/` | Product UI by domain — see `src/domains/README.md`. |
| `src/shared/ipc/` | Tauri `invoke` bindings (`tauri.ts`, `onboarding.ts`). |
| `src/shared/hooks/` | Reusable hooks (`useSearch`, `usePolling`, …). |
| `src/shared/utils/` | Pure helpers (`config`, `search`, `cardCleanup`, `id`, `eval-ui`). |
| `src/shared/theme/` | Palettes and theme tokens. |

TypeScript path alias **`@/*` → `src/*`** is configured in `tsconfig.json`, `vite.config.ts`, and `vitest.config.ts`.

## Backend (`src-tauri/src/`)

| Path | Role |
| --- | --- |
| `ipc/commands/` | Thin Tauri command handlers |
| `capture/` | Screen capture + OCR pipeline hooks |
| `search/` | Hybrid retrieval, memory card projection |
| `storage/` | LanceDB schemas, `graph_store`, migrations |
| `memory/graph/` | Typed insight graph schema, Louvain, traversal |
| `embedding/` | Chunking and embeddings |
| `timeline/` | `classify` + `classify_rules` |
| `README.md` (crate src) | High-level Rust module map |

## Documentation layout

| Path | Role |
| --- | --- |
| `docs/decisions/` | Numbered ADRs (architecture and product decisions) |
| `docs/setup/engineering/` | How-tos, timeline rules, refactoring notes, agent tooling (`gemini-agent-notes.md`) |
| `docs/graph/` | Lance insight graph schema and operational policies (`schema.md`) |
| `docs/product/` | Product-level deep dives (e.g. intelligence engine) |

**Hygiene:** Prefer new prose under `docs/` subfolders above. Keep at repo root only cross-tool entry files: `README.md`, `AGENTS.md`, `CLAUDE.md`, and the short `CONTEXT.md` pointer. Avoid new loose `*.md` at repo root unless they are cross-tool entry contracts.

## Naming note: Memory Vault

The sidebar entry **Memory Vault** opens the same `memoryCards` panel key as before; the UI title is **Memory Vault** while internal identifiers may remain `MemoryCardsPanel` until a dedicated rename pass is justified.
