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
