# Knowledge Graph Overhaul — Design

**Date:** 2026-05-16
**Owner:** anurupkumar18
**Status:** approved (Session 1 in progress)
**Skill route (AGENTS.md):** zoom-out → grill-with-docs → this spec → writing-plans → tdd
**Source brief:** in-conversation prompt + `https://api.anthropic.com/v1/design/h/LS25rLsJo2KKHJm_UmY_7w` (Claude Design bundle: "FNDR Design System")

---

## 1. Goal

Replace the placeholder D3 layout in `src/domains/memory-vault/KnowledgeGraph.tsx` with a production-quality, interactive memory graph that:

- Renders **real** FNDR insight-graph data (`graph_nodes` / `graph_edges` via existing IPC).
- Lets users explore memories, projects, topics, themes, entities with smooth pan/zoom, hover inspection, and a contextual side panel.
- Adopts the **Old Film / Archival Paper** cinematic 60-30-10 palette from the Claude Design bundle as new options in the existing `cinematic-palettes.ts` system.
- Ships in three vertical-slice sessions to keep `origin/main` shippable between handoffs.

Non-negotiables: cinematic palette, light/dark, 60-30-10. No memory text printed over nodes. No invented categories or hard-coded sample data.

---

## 2. Source-of-truth references

| Concern | File |
|---|---|
| Insight graph schema (Lance) | `docs/architecture/graph-schema.md` |
| Current graph component | `src/domains/memory-vault/KnowledgeGraph.tsx` |
| Current graph hook | `src/domains/memory-vault/useGraph.ts` |
| IPC surface | `src/shared/ipc/tauri.ts` (`getFullGraph`, `getGraphForProject`, `getNodeDetail`, `findGraphPath`, `getGodNodes`, `searchGraph`) |
| Theme system | `src/shared/theme/cinematic-palettes.ts` (`PALETTES`, `applyPalette`, `PaletteKey`) |
| Design bundle (read-only ref) | `/tmp/fndr-design/fndr-design-system/` — tokens in `project/colors_and_type.css`, mock in `project/ui_kits/macos/Graph.jsx` |
| Engineering rules | `AGENTS.md`, `.agent-skills/portable-engineering/engineering/grill-with-docs/SKILL.md` |

---

## 3. Multi-session decomposition

| Session | Slice (ships to `origin/main`) |
|---|---|
| **S1** *(this spec's plan)* | Film/Paper palette added to cinematic-palettes.ts (default = film). New `graph/` module suite (typed). Clustered, theme-aware SVG render replacing the placeholder. Instant hover neighborhood + explainable edge reasons. Right-side memory card (vertical, middle-right). Vitest coverage for pure modules + hover behavior. |
| **S2** | Top filter bar (project/topic/theme/entity/app/date/confidence/relationship-type, derived from real data). Right-side compact legend/key (data-driven from visible graph). Bottom-right zoom in/out/reset/fit-to-graph/focus-selected controls. Keyboard shortcuts. |
| **S3** | Graph cache + hourly *active-use* refresh (track app-used time, not wall-clock). Performance pass: LOD, virtualization, throttled hover/pan, incremental layout. Empty / loading / error states polished. Optional app-wide theme adoption + a11y audit. |

Each session terminates with `make test` green, a push to `origin/main`, and an appended HANDOFF entry in `docs/superpowers/specs/2026-05-16-knowledge-graph-overhaul-handoff.md`.

---

## 4. Architecture

### 4.1 New module layout

All new code lives under `src/domains/memory-vault/graph/`:

```
graph/
  types.ts                       # GraphNodeView, GraphEdgeView, GraphCluster, GraphLegendRow, RelationshipReason
  graphDataBuilder.ts            # InsightGraphSubgraph -> { nodes, edges, clusters, palette assignments }
  graphRelationshipResolver.ts   # (edge, source, target) -> RelationshipReason[]
  graphLayoutEngine.ts           # pure d3-force config (no DOM); takes cluster centroids + LOD
  graphFilters.ts                # filter state shape + applyFilters() — wired in S2, scaffolded in S1
  graphLegendBuilder.ts          # legend rows from the currently-visible nodes/edges
  graphPalette.ts                # deterministic community-id -> CSS-var-driven amber-ramp
  __tests__/
    graphDataBuilder.test.ts
    graphRelationshipResolver.test.ts
    graphLegendBuilder.test.ts
```

React components (thin, presentational, no graph maths):

```
src/domains/memory-vault/
  KnowledgeGraph.tsx             # composes the canvas + side panel; replaces current 357-line file
  KnowledgeGraphCanvas.tsx       # SVG + d3-force tick; owns hover/selected state
  KnowledgeGraphSidePanel.tsx    # vertical memory card, middle-right, blur+vibrancy
  KnowledgeGraph.css             # film/paper-aware styles; uses cinematic-palettes CSS vars
```

The `useGraph()` hook stays as-is — it already exposes `subgraph`, `loading`, `error`, `load`, `fetchNodeDetail`, `fetchPath`. The overhaul plugs into it without IPC changes.

### 4.2 Module contracts

```ts
// types.ts
export interface GraphNodeView {
  id: string;
  raw: InsightGraphNode;           // unmodified passthrough
  label: string;                   // display label (truncated for tooltip use only — NEVER drawn on the node)
  nodeType: GraphNodeType;
  community: number | null;        // Louvain community id (from raw.metadata or attached map)
  size: number;                    // computed from connection count + confidence
  importance: number;              // 0..1, derived from confidence × log2(connections+1)
}

export interface GraphEdgeView {
  id: string;
  raw: InsightGraphEdge;
  sourceId: string;
  targetId: string;
  edgeType: GraphEdgeType;
  confidence: number;
  kind: "structural" | "semantic" | "temporal" | "reference" | "conflict";
  reasons: RelationshipReason[];   // built by relationshipResolver
}

export interface GraphCluster {
  id: number;                      // community id
  nodeIds: string[];
  label: string | null;            // cluster_0_name from backend, when present
  centroid?: { x: number; y: number };
}

export interface RelationshipReason {
  text: string;                    // already-templated, ready for the card
  tone: "neutral" | "amber" | "alarm";
}

export interface GraphLegendRow {
  kind: "node-cluster" | "node-type" | "edge-kind" | "encoding";
  label: string;
  swatch: { color: string; shape?: "dot" | "ring" | "dash" | "dot-dot" };
}
```

### 4.3 Data flow

```
useGraph().subgraph  ─►  graphDataBuilder.build(subgraph)
                                 │
   ┌─────────────────────────────┼─────────────────────────────┐
   ▼                             ▼                             ▼
GraphNodeView[]            GraphEdgeView[]               GraphCluster[]
   │                             │                             │
   └────────────────┬────────────┘                             │
                    ▼                                          │
       graphLayoutEngine(positions)  ◄───── cluster centroids ─┘
                    │
                    ▼
       KnowledgeGraphCanvas (SVG)
                    │ hover / select
                    ▼
       KnowledgeGraph (composer)
                    │ selected node id
                    ▼
       KnowledgeGraphSidePanel
                    │ (lazy)
                    ▼
       useGraph().fetchNodeDetail(id) → memory card body
```

Hover state is local React state on `KnowledgeGraph.tsx`. No IPC, no layout re-run. Selection drives only an additive `getNodeDetail` call for the card preview.

---

## 5. Visual encoding (Session 1)

### 5.1 Palette plumbing

Add **one** new palette key to `PALETTES` in `src/shared/theme/cinematic-palettes.ts`:

```ts
film: {
  name: "Old Film",
  year: 2026,
  director: "FNDR",
  description: "Personal memory, processed like film. Amber halation over deep umber.",
  shades: ["#1a1410", "#221915", "#2a2018", "#352a20", "#a37a30", "#d4a04a", "#e8b85a"],
  dark:  { /* maps the 'Old Film' tokens from colors_and_type.css */ },
  light: { /* maps the 'Archival Paper' tokens */ },
}
```

Mode toggles inside this palette key swap dark (`Old Film`) and light (`Archival Paper`). This matches the existing `PaletteKey` × `PaletteMode` contract exactly, and the user's "Add as 2 new palettes" choice — one brand palette with two modes is the closest fit; if a second `paper` key is later wanted as a forced-light alias, it's a trivial follow-up.

`src/app/main.tsx` keeps reading `STORAGE_KEYS.theme` and `STORAGE_KEYS.palette`; default palette becomes `"film"` if `localStorage` is empty.

Extra brand tokens not covered by the existing `PaletteTokens` shape (halation glow, hairline-strong, lift shadow, ease-shutter, font stacks) ship as a new CSS module at `src/shared/theme/film-paper.css` imported by `src/app/styles/index.css`. These tokens use the same `:root` selector and reference `--cp-*` variables where possible, so the brand stays palette-aware.

### 5.2 Node & edge encoding

| Aspect | Rule |
|---|---|
| **Node fill** | Deterministic per Louvain community via `graphPalette.colorForCommunity(id, themeTokens)` — amber ramp keyed off `--cp-accent`. No community → `--cp-accent-muted`. |
| **Node size** | `clamp(6, 8 + log2(connectionCount + 1) * 3, 18)` px radius. |
| **Node label on canvas** | **Never** for individual memory nodes. Cluster labels only, at cluster centroid, mono uppercase, visible only when `zoom > 0.6`. |
| **Edge stroke** | Solid for structural (`PartOf`, `DependsOn`, `Contains`, `Imports`, `Extends`, `Implements`, `UsedIn`, `CreatedBy`). Dashed for semantic (`SimilarTo`). Dotted for reference (`MentionedIn`, `AppliesTo`). Directional arrowhead for temporal (`PrecededBy`, `FollowedBy`) and dependency (`DependsOn`, `Causes`, `TriggeredBy`). |
| **Edge width** | `0.4 + confidence * 1.6` px. |
| **Edge color** | `--cp-border` dimmed (`opacity 0.25`) by default; `--cp-accent` when in hover neighborhood; `--cp-alarm` / `--film-red` for `Contradicts`. |
| **Hover** | Focused node + 1-hop neighbors at full opacity; everything else fades to `opacity: 0.18` over 220ms (`--ease-shutter`). Halation ring grows ~30% around hovered node. |
| **Selection** | Persistent halation; pulsing inner core (radius animation, 2.4s). |
| **Drift** | Each node ±1px sine drift on a 6s loop, phase keyed by `id.charCodeAt(0)` — direct port from `Graph.jsx` in the design bundle. |
| **Cursor trail** | (S2 polish — out of scope for S1.) |

### 5.3 Canvas surface

`KnowledgeGraphCanvas.tsx` wraps the SVG in a container with:

- `background: radial-gradient(ellipse at 50% 40%, var(--cp-surface), var(--cp-bg), #0e0a07)`
- `.film-grain` overlay class (svg noise at 6% opacity, mix-blend `overlay`)
- Hairline border `1px solid var(--cp-border)`
- Border-radius `12px`

---

## 6. Relationship explanations

`graphRelationshipResolver.ts` exposes:

```ts
export function explainEdge(
  edge: InsightGraphEdge,
  source: InsightGraphNode,
  target: InsightGraphNode,
): RelationshipReason[]
```

Rules:

| Trigger | Reason text (template) | Tone |
|---|---|---|
| `edge_type === "PartOf"` | `part of {target.label}` | neutral |
| `edge_type === "Contains"` | `contains {source.label}` | neutral |
| `edge_type ∈ {DependsOn, Imports, UsedIn, Extends, Implements, CreatedBy}` | `{verb} {target.label}` (verb from edge type) | neutral |
| `edge_type === "SimilarTo"` | `semantic similarity · confidence {0.xx}` | amber |
| `edge_type ∈ {MentionedIn, AppliesTo}` | `mentioned in {target.label}` | neutral |
| `edge_type ∈ {PrecededBy, FollowedBy}` | `temporal · {precedes\|follows} {target.label}` | neutral |
| `edge_type ∈ {Contradicts, Supersedes, Resolves, Questions}` | `{verb} {target.label}` | alarm |
| `source.metadata.project === target.metadata.project` and project present | `shared project · {project}` | neutral |
| `source.metadata.topic === target.metadata.topic` and topic present | `shared topic · {topic}` | neutral |
| any unknown edge type | `{edge_type.toLowerCase().replace(/([A-Z])/, " $1")}` (graceful fallback) | neutral |
| `edge.confidence < 0.7` | append `· low confidence` to the primary reason | (preserved) |

All templates take values from real fields. No hard-coded sample strings.

---

## 7. Right-side memory card (vertical, middle-right)

Sizing & chrome (matches design bundle):

- `width: 320px`, hairline left border.
- `background: color-mix(in srgb, var(--cp-bg) 60%, transparent)` + `backdrop-filter: blur(24px) saturate(120%)`.
- `padding: 24px 22px`, `gap: 16px` column.

Content sections (top to bottom):

1. **Stamp** — `FRAME · {paddedIndex}`, mono uppercase, rotated `-1deg`, amber stroke.
2. **Title** — display serif, `node.label`.
3. **Meta line** — mono micro: `{source} · {timestamp}` (from `raw.metadata.source` / `raw.created_at`).
4. **Preview body** — italic EB Garamond; one sentence pulled from `getNodeDetail` (lazy). Falls back to `(preview unavailable)` italic muted if the fetch fails.
5. **Threads pill row** — entity / project / topic from `raw.metadata`, only those that are present.
6. **Connections list** — `connections · N`. Each row: dot + label + mono timestamp; clicking re-selects that node. Subtext shows the first `RelationshipReason` for that edge.
7. **Action row** — `open`, `pin`, `filter related`, `expand neighborhood`. Wired in S1: `open` (re-focus into MemoryCardsPanel) and `filter related`. Other two stubbed with disabled style + tooltip.

No memory body text on the canvas — the card is the only place text lives.

---

## 8. Error handling

- `getFullGraph` failure → quiet italic empty state inside the canvas frame (`nothing to develop yet.`); no toast.
- `getNodeDetail` failure → memory card still renders with the in-graph node fields and shows muted `preview unavailable`; never blocks the rest of the graph.
- No retry loops, no exponential backoff. Manual refresh shipped in S2.

---

## 9. Testing (S1)

Run after each meaningful edit (AGENTS.md verification rule): `npm run typecheck`, then the focused vitest spec, then `make test` before commit.

| Test file | Behavior covered |
|---|---|
| `graphDataBuilder.test.ts` | Empty subgraph → empty views. Single-node subgraph. Multi-community clustering (Louvain map present). Missing Louvain map → null community, no crash. Node size monotone with connection count. |
| `graphRelationshipResolver.test.ts` | Every `GraphEdgeType` produces a non-empty reason. Shared-project surfacing when both nodes carry the same project. Confidence appended for `SimilarTo`. Alarm tone for `Contradicts`. |
| `graphLegendBuilder.test.ts` | Legend only contains node communities and edge kinds actually present in the visible data. No empty rows. |
| `KnowledgeGraph.hover.test.tsx` | Hovering a node sets `data-neighborhood="true"` on 1-hop nodes and `data-neighborhood="dimmed"` on others. Selection persists across hover changes. |

---

## 10. Out of scope for S1 (explicit)

- Top filter bar UI (data shape exists in `graphFilters.ts`, UI is S2).
- Right-side legend rendering (builder exists, panel is S2).
- Bottom-right zoom/reset/fit controls (S2).
- Cache + hourly active-use refresh (S3).
- Performance virtualization / LOD beyond the existing `GRAPH_SIM_MAX_TICKS` cap (S3).
- App-wide theme migration (font stack, surface tokens, button restyles in other panels) (S3).
- Cursor amber trail, semantic search input integration, god-node spotlight, accessibility audit (S3).
- IPC schema changes (none required this epic).

---

## 11. Anti-bloat gate (per AGENTS.md)

- Reuses `useGraph`, `getFullGraph`, `getNodeDetail`, `InsightGraphNode`/`InsightGraphEdge` — no new IPC.
- Replaces (not augments) `KnowledgeGraph.tsx`; the existing 357-line file is removed in S1.
- No new dependencies. `d3-force`, `d3-zoom`, `d3-drag` already vendored.
- Tokens added to existing `cinematic-palettes.ts` registry — same shape, no parallel theme system.

---

## 12. Acceptance (S1)

- Graph renders only real FNDR memories from `getFullGraph`.
- No memory text on individual circles.
- Hover dims unrelated nodes and edges within 220ms; selection persists.
- Click opens the vertical right-side card; card shows real metadata + at least one `RelationshipReason` per connection.
- Switching theme between dark and light swaps Film ↔ Paper without remount.
- `make test` green; new vitest specs pass.
- Pushed to `origin/main` with a handoff doc.
