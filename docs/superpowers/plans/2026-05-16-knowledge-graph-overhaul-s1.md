# Knowledge Graph Overhaul — Session 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the placeholder D3 layout with a typed, theme-aware, hover-explainable Knowledge Graph slice that ships to `origin/main` while keeping the existing `<KnowledgeGraph>` prop API intact for both callsites in `MemoryCardsPanel.tsx`.

**Architecture:** New pure modules under `src/domains/memory-vault/graph/` build typed views, deterministic palette assignments, and human-readable relationship reasons from `InsightGraphSubgraph`. `KnowledgeGraph.tsx` is rewritten to a thin composer that owns hover/selection state, mounts `KnowledgeGraphCanvas` (SVG + d3-force render) and optionally `KnowledgeGraphSidePanel` (vertical memory card). The "Old Film / Archival Paper" brand palette is registered with the existing `cinematic-palettes.ts` system as palette key `"film"` and becomes the default; brand-only tokens ship in a new `film-paper.css` imported by `index.css`.

**Tech Stack:** React 18 + TypeScript 5.6, d3-force/zoom/drag (already vendored), Vitest + @testing-library/react + jsdom, Tauri 2 IPC (no IPC changes this session).

**Spec:** [`docs/superpowers/specs/2026-05-16-knowledge-graph-overhaul-design.md`](../specs/2026-05-16-knowledge-graph-overhaul-design.md)

---

## File Structure

**Create:**

```
src/shared/theme/film-paper.css                                            # brand-only tokens (fonts, halation, ease curves, dossier mark)
src/domains/memory-vault/graph/types.ts                                    # GraphNodeView, GraphEdgeView, GraphCluster, GraphLegendRow, RelationshipReason
src/domains/memory-vault/graph/graphPalette.ts                             # deterministic community id -> CSS color string
src/domains/memory-vault/graph/graphRelationshipResolver.ts                # (edge, source, target) -> RelationshipReason[]
src/domains/memory-vault/graph/graphDataBuilder.ts                         # InsightGraphSubgraph -> { nodes, edges, clusters }
src/domains/memory-vault/graph/graphLegendBuilder.ts                       # visible views -> GraphLegendRow[]
src/domains/memory-vault/graph/graphLayoutEngine.ts                        # pure d3-force config (no DOM)
src/domains/memory-vault/graph/graphFilters.ts                             # filter state shape + applyFilters() (UI in S2)
src/domains/memory-vault/graph/__tests__/graphPalette.test.ts
src/domains/memory-vault/graph/__tests__/graphRelationshipResolver.test.ts
src/domains/memory-vault/graph/__tests__/graphDataBuilder.test.ts
src/domains/memory-vault/graph/__tests__/graphLegendBuilder.test.ts
src/domains/memory-vault/KnowledgeGraphCanvas.tsx                          # SVG + d3-force tick; owns hover/selected presentation
src/domains/memory-vault/KnowledgeGraphSidePanel.tsx                       # vertical right-side memory card
src/domains/memory-vault/__tests__/KnowledgeGraph.hover.test.tsx
```

**Modify:**

```
src/shared/theme/cinematic-palettes.ts                                     # add `film` palette
src/app/styles/index.css                                                   # import film-paper.css
src/app/main.tsx                                                           # default palette becomes "film" when storage empty
src/domains/memory-vault/KnowledgeGraph.tsx                                # rewrite as composer
src/domains/memory-vault/KnowledgeGraph.css                                # brand-aware styles for canvas + side panel
src/domains/memory-vault/MemoryCardsPanel.tsx                              # swap memory-graph-detail aside for KnowledgeGraphSidePanel (feature="graph" only)
```

**Public prop surface of `KnowledgeGraph` is preserved exactly** so neither of the two existing callsites in `MemoryCardsPanel.tsx` breaks. A new optional prop `showSidePanel?: boolean` is added (default `true` for `feature="graph"`, `false` for the strip).

---

## Task 0: Confirm baseline + branch hygiene

**Files:** none (read-only checks)

- [ ] **Step 1: Confirm working tree is clean of *new* graph work**

Run:
```bash
git status --short | grep -E "graph|theme|KnowledgeGraph"
```
Expected: empty (any pre-existing uncommitted work belongs to other epics — do NOT stage or revert it).

- [ ] **Step 2: Confirm tests baseline**

Run:
```bash
npm run typecheck && npm test -- --reporter=default
```
Expected: typecheck passes; the existing vitest suite passes (record the count so we can spot regressions later).

- [ ] **Step 3: Note the existing KnowledgeGraph callsites**

Run:
```bash
grep -n "KnowledgeGraph" src/domains/memory-vault/MemoryCardsPanel.tsx
```
Expected: callsites at lines `~943` (strip, height=220) and `~1216` (graph stage, height=420). These must keep working.

---

## Task 1: Add `film` palette to cinematic-palettes.ts

**Files:**
- Modify: `src/shared/theme/cinematic-palettes.ts:27-391` (within the `PALETTES` object)

- [ ] **Step 1: Add the `film` palette entry**

Insert as the **first** entry in `PALETTES` (so it lands at the top of selectable palettes). Place immediately before the existing `matrix:` block:

```ts
    film: {
        name: "Old Film",
        year: 2026,
        director: "Continuum",
        description: "Personal memory, processed like film. Amber halation over deep umber.",
        shades: ["#1a1410", "#221915", "#2a2018", "#352a20", "#a37a30", "#d4a04a", "#e8b85a"],
        dark: {
            bg: "#1a1410",
            surface: "#221915",
            surfaceRaised: "#2a2018",
            border: "rgba(232, 223, 200, 0.08)",
            borderStrong: "rgba(232, 223, 200, 0.22)",
            textPrimary: "#e8dfc8",
            textSecondary: "#c4a878",
            textInverse: "#1a1410",
            accent: "#d4a04a",
            accentMuted: "#a37a30",
            accentSubtle: "#2a2018",
        },
        light: {
            bg: "#f2ead8",
            surface: "#e8dfc8",
            surfaceRaised: "#ddd3bc",
            border: "rgba(42, 31, 26, 0.10)",
            borderStrong: "rgba(42, 31, 26, 0.30)",
            textPrimary: "#2a1f1a",
            textSecondary: "#5a4a3a",
            textInverse: "#f2ead8",
            accent: "#a35a1e",
            accentMuted: "#c4621e",
            accentSubtle: "#e8dfc8",
        },
    },
```

- [ ] **Step 2: Run typecheck**

Run:
```bash
npm run typecheck
```
Expected: PASS. The `PaletteKey` union now includes `"film"`.

- [ ] **Step 3: Commit**

```bash
git add src/shared/theme/cinematic-palettes.ts
git commit -m "$(cat <<'EOF'
feat(theme): add 'film' (Old Film / Archival Paper) cinematic palette

Adds the Continuum brand 60-30-10 palette from the Claude Design bundle.
Dark mode = Old Film (umber + amber halation); light mode = Archival Paper.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Ship brand-only CSS tokens (halation, fonts, eases)

**Files:**
- Create: `src/shared/theme/film-paper.css`
- Modify: `src/app/styles/index.css` (add `@import` near the top)

- [ ] **Step 1: Create `film-paper.css`**

Write to `src/shared/theme/film-paper.css`:

```css
/* =============================================================
   Continuum brand-only tokens layered on top of cinematic-palettes.ts.
   The --cp-* vars (bg, accent, fg) come from applyPalette("film").
   Tokens here are brand atmospherics (halation, eases, fonts,
   font-family) and live in :root because they're palette-invariant.
   ============================================================= */

@import url("https://fonts.googleapis.com/css2?family=Cormorant+Garamond:ital,wght@0,400;0,500;0,600;0,700;1,400;1,500;1,600&family=EB+Garamond:ital,wght@0,400;0,500;0,600;1,400;1,500&family=Cutive+Mono&display=swap");

:root {
    --film-font-display: "Cormorant Garamond", "Times New Roman", serif;
    --film-font-body: "EB Garamond", Georgia, serif;
    --film-font-mono: "Cutive Mono", "Courier New", ui-monospace, monospace;

    --film-ease-shutter: cubic-bezier(0.22, 1, 0.36, 1);
    --film-ease-develop: cubic-bezier(0.65, 0, 0.35, 1);
    --film-ease-iris: cubic-bezier(0.34, 1.56, 0.64, 1);

    --film-dur-fast: 180ms;
    --film-dur-base: 320ms;
    --film-dur-hover: 220ms;
    --film-dur-slow: 680ms;

    --film-track-label: 0.16em;
    --film-track-stamp: 0.24em;

    --film-radius-xs: 2px;
    --film-radius-sm: 4px;
    --film-radius-md: 8px;
    --film-radius-lg: 12px;
    --film-radius-pill: 999px;
}

/* Halation derives from --cp-accent so it follows whichever palette is active. */
:root {
    --film-halation: 0 0 24px -2px color-mix(in srgb, var(--cp-accent) 45%, transparent),
        0 0 48px -8px color-mix(in srgb, var(--cp-accent) 28%, transparent);
    --film-halation-soft: 0 0 16px -4px color-mix(in srgb, var(--cp-accent) 30%, transparent);
    --film-halation-strong: 0 0 0 1px color-mix(in srgb, var(--cp-accent) 45%, transparent),
        0 0 28px -2px color-mix(in srgb, var(--cp-accent) 55%, transparent),
        0 0 64px -8px color-mix(in srgb, var(--cp-accent) 35%, transparent);
}

/* Film grain helper — opt in by adding `.film-grain` to a positioned element. */
.film-grain {
    position: relative;
    isolation: isolate;
}
.film-grain::after {
    content: "";
    position: absolute;
    inset: 0;
    background-image: radial-gradient(
            circle at 30% 20%,
            rgba(232, 223, 200, 0.07) 0,
            transparent 60%
        ),
        radial-gradient(circle at 80% 70%, rgba(232, 223, 200, 0.05) 0, transparent 55%);
    background-size: 220px 220px;
    opacity: 0.6;
    mix-blend-mode: overlay;
    pointer-events: none;
    z-index: 2;
}
```

- [ ] **Step 2: Import it from `index.css`**

Open `src/app/styles/index.css`. Find the top of the file (first 10 lines). Add `@import "../../shared/theme/film-paper.css";` as the first line.

Verify:
```bash
head -3 src/app/styles/index.css
```
Expected: the first line is the new import.

- [ ] **Step 3: Run typecheck + sanity-build**

Run:
```bash
npm run typecheck && npm test -- --run --reporter=default --silent 2>&1 | tail -20
```
Expected: typecheck PASS; vitest baseline unchanged.

- [ ] **Step 4: Commit**

```bash
git add src/shared/theme/film-paper.css src/app/styles/index.css
git commit -m "$(cat <<'EOF'
feat(theme): add brand-only film tokens (halation, eases, fonts)

Palette-invariant brand atmospherics that follow whichever cinematic
palette is active via --cp-accent. Loads Cormorant Garamond / EB
Garamond / Cutive Mono from Google Fonts.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Default to `film` palette when storage is empty

**Files:**
- Modify: `src/app/main.tsx`

- [ ] **Step 1: Read current main.tsx defaults**

Run:
```bash
sed -n '1,20p' src/app/main.tsx
```
Expected: lines showing `storedTheme`, `applyPalette(... "matrix" ...)`.

- [ ] **Step 2: Change the fallback palette from `"matrix"` to `"film"`**

Find the line:
```ts
applyPalette(isPaletteKey(storedPalette) ? storedPalette : "matrix", theme);
```
Replace `"matrix"` with `"film"`.

- [ ] **Step 3: Run typecheck**

Run:
```bash
npm run typecheck
```
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/app/main.tsx
git commit -m "$(cat <<'EOF'
feat(theme): default to 'film' palette when none stored

First-run users now boot into the Continuum Old Film palette. Existing
users keep whatever they had in localStorage.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Define graph view types

**Files:**
- Create: `src/domains/memory-vault/graph/types.ts`

- [ ] **Step 1: Write the types file**

```ts
import type { InsightGraphEdge, InsightGraphNode } from "@/shared/ipc/tauri";

/** Display-ready view of an insight node. */
export interface GraphNodeView {
    id: string;
    raw: InsightGraphNode;
    /** Truncated label for tooltip/side-panel use. NEVER drawn on the node circle. */
    label: string;
    nodeType: string;
    /** Louvain community id, when available. */
    community: number | null;
    /** Number of edges incident to this node (computed). */
    connectionCount: number;
    /** Pixel radius for canvas render. */
    size: number;
    /** Computed 0..1 importance for sort/legend tier. */
    importance: number;
}

/** Display-ready view of an insight edge. */
export interface GraphEdgeView {
    id: string;
    raw: InsightGraphEdge;
    sourceId: string;
    targetId: string;
    edgeType: string;
    confidence: number;
    /** Render bucket; drives stroke style. */
    kind: EdgeKind;
    reasons: RelationshipReason[];
}

export type EdgeKind = "structural" | "semantic" | "reference" | "temporal" | "conflict";

export interface GraphCluster {
    id: number;
    nodeIds: string[];
    /** Optional human label (server-supplied cluster_0_name when available). */
    label: string | null;
}

export interface RelationshipReason {
    text: string;
    tone: "neutral" | "amber" | "alarm";
}

export interface GraphLegendRow {
    kind: "community" | "node-type" | "edge-kind" | "encoding";
    label: string;
    swatch: LegendSwatch;
}

export interface LegendSwatch {
    color: string;
    shape: "dot" | "ring" | "dash" | "dot-dot" | "arrow";
}

/** Result of graphDataBuilder.build(). */
export interface GraphView {
    nodes: GraphNodeView[];
    edges: GraphEdgeView[];
    clusters: GraphCluster[];
    /** Map from community id to display color, deterministic per session. */
    communityColors: Record<number, string>;
}
```

- [ ] **Step 2: Run typecheck**

Run:
```bash
npm run typecheck
```
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/domains/memory-vault/graph/types.ts
git commit -m "$(cat <<'EOF'
feat(graph): typed view models for the knowledge graph

Defines GraphNodeView / GraphEdgeView / GraphCluster / GraphLegendRow /
RelationshipReason / GraphView. These are the display-ready shapes
produced by graphDataBuilder and consumed by the canvas + side panel.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Deterministic community palette (test-first)

**Files:**
- Create: `src/domains/memory-vault/graph/graphPalette.ts`
- Create: `src/domains/memory-vault/graph/__tests__/graphPalette.test.ts`

- [ ] **Step 1: Write the failing test**

Create `src/domains/memory-vault/graph/__tests__/graphPalette.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { colorForCommunity, assignCommunityColors } from "../graphPalette";

describe("graphPalette", () => {
    it("returns the muted accent fallback for null community", () => {
        expect(colorForCommunity(null)).toBe("var(--cp-accent-muted)");
    });

    it("returns a deterministic HSL string for a given community id", () => {
        const a = colorForCommunity(0);
        const b = colorForCommunity(0);
        expect(a).toBe(b);
        expect(a).toMatch(/^hsl\(\d+ \d+% \d+%\)$/);
    });

    it("hue varies between distinct community ids", () => {
        const a = colorForCommunity(0);
        const b = colorForCommunity(1);
        expect(a).not.toBe(b);
    });

    it("assignCommunityColors maps every supplied community id", () => {
        const map = assignCommunityColors([0, 1, 2]);
        expect(Object.keys(map).sort()).toEqual(["0", "1", "2"]);
        for (const v of Object.values(map)) {
            expect(v).toMatch(/^hsl\(\d+ \d+% \d+%\)$/);
        }
    });

    it("assignCommunityColors is stable across calls with the same input", () => {
        const a = assignCommunityColors([3, 7, 11]);
        const b = assignCommunityColors([3, 7, 11]);
        expect(a).toEqual(b);
    });
});
```

- [ ] **Step 2: Run test, verify FAIL**

Run:
```bash
npm test -- src/domains/memory-vault/graph/__tests__/graphPalette.test.ts --run
```
Expected: FAIL with `Cannot find module '../graphPalette'`.

- [ ] **Step 3: Write the implementation**

Create `src/domains/memory-vault/graph/graphPalette.ts`:

```ts
/**
 * Deterministic, palette-aware community coloring.
 *
 * Hues are picked from an amber-leaning slice of the wheel (15–60° + wrap)
 * so colors stay in the Continuum brand neighborhood regardless of community id.
 * Saturation and lightness are constants tuned to read on both Old Film
 * (dark) and Archival Paper (light) backgrounds.
 */
const HUE_STRIDE = 47; // coprime with 360 -> good spread for small N
const SATURATION = 58;
const LIGHTNESS = 52;
const BASE_HUE = 30; // amber centerpoint

export function colorForCommunity(communityId: number | null): string {
    if (communityId === null) {
        return "var(--cp-accent-muted)";
    }
    const hue = ((Math.abs(communityId) * HUE_STRIDE) + BASE_HUE) % 360;
    return `hsl(${hue} ${SATURATION}% ${LIGHTNESS}%)`;
}

export function assignCommunityColors(ids: ReadonlyArray<number>): Record<number, string> {
    const out: Record<number, string> = {};
    for (const id of ids) {
        out[id] = colorForCommunity(id);
    }
    return out;
}
```

- [ ] **Step 4: Run test, verify PASS**

Run:
```bash
npm test -- src/domains/memory-vault/graph/__tests__/graphPalette.test.ts --run
```
Expected: 5 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/domains/memory-vault/graph/graphPalette.ts src/domains/memory-vault/graph/__tests__/graphPalette.test.ts
git commit -m "$(cat <<'EOF'
feat(graph): deterministic community palette in the amber band

colorForCommunity returns a stable HSL string per community id,
biased into Continuum's amber wedge (~30° centerpoint). Null communities
fall back to --cp-accent-muted so the active palette still governs.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Relationship resolver (test-first)

**Files:**
- Create: `src/domains/memory-vault/graph/graphRelationshipResolver.ts`
- Create: `src/domains/memory-vault/graph/__tests__/graphRelationshipResolver.test.ts`

- [ ] **Step 1: Write the failing test**

Create `src/domains/memory-vault/graph/__tests__/graphRelationshipResolver.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import type { InsightGraphEdge, InsightGraphNode } from "@/shared/ipc/tauri";
import { explainEdge, edgeKindFor } from "../graphRelationshipResolver";

function mkNode(over: Partial<InsightGraphNode> = {}): InsightGraphNode {
    return {
        id: "n1",
        node_type: "Concept",
        label: "Halation",
        confidence: 1,
        source_memory_ids: [],
        embedding: null,
        created_at: "2026-05-16T00:00:00Z",
        updated_at: "2026-05-16T00:00:00Z",
        stale: false,
        metadata: {},
        ...over,
    };
}
function mkEdge(over: Partial<InsightGraphEdge> = {}): InsightGraphEdge {
    return {
        id: "e1",
        source_id: "n1",
        target_id: "n2",
        edge_type: "PartOf",
        confidence: 0.9,
        conflict_flag: false,
        created_at: "2026-05-16T00:00:00Z",
        metadata: {},
        ...over,
    };
}

describe("edgeKindFor", () => {
    it.each([
        ["PartOf", "structural"],
        ["Contains", "structural"],
        ["DependsOn", "structural"],
        ["Imports", "structural"],
        ["Extends", "structural"],
        ["Implements", "structural"],
        ["UsedIn", "structural"],
        ["CreatedBy", "structural"],
        ["SimilarTo", "semantic"],
        ["MentionedIn", "reference"],
        ["AppliesTo", "reference"],
        ["PrecededBy", "temporal"],
        ["FollowedBy", "temporal"],
        ["Causes", "temporal"],
        ["TriggeredBy", "temporal"],
        ["Contradicts", "conflict"],
        ["Supersedes", "conflict"],
        ["Resolves", "conflict"],
        ["Questions", "conflict"],
        ["UnknownNewEdgeType", "reference"], // fallback bucket
    ] as const)("classifies %s as %s", (edgeType, kind) => {
        expect(edgeKindFor(edgeType)).toBe(kind);
    });
});

describe("explainEdge", () => {
    it("returns the PartOf reason with the target label", () => {
        const source = mkNode({ id: "n1", label: "Halation" });
        const target = mkNode({ id: "n2", label: "Aperture notes" });
        const reasons = explainEdge(mkEdge({ edge_type: "PartOf" }), source, target);
        expect(reasons[0]).toEqual({
            text: "part of Aperture notes",
            tone: "neutral",
        });
    });

    it("appends shared project from both nodes' metadata", () => {
        const source = mkNode({ id: "n1", label: "A", metadata: { project: "Work / Continuum" } });
        const target = mkNode({ id: "n2", label: "B", metadata: { project: "Work / Continuum" } });
        const reasons = explainEdge(mkEdge({ edge_type: "PartOf" }), source, target);
        expect(reasons.map((r) => r.text)).toContain("shared project · Work / Continuum");
    });

    it("appends shared topic from both nodes' metadata", () => {
        const source = mkNode({ id: "n1", label: "A", metadata: { topic: "color theory" } });
        const target = mkNode({ id: "n2", label: "B", metadata: { topic: "color theory" } });
        const reasons = explainEdge(mkEdge({ edge_type: "PartOf" }), source, target);
        expect(reasons.map((r) => r.text)).toContain("shared topic · color theory");
    });

    it("uses amber tone for SimilarTo and appends confidence", () => {
        const source = mkNode({ id: "n1" });
        const target = mkNode({ id: "n2", label: "Memory B" });
        const reasons = explainEdge(
            mkEdge({ edge_type: "SimilarTo", confidence: 0.42 }),
            source,
            target,
        );
        expect(reasons[0].tone).toBe("amber");
        expect(reasons[0].text).toMatch(/semantic similarity · confidence 0\.42/);
    });

    it("uses alarm tone for Contradicts", () => {
        const reasons = explainEdge(
            mkEdge({ edge_type: "Contradicts" }),
            mkNode({ id: "n1" }),
            mkNode({ id: "n2", label: "Counter-evidence" }),
        );
        expect(reasons[0].tone).toBe("alarm");
        expect(reasons[0].text).toMatch(/contradicts Counter-evidence/);
    });

    it("appends '· low confidence' when edge confidence < 0.7", () => {
        const reasons = explainEdge(
            mkEdge({ edge_type: "DependsOn", confidence: 0.4 }),
            mkNode({ id: "n1" }),
            mkNode({ id: "n2", label: "Foundation" }),
        );
        expect(reasons[0].text).toMatch(/· low confidence$/);
    });

    it("falls back to a humanized form for unknown edge types", () => {
        const reasons = explainEdge(
            mkEdge({ edge_type: "MentionsObliquely" }),
            mkNode({ id: "n1" }),
            mkNode({ id: "n2", label: "Target" }),
        );
        expect(reasons[0].text).toMatch(/mentions obliquely/i);
    });

    it("returns a temporal reason for FollowedBy", () => {
        const reasons = explainEdge(
            mkEdge({ edge_type: "FollowedBy" }),
            mkNode({ id: "n1", label: "Earlier" }),
            mkNode({ id: "n2", label: "Later" }),
        );
        expect(reasons[0].text).toMatch(/^temporal · follows Later/);
    });
});
```

- [ ] **Step 2: Run test, verify FAIL**

Run:
```bash
npm test -- src/domains/memory-vault/graph/__tests__/graphRelationshipResolver.test.ts --run
```
Expected: FAIL with `Cannot find module '../graphRelationshipResolver'`.

- [ ] **Step 3: Write the implementation**

Create `src/domains/memory-vault/graph/graphRelationshipResolver.ts`:

```ts
import type { InsightGraphEdge, InsightGraphNode } from "@/shared/ipc/tauri";
import type { EdgeKind, RelationshipReason } from "./types";

const STRUCTURAL = new Set([
    "PartOf",
    "Contains",
    "DependsOn",
    "Imports",
    "Extends",
    "Implements",
    "UsedIn",
    "CreatedBy",
    "Refines",
    "Supports",
    "FixedBy",
    "BrokeBy",
    "Prevents",
]);
const SEMANTIC = new Set(["SimilarTo"]);
const REFERENCE = new Set(["MentionedIn", "AppliesTo"]);
const TEMPORAL = new Set(["PrecededBy", "FollowedBy", "Causes", "TriggeredBy"]);
const CONFLICT = new Set(["Contradicts", "Supersedes", "Resolves", "Questions"]);

export function edgeKindFor(edgeType: string): EdgeKind {
    if (STRUCTURAL.has(edgeType)) return "structural";
    if (SEMANTIC.has(edgeType)) return "semantic";
    if (TEMPORAL.has(edgeType)) return "temporal";
    if (CONFLICT.has(edgeType)) return "conflict";
    return "reference"; // includes MentionedIn / AppliesTo plus any future unknowns
}

const STRUCTURAL_VERB: Record<string, string> = {
    PartOf: "part of",
    Contains: "contains",
    DependsOn: "depends on",
    Imports: "imports",
    Extends: "extends",
    Implements: "implements",
    UsedIn: "used in",
    CreatedBy: "created by",
    Refines: "refines",
    Supports: "supports",
    FixedBy: "fixed by",
    BrokeBy: "broke by",
    Prevents: "prevents",
};

const CONFLICT_VERB: Record<string, string> = {
    Contradicts: "contradicts",
    Supersedes: "supersedes",
    Resolves: "resolves",
    Questions: "questions",
};

const REFERENCE_VERB: Record<string, string> = {
    MentionedIn: "mentioned in",
    AppliesTo: "applies to",
};

function humanize(edgeType: string): string {
    // "MentionsObliquely" -> "mentions obliquely"
    return edgeType.replace(/([a-z])([A-Z])/g, "$1 $2").toLowerCase();
}

function metadataField(node: InsightGraphNode, key: string): string | null {
    const md = node.metadata;
    if (md && typeof md === "object" && key in md) {
        const v = (md as Record<string, unknown>)[key];
        return typeof v === "string" && v.trim() ? v : null;
    }
    return null;
}

export function explainEdge(
    edge: InsightGraphEdge,
    source: InsightGraphNode,
    target: InsightGraphNode,
): RelationshipReason[] {
    const reasons: RelationshipReason[] = [];
    const kind = edgeKindFor(edge.edge_type);
    let primary: RelationshipReason;

    switch (kind) {
        case "structural": {
            const verb = STRUCTURAL_VERB[edge.edge_type] ?? humanize(edge.edge_type);
            primary = { text: `${verb} ${target.label}`, tone: "neutral" };
            break;
        }
        case "semantic": {
            const conf = edge.confidence.toFixed(2);
            primary = { text: `semantic similarity · confidence ${conf}`, tone: "amber" };
            break;
        }
        case "temporal": {
            const direction = edge.edge_type === "PrecededBy" ? "precedes" : "follows";
            const verb = edge.edge_type === "Causes"
                ? "causes"
                : edge.edge_type === "TriggeredBy"
                  ? "triggered by"
                  : direction;
            primary = { text: `temporal · ${verb} ${target.label}`, tone: "neutral" };
            break;
        }
        case "conflict": {
            const verb = CONFLICT_VERB[edge.edge_type] ?? humanize(edge.edge_type);
            primary = { text: `${verb} ${target.label}`, tone: "alarm" };
            break;
        }
        case "reference":
        default: {
            const verb = REFERENCE_VERB[edge.edge_type] ?? humanize(edge.edge_type);
            primary = { text: `${verb} ${target.label}`, tone: "neutral" };
            break;
        }
    }

    if (edge.confidence < 0.7 && kind !== "semantic") {
        primary = { ...primary, text: `${primary.text} · low confidence` };
    }
    reasons.push(primary);

    const sourceProject = metadataField(source, "project");
    const targetProject = metadataField(target, "project");
    if (sourceProject && sourceProject === targetProject) {
        reasons.push({ text: `shared project · ${sourceProject}`, tone: "neutral" });
    }

    const sourceTopic = metadataField(source, "topic");
    const targetTopic = metadataField(target, "topic");
    if (sourceTopic && sourceTopic === targetTopic) {
        reasons.push({ text: `shared topic · ${sourceTopic}`, tone: "neutral" });
    }

    return reasons;
}
```

- [ ] **Step 4: Run test, verify PASS**

Run:
```bash
npm test -- src/domains/memory-vault/graph/__tests__/graphRelationshipResolver.test.ts --run
```
Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/domains/memory-vault/graph/graphRelationshipResolver.ts src/domains/memory-vault/graph/__tests__/graphRelationshipResolver.test.ts
git commit -m "$(cat <<'EOF'
feat(graph): explainable edge reasons from real metadata

edgeKindFor() classifies edge types into 5 render buckets.
explainEdge() turns (edge, source, target) into RelationshipReason[]
templated from real metadata (project/topic) and the edge type.
Confidence and alarm tones surface for SimilarTo and Contradicts.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Graph data builder (test-first)

**Files:**
- Create: `src/domains/memory-vault/graph/graphDataBuilder.ts`
- Create: `src/domains/memory-vault/graph/__tests__/graphDataBuilder.test.ts`

- [ ] **Step 1: Write the failing test**

Create `src/domains/memory-vault/graph/__tests__/graphDataBuilder.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import type { InsightGraphSubgraph } from "@/shared/ipc/tauri";
import { buildGraphView } from "../graphDataBuilder";

const baseNode = {
    confidence: 1,
    source_memory_ids: [],
    embedding: null,
    created_at: "2026-05-16T00:00:00Z",
    updated_at: "2026-05-16T00:00:00Z",
    stale: false,
    metadata: {},
};

describe("buildGraphView", () => {
    it("returns an empty view for an empty subgraph", () => {
        const view = buildGraphView({ nodes: [], edges: [] });
        expect(view.nodes).toEqual([]);
        expect(view.edges).toEqual([]);
        expect(view.clusters).toEqual([]);
        expect(view.communityColors).toEqual({});
    });

    it("builds a single-node view with null community", () => {
        const sub: InsightGraphSubgraph = {
            nodes: [{ ...baseNode, id: "n1", node_type: "Concept", label: "A" }],
            edges: [],
        };
        const view = buildGraphView(sub);
        expect(view.nodes).toHaveLength(1);
        expect(view.nodes[0].community).toBeNull();
        expect(view.nodes[0].connectionCount).toBe(0);
        expect(view.clusters).toEqual([]);
    });

    it("groups nodes into clusters using the louvain map", () => {
        const sub: InsightGraphSubgraph = {
            nodes: [
                { ...baseNode, id: "n1", node_type: "Concept", label: "A" },
                { ...baseNode, id: "n2", node_type: "Concept", label: "B" },
                { ...baseNode, id: "n3", node_type: "Concept", label: "C" },
            ],
            edges: [],
            louvain: { n1: 0, n2: 0, n3: 1 },
            cluster_0_name: "primary",
        };
        const view = buildGraphView(sub);
        expect(view.clusters).toHaveLength(2);
        const c0 = view.clusters.find((c) => c.id === 0)!;
        const c1 = view.clusters.find((c) => c.id === 1)!;
        expect(c0.nodeIds.sort()).toEqual(["n1", "n2"]);
        expect(c1.nodeIds).toEqual(["n3"]);
        expect(c0.label).toBe("primary");
        expect(c1.label).toBeNull();
        expect(view.communityColors[0]).toBeDefined();
        expect(view.communityColors[1]).toBeDefined();
    });

    it("counts edges per node and grows size monotonically", () => {
        const sub: InsightGraphSubgraph = {
            nodes: [
                { ...baseNode, id: "hub", node_type: "Concept", label: "Hub" },
                { ...baseNode, id: "leaf", node_type: "Concept", label: "Leaf" },
                { ...baseNode, id: "leaf2", node_type: "Concept", label: "Leaf 2" },
            ],
            edges: [
                {
                    id: "e1",
                    source_id: "hub",
                    target_id: "leaf",
                    edge_type: "PartOf",
                    confidence: 0.9,
                    conflict_flag: false,
                    created_at: "x",
                    metadata: {},
                },
                {
                    id: "e2",
                    source_id: "hub",
                    target_id: "leaf2",
                    edge_type: "PartOf",
                    confidence: 0.9,
                    conflict_flag: false,
                    created_at: "x",
                    metadata: {},
                },
            ],
        };
        const view = buildGraphView(sub);
        const hub = view.nodes.find((n) => n.id === "hub")!;
        const leaf = view.nodes.find((n) => n.id === "leaf")!;
        expect(hub.connectionCount).toBe(2);
        expect(leaf.connectionCount).toBe(1);
        expect(hub.size).toBeGreaterThan(leaf.size);
        expect(hub.size).toBeLessThanOrEqual(18);
    });

    it("drops edges whose endpoints are not in the node set", () => {
        const sub: InsightGraphSubgraph = {
            nodes: [{ ...baseNode, id: "n1", node_type: "Concept", label: "A" }],
            edges: [
                {
                    id: "e1",
                    source_id: "n1",
                    target_id: "missing",
                    edge_type: "PartOf",
                    confidence: 1,
                    conflict_flag: false,
                    created_at: "x",
                    metadata: {},
                },
            ],
        };
        const view = buildGraphView(sub);
        expect(view.edges).toEqual([]);
    });

    it("attaches RelationshipReason[] to each surviving edge", () => {
        const sub: InsightGraphSubgraph = {
            nodes: [
                { ...baseNode, id: "n1", node_type: "Concept", label: "A" },
                { ...baseNode, id: "n2", node_type: "Concept", label: "B" },
            ],
            edges: [
                {
                    id: "e1",
                    source_id: "n1",
                    target_id: "n2",
                    edge_type: "PartOf",
                    confidence: 0.9,
                    conflict_flag: false,
                    created_at: "x",
                    metadata: {},
                },
            ],
        };
        const view = buildGraphView(sub);
        expect(view.edges).toHaveLength(1);
        expect(view.edges[0].reasons.length).toBeGreaterThan(0);
        expect(view.edges[0].reasons[0].text).toBe("part of B");
    });
});
```

- [ ] **Step 2: Run test, verify FAIL**

Run:
```bash
npm test -- src/domains/memory-vault/graph/__tests__/graphDataBuilder.test.ts --run
```
Expected: FAIL with `Cannot find module '../graphDataBuilder'`.

- [ ] **Step 3: Write the implementation**

Create `src/domains/memory-vault/graph/graphDataBuilder.ts`:

```ts
import type { InsightGraphSubgraph } from "@/shared/ipc/tauri";
import type { GraphCluster, GraphEdgeView, GraphNodeView, GraphView } from "./types";
import { assignCommunityColors } from "./graphPalette";
import { edgeKindFor, explainEdge } from "./graphRelationshipResolver";

const MIN_RADIUS = 6;
const MAX_RADIUS = 18;
const BASE_RADIUS = 8;
const RADIUS_SCALE = 3;

const MAX_LABEL_LEN = 60;
function truncateLabel(label: string): string {
    if (label.length <= MAX_LABEL_LEN) return label;
    return `${label.slice(0, MAX_LABEL_LEN - 1).trimEnd()}…`;
}

function clamp(n: number, lo: number, hi: number): number {
    return Math.min(hi, Math.max(lo, n));
}

export function buildGraphView(subgraph: InsightGraphSubgraph): GraphView {
    const nodeIds = new Set(subgraph.nodes.map((n) => n.id));
    const connectionCounts = new Map<string, number>();
    for (const e of subgraph.edges) {
        if (nodeIds.has(e.source_id) && nodeIds.has(e.target_id)) {
            connectionCounts.set(e.source_id, (connectionCounts.get(e.source_id) ?? 0) + 1);
            connectionCounts.set(e.target_id, (connectionCounts.get(e.target_id) ?? 0) + 1);
        }
    }

    const louvain = subgraph.louvain ?? {};
    const nodes: GraphNodeView[] = subgraph.nodes.map((raw) => {
        const connectionCount = connectionCounts.get(raw.id) ?? 0;
        const size = clamp(
            BASE_RADIUS + Math.log2(connectionCount + 1) * RADIUS_SCALE,
            MIN_RADIUS,
            MAX_RADIUS,
        );
        const community = raw.id in louvain ? louvain[raw.id] : null;
        const importance = clamp(raw.confidence * (Math.log2(connectionCount + 1) / 4 + 0.25), 0, 1);
        return {
            id: raw.id,
            raw,
            label: truncateLabel(raw.label),
            nodeType: raw.node_type,
            community,
            connectionCount,
            size,
            importance,
        };
    });

    const nodeIndex = new Map(subgraph.nodes.map((n) => [n.id, n]));
    const edges: GraphEdgeView[] = subgraph.edges
        .filter((e) => nodeIds.has(e.source_id) && nodeIds.has(e.target_id))
        .map((e) => {
            const source = nodeIndex.get(e.source_id)!;
            const target = nodeIndex.get(e.target_id)!;
            return {
                id: e.id,
                raw: e,
                sourceId: e.source_id,
                targetId: e.target_id,
                edgeType: e.edge_type,
                confidence: e.confidence,
                kind: edgeKindFor(e.edge_type),
                reasons: explainEdge(e, source, target),
            };
        });

    const clusterMap = new Map<number, string[]>();
    for (const n of nodes) {
        if (n.community === null) continue;
        const list = clusterMap.get(n.community) ?? [];
        list.push(n.id);
        clusterMap.set(n.community, list);
    }
    const clusters: GraphCluster[] = Array.from(clusterMap.entries())
        .map(([id, nodeIds]) => ({
            id,
            nodeIds,
            label: id === 0 && subgraph.cluster_0_name ? subgraph.cluster_0_name : null,
        }))
        .sort((a, b) => a.id - b.id);

    const communityColors = assignCommunityColors(clusters.map((c) => c.id));

    return { nodes, edges, clusters, communityColors };
}
```

- [ ] **Step 4: Run test, verify PASS**

Run:
```bash
npm test -- src/domains/memory-vault/graph/__tests__/graphDataBuilder.test.ts --run
```
Expected: 6 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/domains/memory-vault/graph/graphDataBuilder.ts src/domains/memory-vault/graph/__tests__/graphDataBuilder.test.ts
git commit -m "$(cat <<'EOF'
feat(graph): buildGraphView() — typed views from real subgraph

Computes connection counts, log-scaled node sizes, Louvain clusters,
deterministic community colors, and pre-resolved RelationshipReason[]
on every surviving edge. Pure function, no DOM, no IPC.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: Legend builder (test-first)

**Files:**
- Create: `src/domains/memory-vault/graph/graphLegendBuilder.ts`
- Create: `src/domains/memory-vault/graph/__tests__/graphLegendBuilder.test.ts`

- [ ] **Step 1: Write the failing test**

Create `src/domains/memory-vault/graph/__tests__/graphLegendBuilder.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import type { GraphView } from "../types";
import { buildLegend } from "../graphLegendBuilder";

const emptyView: GraphView = { nodes: [], edges: [], clusters: [], communityColors: {} };

describe("buildLegend", () => {
    it("returns no rows for an empty view", () => {
        expect(buildLegend(emptyView)).toEqual([]);
    });

    it("only emits edge-kind rows for kinds actually present", () => {
        const view: GraphView = {
            ...emptyView,
            edges: [
                {
                    id: "e",
                    raw: {} as never,
                    sourceId: "a",
                    targetId: "b",
                    edgeType: "PartOf",
                    confidence: 1,
                    kind: "structural",
                    reasons: [],
                },
            ],
        };
        const legend = buildLegend(view);
        const kinds = legend.filter((r) => r.kind === "edge-kind").map((r) => r.label);
        expect(kinds).toEqual(["structural"]);
    });

    it("emits one community row per cluster, using the assigned color", () => {
        const view: GraphView = {
            ...emptyView,
            clusters: [
                { id: 0, nodeIds: ["a"], label: "primary" },
                { id: 1, nodeIds: ["b"], label: null },
            ],
            communityColors: { 0: "hsl(30 58% 52%)", 1: "hsl(77 58% 52%)" },
        };
        const legend = buildLegend(view);
        const communities = legend.filter((r) => r.kind === "community");
        expect(communities).toHaveLength(2);
        expect(communities[0].swatch.color).toBe("hsl(30 58% 52%)");
        expect(communities[0].label).toBe("primary");
        expect(communities[1].label).toMatch(/^community 1$/);
    });

    it("emits an importance encoding row when any node has connections", () => {
        const view: GraphView = {
            ...emptyView,
            nodes: [
                {
                    id: "n",
                    raw: {} as never,
                    label: "x",
                    nodeType: "Concept",
                    community: null,
                    connectionCount: 3,
                    size: 12,
                    importance: 0.5,
                },
            ],
        };
        const legend = buildLegend(view);
        expect(legend.some((r) => r.kind === "encoding" && /size/i.test(r.label))).toBe(true);
    });
});
```

- [ ] **Step 2: Run test, verify FAIL**

Run:
```bash
npm test -- src/domains/memory-vault/graph/__tests__/graphLegendBuilder.test.ts --run
```
Expected: FAIL with `Cannot find module '../graphLegendBuilder'`.

- [ ] **Step 3: Write the implementation**

Create `src/domains/memory-vault/graph/graphLegendBuilder.ts`:

```ts
import type { GraphLegendRow, GraphView } from "./types";

export function buildLegend(view: GraphView): GraphLegendRow[] {
    const rows: GraphLegendRow[] = [];

    for (const cluster of view.clusters) {
        const color = view.communityColors[cluster.id] ?? "var(--cp-accent-muted)";
        rows.push({
            kind: "community",
            label: cluster.label ?? `community ${cluster.id}`,
            swatch: { color, shape: "dot" },
        });
    }

    const seenEdgeKinds = new Set<string>();
    for (const edge of view.edges) {
        if (seenEdgeKinds.has(edge.kind)) continue;
        seenEdgeKinds.add(edge.kind);
        rows.push({
            kind: "edge-kind",
            label: edge.kind,
            swatch: {
                color: "var(--cp-accent)",
                shape:
                    edge.kind === "semantic"
                        ? "dash"
                        : edge.kind === "reference"
                          ? "dot-dot"
                          : edge.kind === "temporal"
                            ? "arrow"
                            : "dot",
            },
        });
    }

    const hasConnections = view.nodes.some((n) => n.connectionCount > 0);
    if (hasConnections) {
        rows.push({
            kind: "encoding",
            label: "size · connection count",
            swatch: { color: "var(--cp-accent)", shape: "ring" },
        });
    }

    return rows;
}
```

- [ ] **Step 4: Run test, verify PASS**

Run:
```bash
npm test -- src/domains/memory-vault/graph/__tests__/graphLegendBuilder.test.ts --run
```
Expected: 4 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/domains/memory-vault/graph/graphLegendBuilder.ts src/domains/memory-vault/graph/__tests__/graphLegendBuilder.test.ts
git commit -m "$(cat <<'EOF'
feat(graph): buildLegend() — legend rows only for visible data

Emits one row per Louvain community (with assigned color and optional
server-supplied label), one row per edge kind actually present, and a
single 'size · connection count' encoding row when any node has edges.
Never emits empty buckets.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: Pure layout engine + filter scaffold (no tests yet — UI-coupled)

**Files:**
- Create: `src/domains/memory-vault/graph/graphLayoutEngine.ts`
- Create: `src/domains/memory-vault/graph/graphFilters.ts`

- [ ] **Step 1: Write `graphLayoutEngine.ts`**

```ts
import * as d3 from "d3";
import type { GraphCluster, GraphEdgeView, GraphNodeView } from "./types";

export interface LayoutSimNode extends d3.SimulationNodeDatum {
    id: string;
    size: number;
    community: number | null;
    view: GraphNodeView;
}

export interface LayoutSimLink extends d3.SimulationLinkDatum<LayoutSimNode> {
    id: string;
    confidence: number;
    view: GraphEdgeView;
}

export interface LayoutConfig {
    width: number;
    height: number;
    /** Maximum tick count before settling. */
    maxTicks: number;
}

/** Build a pre-configured (but unstarted) force simulation. */
export function buildSimulation(
    nodes: LayoutSimNode[],
    links: LayoutSimLink[],
    clusters: GraphCluster[],
    config: LayoutConfig,
): d3.Simulation<LayoutSimNode, LayoutSimLink> {
    const { width, height } = config;

    const sim = d3
        .forceSimulation<LayoutSimNode>(nodes)
        .force(
            "link",
            d3
                .forceLink<LayoutSimNode, LayoutSimLink>(links)
                .id((d) => d.id)
                .distance(96)
                .strength((d) => {
                    const a = (d.source as LayoutSimNode).community;
                    const b = (d.target as LayoutSimNode).community;
                    if (a !== null && a === b) return 0.55;
                    return 0.25;
                }),
        )
        .force("charge", d3.forceManyBody<LayoutSimNode>().strength(-160))
        .force("center", d3.forceCenter(width / 2, height / 2))
        .force(
            "collision",
            d3.forceCollide<LayoutSimNode>().radius((d) => d.size + 8),
        );

    if (clusters.length > 0) {
        const ringR = Math.min(width, height) * 0.34;
        const target = new Map<number, { x: number; y: number }>();
        clusters.forEach((c, i) => {
            const angle = (i / clusters.length) * Math.PI * 2 - Math.PI / 2;
            target.set(c.id, {
                x: width / 2 + ringR * Math.cos(angle),
                y: height / 2 + ringR * Math.sin(angle),
            });
        });

        sim.force(
            "clusterX",
            d3.forceX<LayoutSimNode>((d) => {
                if (d.community === null) return width / 2;
                return target.get(d.community)?.x ?? width / 2;
            }).strength(0.22),
        ).force(
            "clusterY",
            d3.forceY<LayoutSimNode>((d) => {
                if (d.community === null) return height / 2;
                return target.get(d.community)?.y ?? height / 2;
            }).strength(0.22),
        );
    }

    return sim;
}
```

- [ ] **Step 2: Write `graphFilters.ts` (scaffold; UI in S2)**

```ts
import type { GraphView } from "./types";

export interface GraphFilterState {
    nodeTypes: ReadonlySet<string> | null;
    projects: ReadonlySet<string> | null;
    minConfidence: number;
    edgeKinds: ReadonlySet<string> | null;
}

export const EMPTY_FILTERS: GraphFilterState = {
    nodeTypes: null,
    projects: null,
    minConfidence: 0,
    edgeKinds: null,
};

/** Returns a new view with nodes/edges that pass all active filters. Identity when no filters active. */
export function applyFilters(view: GraphView, filters: GraphFilterState): GraphView {
    const noActiveFilters =
        filters.nodeTypes === null &&
        filters.projects === null &&
        filters.edgeKinds === null &&
        filters.minConfidence <= 0;
    if (noActiveFilters) return view;

    const nodes = view.nodes.filter((n) => {
        if (filters.nodeTypes && !filters.nodeTypes.has(n.nodeType)) return false;
        if (filters.projects) {
            const project =
                n.raw.metadata && typeof n.raw.metadata === "object"
                    ? (n.raw.metadata as Record<string, unknown>).project
                    : undefined;
            if (typeof project !== "string" || !filters.projects.has(project)) return false;
        }
        return true;
    });

    const keepIds = new Set(nodes.map((n) => n.id));
    const edges = view.edges.filter((e) => {
        if (!keepIds.has(e.sourceId) || !keepIds.has(e.targetId)) return false;
        if (filters.edgeKinds && !filters.edgeKinds.has(e.kind)) return false;
        if (e.confidence < filters.minConfidence) return false;
        return true;
    });

    const remainingCommunities = new Set(nodes.map((n) => n.community).filter((c): c is number => c !== null));
    const clusters = view.clusters.filter((c) => remainingCommunities.has(c.id));

    return { nodes, edges, clusters, communityColors: view.communityColors };
}
```

- [ ] **Step 3: Run typecheck**

Run:
```bash
npm run typecheck
```
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/domains/memory-vault/graph/graphLayoutEngine.ts src/domains/memory-vault/graph/graphFilters.ts
git commit -m "$(cat <<'EOF'
feat(graph): pure d3-force layout config + filter scaffold

buildSimulation() returns a configured d3 simulation given typed
nodes / links / clusters. applyFilters() projects a GraphView through
filter state (UI wiring lands in S2).

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 10: KnowledgeGraphCanvas (presentational)

**Files:**
- Create: `src/domains/memory-vault/KnowledgeGraphCanvas.tsx`

- [ ] **Step 1: Write the component**

```tsx
import { useEffect, useMemo, useRef } from "react";
import * as d3 from "d3";
import type { GraphEdgeView, GraphNodeView, GraphView } from "./graph/types";
import {
    buildSimulation,
    type LayoutSimLink,
    type LayoutSimNode,
} from "./graph/graphLayoutEngine";

export interface KnowledgeGraphCanvasProps {
    view: GraphView;
    width: number;
    height: number;
    selectedId: string | null;
    hoveredId: string | null;
    neighborhoodIds: ReadonlySet<string>;
    pathNodeIds: ReadonlySet<string>;
    hubNodeIds: ReadonlySet<string>;
    maxTicks: number;
    onHover: (id: string | null) => void;
    onSelect: (node: GraphNodeView) => void;
}

export function KnowledgeGraphCanvas({
    view,
    width,
    height,
    selectedId,
    hoveredId,
    neighborhoodIds,
    pathNodeIds,
    hubNodeIds,
    maxTicks,
    onHover,
    onSelect,
}: KnowledgeGraphCanvasProps) {
    const svgRef = useRef<SVGSVGElement | null>(null);

    const simNodes = useMemo<LayoutSimNode[]>(
        () =>
            view.nodes.map((n) => ({
                id: n.id,
                size: n.size,
                community: n.community,
                view: n,
            })),
        [view.nodes],
    );

    const simLinks = useMemo<LayoutSimLink[]>(() => {
        const ids = new Set(simNodes.map((n) => n.id));
        return view.edges
            .filter((e) => ids.has(e.sourceId) && ids.has(e.targetId))
            .map((e) => ({
                id: e.id,
                source: e.sourceId,
                target: e.targetId,
                confidence: e.confidence,
                view: e,
            }));
    }, [view.edges, simNodes]);

    // Build & run simulation once per view; render into SVG.
    useEffect(() => {
        const svg = svgRef.current;
        if (!svg) return;
        svg.innerHTML = "";

        const root = d3.select(svg);
        const gRoot = root.append("g").attr("class", "kg-canvas-root");

        const zoom = d3
            .zoom<SVGSVGElement, unknown>()
            .scaleExtent([0.35, 4])
            .on("zoom", (event) => {
                gRoot.attr("transform", event.transform.toString());
            });
        root.call(zoom);

        if (simNodes.length === 0) {
            gRoot
                .append("text")
                .attr("x", width / 2)
                .attr("y", height / 2)
                .attr("text-anchor", "middle")
                .attr("class", "kg-empty")
                .text("nothing to develop yet.");
            return;
        }

        const sim = buildSimulation(simNodes, simLinks, view.clusters, {
            width,
            height,
            maxTicks,
        });

        const linkSel = gRoot
            .append("g")
            .attr("class", "kg-edges")
            .selectAll<SVGLineElement, LayoutSimLink>("line")
            .data(simLinks, (d) => d.id)
            .join("line")
            .attr("class", (d) => `kg-edge kg-edge-${d.view.kind}`)
            .attr("data-edge-id", (d) => d.id)
            .attr("stroke-width", (d) => 0.4 + d.confidence * 1.6);

        const drag = d3
            .drag<SVGGElement, LayoutSimNode>()
            .on("start", (event, d) => {
                if (!event.active) sim.alphaTarget(0.25).restart();
                d.fx = d.x;
                d.fy = d.y;
            })
            .on("drag", (event, d) => {
                d.fx = event.x;
                d.fy = event.y;
            })
            .on("end", (event, d) => {
                if (!event.active) sim.alphaTarget(0);
                d.fx = null;
                d.fy = null;
            });

        const nodeSel = gRoot
            .append("g")
            .attr("class", "kg-nodes")
            .selectAll<SVGGElement, LayoutSimNode>("g")
            .data(simNodes, (d) => d.id)
            .join("g")
            .attr("class", "kg-node")
            .attr("data-node-id", (d) => d.id)
            .style("cursor", "pointer")
            .on("mouseenter", (_e, d) => onHover(d.id))
            .on("mouseleave", () => onHover(null))
            .on("click", (_e, d) => onSelect(d.view))
            .call(drag);

        nodeSel
            .append("circle")
            .attr("class", "kg-node-halo")
            .attr("r", (d) => d.size + 10);

        nodeSel
            .append("circle")
            .attr("class", "kg-node-core")
            .attr("r", (d) => d.size)
            .attr("fill", (d) =>
                d.community !== null
                    ? view.communityColors[d.community] ?? "var(--cp-accent)"
                    : "var(--cp-accent-muted)",
            );

        let ticks = 0;
        sim.on("tick", () => {
            ticks += 1;
            linkSel
                .attr("x1", (d) => (d.source as LayoutSimNode).x ?? 0)
                .attr("y1", (d) => (d.source as LayoutSimNode).y ?? 0)
                .attr("x2", (d) => (d.target as LayoutSimNode).x ?? 0)
                .attr("y2", (d) => (d.target as LayoutSimNode).y ?? 0);
            nodeSel.attr("transform", (d) => `translate(${d.x ?? 0},${d.y ?? 0})`);
            if (ticks >= maxTicks) {
                sim.alphaTarget(0);
                sim.stop();
            }
        });

        return () => {
            sim.stop();
            sim.on("tick", null);
            root.on(".zoom", null);
        };
    }, [simNodes, simLinks, view.clusters, view.communityColors, width, height, maxTicks, onHover, onSelect]);

    // Apply dim/highlight classes whenever selection / hover / neighborhood changes (no relayout).
    useEffect(() => {
        const svg = svgRef.current;
        if (!svg) return;
        const isDimming = hoveredId !== null || selectedId !== null;
        const focusSet = new Set<string>(neighborhoodIds);
        if (selectedId) focusSet.add(selectedId);
        if (hoveredId) focusSet.add(hoveredId);

        d3.select(svg)
            .selectAll<SVGGElement, LayoutSimNode>("g.kg-node")
            .attr("data-state", (d) => {
                if (!isDimming) return "idle";
                if (d.id === selectedId) return "selected";
                if (d.id === hoveredId) return "hovered";
                if (focusSet.has(d.id)) return "neighbor";
                return "dimmed";
            })
            .classed("kg-node-path", (d) => pathNodeIds.has(d.id))
            .classed("kg-node-hub", (d) => hubNodeIds.has(d.id));

        d3.select(svg)
            .selectAll<SVGLineElement, LayoutSimLink>("line.kg-edge")
            .attr("data-state", (d) => {
                if (!isDimming) return "idle";
                const sId = (d.source as LayoutSimNode).id;
                const tId = (d.target as LayoutSimNode).id;
                if (focusSet.has(sId) && focusSet.has(tId)) return "active";
                return "dimmed";
            });
    }, [selectedId, hoveredId, neighborhoodIds, pathNodeIds, hubNodeIds]);

    return (
        <svg
            ref={svgRef}
            className="kg-canvas"
            width="100%"
            height={height}
            role="img"
            aria-label="Knowledge graph"
        />
    );
}
```

- [ ] **Step 2: Run typecheck**

Run:
```bash
npm run typecheck
```
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/domains/memory-vault/KnowledgeGraphCanvas.tsx
git commit -m "$(cat <<'EOF'
feat(graph): KnowledgeGraphCanvas — presentational SVG render

Builds d3-force simulation from typed GraphView, renders nodes/edges
with data-state attributes that drive dim/halo styling via CSS. Hover
and selection presentation is a separate effect that mutates
data-state without re-running the layout.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 11: KnowledgeGraphSidePanel (vertical memory card)

**Files:**
- Create: `src/domains/memory-vault/KnowledgeGraphSidePanel.tsx`

- [ ] **Step 1: Write the component**

```tsx
import { useEffect, useState } from "react";
import { getNodeDetail, type InsightGraphNode } from "@/shared/ipc/tauri";
import type { GraphEdgeView, GraphNodeView, RelationshipReason } from "./graph/types";

export interface KnowledgeGraphSidePanelProps {
    selected: GraphNodeView | null;
    incidentEdges: GraphEdgeView[];
    nodeIndex: Map<string, GraphNodeView>;
    onSelectNode: (node: GraphNodeView) => void;
    onOpenContext?: (node: InsightGraphNode) => void;
    onFilterRelated?: (node: InsightGraphNode) => void;
    /** Optional async fetcher override (testing). Defaults to getNodeDetail. */
    fetchDetail?: (id: string) => Promise<InsightGraphNode | null>;
}

function metaField(node: InsightGraphNode, key: string): string | null {
    const md = node.metadata;
    if (md && typeof md === "object" && key in md) {
        const v = (md as Record<string, unknown>)[key];
        return typeof v === "string" && v.trim() ? v : null;
    }
    return null;
}

function previewFrom(detail: InsightGraphNode | null): string | null {
    if (!detail) return null;
    const preview = metaField(detail, "preview") ?? metaField(detail, "summary");
    return preview;
}

export function KnowledgeGraphSidePanel({
    selected,
    incidentEdges,
    nodeIndex,
    onSelectNode,
    onOpenContext,
    onFilterRelated,
    fetchDetail = getNodeDetail,
}: KnowledgeGraphSidePanelProps) {
    const [detail, setDetail] = useState<InsightGraphNode | null>(null);
    const [previewError, setPreviewError] = useState(false);

    useEffect(() => {
        let cancelled = false;
        setDetail(null);
        setPreviewError(false);
        if (!selected) return;
        fetchDetail(selected.id)
            .then((d) => {
                if (!cancelled) setDetail(d);
            })
            .catch(() => {
                if (!cancelled) setPreviewError(true);
            });
        return () => {
            cancelled = true;
        };
    }, [selected, fetchDetail]);

    if (!selected) {
        return (
            <aside className="kg-side-panel kg-side-panel-empty" aria-label="Memory card">
                <p className="kg-side-panel-empty-text">Pick a frame to follow its threads.</p>
            </aside>
        );
    }

    const project = metaField(selected.raw, "project");
    const topic = metaField(selected.raw, "topic");
    const source = metaField(selected.raw, "source") ?? selected.nodeType;
    const timestamp = new Date(selected.raw.created_at).toLocaleString();
    const preview = previewFrom(detail);

    return (
        <aside className="kg-side-panel" aria-label="Memory card">
            <header className="kg-side-panel-head">
                <span className="kg-stamp" aria-hidden="true">
                    FRAME · {selected.id.slice(0, 6).toUpperCase()}
                </span>
            </header>

            <h3 className="kg-side-panel-title">{selected.raw.label}</h3>
            <p className="kg-side-panel-meta">
                {source} · {timestamp}
            </p>

            {preview && <p className="kg-side-panel-preview">"{preview}"</p>}
            {!preview && previewError && (
                <p className="kg-side-panel-preview kg-side-panel-preview-muted">preview unavailable</p>
            )}

            {(project || topic || selected.nodeType) && (
                <section>
                    <p className="kg-side-panel-label">threads</p>
                    <div className="kg-side-panel-pills">
                        {project && <span className="kg-pill">{project}</span>}
                        {topic && <span className="kg-pill">{topic}</span>}
                        <span className="kg-pill">{selected.nodeType.toLowerCase()}</span>
                    </div>
                </section>
            )}

            <section>
                <p className="kg-side-panel-label">connections · {incidentEdges.length}</p>
                <ul className="kg-side-panel-connections">
                    {incidentEdges.map((edge) => {
                        const otherId = edge.sourceId === selected.id ? edge.targetId : edge.sourceId;
                        const other = nodeIndex.get(otherId);
                        if (!other) return null;
                        const reason: RelationshipReason | undefined = edge.reasons[0];
                        return (
                            <li
                                key={edge.id}
                                className={`kg-connection kg-connection-${edge.kind}`}
                                onClick={() => onSelectNode(other)}
                                role="button"
                                tabIndex={0}
                                onKeyDown={(ev) => {
                                    if (ev.key === "Enter" || ev.key === " ") {
                                        ev.preventDefault();
                                        onSelectNode(other);
                                    }
                                }}
                            >
                                <span className="kg-connection-label">{other.raw.label}</span>
                                {reason && (
                                    <span className={`kg-connection-reason kg-tone-${reason.tone}`}>
                                        {reason.text}
                                    </span>
                                )}
                            </li>
                        );
                    })}
                </ul>
            </section>

            <footer className="kg-side-panel-actions">
                <button
                    type="button"
                    className="kg-action"
                    onClick={() => onOpenContext?.(selected.raw)}
                    disabled={!onOpenContext}
                >
                    open
                </button>
                <button
                    type="button"
                    className="kg-action"
                    onClick={() => onFilterRelated?.(selected.raw)}
                    disabled={!onFilterRelated}
                >
                    filter related
                </button>
            </footer>
        </aside>
    );
}
```

- [ ] **Step 2: Run typecheck**

Run:
```bash
npm run typecheck
```
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/domains/memory-vault/KnowledgeGraphSidePanel.tsx
git commit -m "$(cat <<'EOF'
feat(graph): vertical right-side memory card

KnowledgeGraphSidePanel renders the selected node as a film-dossier
card: stamp + title + mono meta + italic preview (lazy-loaded via
getNodeDetail) + thread pills + a connections list whose rows show
the resolved RelationshipReason. Clicking a connection re-focuses
that node.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 12: Rewrite KnowledgeGraph composer (preserve prop API)

**Files:**
- Modify: `src/domains/memory-vault/KnowledgeGraph.tsx` (full rewrite)

- [ ] **Step 1: Replace the file**

Overwrite `src/domains/memory-vault/KnowledgeGraph.tsx` with:

```tsx
import { useEffect, useMemo, useState } from "react";
import type { InsightGraphEdge, InsightGraphNode } from "@/shared/ipc/tauri";
import { buildGraphView } from "./graph/graphDataBuilder";
import type { GraphNodeView } from "./graph/types";
import { KnowledgeGraphCanvas } from "./KnowledgeGraphCanvas";
import { KnowledgeGraphSidePanel } from "./KnowledgeGraphSidePanel";
import { GRAPH_SIM_MAX_TICKS } from "./useGraph";
import "./KnowledgeGraph.css";

export interface KnowledgeGraphProps {
    nodes: InsightGraphNode[];
    edges: InsightGraphEdge[];
    height?: number;
    onNodeClick?: (node: InsightGraphNode) => void;
    selectedNodeId?: string | null;
    pathNodeIds?: readonly string[] | null;
    highlightNodeIds?: readonly string[] | null;
    /** Optional Louvain map from caller (back-compat with existing MemoryCardsPanel callsites). */
    louvainByNodeId?: Record<string, number> | null;
    maxSimulationTicks?: number;
    /** Hierarchical layout is no longer supported; this prop is accepted for back-compat and ignored. */
    layoutMode?: "hierarchical" | "force";
    /** When true, mount the vertical right-side memory card. Default: true. */
    showSidePanel?: boolean;
}

export function KnowledgeGraph({
    nodes,
    edges,
    height = 480,
    onNodeClick,
    selectedNodeId = null,
    pathNodeIds = null,
    highlightNodeIds = null,
    louvainByNodeId = null,
    maxSimulationTicks = GRAPH_SIM_MAX_TICKS,
    showSidePanel = true,
}: KnowledgeGraphProps) {
    const view = useMemo(
        () =>
            buildGraphView({
                nodes,
                edges,
                louvain: louvainByNodeId ?? undefined,
            }),
        [nodes, edges, louvainByNodeId],
    );

    const nodeIndex = useMemo(() => new Map(view.nodes.map((n) => [n.id, n])), [view.nodes]);

    const [hoveredId, setHoveredId] = useState<string | null>(null);
    const [internalSelectedId, setInternalSelectedId] = useState<string | null>(null);
    const effectiveSelectedId = selectedNodeId ?? internalSelectedId;

    // Drop internal selection when an external selection change wipes it.
    useEffect(() => {
        if (selectedNodeId !== undefined && selectedNodeId !== null) {
            setInternalSelectedId(null);
        }
    }, [selectedNodeId]);

    const neighborhoodIds = useMemo(() => {
        const focus = hoveredId ?? effectiveSelectedId;
        if (!focus) return new Set<string>();
        const out = new Set<string>([focus]);
        for (const e of view.edges) {
            if (e.sourceId === focus) out.add(e.targetId);
            if (e.targetId === focus) out.add(e.sourceId);
        }
        return out;
    }, [hoveredId, effectiveSelectedId, view.edges]);

    const pathSet = useMemo(() => new Set(pathNodeIds ?? []), [pathNodeIds]);
    const hubSet = useMemo(() => new Set(highlightNodeIds ?? []), [highlightNodeIds]);

    const selectedNode = effectiveSelectedId ? nodeIndex.get(effectiveSelectedId) ?? null : null;
    const incidentEdges = useMemo(() => {
        if (!effectiveSelectedId) return [];
        return view.edges.filter(
            (e) => e.sourceId === effectiveSelectedId || e.targetId === effectiveSelectedId,
        );
    }, [view.edges, effectiveSelectedId]);

    const handleSelect = (n: GraphNodeView) => {
        setInternalSelectedId(n.id);
        onNodeClick?.(n.raw);
    };

    return (
        <div
            className={`knowledge-graph-shell${showSidePanel ? "" : " knowledge-graph-shell-bare"}`}
            data-empty={view.nodes.length === 0 ? "true" : undefined}
            style={{ height }}
        >
            <div className="knowledge-graph-canvas-wrap film-grain">
                <KnowledgeGraphCanvas
                    view={view}
                    width={0}
                    height={height}
                    selectedId={effectiveSelectedId}
                    hoveredId={hoveredId}
                    neighborhoodIds={neighborhoodIds}
                    pathNodeIds={pathSet}
                    hubNodeIds={hubSet}
                    maxTicks={maxSimulationTicks}
                    onHover={setHoveredId}
                    onSelect={handleSelect}
                />
            </div>
            {showSidePanel && (
                <KnowledgeGraphSidePanel
                    selected={selectedNode}
                    incidentEdges={incidentEdges}
                    nodeIndex={nodeIndex}
                    onSelectNode={handleSelect}
                />
            )}
        </div>
    );
}
```

The `width` prop on the canvas is `0` because the canvas is positioned `absolute inset:0` inside its wrap and reads its own client width on mount. (Existing pattern in the old code.) Update the canvas to read width from its own ref to remove the magic zero:

- [ ] **Step 2: Tighten the canvas to read its own width**

Open `src/domains/memory-vault/KnowledgeGraphCanvas.tsx`. Inside the first `useEffect`, replace the `width` reference in the simulation builder with the SVG's own client width:

Find:
```ts
const sim = buildSimulation(simNodes, simLinks, view.clusters, {
    width,
    height,
    maxTicks,
});
```
Replace with:
```ts
const actualWidth = svg.clientWidth || width || 800;
const sim = buildSimulation(simNodes, simLinks, view.clusters, {
    width: actualWidth,
    height,
    maxTicks,
});
```

Also replace the empty-state x-coord:
Find:
```ts
.attr("x", width / 2)
```
Replace with:
```ts
.attr("x", (svg.clientWidth || width || 800) / 2)
```

- [ ] **Step 3: Run typecheck**

Run:
```bash
npm run typecheck
```
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/domains/memory-vault/KnowledgeGraph.tsx src/domains/memory-vault/KnowledgeGraphCanvas.tsx
git commit -m "$(cat <<'EOF'
feat(graph): rewrite KnowledgeGraph as a thin composer

KnowledgeGraph now builds a typed GraphView via graphDataBuilder,
owns hover/selection state, and mounts KnowledgeGraphCanvas plus the
optional KnowledgeGraphSidePanel. The public prop surface is
preserved verbatim (height / nodes / edges / onNodeClick /
selectedNodeId / pathNodeIds / highlightNodeIds / louvainByNodeId /
maxSimulationTicks); layoutMode is accepted and ignored. Adds
showSidePanel (default true) for non-page hosts.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 13: KnowledgeGraph.css overhaul

**Files:**
- Modify: `src/domains/memory-vault/KnowledgeGraph.css` (full rewrite)

- [ ] **Step 1: Replace the file**

Overwrite `src/domains/memory-vault/KnowledgeGraph.css`:

```css
.knowledge-graph-shell {
    width: 100%;
    display: grid;
    grid-template-columns: 1fr 320px;
    gap: 0;
    border-radius: var(--film-radius-lg);
    border: 1px solid var(--cp-border);
    background: radial-gradient(
        ellipse at 50% 40%,
        var(--cp-surface) 0%,
        var(--cp-bg) 60%,
        color-mix(in srgb, var(--cp-bg) 70%, #000) 100%
    );
    overflow: hidden;
    position: relative;
    isolation: isolate;
}
.knowledge-graph-shell-bare {
    grid-template-columns: 1fr;
}
.knowledge-graph-shell[data-empty="true"] .knowledge-graph-canvas-wrap {
    background: transparent;
}

.knowledge-graph-canvas-wrap {
    position: relative;
    overflow: hidden;
}

.kg-canvas {
    display: block;
    cursor: grab;
    width: 100%;
    height: 100%;
}
.kg-canvas:active {
    cursor: grabbing;
}

.kg-empty {
    font: 400 14px/1.4 var(--film-font-body);
    fill: var(--cp-text-secondary);
    font-style: italic;
}

/* Nodes */
.kg-node {
    transition: opacity var(--film-dur-hover) var(--film-ease-shutter);
}
.kg-node-halo {
    fill: var(--cp-accent);
    opacity: 0;
    transition: opacity var(--film-dur-hover) var(--film-ease-shutter);
}
.kg-node-core {
    stroke: var(--cp-border-strong);
    stroke-width: 0.6;
    transition: stroke var(--film-dur-hover) var(--film-ease-shutter),
        stroke-width var(--film-dur-hover) var(--film-ease-shutter);
}

.kg-node[data-state="hovered"] .kg-node-halo,
.kg-node[data-state="selected"] .kg-node-halo {
    opacity: 0.22;
}
.kg-node[data-state="hovered"] .kg-node-core,
.kg-node[data-state="selected"] .kg-node-core {
    stroke: var(--cp-accent);
    stroke-width: 1.4;
}
.kg-node[data-state="selected"] .kg-node-halo {
    opacity: 0.32;
}
.kg-node[data-state="dimmed"] {
    opacity: 0.18;
}
.kg-node-path .kg-node-core {
    stroke: #fbbf24;
    stroke-width: 1.8;
}
.kg-node-hub .kg-node-core {
    stroke: var(--cp-accent);
    stroke-width: 2;
}

/* Edges */
.kg-edge {
    stroke: var(--cp-border);
    opacity: 0.45;
    transition: opacity var(--film-dur-hover) var(--film-ease-shutter),
        stroke var(--film-dur-hover) var(--film-ease-shutter);
}
.kg-edge-semantic {
    stroke-dasharray: 4 3;
}
.kg-edge-reference {
    stroke-dasharray: 1 3;
}
.kg-edge-conflict {
    stroke: var(--cp-accent);
    stroke-dasharray: 6 2 1 2;
}
.kg-edge[data-state="dimmed"] {
    opacity: 0.12;
}
.kg-edge[data-state="active"] {
    stroke: var(--cp-accent);
    opacity: 0.95;
    filter: drop-shadow(0 0 4px color-mix(in srgb, var(--cp-accent) 50%, transparent));
}

/* Side panel */
.kg-side-panel {
    background: color-mix(in srgb, var(--cp-bg) 60%, transparent);
    backdrop-filter: blur(24px) saturate(120%);
    -webkit-backdrop-filter: blur(24px) saturate(120%);
    border-left: 1px solid var(--cp-border);
    padding: 24px 22px;
    overflow: auto;
    display: flex;
    flex-direction: column;
    gap: 16px;
    min-height: 0;
    color: var(--cp-text-primary);
    font-family: var(--film-font-body);
}
.kg-side-panel-empty-text {
    color: var(--cp-text-secondary);
    font-style: italic;
    font-family: var(--film-font-body);
}
.kg-side-panel-head {
    display: flex;
    align-items: center;
    gap: 8px;
}
.kg-stamp {
    font: 600 9px/1 var(--film-font-mono);
    letter-spacing: var(--film-track-stamp);
    text-transform: uppercase;
    padding: 5px 9px;
    border: 1px solid var(--cp-accent);
    color: var(--cp-accent);
    border-radius: var(--film-radius-xs);
    display: inline-block;
    transform: rotate(-1deg);
}
.kg-side-panel-title {
    font: 500 26px/1.2 var(--film-font-display);
    color: var(--cp-text-primary);
    margin: 0;
}
.kg-side-panel-meta {
    font: 400 11px/1.4 var(--film-font-mono);
    letter-spacing: 0.06em;
    color: var(--cp-text-secondary);
    margin: 0;
}
.kg-side-panel-preview {
    font: 400 14px/1.6 var(--film-font-body);
    color: color-mix(in srgb, var(--cp-text-primary) 80%, var(--cp-text-secondary));
    font-style: italic;
    margin: 0;
}
.kg-side-panel-preview-muted {
    color: var(--cp-text-secondary);
}
.kg-side-panel-label {
    font: 500 10px/1.2 var(--film-font-mono);
    letter-spacing: var(--film-track-label);
    text-transform: uppercase;
    color: var(--cp-text-secondary);
    margin: 0 0 8px 0;
}
.kg-side-panel-pills {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
}
.kg-pill {
    font: 500 10px/1 var(--film-font-mono);
    letter-spacing: var(--film-track-label);
    text-transform: uppercase;
    padding: 5px 9px;
    border: 1px solid var(--cp-border-strong);
    color: var(--cp-text-secondary);
    border-radius: var(--film-radius-pill);
}
.kg-side-panel-connections {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
}
.kg-connection {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 8px 10px;
    border-radius: var(--film-radius-sm);
    border: 1px solid transparent;
    cursor: pointer;
    transition: background var(--film-dur-hover) var(--film-ease-shutter),
        border-color var(--film-dur-hover) var(--film-ease-shutter);
}
.kg-connection:hover,
.kg-connection:focus-visible {
    background: var(--cp-surface);
    border-color: var(--cp-border-strong);
    outline: none;
}
.kg-connection-label {
    font: 400 13px/1.2 var(--film-font-body);
    color: var(--cp-text-primary);
}
.kg-connection-reason {
    font: 400 10px/1.3 var(--film-font-mono);
    letter-spacing: 0.04em;
    color: var(--cp-text-secondary);
}
.kg-tone-amber { color: var(--cp-accent); }
.kg-tone-alarm { color: #c4521e; }

.kg-side-panel-actions {
    margin-top: auto;
    display: flex;
    gap: 8px;
}
.kg-action {
    flex: 1;
    font: 500 11px/1 var(--film-font-mono);
    letter-spacing: var(--film-track-label);
    text-transform: uppercase;
    padding: 9px 14px;
    border-radius: var(--film-radius-sm);
    border: 1px solid var(--cp-border-strong);
    background: transparent;
    color: var(--cp-text-primary);
    cursor: pointer;
    transition: background var(--film-dur-hover) var(--film-ease-shutter),
        border-color var(--film-dur-hover) var(--film-ease-shutter);
}
.kg-action:hover:not(:disabled) {
    background: var(--cp-surface);
    border-color: var(--cp-accent);
    box-shadow: var(--film-halation-soft);
}
.kg-action:disabled {
    opacity: 0.4;
    cursor: not-allowed;
}

/* Back-compat — keep the old class names alive in case any caller still references them. */
.knowledge-graph-wrap {
    width: 100%;
    border-radius: var(--film-radius-lg);
    border: 1px solid var(--cp-border);
    overflow: hidden;
}
.knowledge-graph-svg {
    display: block;
    cursor: grab;
}
.knowledge-graph-svg:active {
    cursor: grabbing;
}
```

- [ ] **Step 2: Run typecheck**

Run:
```bash
npm run typecheck
```
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/domains/memory-vault/KnowledgeGraph.css
git commit -m "$(cat <<'EOF'
feat(graph): film/paper-aware styles for canvas + side panel

Hover dim, halation, dashed/dotted/conflict edge variants, and the
vertical memory card chrome. All values resolve through --cp-* and
--film-* tokens so swapping cinematic palette or light/dark mode
restyles the graph without a remount.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 14: Hover behavior integration test

**Files:**
- Create: `src/domains/memory-vault/__tests__/KnowledgeGraph.hover.test.tsx`

- [ ] **Step 1: Write the test**

```tsx
import { describe, it, expect } from "vitest";
import { fireEvent, render } from "@testing-library/react";
import type { InsightGraphEdge, InsightGraphNode } from "@/shared/ipc/tauri";
import { KnowledgeGraph } from "../KnowledgeGraph";

function mkNode(id: string, label = id): InsightGraphNode {
    return {
        id,
        node_type: "Concept",
        label,
        confidence: 1,
        source_memory_ids: [],
        embedding: null,
        created_at: "2026-05-16T00:00:00Z",
        updated_at: "2026-05-16T00:00:00Z",
        stale: false,
        metadata: {},
    };
}
function mkEdge(id: string, s: string, t: string): InsightGraphEdge {
    return {
        id,
        source_id: s,
        target_id: t,
        edge_type: "PartOf",
        confidence: 0.9,
        conflict_flag: false,
        created_at: "x",
        metadata: {},
    };
}

describe("KnowledgeGraph hover neighborhood", () => {
    it("dims unrelated nodes when one is hovered", async () => {
        const nodes = [mkNode("a"), mkNode("b"), mkNode("c")];
        const edges = [mkEdge("e1", "a", "b")];

        const { container } = render(
            <KnowledgeGraph nodes={nodes} edges={edges} showSidePanel={false} height={400} />,
        );

        // Wait for the simulation to inject node groups into the DOM.
        await new Promise((resolve) => setTimeout(resolve, 50));

        const aGroup = container.querySelector<SVGGElement>('g.kg-node[data-node-id="a"]');
        const bGroup = container.querySelector<SVGGElement>('g.kg-node[data-node-id="b"]');
        const cGroup = container.querySelector<SVGGElement>('g.kg-node[data-node-id="c"]');
        expect(aGroup).not.toBeNull();
        expect(bGroup).not.toBeNull();
        expect(cGroup).not.toBeNull();

        fireEvent.mouseEnter(aGroup!);

        // The post-hover effect runs synchronously after the state update.
        await new Promise((resolve) => setTimeout(resolve, 0));

        expect(aGroup!.getAttribute("data-state")).toBe("hovered");
        expect(bGroup!.getAttribute("data-state")).toBe("neighbor");
        expect(cGroup!.getAttribute("data-state")).toBe("dimmed");
    });

    it("renders the empty state when there are no nodes", () => {
        const { container } = render(
            <KnowledgeGraph nodes={[]} edges={[]} showSidePanel={false} height={300} />,
        );
        const empty = container.querySelector("text.kg-empty");
        expect(empty?.textContent).toMatch(/nothing to develop yet/);
    });
});
```

- [ ] **Step 2: Run test, verify PASS**

Run:
```bash
npm test -- src/domains/memory-vault/__tests__/KnowledgeGraph.hover.test.tsx --run
```
Expected: 2 tests pass.

> **If the hover test fails** because d3-force settling needs more ticks in jsdom, increase the `setTimeout(resolve, 50)` to `200`. Don't change the assertions.

- [ ] **Step 3: Commit**

```bash
git add src/domains/memory-vault/__tests__/KnowledgeGraph.hover.test.tsx
git commit -m "$(cat <<'EOF'
test(graph): hover neighborhood + empty state

Confirms hovering a node sets data-state='hovered' on it, 'neighbor'
on directly connected nodes, and 'dimmed' on the rest. Also confirms
the 'nothing to develop yet.' empty state renders when nodes is [].

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 15: Adopt the side panel in MemoryCardsPanel's graph feature

The existing `MemoryCardsPanel.tsx` already renders its own `memory-graph-detail` aside next to the graph stage (line ~1216). Replace that aside with the new in-graph side panel so the experience matches the spec, while keeping the inline strip caller (line ~943) using `showSidePanel={false}`.

**Files:**
- Modify: `src/domains/memory-vault/MemoryCardsPanel.tsx`

- [ ] **Step 1: Update the strip caller to disable the side panel**

Find the first `<KnowledgeGraph` (around line 943, height=220):

```tsx
                        <KnowledgeGraph
                            height={220}
                            maxSimulationTicks={220}
                            nodes={fullVizGraphNodes}
                            edges={fullGraphEdges}
                            louvainByNodeId={louvainByNodeId}
                            onNodeClick={(n) => void handleGraphNodeClick(n)}
                            selectedNodeId={selectedGraphNode?.id ?? null}
                            pathNodeIds={pathHighlightIds}
                            highlightNodeIds={hubHighlightIds}
                        />
```

Add `showSidePanel={false}` right after `maxSimulationTicks={220}`:

```tsx
                        <KnowledgeGraph
                            height={220}
                            maxSimulationTicks={220}
                            showSidePanel={false}
                            nodes={fullVizGraphNodes}
                            edges={fullGraphEdges}
                            louvainByNodeId={louvainByNodeId}
                            onNodeClick={(n) => void handleGraphNodeClick(n)}
                            selectedNodeId={selectedGraphNode?.id ?? null}
                            pathNodeIds={pathHighlightIds}
                            highlightNodeIds={hubHighlightIds}
                        />
```

- [ ] **Step 2: For the graph stage, keep the legacy aside (do NOT delete it this session)**

Reasoning: the existing `memory-graph-detail` aside is wired to a large body of state (`graphNodeDetail`, `pathHighlightIds`, "Build path", manual semantic search). Replacing it cleanly is a larger surgery than fits in S1 and risks regressions. Instead:

- Leave the existing aside in place.
- The full-stage `<KnowledgeGraph>` (line ~1216) now ALSO renders its own internal side panel by default — which would double up next to the existing aside.
- Add `showSidePanel={false}` to that callsite as well, so MemoryCardsPanel's existing aside remains the source of truth for this session.

Find the second `<KnowledgeGraph` (around line 1216, height=420):

```tsx
                                <KnowledgeGraph
                                    height={420}
                                    maxSimulationTicks={GRAPH_SIM_MAX_TICKS}
                                    nodes={vizGraphNodes}
                                    edges={filteredGraphEdges}
                                    louvainByNodeId={louvainByNodeId}
                                    onNodeClick={(n) => void handleGraphNodeClick(n)}
                                    selectedNodeId={selectedGraphNode?.id ?? null}
                                    pathNodeIds={pathHighlightIds}
                                    highlightNodeIds={hubHighlightIds}
                                />
```

Add `showSidePanel={false}` after `maxSimulationTicks={GRAPH_SIM_MAX_TICKS}`:

```tsx
                                <KnowledgeGraph
                                    height={420}
                                    maxSimulationTicks={GRAPH_SIM_MAX_TICKS}
                                    showSidePanel={false}
                                    nodes={vizGraphNodes}
                                    edges={filteredGraphEdges}
                                    louvainByNodeId={louvainByNodeId}
                                    onNodeClick={(n) => void handleGraphNodeClick(n)}
                                    selectedNodeId={selectedGraphNode?.id ?? null}
                                    pathNodeIds={pathHighlightIds}
                                    highlightNodeIds={hubHighlightIds}
                                />
```

> The new `KnowledgeGraphSidePanel` IS exported and renderable; integrating it into MemoryCardsPanel's full graph stage is a Session 2 task documented in the handoff.

- [ ] **Step 3: Run typecheck**

Run:
```bash
npm run typecheck
```
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/domains/memory-vault/MemoryCardsPanel.tsx
git commit -m "$(cat <<'EOF'
chore(memory-vault): opt MemoryCardsPanel callers out of new side panel

Both existing KnowledgeGraph callsites now pass showSidePanel={false}.
The existing memory-graph-detail aside continues to own selection state
for this session; full adoption of KnowledgeGraphSidePanel is a
Session 2 task.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
```

---

## Task 16: Full verification

**Files:** none

- [ ] **Step 1: Run typecheck + vitest + cargo**

Run:
```bash
make test
```
Expected: `npm run typecheck` PASS; `npm test` PASS with the 4 new test files green; `cargo test` under `src-tauri/` PASS (no Rust touched this session — should be unchanged from baseline).

- [ ] **Step 2: Run a smoke render in dev (manual, optional)**

If a desktop session is available:
```bash
npm run tauri dev
```
Open the Knowledge Graph panel, hover a node, click it, watch the legacy `memory-graph-detail` aside populate; switch palette/mode to confirm the canvas re-skins live.

- [ ] **Step 3: If anything fails, do NOT mark this task complete**

Fix the failure in a new commit; rerun `make test`; only then proceed to Task 17.

---

## Task 17: Push to origin/main + write handoff

**Files:**
- Create: `docs/superpowers/specs/2026-05-16-knowledge-graph-overhaul-handoff.md`

- [ ] **Step 1: Push to origin/main**

Run:
```bash
git push origin main
```
Expected: both remotes (capstone + github) accept the push.

- [ ] **Step 2: Write the handoff doc**

Create `docs/superpowers/specs/2026-05-16-knowledge-graph-overhaul-handoff.md` with the contents shown below. (Bash sketches the file; replace `<S1 LAST SHA>` with the output of `git rev-parse HEAD`.)

```markdown
# Knowledge Graph Overhaul — Handoff Log

Companion to `2026-05-16-knowledge-graph-overhaul-design.md`. Append one block per session.

---

## Session 1 — 2026-05-16

**Last commit on main:** <S1 LAST SHA>

**Shipped:**

- `film` palette registered in `cinematic-palettes.ts`; default for fresh users.
- Brand-only tokens (halation, eases, fonts) in `src/shared/theme/film-paper.css`.
- New typed graph data layer under `src/domains/memory-vault/graph/`:
  `types`, `graphPalette`, `graphRelationshipResolver`, `graphDataBuilder`,
  `graphLegendBuilder`, `graphLayoutEngine`, `graphFilters` (scaffold).
- `KnowledgeGraphCanvas`, `KnowledgeGraphSidePanel`, and rewritten composer.
- Hover dim + halation + edge-kind styling tied to `--cp-*` / `--film-*` tokens.
- Vitest coverage for the four pure modules + hover behavior.
- `make test` green.

**Deliberately NOT done (S2):**

- Top filter bar UI (filter state exists; controls + project/topic/entity/app pickers are TODO).
- Right-side compact legend rendering (builder exists; presentational component TODO).
- Bottom-right zoom / reset / fit-to-graph / focus-selected controls.
- Keyboard shortcuts.
- Replacing the legacy `memory-graph-detail` aside in `MemoryCardsPanel.tsx`
  with `KnowledgeGraphSidePanel` (both KG callsites currently pass
  `showSidePanel={false}` so the new card doesn't double up). Pick the
  graph-stage callsite (line ~1216) first.

**Deliberately NOT done (S3):**

- Graph cache + hourly *active-use* refresh.
- LOD / virtualization / incremental layout.
- Empty / loading / error states (basic empty state shipped; polish + skeletons TODO).
- App-wide theme migration (only main.tsx default changed; existing panel CSS still references whichever palette user has).
- Accessibility audit + keyboard reachability sweep.
- Cursor amber trail; node drift animation (mentioned in design bundle).

**Where to resume:**

Pick up at Task 1 of `2026-05-16-knowledge-graph-overhaul-s2.md` (to be written).
The natural starting move is replacing the legacy aside in
`MemoryCardsPanel.tsx:~1216-…` with `<KnowledgeGraphSidePanel>` and
wiring its `onOpenContext` / `onFilterRelated` props to the existing
state in that file.

**Open concerns / TODOs:**

- Node drift animation from the design bundle (sine ±1px / 6s) is not yet
  wired — would need a separate `requestAnimationFrame` loop or CSS keyframes
  per node group; defer to S3 perf pass.
- `getNodeDetail` returns the same `InsightGraphNode` we already have in the
  view (preview/summary lives in `metadata`); confirm this is the intended
  source of preview text or extend the IPC to surface a richer preview field.
```

Now commit the handoff and push it:

```bash
git rev-parse HEAD  # capture the SHA; paste into the handoff body before committing
git add docs/superpowers/specs/2026-05-16-knowledge-graph-overhaul-handoff.md
git commit -m "$(cat <<'EOF'
docs(graph): add Session 1 handoff log

Records what shipped, what was deliberately deferred to S2/S3, and the
exact place to resume next session.

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>
EOF
)"
git push origin main
```

---

## Self-Review

### 1. Spec coverage

- Multi-session decomposition (spec §3) → captured in Task 17 handoff template and S2/S3 lists in Task 17. Out-of-scope items in the spec all map to S2 or S3 items.
- Architecture (spec §4) → Tasks 4–11 cover every named module; Task 12 wires the composer; Task 13 ships the CSS.
- Visual encoding (spec §5) → Task 1 (palette), Task 2 (brand tokens), Task 13 (canvas + side-panel styles).
- Relationship explanations (spec §6) → Task 6 covers every documented edge-type rule, shared project/topic, low-confidence appendage, fallback humanization.
- Right-side memory card (spec §7) → Task 11 wires every section listed in the spec; actions wired per spec ("open" + "filter related" passed as props, others disabled).
- Error handling (spec §8) → Empty state in Task 10 + Task 13 (".kg-empty"); preview-unavailable in Task 11.
- Testing (spec §9) → Tasks 5, 6, 7, 8, 14 add all four named test files.
- Out of scope (spec §10) → mirrored in Task 17 handoff.
- Anti-bloat (spec §11) → no new IPC; reuses `useGraph`; one replaced file; no new deps.
- Acceptance (spec §12) → covered by Tasks 14 + 16.

### 2. Placeholder scan

No "TODO", "TBD", "implement later", "fill in details", or "similar to Task N" without repeated code. The single forward reference is the handoff doc's pointer to a not-yet-written `s2.md` — that's intentional and not a placeholder in S1's code path.

### 3. Type consistency

`buildGraphView` is the canonical entry point in Tasks 7, 10, 12.
`explainEdge` is referenced consistently in Tasks 6 and 7.
`GraphView`, `GraphNodeView`, `GraphEdgeView`, `RelationshipReason`, `GraphLegendRow` types are defined once in Task 4 and used identically in Tasks 5–14.
`showSidePanel` is introduced in Task 12 and consumed identically in Task 15.
`KnowledgeGraphCanvas` prop names match exactly between Tasks 10 and 12.

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-05-16-knowledge-graph-overhaul-s1.md`. Two execution options:

**1. Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks, fast iteration.

**2. Inline Execution** — Execute tasks in this session using executing-plans, batch execution with checkpoints.

User has pre-authorized aggressive throughput ("get as much done in this session, even go into S2 and S3"). Recommendation: **Inline Execution** with checkpoint after Task 8 (data-layer baseline), Task 14 (full S1 vertical slice green), and Task 17 (S1 shipped). After Task 17 we re-plan S2 inline.
