import { useState, useEffect } from "react";
import { MemoryCard } from "../api/tauri";
import { cleanupCardsForRender, tokenOverlap } from "../lib/cardCleanup";
import { extractAnchorTerms, scoreAnchorCoverage } from "../lib/search";
import "./Timeline.css";

const INITIAL_VISIBLE = 30;
const LOAD_MORE_STEP = 30;

interface TimelineProps {
    results: MemoryCard[];
    isLoading: boolean;
    query: string;
    selectedResultId: string | null;
    onSelectResult: (result: MemoryCard) => void;
    onDeleteMemory?: (memoryId: string) => void;
    evalUi?: boolean;
}

function formatDay(timestamp: number): string {
    const date = new Date(timestamp);
    const today = new Date();
    const yesterday = new Date(today);
    yesterday.setDate(yesterday.getDate() - 1);

    if (date.toDateString() === today.toDateString()) {
        return "Today";
    }
    if (date.toDateString() === yesterday.toDateString()) {
        return "Yesterday";
    }
    return date.toLocaleDateString(undefined, {
        weekday: "long",
        month: "long",
        day: "numeric",
    });
}

function normalizePreview(value: string): string {
    return value
        .trim()
        .toLowerCase()
        .replace(/\s+/g, " ");
}

function isLowSignalPreview(summary: string, appName: string): boolean {
    const normalized = normalizePreview(summary);
    if (!normalized) {
        return true;
    }
    const app = normalizePreview(appName);
    if (normalized === app || normalized === "fndr" || normalized === "codex") {
        return true;
    }
    return normalized.split(" ").length <= 2;
}

function stripLegacySources(summary: string): string {
    return summary.replace(/\s*Sources:\s*[A-Za-z0-9,\-\s]+\.?$/i, "").trim();
}

export function Timeline({
    results,
    isLoading,
    query,
    selectedResultId,
    onSelectResult,
    onDeleteMemory,
    evalUi = false,
}: TimelineProps) {
    const [visibleCount, setVisibleCount] = useState(INITIAL_VISIBLE);

    useEffect(() => {
        setVisibleCount(INITIAL_VISIBLE);
    }, [query]);

    if (isLoading) {
        return (
            <div className="timeline-state">
                <div className="thinking-loader thinking-loader-lg" aria-hidden="true" />
                <p>Searching memories...</p>
            </div>
        );
    }

    if (results.length === 0) {
        if (!query.trim()) {
            return (
                <div className="timeline-state timeline-welcome">
                    <div className="welcome-icon">⌘</div>
                    <h2>Welcome to FNDR</h2>
                    <p>Your memories are being captured. Type a query and press Enter to search.</p>
                </div>
            );
        }
        return (
            <div className="timeline-state">
                <div className="empty-icon">🔍</div>
                <h3>No memories found</h3>
                <p>Try a different search term</p>
            </div>
        );
    }

    const visibleResults = cleanupCardsForRender(results.slice(0, visibleCount));
    const hasMore = results.length > visibleCount;
    const filteredResults = filterConsecutiveSimilar(visibleResults);
    return (
        <div className="timeline-container">
            <div className="timeline-stream">
                {filteredResults.map((result) => {
                    const cleanSummary = stripLegacySources(result.display_summary ?? result.summary);
                    const displayTitle = preferredTitle(result);
                    const primaryText = cleanSummary || displayTitle || "Captured memory";
                    const matchReason = preferredMatchReason(result, query);
                    const domain = domainFromUrl(result.url);
                    const confidence = result.confidence ?? result.score;
                    const evidence = (result.raw_snippets ?? [])
                        .map((snippet) => stripLegacySources(snippet).trim())
                        .filter(Boolean)
                        .slice(0, 3);
                    return (
                        <article
                            key={result.id}
                            className={`result-card ${selectedResultId === result.id ? "selected" : ""}`}
                            onClick={() => onSelectResult(result)}
                            role="button"
                            tabIndex={0}
                            onKeyDown={(event) => {
                                if (event.key === "Enter" || event.key === " ") {
                                    event.preventDefault();
                                    onSelectResult(result);
                                }
                            }}
                        >
                            <div className={`result-meta ${evalUi ? "result-meta-eval" : ""}`}>
                                <div className="result-meta-main">
                                    <span className="result-app">{result.app_name}</span>
                                    <span className="result-time">
                                        {formatDay(result.timestamp)} ·{" "}
                                        {new Date(result.timestamp).toLocaleTimeString(undefined, {
                                            hour: "2-digit",
                                            minute: "2-digit",
                                        })}
                                    </span>
                                </div>
                                <div className="result-meta-actions">
                                    {evalUi && (
                                        <span className="result-score" title="Relevance score">
                                            score {result.score.toFixed(3)}
                                        </span>
                                    )}
                                    {onDeleteMemory && (
                                        <button
                                            className="ui-action-btn timeline-delete-btn"
                                            onClick={(event) => {
                                                event.stopPropagation();
                                                onDeleteMemory(result.id);
                                            }}
                                            aria-label="Delete this memory"
                                            title="Delete this memory"
                                        >
                                            Delete
                                        </button>
                                    )}
                                </div>
                            </div>
                            {displayTitle && (
                                <h3 className="result-title">
                                    {displayTitle}
                                </h3>
                            )}
                            <p className="result-primary">
                                {!isLowSignalPreview(primaryText, result.app_name)
                                    ? primaryText
                                    : (displayTitle || "Untitled memory")}
                            </p>
                            <div className="result-context-chips" aria-label="Match details">
                                {matchReason && <span className="result-chip">{matchReason}</span>}
                                <span className="result-chip">{qualityLabel(result, query, confidence)}</span>
                                {domain && <span className="result-chip">{domain}</span>}
                                {result.source_count > 1 && (
                                    <span className="result-chip">{result.source_count} sources</span>
                                )}
                            </div>
                            {evidence.length > 0 && (
                                <details
                                    className="source-details"
                                    onClick={(event) => event.stopPropagation()}
                                >
                                    <summary>Evidence snippets</summary>
                                    <ul>
                                        {evidence.map((snippet, index) => (
                                            <li key={`${result.id}-evidence-${index}`}>{snippet}</li>
                                        ))}
                                    </ul>
                                </details>
                            )}
                        </article>
                    );
                })}
            </div>

            {hasMore && (
                <div className="load-more-container">
                    <button
                        onClick={() => setVisibleCount((n) => n + LOAD_MORE_STEP)}
                        className="load-more-btn"
                    >
                        Load {Math.min(LOAD_MORE_STEP, results.length - visibleCount)} more
                    </button>
                </div>
            )}
        </div>
    );
}

function preferredTitle(result: MemoryCard): string {
    const title = (result.title || "").trim();
    const windowTitle = (result.window_title || "").trim();
    const app = result.app_name.toLowerCase();
    const lowerWindow = windowTitle.toLowerCase();
    const genericWindow =
        !windowTitle
        || lowerWindow === app
        || includesAny(lowerWindow, ["new tab", "dashboard", "home", "settings"]);

    if (!genericWindow && (title.endsWith("...") || !title)) {
        return windowTitle;
    }

    return title || windowTitle;
}

function includesAny(haystack: string, needles: string[]): boolean {
    return needles.some((needle) => haystack.includes(needle));
}

function domainFromUrl(url: string | undefined): string {
    if (!url) {
        return "";
    }
    try {
        return new URL(url).hostname.replace(/^www\./, "");
    } catch {
        return url
            .replace(/^https?:\/\//i, "")
            .split("/")[0]
            .replace(/^www\./, "");
    }
}

function preferredMatchReason(result: MemoryCard, query: string): string {
    const explicit = (result.context ?? [])
        .map((value) => value.trim())
        .find((value) => value.length > 0);
    if (explicit) {
        return explicit;
    }
    if (!query.trim()) {
        return "Recent memory";
    }
    if ((result.confidence ?? result.score) < 0.42) {
        return "Low-confidence match";
    }
    return "Semantic + keyword match";
}

function qualityLabel(result: MemoryCard, query: string, confidence: number): string {
    if (!query.trim()) {
        return "Recent memory";
    }

    const coverage = result.anchor_coverage_score
        ?? scoreAnchorCoverage(
            `${result.title} ${result.display_summary ?? result.summary} ${(result.raw_snippets ?? []).join(" ")}`,
            extractAnchorTerms(query)
        );

    if (coverage >= 0.8) {
        return "Direct match";
    }
    if (coverage >= 0.4) {
        return "Related";
    }
    if (confidence >= 0.72) {
        return "Strong match";
    }
    return "Contextual";
}

function filterConsecutiveSimilar(results: MemoryCard[]): MemoryCard[] {
    if (results.length <= 1) return results;

    const filtered: MemoryCard[] = [results[0]];
    for (let i = 1; i < results.length; i++) {
        const prev = filtered[filtered.length - 1];
        const curr = results[i];

        // Skip if same app and < 30s diff and highly similar title.
        const summaryOverlap = tokenOverlap(
            curr.display_summary ?? curr.summary,
            prev.display_summary ?? prev.summary
        );
        if (
            curr.app_name === prev.app_name &&
            Math.abs(curr.timestamp - prev.timestamp) < 30_000 &&
            (curr.title.toLowerCase() === prev.title.toLowerCase() || summaryOverlap > 0.85)
        ) {
            continue;
        }
        filtered.push(curr);
    }
    return filtered;
}
