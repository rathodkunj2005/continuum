<a id="readme-top"></a>

# FNDR

[![Version][version-shield]][github-url]
[![Tauri][tauri-shield]][tauri-url]
[![React][react-shield]][react-url]
[![Rust][rust-shield]][rust-url]
[![macOS][macos-shield]][tauri-config]
[![License: MIT][license-shield]][cargo-manifest]

FNDR is a macOS desktop app for building a searchable local memory from screen context, meetings, tasks, downloads, and app activity. The app combines a React/Tauri UI with a Rust capture and search backend, LanceDB storage, local ONNX embeddings, and selectable local GGUF models.

## Table Of Contents

| Section | Description |
| --- | --- |
| [About](#about) | Current product scope and capabilities |
| [Architecture](#architecture) | Repository layout and major runtime components |
| [Getting Started](#getting-started) | Prerequisites, setup, and local launch |
| [Configuration](#configuration) | Environment variables and runtime settings |
| [MCP Deployment](#mcp-deployment) | Local, tunnel, and public MCP transport setup |
| [Local Models](#local-models) | Model catalog used by onboarding and settings |
| [Privacy Controls](#privacy-controls) | Verified capture and data controls present in source |
| [Known Limitations](#known-limitations) | Current stable-pipeline boundaries |
| [Development](#development) | Test and verification commands |
| [Links](#links) | Repository remotes |

<p align="right">(<a href="#readme-top">back to top</a>)</p>

## About

FNDR captures macOS screen context, extracts OCR text, stores compact memory records, and exposes search and reconstruction workflows in the desktop UI. The current codebase includes the following product areas:

| Area | Current implementation |
| --- | --- |
| Capture | macOS screen capture, OCR, adaptive sampling, perceptual deduplication, semantic deduplication, and batched memory writes |
| Search | Hybrid vector and keyword search, sentence-aware reranking, Memory Vault / memory cards, timeline browsing, and raw result inspection |
| Memory Vault | Expanded memory cards, surfacing-reason chips, query-scoped knowledge graph, memory provenance strip, and one-click copy for agent context |
| Context Runtime | Agentic retrieval pipeline: rule-based query planner, multi-route modular retrieval, fusion ranker, evidence packer, verifier, and context composer |
| Summaries | Local model-backed memory summaries, daily summaries, daily briefings, and search-result synthesis |
| Tasks | Todo, reminder, and follow-up parsing with persisted task state |
| Meetings | Meeting detection heuristics, ffmpeg-based segmented audio capture, Whisper sidecar transcription, transcript search, and markdown/json export |
| Speech | Voice transcription and local text-to-speech command paths |
| Graph | Local graph store, typed-entity/edge graph with Window, App, and Command nodes, agentic graph-RAG via `fndr.*` MCP namespace, and graph visualization panel |
| Immersive UI | Full-screen cinematic scroll experience (ScrollModeShell) with parallax sections, Aurora wallpaper, chapter rail, sticky scenes, and section-transition bridges |
| Downloads | Downloads folder watcher that injects local file-arrival memory records |
| Autofill | Global shortcut-driven autofill retrieval and injection settings |

<p align="right">(<a href="#readme-top">back to top</a>)</p>

## Architecture

### Project map

**Frontend (`src/`)**

| Path | Role |
| --- | --- |
| `src/app/` | App shell: `App.tsx`, `main.tsx`, `ScrollModeShell.tsx` (immersive), `WorkModeShell.tsx` (standard), autofill entry, sidebar, panels host, global `styles/`. |
| `src/domains/immersive/` | Full-screen cinematic scroll experience: `sections/` (Hero, Capture, Search, Graph, Agent, Privacy, Workspace), `components/` (ParallaxLayer, StickyScene, ChapterRail, SectionTransitionBridge, ScrollProgressIndicator, MorphMemoryCard). |
| `src/domains/` | User-facing product areas (Memory Vault, search bar, timeline, command palette, workspace panels). See `src/domains/README.md`. |
| `src/shared/components/` | Reusable UI: `atoms/` (Button, Field, Pill, Stamp, DossierFrame, …), `AuroraWallpaper.tsx`, `CursorInverter.tsx`, `StatusBar.tsx`. |
| `src/shared/motion/` | Animation primitives: motion tokens, scroll config, Framer Motion variants, and a `useReducedMotionSafe` hook. |
| `src/shared/theme/` | Design tokens: `cinematic-palettes.ts` (10 palettes) and `film-paper.css` (CSS custom properties). |
| `src/shared/` | Reusable UI glue: `ipc/` (Tauri `invoke` bindings), `hooks/`, `utils/`. |

**Backend (`src-tauri/`)**

| Path | Role |
| --- | --- |
| `src-tauri/src/ipc/` | Thin Tauri command handlers (`ipc/commands/*`), including `retrieval.rs` for agentic context-pack commands. |
| `src-tauri/src/capture/` | Screen capture pipeline. |
| `src-tauri/src/ocr/` | Apple Vision OCR. |
| `src-tauri/src/embedding/` | Chunking and embeddings. |
| `src-tauri/src/search/` | Hybrid retrieval and memory cards. |
| `src-tauri/src/context_runtime/` | Agentic retrieval: query planner, multi-route retrieval, fusion ranker, evidence packer, verifier, and context composer. |
| `src-tauri/src/storage/` | LanceDB and filesystem persistence. |
| `src-tauri/src/memory/` | Memory-centric typed graph with Window, App, Command, and 5+ edge variants (`memory/graph/`). |
| `src-tauri/src/inference/` | Local LLM / VLM. |
| `src-tauri/src/privacy/` | Privacy enforcement. |
| `src-tauri/src/mcp/` | MCP server with `fndr.*` namespace for agentic graph-RAG tools. |
| `src-tauri/sidecars/` | Python helpers (Whisper, TTS, etc.). |

```text
fndr/
├── src/                 # React + Vite frontend (see Project map above)
├── src-tauri/           # Tauri / Rust backend + `sidecars/` Python helpers
├── docs/                # Product, architecture, setup — start at docs/README.md
├── scripts/             # bootstrap/, dev utilities, release helpers
├── tools/bin/           # Local tool binaries (e.g. pinned npm)
├── public/              # Static assets copied verbatim at build (see public/README.md)
├── Makefile
├── package.json
└── README.md
```

| Component | Primary paths |
| --- | --- |
| Frontend shell | `src/app/App.tsx`, `src/app/main.tsx`, `src/app/ScrollModeShell.tsx`, `src/app/WorkModeShell.tsx` |
| Immersive scroll | `src/domains/immersive/` |
| Design system | `src/shared/theme/`, `src/shared/motion/`, `src/shared/components/atoms/` |
| Tauri commands | `src-tauri/src/ipc/commands/` |
| Capture pipeline | `src-tauri/src/capture/` |
| Search + Memory Vault | `src-tauri/src/search/` |
| Context runtime | `src-tauri/src/context_runtime/` |
| LanceDB | `src-tauri/src/storage/` |
| Graph store | `src-tauri/src/memory/graph/` |
| MCP (fndr.* namespace) | `src-tauri/src/mcp/` |
| Model catalog | `src-tauri/src/models.rs` |
| Runtime config | `src-tauri/src/config.rs` |
| Privacy controls | `src-tauri/src/privacy/` |
| Meeting recorder | `src-tauri/src/meeting/`, `src-tauri/sidecars/whisper_gguf_runner.py` |

See `docs/architecture/ARCHITECTURE.md` for the capture → OCR → chunking → embedding → LanceDB → hybrid search → Memory Vault / cards → UI pipeline map.

<p align="right">(<a href="#readme-top">back to top</a>)</p>

## Getting Started

| Requirement | Notes |
| --- | --- |
| macOS | macOS 13.0 or newer, matching `src-tauri/tauri.conf.json` |
| Xcode Command Line Tools | Required for native macOS and Rust builds |
| Node.js and npm | Runs the Vite/React frontend and Tauri CLI |
| Rust toolchain | Builds the Tauri backend |
| Python 3 | Runs optional sidecar workflows |
| ffmpeg | Required for meeting audio capture |

Install dependencies and launch the development app from the repository root:

```bash
npm install
./scripts/bootstrap/download-minilm.sh
npm run tauri dev
```

Complete onboarding in the desktop app to grant macOS permissions. FNDR uses two local models in this repo setup: `Qwen3-VL-2B` for memory creation and `all-MiniLM-L6-v2` ONNX for semantic search.

The BGE v5 embedding assets are optional for the staged 1024-d reindex path. Install them only when running the explicit v5 migration command:

```bash
./scripts/bootstrap/download-embedding-model.sh
```

### Meta AI glasses (manual import MVP)

Photos captured on Meta AI glasses typically sync to your phone first, then you can AirDrop or add them to Photos and move them to your Mac. In FNDR, use **Command Palette → Import Meta glasses photo** to index a JPEG/PNG/HEIC: Apple Vision OCR plus the local all-MiniLM-L6-v2 text embeddings (384-d) power search today; a small CLIP vision encoder stores a 512-d vector for future image-aware retrieval.

<p align="right">(<a href="#readme-top">back to top</a>)</p>

## Configuration

Runtime app configuration is written through `src-tauri/src/config.rs`. The `.env.example` file documents optional environment variables used by experimental or sidecar features:

| Variable | Required | Purpose |
| --- | --- | --- |
| `ANTHROPIC_API_KEY` | No | Enables experimental Claude Agent SDK UI paths |
| `OPENAI_API_KEY` | No | Supports optional graph or external knowledge workflows |
| `NEO4J_URI` | No | Connects optional graph workflows to Neo4j |
| `NEO4J_USER` | No | Username for optional Neo4j graph workflows |
| `NEO4J_PASSWORD` | No | Password for optional Neo4j graph workflows |
| `VITE_EVAL_UI` | No | Hides selected feature panels when set to `true` for evaluation builds |
| `FNDR_MEETING_AUDIO_DEVICE` | No | Overrides macOS avfoundation meeting-recorder audio device selection |
| `FNDR_MCP_MODE` | No | MCP deployment mode: `local` (default), `tunnel`, or `public` |
| `FNDR_MCP_REQUIRE_AUTH` | No | Forces MCP bearer auth on or off (default follows mode) |
| `FNDR_MCP_ALLOW_LOOPBACK_AUTH_BYPASS` | No | Allows localhost initialize/tools-list bypass (default only in `local` mode) |
| `FNDR_MCP_ENABLE_TLS` | No | Enables self-signed HTTPS for the MCP server |
| `FNDR_MCP_ALLOWED_ORIGINS` | No | Comma-separated allowed `Origin` list for non-local modes |
| `FNDR_MCP_PUBLIC_BASE_URL` | No | Public tunnel base URL exposed in MCP status/discovery metadata |

Core runtime settings include capture cadence, dedupe threshold, retention days, app blocklist, screenshot retention, proactive surface behavior, and autofill behavior.

<p align="right">(<a href="#readme-top">back to top</a>)</p>

## MCP Deployment

FNDR MCP supports both the legacy HTTP+SSE flow and streamable-HTTP style `GET/POST /mcp` routing. Recommended deployment modes:

- `local` (default): localhost-only personal use.
- `tunnel`: localhost bind with auth required, intended for Cloudflare/ngrok/Tailscale tunneling.
- `public`: explicit non-loopback bind for controlled network environments.

For remote ChatGPT/Claude/Cursor-style access with Cloudflare tunnel:

```bash
export FNDR_MCP_MODE=tunnel
export FNDR_MCP_REQUIRE_AUTH=true
export FNDR_MCP_ALLOW_LOOPBACK_AUTH_BYPASS=false
cloudflared tunnel --url http://127.0.0.1:58596
```

Then set:

```bash
export FNDR_MCP_PUBLIC_BASE_URL=https://your-subdomain.trycloudflare.com
```

The MCP control panel and `~/.fndr/mcp.json` discovery file will surface both local and public endpoints. Keep the bearer token secret; in tunnel/public mode, requests without a valid `Authorization: Bearer <token>` header are rejected.

<p align="right">(<a href="#readme-top">back to top</a>)</p>

## Local Models

FNDR runs fully local with two models in this setup:

| Model | Size | RAM | Role |
| --- | --- | --- | --- |
| `Qwen3-VL-2B` GGUF | ~1.5 GB | ~3.5 GB | Multimodal memory creation, OCR-grounded extraction |
| `all-MiniLM-L6-v2.onnx` + `tokenizer.json` | ~90 MB | ~0.5 GB | Semantic memory search embeddings (384-d) |
| `bge-large-en-v1.5-quantized.onnx` + `tokenizer.json` | ~300 MB+ | loaded only during explicit reindex | Staged v5 parent-memory target embeddings (1024-d) |

`all-MiniLM-L6-v2` is required for search and can be installed with:

```bash
./scripts/bootstrap/download-minilm.sh
```

Models are stored under `~/Library/Application Support/com.fndr.app/models/`.

Validate the local embedding and LanceDB path with:

```bash
make diagnostic
```

The current live search and capture path remains v4 MiniLM 384-d in `memories_v4_minilm_384`. The staged BGE contract writes only to `memories_v5_bge_1024` through the explicit `reindex_memories_v5` maintenance IPC command. FNDR does not silently fall back across dimensions: 384-d MiniLM vectors are refused by the v5 schema path, and 1024-d BGE vectors are kept in the v5 table.

If an older prototype database was created with a different vector dimension, back it up and let FNDR recreate the current 384-d MiniLM schema with:

```bash
make reset-lancedb
```

The v5 BGE path is additive. It does not delete or reset v4 rows, and missing BGE assets produce a clear maintenance-command error instead of breaking startup.

Generated Rust/Tauri artifacts can become large during repeated local builds. Clear only
build outputs with:

```bash
make clean-dev-cache
```

For a full local reset of generated build outputs, runtime memory data, backups, and
downloaded model blobs:

```bash
make clean-all-generated
```

<p align="right">(<a href="#readme-top">back to top</a>)</p>

## Privacy Controls

The controls below are implemented in source and exposed through Tauri commands or configuration. Optional environment variables can enable external services, so review `.env.example` before enabling experimental workflows.

| Control | Source-backed behavior |
| --- | --- |
| Pause and resume | `pause_capture` and `resume_capture` toggle capture state in `src-tauri/src/ipc/commands/` |
| App blocklist | `get_blocklist` and `set_blocklist` read/write blocked app names in runtime config |
| Default blocked apps | `1Password`, `Keychain Access`, `System Preferences`, and `System Settings` are seeded in `Config::default` |
| Sensitive-context alerts | `Blocklist::is_sensitive_context` detects selected banking and finance keywords for proactive alerts |
| Add site to blocklist | `add_to_blocklist` adds a site and attempts retroactive deletion for matching stored memories |
| Delete one memory | `delete_memory` deletes the memory record and its screenshot artifact when present |
| Delete older memories | `delete_older_than` removes memory records older than the requested day count |
| Delete all data | `delete_all_data` clears memory records, graph data, frames, screenshots, and meetings under the app data store |
| Retention | `retention_days` defaults to `7`; `screenshot_retention_days` defaults to `30` |

<p align="right">(<a href="#readme-top">back to top</a>)</p>

## Known Limitations

- Image-to-image visual similarity is live for screen captures and imported photos (CLIP `image_embedding` column). Cross-modal text->image retrieval is not yet supported and is gated on an explicit privacy design (see ADR-004).
- Meeting diarization is experimental.
- Search quality depends on OCR and embedding quality.
- Old LanceDB schemas may need migration after embedding dimension changes.

<p align="right">(<a href="#readme-top">back to top</a>)</p>

## Development

Run the full local test target from the repository root:

```bash
make test
```

The target runs TypeScript typechecking, Vitest, and Rust tests:

| Phase | Underlying command |
| --- | --- |
| TypeScript | `npm run typecheck` |
| Frontend tests | `npm test` |
| Rust tests | `cd src-tauri && cargo test` |

<p align="right">(<a href="#readme-top">back to top</a>)</p>

## Links

| Host | Remote |
| --- | --- |
| GitLab | `git@capstone.cs.utah.edu:fndr/fndr.git` |
| GitHub | `git@github.com:anurupkumar18/FNDR.git` |

<p align="right">(<a href="#readme-top">back to top</a>)</p>

[version-shield]: https://img.shields.io/badge/version-0.2.11-0f766e?style=for-the-badge
[tauri-shield]: https://img.shields.io/badge/Tauri-2-24C8DB?style=for-the-badge&logo=tauri&logoColor=white
[react-shield]: https://img.shields.io/badge/React-18-61DAFB?style=for-the-badge&logo=react&logoColor=111111
[rust-shield]: https://img.shields.io/badge/Rust-2021-000000?style=for-the-badge&logo=rust&logoColor=white
[macos-shield]: https://img.shields.io/badge/macOS-13%2B-111111?style=for-the-badge&logo=apple&logoColor=white
[license-shield]: https://img.shields.io/badge/License-MIT-yellow?style=for-the-badge
[github-url]: https://github.com/anurupkumar18/FNDR
[tauri-url]: https://tauri.app/
[react-url]: https://react.dev/
[rust-url]: https://www.rust-lang.org/
[tauri-config]: src-tauri/tauri.conf.json
[cargo-manifest]: src-tauri/Cargo.toml
