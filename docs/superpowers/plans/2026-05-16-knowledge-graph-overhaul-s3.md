# Knowledge Graph Overhaul — Session 3 Implementation Plan

> **For agentic workers:** Use `superpowers:executing-plans` to implement task-by-task.

**Goal:** Land the correctness-critical S3 items: graph cache + hourly *active-use* refresh, loading / error skeleton states, node drift animation from the design bundle, and the topic filter pill in the top bar.

**Architecture:** A new `graphCache` module owns a single in-memory cache slot per IPC key and an "active-use clock" that ticks only when the app is in the foreground (or when explicit calls fire). The `useGraph()` hook routes loads through the cache so repeat opens reuse cached subgraphs and only refetch on the next active-use-hour boundary. Loading and error states render inside the `.knowledge-graph-canvas-wrap` chrome (no host changes). Node drift is pure CSS keyframes phased per node via inline animation-delay; no JS loop. Topic filter is a fourth `FilterMenu` in the existing top bar.

**Tech Stack:** No new deps. Vitest for the active-use clock + cache + filter tests. CSS for drift.

**Deferred (out-of-scope for S3 inline, each its own epic):**
- LOD / canvas virtualization for thousand-node graphs.
- App-wide theme migration (would touch the 32 skip-worktree panel CSS files).
- Folding the legacy `memory-graph-detail` aside into `KnowledgeGraphSidePanel`'s actions (requires product decision on Build Path semantics).
- Full WCAG audit + tab-order pass.
- Cursor amber trail.

---

## File Structure

**Create:**

```
src/domains/memory-vault/graph/activeUseClock.ts            # singleton tracking app foreground time
src/domains/memory-vault/graph/graphCache.ts                # cache slots + hourly refresh predicate
src/domains/memory-vault/graph/__tests__/activeUseClock.test.ts
src/domains/memory-vault/graph/__tests__/graphCache.test.ts
```

**Modify:**

```
src/domains/memory-vault/useGraph.ts                        # cache-aware load() that preserves selection
src/domains/memory-vault/KnowledgeGraph.tsx                 # loading/error scrim; pass loading prop
src/domains/memory-vault/KnowledgeGraphCanvas.tsx           # animation-delay style per node group (drift)
src/domains/memory-vault/KnowledgeGraph.css                 # drift keyframes + loading scrim + error chip
src/domains/memory-vault/KnowledgeGraphTopBar.tsx           # topic FilterMenu
```

---

## Task 1: Active-use clock (TDD)

A lightweight ticker that accumulates ms while the document is visible. Exposes `getActiveMillis()` and a test-only `__setActiveMillis()` so the cache can be unit-tested deterministically.

- [ ] **Step 1: Failing test** — `activeUseClock.test.ts`:

```ts
import { describe, it, expect, beforeEach } from "vitest";
import { getActiveMillis, __setActiveMillis, __resetActiveClock } from "../activeUseClock";

describe("activeUseClock", () => {
    beforeEach(() => __resetActiveClock());

    it("starts at 0", () => {
        expect(getActiveMillis()).toBe(0);
    });

    it("returns the value set via the test-only setter", () => {
        __setActiveMillis(3_600_000);
        expect(getActiveMillis()).toBe(3_600_000);
    });

    it("monotonically advances after re-setting to a larger value", () => {
        __setActiveMillis(1_000);
        __setActiveMillis(2_000);
        expect(getActiveMillis()).toBe(2_000);
    });
});
```

- [ ] **Step 2: Implementation** — `activeUseClock.ts`:

```ts
const HOUR_MS = 60 * 60 * 1000;

let activeMillis = 0;
let lastTick = typeof performance !== "undefined" ? performance.now() : Date.now();
let started = false;

function now(): number {
    return typeof performance !== "undefined" ? performance.now() : Date.now();
}

function isForeground(): boolean {
    if (typeof document === "undefined") return true;
    return document.visibilityState === "visible";
}

function tick() {
    const t = now();
    if (isForeground()) {
        activeMillis += t - lastTick;
    }
    lastTick = t;
}

function start() {
    if (started) return;
    started = true;
    if (typeof document !== "undefined") {
        document.addEventListener("visibilitychange", tick);
    }
    if (typeof window !== "undefined") {
        window.setInterval(tick, 30_000);
    }
}

export function getActiveMillis(): number {
    if (!started) start();
    tick();
    return activeMillis;
}

export const ACTIVE_USE_HOUR_MS = HOUR_MS;

/** Test-only: force the active-use accumulator (does not auto-start the ticker). */
export function __setActiveMillis(ms: number): void {
    activeMillis = ms;
    lastTick = now();
}

/** Test-only: reset state. */
export function __resetActiveClock(): void {
    activeMillis = 0;
    lastTick = now();
}
```

- [ ] **Step 3: Run test (expect PASS).** Commit.

---

## Task 2: Graph cache (TDD)

A tiny per-key cache that records the active-use timestamp of each load and returns the cached value when the next-hour boundary hasn't been crossed.

- [ ] **Step 1: Failing test** — `graphCache.test.ts`:

```ts
import { describe, it, expect, beforeEach, vi } from "vitest";
import { __resetActiveClock, __setActiveMillis } from "../activeUseClock";
import { graphCache, GRAPH_CACHE_HOUR_MS } from "../graphCache";

describe("graphCache", () => {
    beforeEach(() => {
        __resetActiveClock();
        graphCache.clear();
    });

    it("calls the loader the first time and caches the result", async () => {
        const loader = vi.fn().mockResolvedValue({ nodes: [{ id: "a" } as never], edges: [] });
        const a = await graphCache.get("full", loader);
        const b = await graphCache.get("full", loader);
        expect(loader).toHaveBeenCalledTimes(1);
        expect(a).toBe(b);
    });

    it("refetches once the active-use hour boundary has passed", async () => {
        const loader = vi
            .fn()
            .mockResolvedValueOnce({ nodes: [{ id: "a" } as never], edges: [] })
            .mockResolvedValueOnce({ nodes: [{ id: "b" } as never], edges: [] });

        __setActiveMillis(0);
        const first = await graphCache.get("full", loader);
        __setActiveMillis(GRAPH_CACHE_HOUR_MS + 1);
        const second = await graphCache.get("full", loader);

        expect(loader).toHaveBeenCalledTimes(2);
        expect(first.nodes[0].id).toBe("a");
        expect(second.nodes[0].id).toBe("b");
    });

    it("isolates entries by key", async () => {
        const loaderA = vi.fn().mockResolvedValue({ nodes: [], edges: [] });
        const loaderB = vi.fn().mockResolvedValue({ nodes: [], edges: [] });
        await graphCache.get("project:Work", loaderA);
        await graphCache.get("full", loaderB);
        expect(loaderA).toHaveBeenCalledTimes(1);
        expect(loaderB).toHaveBeenCalledTimes(1);
    });

    it("invalidate(key) forces the next get to refetch", async () => {
        const loader = vi
            .fn()
            .mockResolvedValueOnce({ nodes: [], edges: [] })
            .mockResolvedValueOnce({ nodes: [], edges: [] });
        await graphCache.get("full", loader);
        graphCache.invalidate("full");
        await graphCache.get("full", loader);
        expect(loader).toHaveBeenCalledTimes(2);
    });
});
```

- [ ] **Step 2: Implementation** — `graphCache.ts`:

```ts
import type { InsightGraphSubgraph } from "@/shared/ipc/tauri";
import { getActiveMillis, ACTIVE_USE_HOUR_MS } from "./activeUseClock";

export const GRAPH_CACHE_HOUR_MS = ACTIVE_USE_HOUR_MS;

interface CacheEntry {
    value: InsightGraphSubgraph;
    loadedAtActiveMs: number;
}

const slots = new Map<string, CacheEntry>();

function isStale(entry: CacheEntry): boolean {
    const now = getActiveMillis();
    const currentHour = Math.floor(now / GRAPH_CACHE_HOUR_MS);
    const loadedHour = Math.floor(entry.loadedAtActiveMs / GRAPH_CACHE_HOUR_MS);
    return currentHour > loadedHour;
}

export const graphCache = {
    async get(
        key: string,
        loader: () => Promise<InsightGraphSubgraph>,
    ): Promise<InsightGraphSubgraph> {
        const existing = slots.get(key);
        if (existing && !isStale(existing)) {
            return existing.value;
        }
        const value = await loader();
        slots.set(key, { value, loadedAtActiveMs: getActiveMillis() });
        return value;
    },
    invalidate(key: string): void {
        slots.delete(key);
    },
    clear(): void {
        slots.clear();
    },
    peek(key: string): InsightGraphSubgraph | null {
        return slots.get(key)?.value ?? null;
    },
};
```

- [ ] **Step 3: Run tests (expect PASS).** Commit.

---

## Task 3: Wire useGraph through the cache

**Files:**
- Modify: `src/domains/memory-vault/useGraph.ts`

Replace the direct `getFullGraph` / `getGraphForProject` calls inside `load()` with `graphCache.get(...)`. Add a `refresh(): Promise<void>` callback that calls `graphCache.invalidate(...)` then re-loads with the current opts. Add a `manuallyRefresh` ref that the composer can call.

```ts
import { useCallback, useRef, useState } from "react";
import {
    findGraphPath,
    getFullGraph,
    getGodNodes,
    getGraphForProject,
    getNodeDetail,
    searchGraph,
    type InsightGraphSubgraph,
} from "@/shared/ipc/tauri";
import { graphCache } from "./graph/graphCache";

export const GRAPH_SIM_MAX_TICKS = 300;

interface LoadOpts {
    mode: "full" | "project";
    projectLabel?: string;
}

function cacheKey(opts: LoadOpts): string {
    if (opts.mode === "project" && opts.projectLabel?.trim()) {
        return `project:${opts.projectLabel.trim()}`;
    }
    return "full";
}

export function useGraph() {
    const [subgraph, setSubgraph] = useState<InsightGraphSubgraph | null>(null);
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const lastOptsRef = useRef<LoadOpts | null>(null);

    const load = useCallback(async (opts: LoadOpts) => {
        lastOptsRef.current = opts;
        setLoading(true);
        setError(null);
        try {
            const key = cacheKey(opts);
            const data = await graphCache.get(key, () =>
                opts.mode === "project" && opts.projectLabel?.trim()
                    ? getGraphForProject(opts.projectLabel.trim())
                    : getFullGraph(),
            );
            setSubgraph(data);
        } catch (e) {
            setError(e instanceof Error ? e.message : "Graph load failed");
            setSubgraph(null);
        } finally {
            setLoading(false);
        }
    }, []);

    const refresh = useCallback(async () => {
        const opts = lastOptsRef.current;
        if (!opts) return;
        graphCache.invalidate(cacheKey(opts));
        await load(opts);
    }, [load]);

    const fetchNodeDetail = useCallback(async (id: string) => getNodeDetail(id), []);
    const fetchPath = useCallback(async (from: string, to: string) => findGraphPath(from, to), []);
    const fetchGodNodes = useCallback(async (k: number) => getGodNodes(k), []);
    const runSemanticSearch = useCallback(
        async (queryEmbedding: number[], k: number) => searchGraph(queryEmbedding, k),
        [],
    );

    return {
        subgraph,
        loading,
        error,
        load,
        refresh,
        fetchNodeDetail,
        fetchPath,
        fetchGodNodes,
        runSemanticSearch,
    };
}
```

- [ ] **Step:** Typecheck, ensure `MemoryCardsPanel.tsx`'s consumer doesn't break (it destructures known fields from `useGraph()`, all of which still exist). Commit.

---

## Task 4: Loading + error states in the graph shell

**Files:**
- Modify: `src/domains/memory-vault/KnowledgeGraph.tsx`
- Modify: `src/domains/memory-vault/KnowledgeGraph.css`

Add two new props on `KnowledgeGraphProps`:

```ts
loading?: boolean;
errorMessage?: string | null;
```

In the JSX, after the canvas, render:

```tsx
{loading && (
    <div className="kg-state-scrim kg-state-loading" role="status" aria-live="polite">
        <span className="kg-state-line" />
        <span className="kg-state-line" />
        <span className="kg-state-line" />
        <p className="kg-state-text">developing…</p>
    </div>
)}
{errorMessage && !loading && (
    <div className="kg-state-scrim kg-state-error" role="alert">
        <p className="kg-state-text">{errorMessage}</p>
    </div>
)}
```

CSS:

```css
.kg-state-scrim {
    position: absolute;
    inset: 0;
    z-index: 2;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 14px;
    background: color-mix(in srgb, var(--cp-bg) 70%, transparent);
    backdrop-filter: blur(6px);
    -webkit-backdrop-filter: blur(6px);
    pointer-events: none;
}
.kg-state-line {
    display: block;
    width: 140px;
    height: 2px;
    background: linear-gradient(
        90deg,
        transparent 0%,
        var(--cp-accent) 50%,
        transparent 100%
    );
    opacity: 0.35;
    animation: kg-shutter 1600ms var(--film-ease-shutter) infinite;
}
.kg-state-line:nth-child(2) { animation-delay: 220ms; opacity: 0.25; }
.kg-state-line:nth-child(3) { animation-delay: 440ms; opacity: 0.18; }
.kg-state-text {
    font: 500 11px/1 var(--film-font-mono);
    letter-spacing: var(--film-track-stamp);
    text-transform: uppercase;
    color: var(--cp-text-secondary);
    margin: 0;
}
.kg-state-error .kg-state-text {
    color: var(--cp-accent);
}
@keyframes kg-shutter {
    0%   { transform: translateX(-30%); opacity: 0; }
    50%  { transform: translateX(0); opacity: 1; }
    100% { transform: translateX(30%); opacity: 0; }
}
```

Plumb `loading={loading}` + `errorMessage={error}` from `MemoryCardsPanel.tsx` if/where the consumer wants it; otherwise the defaults (`undefined` → not shown) preserve current behavior. **Don't** wire it inside the strip caller (height=220) — those small previews shouldn't blink.

- [ ] **Step:** typecheck, commit.

---

## Task 5: Node drift animation

**Files:**
- Modify: `src/domains/memory-vault/KnowledgeGraphCanvas.tsx`
- Modify: `src/domains/memory-vault/KnowledgeGraph.css`

After the node `g` is created, add an `animation-delay` style based on a deterministic hash of the id:

```ts
nodeSel.style("animation-delay", (d) => `${(d.id.charCodeAt(0) % 6) * 0.5}s`);
```

CSS:

```css
@keyframes kg-drift {
    0%   { transform: translate(0, 0); }
    50%  { transform: translate(1px, -1px); }
    100% { transform: translate(0, 0); }
}
.kg-node {
    animation: kg-drift 6s ease-in-out infinite alternate;
    transform-box: fill-box;
}
.kg-node[data-state="selected"],
.kg-node[data-state="hovered"] {
    animation-play-state: paused;
}
```

> Drift is additive to the `translate(x,y)` set by the simulation tick. Browsers compose transforms in CSS over the SVG `transform` attribute, so the drift is small and won't fight d3-force.

- [ ] **Step:** typecheck, run hover test to ensure node enumeration still works, commit.

---

## Task 6: Topic filter pill

**Files:**
- Modify: `src/domains/memory-vault/KnowledgeGraphTopBar.tsx`
- Modify: `src/domains/memory-vault/graph/graphFilters.ts`
- Modify: `src/domains/memory-vault/graph/__tests__/graphFilters.test.ts`

Add `topics: ReadonlySet<string> | null` to `GraphFilterState` and to `EMPTY_FILTERS`. Filter in `applyFilters` by matching `metadata.topic` exactly. Add a test case for the new path. Add a "topic" `FilterMenu` to the top bar (between project and edge).

- [ ] **Step:** typecheck, run all graph tests, commit.

---

## Task 7: Verify + push + handoff

- [ ] `npm run typecheck` PASS.
- [ ] `npm test -- --run` shows 14 files passing (12 from S2 + activeUseClock + graphCache). +12 new passing tests.
- [ ] Push to `origin/main`.
- [ ] Append a Session 3 block to `docs/superpowers/specs/2026-05-16-knowledge-graph-overhaul-handoff.md`.

---

## Self-Review

- **Spec coverage:** Cache + hourly active-use refresh (Tasks 1-3), loading/error states (Task 4), node drift from the design bundle (Task 5), topic filter from the design bundle (Task 6). All deferred items are explicit at the top of this plan.
- **Placeholder scan:** None.
- **Type consistency:** `GraphFilterState` gains one field (`topics`); both consumers (composer + top bar + tests) updated in Task 6. `useGraph` keeps every field its consumer reads; adds `refresh()`.
