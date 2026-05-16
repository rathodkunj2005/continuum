import { describe, it, expect, beforeEach, vi } from "vitest";
import { __resetActiveClock, __setActiveMillis } from "../activeUseClock";
import { graphCache, GRAPH_CACHE_HOUR_MS } from "../graphCache";

describe("graphCache", () => {
    beforeEach(() => {
        __resetActiveClock();
        graphCache.clear();
    });

    it("calls the loader the first time and caches the result", async () => {
        const loader = vi
            .fn()
            .mockResolvedValue({ nodes: [{ id: "a" } as never], edges: [] });
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
