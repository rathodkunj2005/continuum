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
| Summaries | Local model-backed memory summaries, daily summaries, daily briefings, and search-result synthesis |
| Tasks | Todo, reminder, and follow-up parsing with persisted task state |
| Meetings | Meeting detection heuristics, ffmpeg-based segmented audio capture, Whisper sidecar transcription, transcript search, and markdown/json export |
| Speech | Voice transcription and local text-to-speech command paths |
| Graph | Local graph store and graph visualization panel |
| Downloads | Downloads folder watcher that injects local file-arrival memory records |
| Autofill | Global shortcut-driven autofill retrieval and injection settings |

<p align="right">(<a href="#readme-top">back to top</a>)</p>

## Architecture

### Project map

**Frontend (`src/`)**

| Path | Role |
| --- | --- |
| `src/app/` | App shell: `App.tsx`, `main.tsx`, autofill entry, sidebar, panels host, global `styles/`. |
| `src/domains/` | User-facing product areas (Memory Vault, search bar, timeline, command palette, workspace panels). See `src/domains/README.md`. |
| `src/shared/` | Reusable UI glue: `ipc/` (Tauri `invoke` bindings), `hooks/`, `utils/`, `theme/`. |

**Backend (`src-tauri/`)**

| Path | Role |
| --- | --- |
| `src-tauri/src/ipc/` | Thin Tauri command handlers (`ipc/commands/*`). |
| `src-tauri/src/capture/` | Screen capture pipeline. |
| `src-tauri/src/ocr/` | Apple Vision OCR. |
| `src-tauri/src/embedding/` | Chunking and embeddings. |
| `src-tauri/src/search/` | Hybrid retrieval and memory cards. |
| `src-tauri/src/storage/` | LanceDB and filesystem persistence. |
| `src-tauri/src/memory/` | Memory-centric graph (`memory/graph/`). |
| `src-tauri/src/inference/` | Local LLM / VLM. |
| `src-tauri/src/privacy/` | Privacy enforcement. |
| `src-tauri/src/mcp/` | MCP server. |
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
| Frontend shell | `src/app/App.tsx`, `src/app/main.tsx`, `src/app/AppPanels.tsx` |
| Tauri commands | `src-tauri/src/ipc/commands/` |
| Capture pipeline | `src-tauri/src/capture/` |
| Search + Memory Vault | `src-tauri/src/search/` |
| LanceDB | `src-tauri/src/storage/` |
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
make install
./scripts/download_model.sh
npm run tauri dev
```

Complete onboarding in the desktop app to grant macOS permissions and select/download a local GGUF (default: Llama 3.2 1B). `scripts/download_model.sh` also pulls the CLIP vision ONNX used when you import Meta glasses photos (Cmd+K → “Import Meta glasses photo”).

### Meta AI glasses (manual import MVP)

Photos captured on Meta AI glasses typically sync to your phone first, then you can AirDrop or add them to Photos and move them to your Mac. In FNDR, use **Command Palette → Import Meta glasses photo** to index a JPEG/PNG/HEIC: Apple Vision OCR plus BGE text embeddings power search today; a small CLIP vision encoder stores a 512-d vector for future image-aware retrieval.

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

The onboarding and settings flows read the model catalog from `src-tauri/src/models.rs`:

| ID | Display name | Size | RAM | Role |
| --- | --- | --- | --- | --- |
| `llama-3.2-1b` | Llama 3.2 · 1B | 770 MB | 2.0 GB | **Recommended** default for summaries and OCR-grounded prompts on ~8 GB RAM |
| `qwen3-vl-4b` | Qwen3-VL · 4B (advanced) | 2.5 GB | 6.0 GB | Optional multimodal GGUF: **screen VLM (OCR-grounded)** and **Meta glasses photo import** (pixels via llama.cpp MTMD). Requires a matching **`mmproj-*.gguf`** in the same models directory (see filenames in `src-tauri/src/models.rs` → `QWEN3_VL_MMPROJ_FILENAMES`). |

Separate ONNX assets (not in the GGUF catalog):

| File | Purpose |
| --- | --- |
| `bge-large-en-v1.5-quantized.onnx` + `tokenizer.json` | 1024-d text embeddings (hybrid search) |
| `clip-vit-base-patch32-vision_q4.onnx` | 512-d CLIP vision embeddings for imported photos and screen captures (image-to-image similarity over the `image_embedding` column) |

Install BGE + CLIP with `./scripts/download_model.sh` (or run `scripts/bootstrap/download-embedding-model.sh` and `scripts/bootstrap/download-clip-vision-onnx.sh` separately). Override CLIP path with `FNDR_CLIP_VISION_ONNX` if needed.

Optional: `./scripts/bootstrap/download-local-llm.sh` downloads only Llama 3.2 1B. For **photo import vision**, add Qwen3-VL GGUF plus an **mmproj** from the [official GGUF repo](https://huggingface.co/Qwen/Qwen3-VL-4B-Instruct-GGUF/tree/main). Hugging Face names them `mmproj-Qwen3VL-4B-Instruct-*.gguf` (note **Qwen3VL**). Run `./scripts/bootstrap/download-qwen3-vl-4b.sh` for the main `Qwen3VL-4B-Instruct-Q4_K_M.gguf` (~2.5 GiB) and `./scripts/bootstrap/download-qwen3-vl-mmproj.sh` (defaults to Q8_0) for the mmproj, or place files next to each other in the models directory; see `QWEN3_VL_MMPROJ_FILENAMES` for every accepted mmproj name.

Validate the local embedding and LanceDB path with:

```bash
make diagnostic
```

If an older prototype database was created with a different vector dimension, back it up and let FNDR recreate the 1024-dimensional schema with:

```bash
make reset-lancedb
```

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
