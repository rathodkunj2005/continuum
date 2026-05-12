import type { MemoryCard } from "../api/tauri";

const TRAILING_NARRATION_RE = /,?\s*(User|Then You|You reviewed)[^.]*$/i;
const DANGLING_WORD_RE = /(\b(for|with|and|or|to|of|in|on|by|at|from|via|about|after|before|than|then|while|during|including))$/i;

function normalizeSummary(summary: string): string {
    return summary
        .replace(/\s+/g, " ")
        .trim();
}

function tokenize(text: string): Set<string> {
    return new Set(
        text
            .toLowerCase()
            .replace(/[^a-z0-9\s]/g, " ")
            .split(/\s+/)
            .map((token) => token.trim())
            .filter((token) => token.length > 1)
    );
}

export function tokenOverlap(left: string, right: string): number {
    const leftTokens = tokenize(left);
    const rightTokens = tokenize(right);
    if (leftTokens.size === 0 || rightTokens.size === 0) {
        return 0;
    }

    let intersection = 0;
    for (const token of leftTokens) {
        if (rightTokens.has(token)) {
            intersection += 1;
        }
    }

    const union = new Set([...leftTokens, ...rightTokens]).size;
    if (union === 0) {
        return 0;
    }
    return intersection / union;
}

export function cleanCardSummary(summary: string): string {
    let cleaned = normalizeSummary(summary.replace(TRAILING_NARRATION_RE, ""));
    cleaned = cleaned.replace(/\s+/g, " ").trim();

    while (DANGLING_WORD_RE.test(cleaned)) {
        cleaned = cleaned.replace(DANGLING_WORD_RE, "").trim();
    }

    if (cleaned && !/[.!?]$/.test(cleaned)) {
        cleaned = `${cleaned}.`;
    }

    return cleaned;
}

export function cleanupCardsForRender(cards: MemoryCard[]): MemoryCard[] {
    const cleaned: MemoryCard[] = [];

    for (const card of cards) {
        const displaySummary = cleanCardSummary(card.display_summary ?? card.summary ?? "");
        const nextCard: MemoryCard = {
            ...card,
            summary: displaySummary || card.summary,
            display_summary: displaySummary || card.display_summary || card.summary,
        };

        const duplicate = cleaned.some((existing) => {
            const left = existing.display_summary ?? existing.summary ?? "";
            const right = nextCard.display_summary ?? nextCard.summary ?? "";
            return tokenOverlap(left, right) > 0.85;
        });

        if (!duplicate) {
            cleaned.push(nextCard);
        }
    }

    return cleaned;
}
