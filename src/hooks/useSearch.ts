import { useEffect, useRef, useState } from "react";
import { searchMemoryCards, type MemoryCard } from "../api/tauri";
import { SEARCH_LIMITS } from "../lib/config";

function getAdaptiveDebounceMs(query: string): number {
    if (!query.trim()) {
        return 0;
    }
    return SEARCH_LIMITS.typingDebounceMs;
}

function getAdaptiveTimeoutMs(query: string, attempt: number): number {
    const words = query.trim().split(/\s+/).filter(Boolean).length;
    const extraForLength = Math.min(
        SEARCH_LIMITS.timeoutBonusCapMs,
        query.length * SEARCH_LIMITS.perCharBonusMs
    );
    const extraForWords = Math.min(
        SEARCH_LIMITS.timeoutBonusCapMs,
        words * SEARCH_LIMITS.perWordBonusMs
    );
    const retryBonus = attempt > 0 ? SEARCH_LIMITS.retryBonusMs : 0;
    return SEARCH_LIMITS.baseTimeoutMs + extraForLength + extraForWords + retryBonus;
}

export function useSearch(query: string, timeFilter: string | null, appFilter: string | null) {
    const [results, setResults] = useState<MemoryCard[]>([]);
    const [isLoading, setIsLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const requestIdRef = useRef(0);

    useEffect(() => {
        const trimmedQuery = query.trim();
        const requestId = ++requestIdRef.current;
        const debounceMs = getAdaptiveDebounceMs(trimmedQuery);

        if (!trimmedQuery) {
            setResults([]);
            setError(null);
            setIsLoading(false);
            return;
        }

        let cancelled = false;
        setIsLoading(true);
        setError(null);

        const timer = setTimeout(async () => {
            try {
                const timeoutMs = getAdaptiveTimeoutMs(trimmedQuery, 0);
                const timeoutPromise = new Promise<never>((_, reject) => {
                    setTimeout(() => reject(new Error("Search timed out")), timeoutMs);
                });

                const searchPromise = searchMemoryCards(
                    trimmedQuery,
                    timeFilter ?? undefined,
                    appFilter ?? undefined,
                    SEARCH_LIMITS.resultLimit
                );

                const res = await Promise.race([searchPromise, timeoutPromise]);

                if (cancelled || requestId !== requestIdRef.current) {
                    return;
                }
                setResults(res.slice(0, SEARCH_LIMITS.resultLimit));
            } catch (e) {
                if (cancelled || requestId !== requestIdRef.current) {
                    return;
                }
                const errorMessage = e instanceof Error ? e.message : "Search failed";
                setError(
                    errorMessage.toLowerCase().includes("timed out")
                        ? "Search timed out. Try a shorter query or remove filters."
                        : errorMessage
                );
                setResults([]);
            } finally {
                if (!cancelled && requestId === requestIdRef.current) {
                    setIsLoading(false);
                }
            }
        }, debounceMs);

        return () => {
            cancelled = true;
            clearTimeout(timer);
        };
    }, [query, timeFilter, appFilter]);

    return { results, isLoading, error };
}
