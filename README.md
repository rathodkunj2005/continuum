<p align="center">
  <img src="docs/brand/continuum-mark.svg" alt="Continuum mark" width="88" height="88">
</p>

<h1 align="center">Continuum</h1>

<p align="center">
  A local-first macOS memory system for finding and resuming work across apps, searched with EmbeddingGemma 2.
</p>

<p align="center">
  <strong>React + TypeScript</strong> | <strong>Tauri 2</strong> | <strong>Rust</strong> | <strong>LanceDB</strong> | <strong>EmbeddingGemma 2</strong> | <strong>Apple Silicon</strong>
</p>

Continuum is the EmbeddingGemma 2 edition of [FNDR](https://github.com/anurupkumar18/FNDR), kept in step with its main branch. Capture, chunking, dedup, privacy admission and the product surfaces are FNDR's; the text embedding engine is replaced (see [ADR 039](docs/decisions/039-embeddinggemma-2-engine.md)). Source paths, crate names and commands below keep the `fndr` name.

Continuum turns explicitly enabled desktop context into local, source-aware memory records. It combines native screen access, OCR and text cleanup, local EmbeddingGemma 2 embeddings, lexical and semantic retrieval, evidence-grounded answers, and bounded integrations for agents. The durable product path is a structured memory index, not a screenshot archive.

> **Privacy boundary:** memory and indexes stay on the Mac by default. Network integrations are separate, configured paths. Outbound tasks and actions require their own user review and policy checks.

## System map

The diagram shows the active text-memory path and the boundary between local recall and external actions.

<p align="center">
  <img src="docs/architecture/fndr-runtime.svg" alt="FNDR runtime architecture: privacy-admitted capture flows into local records and routed retrieval; egress crosses a separate review and policy boundary" width="100%">
</p>

## Runtime in one view

```mermaid
flowchart LR
  subgraph MAC["macOS app process"]
    UI["React surfaces"] <-->|"typed Tauri IPC"| API["Rust command boundary"]
    MCP["Authenticated MCP server"]
    API --> CAP["Capture and privacy admission"]
    CAP --> OCR["Accessibility text and Vision OCR"]
    OCR --> DOC["Canonical memory document"]
    DOC --> EMB["EmbeddingGemma 2 helper process"]
    DOC --> DB["LanceDB records and indexes"]
    EMB --> DB
    UI --> RET["Shared retrieval runtime"]
    API --> RET
    DB --> RET
    RET --> EVID["Visibility checks, evidence, verification"]
    EVID --> UI
    MCP --> RET
  end
  MCP <-->|"configured integration"| AGENT["External agent"]
  UI --> REVIEW["Explicit review and action policy"]
  REVIEW -->|"approved request only"| PEER["Configured peer or provider"]
```

## Core paths, decomposed

### 1. Capture and memory construction

```mermaid
flowchart LR
  A["Capture enabled + macOS permission"] --> B["Frontmost app and display sample"]
  B --> C{"Privacy and quality admission"}
  C -->|"blocked / private / low signal"| X["Skip; do not create a memory"]
  C -->|"eligible"| D["Accessibility context"]
  D --> E{"Usable text?"}
  E -->|"yes"| G["Normalize observed text"]
  E -->|"no"| F["Apple Vision OCR fallback"]
  F --> G
  G --> H["Source evidence + app, URL, time, reopen metadata"]
  H --> I["Continuity merge and canonical embedding document"]
  I --> J["768-d EmbeddingGemma 2 vector + local lexical index"]
  J --> K["LanceDB parent record"]
  K -. "optional indexing paths" .-> L["BGE chunks and graph relations"]
```

The admission and normalization stages are separate because a readable screen is not automatically a useful or permitted memory. Private and blocked contexts, FNDR's own windows, low-signal frames, and duplicate frames are filtered before they become ordinary records. Accessibility data is preferred where usable; Apple Vision provides OCR from the sampled display when needed.

The durable object is a `MemoryRecord` with text, time, app/window and source metadata, embedding provenance, and any supported reopen target. Screen pixels are transient capture inputs. New text extraction can preserve exact source statements with snapshot and line references; those statements are observations, not proof of speaker, intent, task status, or permission to act.

### 2. Record and index contracts

| Contract | Role | Runtime status |
|---|---|---|
| `MemoryRecord` | Parent record used for cards, context, citations, and reopen decisions | Primary durable memory |
| `MemoryEmbeddingDocument` | One canonical builder for primary, snippet, support, chunk-source, visual-semantic, and graph-node text | Source for role-specific embedding inputs and provenance |
| `MemoryChunkRecord` | Child text window with `parent_id` and its own BGE vector | Optional chunk-first retrieval; child hits resolve back to parent records |
| `raw_evidence.source_evidence` | Exact, bounded screen statements with source line and snapshot identity | Evidence for review and display; never action authority |
| `raw_evidence.embedding_manifest` | Input and vector-role provenance for embedding writes | Supports repair and contract audits |
| Graph nodes and edges | Typed, persisted relationships attached to source memories | Available to graph and browse surfaces; the active query planner does not select graph retrieval |

**Embedding contract.** The live durable parent table is `memories_v7_embeddinggemma2_768`: `google/embeddinggemma-2` (Q8_0 GGUF), 768 dimensions, 2048-token inputs, with DigUp's prompts (`task: search result | query: ` and `title: none | text: `). The model runs in `fndr-eg2-helper`, a child process with its own pinned llama.cpp b11461 and a no-logits patch, because the `llama-cpp-2` crate used for Qwen cannot load it and two ggml copies cannot share one binary. Model identity, dimension, and table are one contract. A vector from another model space cannot be written as if it belonged to this one, and there is no silent fallback to another model: a missing helper or model file is an error. The earlier MiniLM table, `memories_v4_minilm_384`, is left in place and is not read.

The optional child table, `memory_chunks_v1_bge_1024`, uses BGE-large vectors and a separate BM25 path. The chunk route is disabled unless configured and usable chunk rows exist. It scores child text, rolls evidence up to its parent, and returns the parent record for presentation. Model candidates and reindex work do not become the active contract just because they compile or match a vector dimension.

### 3. Query planning, retrieval, and grounded output

```mermaid
flowchart TB
  Q["Search or Ask request"] --> P["Query plan: intent, entities, time, context needs"]
  P --> R["Select routes"]
  R --> V["Dense vector"]
  R --> B["BM25 keyword"]
  R --> T["Temporal filter"]
  R --> E["Entity route"]
  R --> C["Optional BGE chunk route"]
  V --> A["Reauthorize current source records"]
  B --> A
  T --> A
  E --> A
  C --> A
  A --> F["Weighted fusion + strong-match decision"]
  F --> X["Collect cited evidence"]
  X --> Y{"Verifier outcome"}
  Y -->|"supported"| S["Cards or cited answer"]
  Y -->|"weak or unsupported"| N["Partial answer or useful refusal"]
```

Search, Ask, and connected read surfaces reuse the Rust retrieval runtime rather than inventing separate rankers. The planner selects vector and keyword routes, adding temporal and entity routes when the query has matching hints; chunk-first BGE retrieval is optional and requires its separate index. Persisted graph relations are available to graph and browse surfaces, but the active query planner does not select a graph retrieval route. The runtime checks current visibility before route scores are fused and again when evidence is assembled for an answer.

Fusion preserves the route reasons and matched terms that explain a hit. A separate `strong_match` decision prevents a ranked candidate from being presented as a confident answer merely because it was the best available result. The answer path collects bounded source evidence, verifies support, and returns citations or withholds unsupported claims. Card construction and answer composition remain separate outputs of the same retrieval evidence.

### 4. Reopen and resume

Reopen targets are evidence-backed metadata, not guesses from a summary. FNDR can retain browser URLs and text anchors, native document paths exposed by macOS Accessibility, app identifiers, and supported page positions. Before opening a file, the app checks that the path still exists and uses a typed result for success or fallback. Unsupported or missing targets remain explicit outcomes.

Related-memory links are persisted references with source identity. A deleted or currently excluded target is filtered at read time. A graph-derived relationship is presented as a path through recorded nodes and edges, not as a fact stated by the original screen.

## User and integration surfaces

| Surface | What it owns | Boundary |
|---|---|---|
| Search | Query filters, ranked memory hits, match reasons, open/reopen actions | Uses shared retrieval; weak matches remain visibly weak |
| Ask | Evidence collection, verification, cited answer or refusal | No unsupported completion claims |
| Memory Vault | Browse, inspect, and manage persisted memory records and relationships | Current visibility is checked for direct and nested reads |
| Home and Resume | Recent work context and supported return targets | Resume state is derived from currently visible records |
| To-dos and Daily Brief | Mutable tasks plus a deterministic brief composed from recent activity and open tasks | Briefing text is assembled in code without a generation model |
| Screen Guide | Explicit read-only question about the current display or a scoped filename lookup | Ephemeral turn; does not write ordinary memory or open a found file |
| Notch Do | Registered computer-use actions with local risk policy | Each action is classified to run, ask, or never; failures and stop states are explicit |
| MCP server | Authenticated read tools and separately gated write/action tools | Local, tunnel, and public modes have distinct network controls |
| A2A peer send | Person-reviewed task text and selected memory sources | Endpoint and source visibility are rechecked; peer output remains untrusted |

Voice is in migration: the Rust session manager and shared UI hook are present, while Search and Screen Guide still record through renderer-owned `MediaRecorder` flows. A unified cross-surface microphone lifecycle is not yet complete.

### Agent and action paths are not memory paths

```mermaid
sequenceDiagram
  actor Person
  participant FNDR as FNDR local runtime
  participant Store as Local memory store
  participant Policy as Endpoint and action policy
  participant Peer as Configured peer
  Person->>FNDR: Draft a bounded task
  FNDR->>Store: Resolve currently visible selected sources
  FNDR-->>Person: Show exact destination and payload
  Person->>FNDR: Review and send
  FNDR->>Store: Recheck source visibility
  FNDR->>Policy: Validate HTTPS endpoint, DNS, destination, and request
  Policy->>Peer: Send one A2A task
  Peer-->>FNDR: Return task state or artifact
  FNDR->>Store: Save content-free egress metadata
  FNDR-->>Person: Present external response as untrusted evidence
```

MCP exposes local context to a connected client under bearer authentication and origin rules. Write-capable tools have additional settings and action gates. Peer sending follows a distinct path: the person reviews the exact payload; FNDR rebuilds it from currently authorized sources before egress; the remote result does not become a memory or authorize another action. Bearer-protected peers are refused until a peer-scoped credential binding exists.

Notch Do separates planning from execution. A request may be planned by an explicitly selected provider, but the Rust registry and risk policy decide which registered step may run, needs approval, or is never allowed. Captured screen text, web content, agent output, and peer artifacts are data to inspect, not instructions to follow.

## Runtime and module boundaries

| Layer | Main implementation | Responsibility |
|---|---|---|
| Desktop shell | `src/app/`, `src/domains/`, `src/shared/` | React surfaces, domain state, typed IPC wrappers, event subscriptions |
| Native boundary | `src-tauri/src/ipc/`, Tauri command registration | Permission-aware macOS operations and frontend/backend contract |
| Capture | `src-tauri/src/capture/`, `src-tauri/src/privacy/`, `src-tauri/src/ocr/` | Sampling, admission, OCR, normalization, dedupe, memory assembly |
| Embedding | `src-tauri/src/inference/`, `src-tauri/src/embedding/`, `src-tauri/helpers/fndr-eg2/`, `memory_embedding_document.rs` | Versioned text contracts, the EmbeddingGemma 2 helper client, canonical input provenance |
| Persistence | `src-tauri/src/storage/lance_store/`, `src-tauri/src/graph/` | LanceDB schemas, migrations, records, chunks, indexes, graph relations |
| Retrieval and answers | `src-tauri/src/context_runtime/`, `src-tauri/src/search/` | Query planning, route execution, access filtering, fusion, evidence, verification |
| Resume and actions | `src-tauri/src/operator/`, `src-tauri/src/agent/`, reopen commands | Typed targets, action policy, reviewed peer tasks, outcomes and activity records |
| Agent integration | `src-tauri/src/mcp/`, `src-tauri/src/agent/` | Authenticated MCP tools and person-reviewed outbound peer tasks |

Tauri commands are the native capability boundary. The renderer does not read the database or call macOS APIs directly. Backend status is pushed to the UI over typed events where the state is continuous; UI components do not need to poll native state independently.

Local model initialization and inference can block on filesystem, tokenizer, or native runtime work, so these jobs run outside Tokio's async executor. The embedding helper is one shared process per model contract; per-caller batching, caching and preprocessing remain local. Model residency is managed deliberately; a replaced handle is not evidence that native weights have been unloaded.

### Stack

| Concern | Technology |
|---|---|
| Desktop | Tauri 2, macOS native APIs, minimum macOS version 13 |
| UI | React 18, TypeScript, Vite |
| Backend | Rust 2021, Tokio, Serde, Specta-typed IPC |
| Local memory | LanceDB and Arrow schemas, versioned vector tables, local full-text indexes |
| Text embeddings | EmbeddingGemma 2 Q8_0 on llama.cpp (Metal) in `fndr-eg2-helper`; 768-d active contract. ONNX Runtime remains for the optional BGE chunk index |
| Optional local generation | `llama.cpp` through `llama-cpp-2` with Metal; model files are separate assets |
| Native screen path | ScreenCaptureKit, Apple Vision, Accessibility and AppKit |
| MCP transport | Axum, Streamable HTTP and supported SSE compatibility, bearer auth and origin controls |

## Privacy and evidence invariants

- Capture has an explicit enabled/paused state and macOS permission boundary.
- Private-context heuristics, app/site blocklists, FNDR's own process, low-signal checks, and dedupe act before a normal record is admitted.
- The normal memory path persists structured text and metadata, not the screen image. The debug-only Memory Journey has a separately bounded local artifact contract.
- Every derived read surface must authorize the current durable source records. Search filtering alone does not secure Vault, graph, timeline, MCP, activity, or aggregate projections.
- Source statements preserve what appeared on screen; summaries and model-selected fields do not promote that text into verified intent or permission.
- An embedding model, tokenizer, prefix, dimension, and table define one vector space. Reindexing is explicit and must retain rollback and old-data safety.
- Network access is visible and separately configured. MCP authentication, origin checks, action approval, endpoint validation, and content-free activity records protect different boundaries.

## Build and run

FNDR is a macOS application. A development build requires macOS 13 or later, Xcode Command Line Tools, Node.js with npm, and the Rust toolchain.

```bash
npm install
npm run tauri dev
```

Build the embedding helper once. It compiles a pinned, patched llama.cpp, which takes several minutes and needs CMake:

```bash
scripts/eg2/build-helper.sh
```

Development builds find the result under `src-tauri/binaries/` or `src-tauri/helpers/fndr-eg2/target/release/`; `FNDR_EG2_HELPER` overrides the path. Check it against DigUp's reference vectors with `fndr-eg2-helper --text <model.gguf> --check src-tauri/helpers/fndr-eg2/reference`; every cosine must be at least 0.999.

On first run, onboarding downloads the 310 MB `embeddinggemma-2-Q8_0.gguf` (sha256 pinned). A profile that holds MiniLM vectors needs a re-embed, and a `config.toml` that still sets 384 dimensions is refused at startup.

Capture and the rest of the first run are unchanged. macOS may request Screen Recording, Accessibility, or microphone permission for the enabled feature being used. Permission denial should leave an explicit unavailable state; it should not be represented as a successful capture or voice turn.

## Verification

```bash
make test
```

The default gate runs Python script tests, TypeScript typecheck, frontend tests, the production frontend build, and Rust tests. GitHub Actions runs the repository checks in separate workflows.

The preview browser journeys run separately. Install Chromium once after `npm install`, then run the command below. It starts Vite, clicks through six synthetic journeys in light and dark themes, includes keyboard-only variants, and saves screenshots under the ignored `test-results/e2e/` directory.

```bash
npx playwright install chromium
npm run test:e2e
```

Retrieval changes also use the seeded synthetic-persona gate:

```bash
make qa-retrieval-check PERSONA=knowledge-worker
make qa-retrieval-check PERSONA=office-pm
make qa-retrieval-check PERSONA=software-engineer
```

The gate reseeds a disposable QA profile and compares per-surface results with accepted references. It is evidence about the labeled synthetic cases, not proof of native permissions, capture quality on every app, owner-vault usefulness, or participant outcomes. Native and human checks are reported separately. Never point a seeder or migration at the real profile to make a README claim.

## Status of this edition

Measured with `make qa-retrieval-check` on disposable profiles against the MiniLM references, with no other change:

| Persona | Recall@5 | MRR@10 | Gate |
|---|---|---|---|
| knowledge-worker | 1.000 to 0.955 | 0.966 to 0.937 | pass |
| office-pm | 0.900 to 0.850 | 0.661 to 0.785 | fail: one paraphrase fell from rank 9 to a miss |
| software-engineer | 1.000 to 1.000 | 0.831 to 0.883 | pass |

The model ranks the top result better on two of three personas, and search p95 fell from 315 to 372 ms to 167 to 199 ms (the reference timings come from another run). Two gaps are open:

- The strong-match bar (0.25) and the other score thresholds (focus drift 0.30, proactive 0.82, same-task 0.80, graph link 0.82) were tuned for MiniLM. Under EmbeddingGemma 2, no-match queries score 0.55 to 0.60, so every no-match query is currently reported as a strong match. Recalibration is the next step.
- The helper is not yet bundled as a Tauri `externalBin`, so a packaged app does not carry it.

Use of the model is under the Gemma Terms of Use.

## Repository map

```text
src/
  app/                         Shell, navigation and mounted product surfaces
  domains/                     Search, Vault, Home, settings and other UI domains
  shared/                      Typed IPC wrappers and shared UI primitives
src-tauri/src/
  capture/                     Screen sampling and memory assembly
  context_runtime/             Query planning, routes, fusion and answer evidence
  inference/                   Embedding contracts and local generation runtimes
  embedding/                   Chunking, prompts, and the EmbeddingGemma 2 helper client
  ipc/                         Tauri command handlers and native API boundary
  mcp/                         MCP server, tools, resources, authorization
  operator/                    Registered local actions and risk policy
  storage/lance_store/         Local records, vector schemas and indexes
src-tauri/helpers/fndr-eg2/    EmbeddingGemma 2 helper (own llama.cpp, JSON over stdio)
scripts/eg2/                   Pinned llama.cpp build and helper staging
docs/
  architecture/                System maps and schema notes
  decisions/                   Accepted and proposed architecture decisions
  product/                     User-visible contracts and QA protocols
  evidence/                    Sanitized, dated validation artifacts
```

## Technical references

- [Architecture overview](docs/architecture/ARCHITECTURE.md)
- [Documentation index](docs/README.md)
- [Domain and data vocabulary](docs/CONTEXT.md)
- [MCP integration and setup](docs/mcp.md)
- [Agent surfaces](docs/agent.md)
- [Action policy](docs/product/actions-policy.md)
- [Memory Journey evidence contract](docs/product/memory-journey.md)
- [Quality Lab](docs/product/quality-lab.md)
- [October product plan](docs/team/2026-10-month-plan.md)
- [ADR 039: EmbeddingGemma 2 engine](docs/decisions/039-embeddinggemma-2-engine.md)
- [Change history](CHANGELOG.md)

## License

Source code is licensed under the [Apache License 2.0](LICENSE). llama.cpp (MIT) and DigUp's embedding pipeline (MIT) are used under their own licenses; the EmbeddingGemma 2 weights carry the Gemma Terms of Use.
