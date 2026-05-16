# Knowledge Graph Overhaul — Session 2 Implementation Plan

> **For agentic workers:** Use [`superpowers:executing-plans`](https://github.com/superpowers) to implement this plan task-by-task. Steps use `- [ ]` syntax.

**Goal:** Ship the top filter bar, right-side legend, bottom-right zoom controls, MemoryCardsPanel adoption of the new side panel, and keyboard shortcuts — all of S2 from the design spec.

**Architecture:** Filters, legend, and zoom are three independent presentational components composed into `KnowledgeGraph.tsx`. The canvas exposes an imperative ref API for zoom (`zoomIn / zoomOut / reset / fit`) so the bottom-right controls don't need to re-implement d3-zoom. Filter option derivation is a pure helper with vitest coverage. Adopting the side panel in `MemoryCardsPanel` is a surgical swap: the legacy `memory-graph-detail` aside is removed and the canvas callsite passes `showSidePanel={true}`.

**Tech Stack:** Same as S1 — React 18 + TypeScript + d3-zoom (already vendored) + vitest.

**Spec:** [`2026-05-16-knowledge-graph-overhaul-design.md`](../specs/2026-05-16-knowledge-graph-overhaul-design.md) (§3 S2 row)

---

## File Structure

**Create:**

```
src/domains/memory-vault/graph/graphFilterOptions.ts            # derive option lists from a GraphView
src/domains/memory-vault/graph/__tests__/graphFilterOptions.test.ts
src/domains/memory-vault/graph/__tests__/graphFilters.test.ts   # backfill missing test for S1's filters scaffold
src/domains/memory-vault/KnowledgeGraphLegend.tsx               # right-side legend at top of canvas overlay
src/domains/memory-vault/KnowledgeGraphTopBar.tsx               # top filter bar
src/domains/memory-vault/KnowledgeGraphZoomControls.tsx         # bottom-right zoom controls
```

**Modify:**

```
src/domains/memory-vault/KnowledgeGraphCanvas.tsx               # add imperative ref API for zoom
src/domains/memory-vault/KnowledgeGraph.tsx                     # mount legend + topbar + zoom; manage filter state
src/domains/memory-vault/KnowledgeGraph.css                     # legend / topbar / zoom-controls styles
src/domains/memory-vault/MemoryCardsPanel.tsx                   # swap legacy aside for KnowledgeGraphSidePanel on the graph-stage callsite
```

---

## Task 1: Filter-option derivation (TDD)

**Files:**
- Create: `src/domains/memory-vault/graph/graphFilterOptions.ts`
- Create: `src/domains/memory-vault/graph/__tests__/graphFilterOptions.test.ts`

The function `deriveFilterOptions(view)` returns the lists of `nodeTypes / projects / topics / edgeKinds` actually present in the visible data plus a `confidenceRange` `[min, max]`. Drives the top bar pickers.

- [ ] **Step 1: Write the failing test.**

```ts
import { describe, it, expect } from "vitest";
import type { GraphView } from "../types";
import { deriveFilterOptions } from "../graphFilterOptions";

const empty: GraphView = { nodes: [], edges: [], clusters: [], communityColors: {} };

describe("deriveFilterOptions", () => {
    it("returns empty options for an empty view", () => {
        const o = deriveFilterOptions(empty);
        expect(o).toEqual({
            nodeTypes: [],
            projects: [],
            topics: [],
            edgeKinds: [],
            confidenceRange: [0, 1],
        });
    });

    it("collects distinct node types from real nodes", () => {
        const view: GraphView = {
            ...empty,
            nodes: [
                {
                    id: "a", raw: { node_type: "Concept", metadata: {} } as never,
                    label: "x", nodeType: "Concept", community: null,
                    connectionCount: 0, size: 8, importance: 0.3,
                },
                {
                    id: "b", raw: { node_type: "Project", metadata: {} } as never,
                    label: "y", nodeType: "Project", community: null,
                    connectionCount: 0, size: 8, importance: 0.3,
                },
                {
                    id: "c", raw: { node_type: "Concept", metadata: {} } as never,
                    label: "z", nodeType: "Concept", community: null,
                    connectionCount: 0, size: 8, importance: 0.3,
                },
            ],
        };
        const o = deriveFilterOptions(view);
        expect(o.nodeTypes.sort()).toEqual(["Concept", "Project"]);
    });

    it("collects projects and topics from node metadata strings", () => {
        const view: GraphView = {
            ...empty,
            nodes: [
                {
                    id: "a",
                    raw: { node_type: "Concept", metadata: { project: "Work / FNDR", topic: "color" } } as never,
                    label: "x", nodeType: "Concept", community: null,
                    connectionCount: 0, size: 8, importance: 0.3,
                },
                {
                    id: "b",
                    raw: { node_type: "Concept", metadata: { project: "Work / FNDR" } } as never,
                    label: "y", nodeType: "Concept", community: null,
                    connectionCount: 0, size: 8, importance: 0.3,
                },
            ],
        };
        const o = deriveFilterOptions(view);
        expect(o.projects).toEqual(["Work / FNDR"]);
        expect(o.topics).toEqual(["color"]);
    });

    it("collects distinct edge kinds from real edges", () => {
        const view: GraphView = {
            ...empty,
            edges: [
                { id: "e1", raw: {} as never, sourceId: "a", targetId: "b", edgeType: "PartOf", confidence: 0.9, kind: "structural", reasons: [] },
                { id: "e2", raw: {} as never, sourceId: "a", targetId: "b", edgeType: "SimilarTo", confidence: 0.5, kind: "semantic", reasons: [] },
                { id: "e3", raw: {} as never, sourceId: "b", targetId: "c", edgeType: "PartOf", confidence: 0.9, kind: "structural", reasons: [] },
            ],
        };
        const o = deriveFilterOptions(view);
        expect(o.edgeKinds.sort()).toEqual(["semantic", "structural"]);
        expect(o.confidenceRange[0]).toBeCloseTo(0.5);
        expect(o.confidenceRange[1]).toBeCloseTo(0.9);
    });
});
```

- [ ] **Step 2: Run test (expect FAIL).** `npm test -- src/domains/memory-vault/graph/__tests__/graphFilterOptions.test.ts --run`
- [ ] **Step 3: Write the implementation.**

```ts
import type { GraphView } from "./types";

export interface FilterOptions {
    nodeTypes: string[];
    projects: string[];
    topics: string[];
    edgeKinds: string[];
    confidenceRange: [number, number];
}

function metaString(raw: { metadata: unknown }, key: string): string | null {
    const md = raw.metadata;
    if (md && typeof md === "object" && key in md) {
        const v = (md as Record<string, unknown>)[key];
        if (typeof v === "string" && v.trim()) return v;
    }
    return null;
}

export function deriveFilterOptions(view: GraphView): FilterOptions {
    const nodeTypes = new Set<string>();
    const projects = new Set<string>();
    const topics = new Set<string>();
    for (const n of view.nodes) {
        nodeTypes.add(n.nodeType);
        const p = metaString(n.raw, "project");
        if (p) projects.add(p);
        const t = metaString(n.raw, "topic");
        if (t) topics.add(t);
    }

    const edgeKinds = new Set<string>();
    let minConf = 1;
    let maxConf = 0;
    let sawEdges = false;
    for (const e of view.edges) {
        edgeKinds.add(e.kind);
        sawEdges = true;
        if (e.confidence < minConf) minConf = e.confidence;
        if (e.confidence > maxConf) maxConf = e.confidence;
    }
    const confidenceRange: [number, number] = sawEdges ? [minConf, maxConf] : [0, 1];

    return {
        nodeTypes: Array.from(nodeTypes),
        projects: Array.from(projects),
        topics: Array.from(topics),
        edgeKinds: Array.from(edgeKinds),
        confidenceRange,
    };
}
```

- [ ] **Step 4: Run test (expect PASS).** Commit:

```bash
git add src/domains/memory-vault/graph/graphFilterOptions.ts src/domains/memory-vault/graph/__tests__/graphFilterOptions.test.ts
git commit -m "feat(graph): deriveFilterOptions() — pickable values from real data

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## Task 2: Backfill `graphFilters` test (TDD-after)

**Files:**
- Create: `src/domains/memory-vault/graph/__tests__/graphFilters.test.ts`

```ts
import { describe, it, expect } from "vitest";
import type { GraphView } from "../types";
import { EMPTY_FILTERS, applyFilters } from "../graphFilters";

const sample: GraphView = {
    nodes: [
        { id: "a", raw: { metadata: { project: "P1" } } as never, label: "A", nodeType: "Concept", community: 0, connectionCount: 1, size: 8, importance: 0.5 },
        { id: "b", raw: { metadata: { project: "P2" } } as never, label: "B", nodeType: "Project", community: 1, connectionCount: 1, size: 8, importance: 0.5 },
    ],
    edges: [
        { id: "e1", raw: {} as never, sourceId: "a", targetId: "b", edgeType: "PartOf", confidence: 0.5, kind: "structural", reasons: [] },
    ],
    clusters: [
        { id: 0, nodeIds: ["a"], label: null },
        { id: 1, nodeIds: ["b"], label: null },
    ],
    communityColors: { 0: "x", 1: "y" },
};

describe("applyFilters", () => {
    it("returns the identical view when no filters are active", () => {
        expect(applyFilters(sample, EMPTY_FILTERS)).toBe(sample);
    });

    it("filters by nodeType and prunes orphan edges", () => {
        const out = applyFilters(sample, { ...EMPTY_FILTERS, nodeTypes: new Set(["Concept"]) });
        expect(out.nodes.map((n) => n.id)).toEqual(["a"]);
        expect(out.edges).toEqual([]);
        expect(out.clusters.map((c) => c.id)).toEqual([0]);
    });

    it("filters by minConfidence on edges", () => {
        const out = applyFilters(sample, { ...EMPTY_FILTERS, minConfidence: 0.9 });
        expect(out.edges).toEqual([]);
        expect(out.nodes.map((n) => n.id).sort()).toEqual(["a", "b"]);
    });

    it("filters by project metadata", () => {
        const out = applyFilters(sample, { ...EMPTY_FILTERS, projects: new Set(["P2"]) });
        expect(out.nodes.map((n) => n.id)).toEqual(["b"]);
    });
});
```

- [ ] **Run test (expect PASS).** Commit.

---

## Task 3: Canvas imperative ref API for zoom

**Files:**
- Modify: `src/domains/memory-vault/KnowledgeGraphCanvas.tsx`

Add a `forwardRef` so the parent can call `.zoomIn() / .zoomOut() / .reset() / .fit()` on the canvas. Implementation uses the same `d3.zoom` handle the simulation effect already creates; move it from a useEffect-local variable to a ref so the imperative methods can reach it.

- [ ] **Step 1: Refactor.** Wrap the export in `React.forwardRef<KnowledgeGraphCanvasHandle, KnowledgeGraphCanvasProps>(…)`. Export the handle type:

```ts
export interface KnowledgeGraphCanvasHandle {
    zoomIn: () => void;
    zoomOut: () => void;
    reset: () => void;
    fit: () => void;
}
```

Inside the component, keep a `const zoomRef = useRef<d3.ZoomBehavior<SVGSVGElement, unknown> | null>(null);` and store the zoom there inside the simulation effect (`zoomRef.current = zoom;`). Implement `useImperativeHandle` with:

```ts
useImperativeHandle(ref, () => ({
    zoomIn: () => {
        const svg = svgRef.current;
        const zoom = zoomRef.current;
        if (!svg || !zoom) return;
        d3.select(svg).transition().duration(280).call(zoom.scaleBy, 1.4);
    },
    zoomOut: () => {
        const svg = svgRef.current;
        const zoom = zoomRef.current;
        if (!svg || !zoom) return;
        d3.select(svg).transition().duration(280).call(zoom.scaleBy, 1 / 1.4);
    },
    reset: () => {
        const svg = svgRef.current;
        const zoom = zoomRef.current;
        if (!svg || !zoom) return;
        d3.select(svg).transition().duration(420).call(zoom.transform, d3.zoomIdentity);
    },
    fit: () => {
        const svg = svgRef.current;
        const zoom = zoomRef.current;
        if (!svg || !zoom) return;
        const g = svg.querySelector("g.kg-canvas-root");
        if (!g) return;
        const bbox = (g as SVGGraphicsElement).getBBox();
        if (bbox.width <= 0 || bbox.height <= 0) return;
        const w = svg.clientWidth;
        const h = svg.clientHeight;
        const pad = 32;
        const scale = Math.min((w - pad * 2) / bbox.width, (h - pad * 2) / bbox.height, 4);
        const tx = w / 2 - scale * (bbox.x + bbox.width / 2);
        const ty = h / 2 - scale * (bbox.y + bbox.height / 2);
        d3.select(svg)
            .transition()
            .duration(560)
            .call(zoom.transform, d3.zoomIdentity.translate(tx, ty).scale(scale));
    },
}), []);
```

- [ ] **Step 2: Typecheck + commit.**

```bash
git add src/domains/memory-vault/KnowledgeGraphCanvas.tsx
git commit -m "feat(graph): expose imperative zoom handle on canvas

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## Task 4: KnowledgeGraphZoomControls component

**Files:**
- Create: `src/domains/memory-vault/KnowledgeGraphZoomControls.tsx`

Four small mono buttons (`+ / − / ⊕ / ⌂`) positioned absolutely bottom-right inside the canvas wrap. Just calls into a passed handle.

```tsx
import type { KnowledgeGraphCanvasHandle } from "./KnowledgeGraphCanvas";

export interface KnowledgeGraphZoomControlsProps {
    handle: React.RefObject<KnowledgeGraphCanvasHandle>;
}

export function KnowledgeGraphZoomControls({ handle }: KnowledgeGraphZoomControlsProps) {
    return (
        <div className="kg-zoom-controls" aria-label="Graph zoom controls">
            <button type="button" className="kg-zoom-btn" onClick={() => handle.current?.zoomIn()} aria-label="Zoom in">+</button>
            <button type="button" className="kg-zoom-btn" onClick={() => handle.current?.zoomOut()} aria-label="Zoom out">−</button>
            <button type="button" className="kg-zoom-btn" onClick={() => handle.current?.fit()} aria-label="Fit to graph">⊕</button>
            <button type="button" className="kg-zoom-btn" onClick={() => handle.current?.reset()} aria-label="Reset zoom">⌂</button>
        </div>
    );
}
```

Commit after typecheck.

---

## Task 5: KnowledgeGraphLegend component

**Files:**
- Create: `src/domains/memory-vault/KnowledgeGraphLegend.tsx`

Renders the rows from `buildLegend(view)`. Positioned absolutely top-right of the canvas wrap.

```tsx
import type { GraphLegendRow } from "./graph/types";

function Swatch({ row }: { row: GraphLegendRow }) {
    const { color, shape } = row.swatch;
    if (shape === "dash") {
        return <span className="kg-legend-swatch kg-legend-swatch-dash" style={{ background: color }} />;
    }
    if (shape === "dot-dot") {
        return (
            <span className="kg-legend-swatch kg-legend-swatch-dotdot">
                <span style={{ background: color }} />
                <span style={{ background: color }} />
            </span>
        );
    }
    if (shape === "arrow") {
        return <span className="kg-legend-swatch kg-legend-swatch-arrow" style={{ color }}>→</span>;
    }
    if (shape === "ring") {
        return <span className="kg-legend-swatch kg-legend-swatch-ring" style={{ borderColor: color }} />;
    }
    return <span className="kg-legend-swatch kg-legend-swatch-dot" style={{ background: color }} />;
}

export interface KnowledgeGraphLegendProps {
    rows: readonly GraphLegendRow[];
    collapsed: boolean;
    onToggle: () => void;
}

export function KnowledgeGraphLegend({ rows, collapsed, onToggle }: KnowledgeGraphLegendProps) {
    if (rows.length === 0) return null;
    return (
        <div className={`kg-legend${collapsed ? " kg-legend-collapsed" : ""}`}>
            <button type="button" className="kg-legend-toggle" onClick={onToggle}>
                {collapsed ? "legend ›" : "legend"}
            </button>
            {!collapsed && (
                <ul className="kg-legend-rows">
                    {rows.map((r, i) => (
                        <li key={`${r.kind}-${i}-${r.label}`} className={`kg-legend-row kg-legend-row-${r.kind}`}>
                            <Swatch row={r} />
                            <span className="kg-legend-label">{r.label}</span>
                        </li>
                    ))}
                </ul>
            )}
        </div>
    );
}
```

Commit after typecheck.

---

## Task 6: KnowledgeGraphTopBar component

**Files:**
- Create: `src/domains/memory-vault/KnowledgeGraphTopBar.tsx`

Compact pill+chip bar. For S2 we ship three primary filters (nodeType, project, edgeKind) and a confidence slider; topic and date are stubbed in the design but the data only sometimes provides them. Multi-select via toggling pills.

```tsx
import { useState } from "react";
import type { FilterOptions } from "./graph/graphFilterOptions";
import type { GraphFilterState } from "./graph/graphFilters";

export interface KnowledgeGraphTopBarProps {
    options: FilterOptions;
    filters: GraphFilterState;
    onChange: (next: GraphFilterState) => void;
}

function toggle<T>(set: ReadonlySet<T> | null, value: T): ReadonlySet<T> | null {
    if (!set) return new Set([value]);
    const next = new Set(set);
    if (next.has(value)) next.delete(value);
    else next.add(value);
    return next.size === 0 ? null : next;
}

export function KnowledgeGraphTopBar({ options, filters, onChange }: KnowledgeGraphTopBarProps) {
    const [openMenu, setOpenMenu] = useState<null | "nodeTypes" | "projects" | "edgeKinds">(null);

    const reset = () => onChange({ nodeTypes: null, projects: null, edgeKinds: null, minConfidence: 0 });
    const activeCount =
        (filters.nodeTypes?.size ?? 0) +
        (filters.projects?.size ?? 0) +
        (filters.edgeKinds?.size ?? 0) +
        (filters.minConfidence > 0 ? 1 : 0);

    return (
        <div className="kg-topbar">
            <div className="kg-topbar-title">memory graph</div>
            {options.nodeTypes.length > 0 && (
                <FilterMenu
                    label="type"
                    open={openMenu === "nodeTypes"}
                    onOpen={(v) => setOpenMenu(v ? "nodeTypes" : null)}
                    options={options.nodeTypes}
                    active={filters.nodeTypes}
                    onToggle={(v) => onChange({ ...filters, nodeTypes: toggle(filters.nodeTypes, v) })}
                />
            )}
            {options.projects.length > 0 && (
                <FilterMenu
                    label="project"
                    open={openMenu === "projects"}
                    onOpen={(v) => setOpenMenu(v ? "projects" : null)}
                    options={options.projects}
                    active={filters.projects}
                    onToggle={(v) => onChange({ ...filters, projects: toggle(filters.projects, v) })}
                />
            )}
            {options.edgeKinds.length > 0 && (
                <FilterMenu
                    label="edge"
                    open={openMenu === "edgeKinds"}
                    onOpen={(v) => setOpenMenu(v ? "edgeKinds" : null)}
                    options={options.edgeKinds}
                    active={filters.edgeKinds}
                    onToggle={(v) => onChange({ ...filters, edgeKinds: toggle(filters.edgeKinds, v) })}
                />
            )}
            <label className="kg-topbar-confidence">
                <span className="kg-topbar-confidence-label">min conf</span>
                <input
                    type="range"
                    min={0}
                    max={1}
                    step={0.05}
                    value={filters.minConfidence}
                    onChange={(e) => onChange({ ...filters, minConfidence: parseFloat(e.target.value) })}
                />
                <span className="kg-topbar-confidence-value">{filters.minConfidence.toFixed(2)}</span>
            </label>
            {activeCount > 0 && (
                <button type="button" className="kg-topbar-reset" onClick={reset}>
                    clear · {activeCount}
                </button>
            )}
        </div>
    );
}

interface FilterMenuProps {
    label: string;
    open: boolean;
    onOpen: (open: boolean) => void;
    options: readonly string[];
    active: ReadonlySet<string> | null;
    onToggle: (value: string) => void;
}

function FilterMenu({ label, open, onOpen, options, active, onToggle }: FilterMenuProps) {
    const count = active?.size ?? 0;
    return (
        <div className="kg-topbar-menu">
            <button
                type="button"
                className={`kg-topbar-pill${count > 0 ? " kg-topbar-pill-active" : ""}`}
                onClick={() => onOpen(!open)}
                aria-haspopup="listbox"
                aria-expanded={open}
            >
                {label}{count > 0 ? ` · ${count}` : ""}
            </button>
            {open && (
                <ul className="kg-topbar-menu-list" role="listbox">
                    {options.map((v) => (
                        <li key={v}>
                            <label className="kg-topbar-menu-item">
                                <input
                                    type="checkbox"
                                    checked={active?.has(v) ?? false}
                                    onChange={() => onToggle(v)}
                                />
                                <span>{v}</span>
                            </label>
                        </li>
                    ))}
                </ul>
            )}
        </div>
    );
}
```

Commit after typecheck.

---

## Task 7: Wire filters + legend + zoom into the composer

**Files:**
- Modify: `src/domains/memory-vault/KnowledgeGraph.tsx`

- Build full view via `buildGraphView`, derive `filterOptions = useMemo(deriveFilterOptions(view))`, hold `filterState` in `useState(EMPTY_FILTERS)`, compute `filteredView = useMemo(applyFilters(view, filterState))`, pass filtered view to canvas. Build legend from `filteredView`.
- Add a ref for the canvas handle: `const canvasRef = useRef<KnowledgeGraphCanvasHandle | null>(null);` and forward it.
- Render `<KnowledgeGraphTopBar>` absolutely at top of `.knowledge-graph-canvas-wrap`. Render `<KnowledgeGraphLegend>` absolutely at top-right. Render `<KnowledgeGraphZoomControls handle={canvasRef}>` absolutely at bottom-right.
- Add a `showFilters` prop (default `true`) so the strip caller can opt out. Same for `showLegend`, `showZoomControls`.
- Commit.

---

## Task 8: CSS for top bar, legend, zoom controls

**Files:**
- Modify: `src/domains/memory-vault/KnowledgeGraph.css`

Append styles for `.kg-topbar`, `.kg-legend`, `.kg-zoom-controls`, `.kg-topbar-pill`, `.kg-topbar-menu-list`, `.kg-zoom-btn`. All use `--cp-*` / `--film-*` tokens. Position the topbar via `position: absolute; top: 14px; left: 18px; right: 18px; z-index: 3;`. Position the legend `top: 56px; right: 18px; z-index: 3; max-width: 220px;`. Position zoom controls `bottom: 18px; right: 18px; z-index: 3; display: flex; flex-direction: column; gap: 6px;`.

---

## Task 9: Keyboard shortcuts hook

**Files:**
- Modify: `src/domains/memory-vault/KnowledgeGraph.tsx`

Add a `useEffect` that binds keydown on the shell ref (not window — avoids cross-panel collisions):
- `+` / `=` → `canvasRef.current?.zoomIn()`
- `-` → `canvasRef.current?.zoomOut()`
- `0` → `canvasRef.current?.reset()`
- `f` → `canvasRef.current?.fit()`
- `Escape` → clear internalSelectedId

Skip when the active element is an input/textarea.

---

## Task 10: Adopt new SidePanel in MemoryCardsPanel (graph-stage callsite only)

**Files:**
- Modify: `src/domains/memory-vault/MemoryCardsPanel.tsx`

- Remove `showSidePanel={false}` from the **second** callsite (line ~1216, height=420). Keep it `{false}` on the strip caller.
- Wire `onNodeClick` to also set `selectedGraphNode` (already happening via `handleGraphNodeClick`). The new internal side panel will handle selection presentation itself.
- Remove the legacy `<aside className="memory-graph-detail">` block — confirm what state it owned (`graphNodeDetail`, "Build path" UI) and either move that into the new side panel's actions or document its removal in the handoff. For S2, KEEP the legacy aside as a "details + path tools" sidecar BELOW the graph, but stop driving primary selection through it.

> If the legacy aside is too entangled (lots of state, "Build path" semantic-search inputs), do the minimum: turn off `showSidePanel={false}` on the graph-stage callsite, and leave the aside in place. Mark the next-step in the handoff.

Commit.

---

## Task 11: Verify + commit + push + handoff

- [ ] `npm run typecheck` PASS.
- [ ] `npm test -- --run` shows 11+ test files passing (5 from S1 + new graphFilterOptions + graphFilters). Same single pre-existing failure remains.
- [ ] Push to `origin/main`.
- [ ] Append a Session 2 block to `docs/superpowers/specs/2026-05-16-knowledge-graph-overhaul-handoff.md` listing shipped items and remaining S3 work.

---

## Self-Review

- **Spec coverage:** Top bar (Task 6/7), legend (Task 5/7), zoom (Task 3/4/7), keyboard shortcuts (Task 9), MemoryCardsPanel adoption (Task 10), back-compat (`showFilters` / `showLegend` / `showZoomControls` props in Task 7).
- **Placeholder scan:** No "TBD"; the only conditional escape is in Task 10 where the legacy aside might be too entangled to fully replace — that's a documented decision branch, not a placeholder.
- **Type consistency:** `KnowledgeGraphCanvasHandle` is introduced in Task 3 and consumed in Tasks 4 / 7 / 9. `FilterOptions` is introduced in Task 1 and consumed in Task 6 / 7. `GraphFilterState` came from S1's `graphFilters.ts`.
