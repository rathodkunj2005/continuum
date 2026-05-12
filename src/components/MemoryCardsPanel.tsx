import { useEffect, useMemo, useState } from "react";
import {
    MemoryCard,
    MemoryDebugInspector,
    deleteMemory,
    getMemoryDebugInspector,
    listMemoryCards,
} from "../api/tauri";
import "./MemoryCardsPanel.css";

interface MemoryCardsPanelProps {
    isVisible: boolean;
    onClose: () => void;
    appNames: string[];
    onMemoryDeleted?: (memoryId: string) => void;
}

const APP_FILTER_ALL = "__all__";
const TIME_FILTER_ALL = "__time_all__";
const PERSPECTIVE_FILTER_ALL = "__perspective_all__";
const MAX_RENDERED_CARDS = 300;

type TimeFilter =
    | typeof TIME_FILTER_ALL
    | "last_hour"
    | "today"
    | "last_24h"
    | "last_7d";

type PerspectiveFilter =
    | typeof PERSPECTIVE_FILTER_ALL
    | "web"
    | "coding"
    | "meetings"
    | "communication"
    | "docs";

const TIME_FILTER_OPTIONS: Array<{ value: TimeFilter; label: string }> = [
    { value: TIME_FILTER_ALL, label: "All history" },
    { value: "last_hour", label: "Last hour" },
    { value: "today", label: "Today" },
    { value: "last_24h", label: "Last 24 hours" },
    { value: "last_7d", label: "Last 7 days" },
];

const PERSPECTIVE_FILTER_OPTIONS: Array<{ value: PerspectiveFilter; label: string }> = [
    { value: PERSPECTIVE_FILTER_ALL, label: "All perspectives" },
    { value: "web", label: "Web pages" },
    { value: "coding", label: "Coding sessions" },
    { value: "meetings", label: "Meetings" },
    { value: "communication", label: "Communication" },
    { value: "docs", label: "Docs & writing" },
];

function normalizeText(value: string | undefined | null): string {
    if (!value) {
        return "";
    }
    return value
        .replace(/[\u0000-\u001f\u007f-\u009f]/g, " ")
        .replace(/\s*Sources:\s*[A-Za-z0-9,\-\s]+\.?$/i, "")
        .replace(/\s+/g, " ")
        .trim();
}

function hasReadableCharacters(value: string): boolean {
    return /[\p{L}\p{N}]/u.test(value);
}

function pickReadable(...candidates: Array<string | undefined | null>): string {
    for (const candidate of candidates) {
        const cleaned = normalizeText(candidate);
        if (cleaned && hasReadableCharacters(cleaned)) {
            return cleaned;
        }
    }
    return "";
}

function fallbackTitle(card: MemoryCard): string {
    const windowTitle = normalizeText(card.window_title);
    const title = normalizeText(card.title);
    const lowerWindow = windowTitle.toLowerCase();
    const lowerApp = card.app_name.toLowerCase();
    const genericWindow = !windowTitle
        || lowerWindow === lowerApp
        || includesAny(lowerWindow, ["new tab", "dashboard", "home", "settings"]);

    if (!genericWindow && (title.endsWith("...") || !title)) {
        return windowTitle;
    }

    return pickReadable(card.title, card.window_title)
        || `Memory in ${card.app_name}`;
}

function fallbackSummary(card: MemoryCard): string {
    const raw = pickReadable(card.summary, card.raw_snippets[0], card.window_title)
        || `Captured context in ${card.app_name}.`;
    return raw
        .replace(/^\s*(then|and then|after that|next)\s*[,:-]?\s+/i, "")
        .replace(/\.\s*(then|and then|after that|next)\s+/gi, ". ")
        .replace(/\s+/g, " ")
        .trim();
}

function includesAny(haystack: string, needles: string[]): boolean {
    return needles.some((needle) => haystack.includes(needle));
}

function getActivityIcon(activityType?: string): string {
    switch (activityType) {
        case "coding": return "💻";
        case "browsing": return "🌐";
        case "communication": return "💬";
        case "docs": return "📝";
        case "design": return "🎨";
        default: return "";
    }
}

function matchesFilters(
    card: MemoryCard,
    timeFilter: TimeFilter,
    perspectiveFilter: PerspectiveFilter
): boolean {
    const now = Date.now();
    const timestamp = Number(card.timestamp) || 0;

    // 1. Time Filtering
    if (timeFilter !== TIME_FILTER_ALL && timestamp > 0) {
        if (timeFilter === "last_hour" && timestamp < now - 60 * 60 * 1000) return false;
        if (timeFilter === "today" && new Date(timestamp).toDateString() !== new Date(now).toDateString()) return false;
        if (timeFilter === "last_24h" && timestamp < now - 24 * 60 * 60 * 1000) return false;
        if (timeFilter === "last_7d" && timestamp < now - 7 * 24 * 60 * 60 * 1000) return false;
    }

    // 2. Perspective Filtering — prefer structured activity_type when present
    if (perspectiveFilter === PERSPECTIVE_FILTER_ALL) {
        return true;
    }

    // Use structured field first for accuracy
    if (card.activity_type && card.activity_type !== "other") {
        if (perspectiveFilter === "coding") return card.activity_type === "coding";
        if (perspectiveFilter === "docs") return card.activity_type === "docs";
        if (perspectiveFilter === "communication") return card.activity_type === "communication";
        if (perspectiveFilter === "web") return card.activity_type === "browsing";
    }

    // Fall back to generic text signals when structured activity_type is absent.
    const text = normalizeText(
        `${card.window_title ?? ""} ${(card.context ?? []).join(" ")} ${card.summary ?? ""}`
    ).toLowerCase();
    const url = (card.url ?? "").toLowerCase();
    const hasAny = (terms: string[]) => terms.some((term) => text.includes(term));

    if (perspectiveFilter === "web") {
        return Boolean(card.url) || /^https?:\/\//i.test(url);
    }

    if (perspectiveFilter === "coding") {
        return hasAny(["code", "debug", "build", "compile", "branch", "commit", "pull request", "repo"]);
    }

    if (perspectiveFilter === "meetings") {
        return hasAny(["meeting", "agenda", "call", "transcript", "attendee", "follow-up"]);
    }

    if (perspectiveFilter === "communication") {
        return hasAny(["message", "email", "chat", "inbox", "reply", "thread"]);
    }

    if (perspectiveFilter === "docs") {
        return hasAny(["doc", "document", "summary", "outline", "spec", "readme", "note", "draft", "pdf"]);
    }

    return true;
}

function isContinuityCard(card: MemoryCard): boolean {
    return Boolean(card.continuity) || card.source_count > 1 || Boolean(card.continuation_of);
}

const MEMORY_BODY_TRUNCATE_CHARS = 360;

function memoryBodyText(card: MemoryCard): string {
    const raw = normalizeText(card.internal_context) || fallbackSummary(card);
    return raw
        .replace(/^Continues from\s+\S+(?::[^\n]*)?\n?/m, "")
        .replace(/^Reopen:\s+\S+\s*\n?/m, "")
        .trim();
}

function cardCopy(
    card: MemoryCard
): {
    title: string;
    summary: string;
    body: string;
    continuity: boolean;
} {
    const title = fallbackTitle(card);
    const continuity = isContinuityCard(card);
    const summary = fallbackSummary(card);
    const body = memoryBodyText(card) || summary;

    return { title, summary, body, continuity };
}

function isHttpUrl(target: string): boolean {
    return /^https?:\/\//i.test(target);
}

function isFileUrl(target: string): boolean {
    return /^file:\/\//i.test(target);
}

async function handleReopen(target: string) {
    if (isHttpUrl(target)) {
        window.open(target, "_blank", "noopener,noreferrer");
        return;
    }
    if (isFileUrl(target)) {
        try {
            const shellModule = await import("@tauri-apps/plugin-shell");
            await shellModule.open(target);
            return;
        } catch (err) {
            console.warn("Shell open failed; copying target to clipboard", err);
        }
    }
    try {
        await navigator.clipboard.writeText(target);
    } catch (err) {
        console.warn("Clipboard write failed", err);
    }
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
        weekday: "short",
        month: "short",
        day: "numeric",
    });
}

export function MemoryCardsPanel({ isVisible, onClose, appNames, onMemoryDeleted }: MemoryCardsPanelProps) {
    const [cards, setCards] = useState<MemoryCard[]>([]);
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [appFilter, setAppFilter] = useState<string>(APP_FILTER_ALL);
    const [timeFilter, setTimeFilter] = useState<TimeFilter>(TIME_FILTER_ALL);
    const [perspectiveFilter, setPerspectiveFilter] = useState<PerspectiveFilter>(PERSPECTIVE_FILTER_ALL);
    const [deletingId, setDeletingId] = useState<string | null>(null);
    const [openDebugIds, setOpenDebugIds] = useState<Set<string>>(new Set());
    const [debugById, setDebugById] = useState<Record<string, MemoryDebugInspector>>({});
    const [debugLoadingId, setDebugLoadingId] = useState<string | null>(null);
    const [expandedBodyIds, setExpandedBodyIds] = useState<Set<string>>(new Set());

    const toggleExpandedBody = (memoryId: string) => {
        setExpandedBodyIds((previous) => {
            const next = new Set(previous);
            if (next.has(memoryId)) {
                next.delete(memoryId);
            } else {
                next.add(memoryId);
            }
            return next;
        });
    };

    const selectableApps = useMemo(() => {
        return appNames
            .map((name) => name.trim())
            .filter((name) => name.length > 0)
            .sort((a, b) => a.localeCompare(b));
    }, [appNames]);

    const filteredCards = useMemo(
        () => cards.filter((card) => matchesFilters(card, timeFilter, perspectiveFilter)),
        [cards, timeFilter, perspectiveFilter]
    );

    useEffect(() => {
        if (!isVisible) {
            return;
        }

        let cancelled = false;
        const selectedApp = appFilter === APP_FILTER_ALL ? null : appFilter;

        setLoading(cards.length === 0);
        setError(null);

        void listMemoryCards(1500, selectedApp)
            .then((items) => {
                if (cancelled) {
                    return;
                }
                setCards(items);
            })
            .catch((err) => {
                if (cancelled) {
                    return;
                }
                // Preserve existing cards if refresh fails so the panel remains usable.
                setError(err instanceof Error ? err.message : "Unable to load memory cards.");
            })
            .finally(() => {
                if (!cancelled) {
                    setLoading(false);
                }
            });

        return () => {
            cancelled = true;
        };
    }, [isVisible, appFilter]);

    if (!isVisible) {
        return null;
    }

    const handleDeleteCard = async (memoryId: string) => {
        if (deletingId) {
            return;
        }

        setDeletingId(memoryId);
        try {
            const deleted = await deleteMemory(memoryId);
            if (deleted) {
                setCards((previous) => previous.filter((card) => card.id !== memoryId));
                onMemoryDeleted?.(memoryId);
            }
        } catch (err) {
            setError(err instanceof Error ? err.message : "Unable to delete memory.");
        } finally {
            setDeletingId(null);
        }
    };

    const handleToggleDebug = async (memoryId: string) => {
        const isOpen = openDebugIds.has(memoryId);
        if (isOpen) {
            setOpenDebugIds((previous) => {
                const next = new Set(previous);
                next.delete(memoryId);
                return next;
            });
            return;
        }
        if (!debugById[memoryId]) {
            setDebugLoadingId(memoryId);
            try {
                const debug = await getMemoryDebugInspector(memoryId);
                setDebugById((previous) => ({
                    ...previous,
                    [memoryId]: debug,
                }));
            } catch (err) {
                setError(err instanceof Error ? err.message : "Unable to load memory debug details.");
            } finally {
                setDebugLoadingId(null);
            }
        }
        setOpenDebugIds((previous) => {
            const next = new Set(previous);
            next.add(memoryId);
            return next;
        });
    };

    return (
        <div className="memory-cards-panel">
            <div className="memory-cards-header">
                <div className="memory-cards-heading">
                    <h2>All Memory Cards</h2>
                    <p>Newest to oldest</p>
                </div>
                <button className="ui-action-btn memory-cards-close-btn" onClick={onClose}>X</button>
            </div>

            <div className="memory-cards-toolbar">
                <div className="memory-cards-filters">
                    <label className="memory-cards-filter">
                        Universe
                        <div className="memory-cards-filter-control">
                            <select
                                value={appFilter}
                                onChange={(event) => setAppFilter(event.target.value)}
                            >
                                <option value={APP_FILTER_ALL}>All Apps</option>
                                {selectableApps.map((name) => (
                                    <option key={name} value={name}>
                                        {name}
                                    </option>
                                ))}
                            </select>
                            <svg className="memory-cards-filter-arrow" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
                                <path d="M6 9l6 6 6-6" />
                            </svg>
                        </div>
                    </label>

                    <label className="memory-cards-filter">
                        History
                        <div className="memory-cards-filter-control">
                            <select
                                value={timeFilter}
                                onChange={(event) => setTimeFilter(event.target.value as TimeFilter)}
                            >
                                {TIME_FILTER_OPTIONS.map((option) => (
                                    <option key={option.value} value={option.value}>
                                        {option.label}
                                    </option>
                                ))}
                            </select>
                            <svg className="memory-cards-filter-arrow" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
                                <path d="M6 9l6 6 6-6" />
                            </svg>
                        </div>
                    </label>

                    <label className="memory-cards-filter">
                        Perspective
                        <div className="memory-cards-filter-control">
                            <select
                                value={perspectiveFilter}
                                onChange={(event) => setPerspectiveFilter(event.target.value as PerspectiveFilter)}
                            >
                                {PERSPECTIVE_FILTER_OPTIONS.map((option) => (
                                    <option key={option.value} value={option.value}>
                                        {option.label}
                                    </option>
                                ))}
                            </select>
                            <svg className="memory-cards-filter-arrow" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
                                <path d="M6 9l6 6 6-6" />
                            </svg>
                        </div>
                    </label>
                </div>
                <div className="memory-cards-count">{filteredCards.length} cards</div>
            </div>

            <div className="memory-cards-body">
                {loading && cards.length === 0 && (
                    <div className="memory-cards-state">
                        <div className="thinking-loader thinking-loader-lg" aria-hidden="true" />
                        <p>Loading memory cards...</p>
                    </div>
                )}

                {error && filteredCards.length > 0 && (
                    <div className="memory-cards-inline-error">
                        {error}
                    </div>
                )}

                {error && filteredCards.length === 0 && (
                    <div className="memory-cards-state">
                        <p>{error}</p>
                    </div>
                )}

                {!loading && !error && filteredCards.length === 0 && (
                    <div className="memory-cards-state">
                        <p>No memory cards yet for this filter.</p>
                    </div>
                )}

                {filteredCards.length > 0 && (
                    <div className="memory-cards-stream">
                        {filteredCards.slice(0, MAX_RENDERED_CARDS).map((card) => {
                            const { body } = cardCopy(card);
                            const expanded = expandedBodyIds.has(card.id);
                            const truncated = body.length > MEMORY_BODY_TRUNCATE_CHARS;
                            const displayBody = !expanded && truncated
                                ? `${body.slice(0, MEMORY_BODY_TRUNCATE_CHARS).trimEnd()}…`
                                : body;
                            const reopenTarget = card.reopen_target?.trim();
                            const continuationId = card.continuation_of?.trim();

                            return (
                                <article
                                    key={card.id}
                                    className="result-card memory-browse-card"
                                >
                                    <div className="result-meta memory-browse-meta">
                                        <div className="memory-browse-meta-main">
                                            <span className="result-app">
                                                {getActivityIcon(card.activity_type) && (
                                                    <span className="memory-activity-icon" title={card.activity_type}>
                                                        {getActivityIcon(card.activity_type)}
                                                    </span>
                                                )}
                                                {card.app_name}
                                            </span>
                                            <span className="result-time">
                                                {formatDay(card.timestamp)} ·{" "}
                                                {new Date(card.timestamp).toLocaleTimeString(undefined, {
                                                    hour: "2-digit",
                                                    minute: "2-digit",
                                                })}
                                            </span>
                                            {card.source_count > 1 && (
                                                <span className="memory-source-count" title={`Composed from ${card.source_count} captures`}>
                                                    {card.source_count} captures
                                                </span>
                                            )}
                                            {card.session_duration_mins !== undefined && card.session_duration_mins > 0 && (
                                                <span className="memory-duration" title="Session duration">
                                                    {card.session_duration_mins}m
                                                </span>
                                            )}
                                        </div>
                                        <div className="memory-card-actions">
                                            <button
                                                className="ui-action-btn memory-delete-btn"
                                                onClick={(e) => { e.stopPropagation(); void handleToggleDebug(card.id); }}
                                                disabled={debugLoadingId === card.id}
                                                aria-label="Toggle memory debug details"
                                                title="Inspect memory debug data"
                                            >
                                                {debugLoadingId === card.id ? "Loading..." : openDebugIds.has(card.id) ? "Hide Debug" : "Debug"}
                                            </button>
                                            <button
                                                className="ui-action-btn memory-delete-btn"
                                                onClick={(e) => { e.stopPropagation(); void handleDeleteCard(card.id); }}
                                                disabled={deletingId === card.id}
                                                aria-label="Delete memory card"
                                                title="Delete this memory"
                                            >
                                                {deletingId === card.id ? "Deleting..." : "Delete"}
                                            </button>
                                        </div>
                                    </div>
                                    <div className="memory-browse-content">
                                        <div className="memory-browse-summary memory-browse-summary-primary">
                                            {displayBody}
                                            {truncated && (
                                                <button
                                                    type="button"
                                                    className="memory-body-expand"
                                                    onClick={() => toggleExpandedBody(card.id)}
                                                >
                                                    {expanded ? "Show less" : "Show more"}
                                                </button>
                                            )}
                                        </div>
                                        {(reopenTarget || continuationId) && (
                                            <div className="memory-browse-affordances">
                                                {reopenTarget && (
                                                    <button
                                                        type="button"
                                                        className="memory-reopen-anchor"
                                                        onClick={() => { void handleReopen(reopenTarget); }}
                                                        title={reopenTarget}
                                                    >
                                                        Reopen
                                                    </button>
                                                )}
                                                {continuationId && (
                                                    <span
                                                        className="memory-continuation-chip"
                                                        title={`Continues from ${continuationId}`}
                                                    >
                                                        Continues from earlier capture
                                                    </span>
                                                )}
                                            </div>
                                        )}
                                        {card.files_touched && card.files_touched.length > 0 && (
                                            <div className="memory-browse-files">
                                                {card.files_touched.slice(0, 4).map((f) => (
                                                    <span key={f} className="memory-file-chip" title={f}>{f}</span>
                                                ))}
                                            </div>
                                        )}
                                        {openDebugIds.has(card.id) && (
                                            <div className="memory-debug-drawer">
                                                <pre>
{JSON.stringify(debugById[card.id] ?? { memory_id: card.id, status: "loading" }, null, 2)}
                                                </pre>
                                            </div>
                                        )}
                                    </div>
                                </article>
                            );
                        })}
                    </div>
                )}
            </div>
        </div>
    );
}
