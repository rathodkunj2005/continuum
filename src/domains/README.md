# `src/domains` — product UI by domain

Continuum UI is grouped by **product domain** (search, memory vault, timeline, workspace panels). This replaces a single flat `components/` tree.

| Directory | Responsibility |
| --- | --- |
| **`memory-vault/`** | Memory Vault: list/graph/project views, `KnowledgeGraph`, `InsightLayers`, `useGraph`. |
| **`search/`** | Search bar, placeholders, search tests. |
| **`timeline/`** | Timeline stream, `timelineConfig`, timeline tests. |
| **`command-palette/`** | Command palette + exported `PanelKey`. |
| **`workspace/`** | Full-screen panels (agent, meetings, stats, onboarding, etc.). |

Cross-cutting code lives under **`src/shared/`** (`ipc` for Tauri invokes, `hooks`, `utils`, `theme`). The app shell lives under **`src/app/`**. Use the **`@/`** path alias (`tsconfig.json`) instead of deep relative imports.
