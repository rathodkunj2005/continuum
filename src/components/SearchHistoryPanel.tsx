import { useEffect, useState } from "react";
import { SEARCH_HISTORY, STORAGE_KEYS } from "../lib/config";
import "./SearchHistoryPanel.css";

const STORAGE_KEY = STORAGE_KEYS.searchHistory;
const MAX_HISTORY = SEARCH_HISTORY.maxEntries;

export interface SearchHistoryEntry {
    query: string;
    timestamp: number;
}

function loadSearchHistory(): SearchHistoryEntry[] {
    try {
        const raw = localStorage.getItem(STORAGE_KEY);
        if (!raw) return [];
        return JSON.parse(raw) as SearchHistoryEntry[];
    } catch {
        return [];
    }
}

export function appendToSearchHistory(query: string): void {
    const trimmed = query.trim();
    if (!trimmed) return;
    try {
        const history = loadSearchHistory().filter((e) => e.query !== trimmed);
        history.unshift({ query: trimmed, timestamp: Date.now() });
        localStorage.setItem(STORAGE_KEY, JSON.stringify(history.slice(0, MAX_HISTORY)));
    } catch {
        // ignore storage errors
    }
}

interface SearchHistoryPanelProps {
    isVisible: boolean;
    onClose: () => void;
    onRunQuery: (query: string) => void;
}

function formatRelative(ts: number): string {
    const diff = Date.now() - ts;
    const m = Math.floor(diff / 60_000);
    if (m < 1) return "just now";
    if (m < 60) return `${m}m ago`;
    const h = Math.floor(m / 60);
    if (h < 24) return `${h}h ago`;
    const d = Math.floor(h / 24);
    return `${d}d ago`;
}

export function SearchHistoryPanel({ isVisible, onClose, onRunQuery }: SearchHistoryPanelProps) {
    const [history, setHistory] = useState<SearchHistoryEntry[]>([]);
    const [filter, setFilter] = useState("");

    const reload = () => setHistory(loadSearchHistory());

    useEffect(() => {
        if (!isVisible) return;
        reload();
        // Sync on storage changes from other tabs
        const handler = (e: StorageEvent) => {
            if (e.key === STORAGE_KEY) reload();
        };
        window.addEventListener("storage", handler);
        return () => window.removeEventListener("storage", handler);
    }, [isVisible]);

    const handleClear = () => {
        localStorage.removeItem(STORAGE_KEY);
        setHistory([]);
    };

    const handleRemove = (query: string) => {
        const next = history.filter((e) => e.query !== query);
        localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
        setHistory(next);
    };

    const visible = filter.trim()
        ? history.filter((e) => e.query.toLowerCase().includes(filter.trim().toLowerCase()))
        : history;

    if (!isVisible) return null;

    return (
        <div className="sh-page">
            <header className="sh-header">
                <div>
                    <h2>Search History</h2>
                    <p>Recent queries — click any to re-run</p>
                </div>
                <div className="sh-header-actions">
                    {history.length > 0 && (
                        <button className="ui-action-btn sh-clear-btn" onClick={handleClear}>
                            Clear all
                        </button>
                    )}
                    <button className="ui-action-btn sh-close-btn" onClick={onClose}>X</button>
                </div>
            </header>

            {history.length > 4 && (
                <div className="sh-filter-row">
                    <input
                        className="sh-filter-input"
                        placeholder="Filter history…"
                        value={filter}
                        onChange={(e) => setFilter(e.target.value)}
                        autoFocus
                    />
                </div>
            )}

            <div className="sh-body">
                {history.length === 0 && (
                    <div className="sh-empty">
                        <p>No searches yet.</p>
                        <p className="sh-empty-hint">Your queries will appear here as you search.</p>
                    </div>
                )}

                {history.length > 0 && visible.length === 0 && (
                    <div className="sh-empty">
                        <p>No matches for "{filter}"</p>
                    </div>
                )}

                {visible.length > 0 && (
                    <ul className="sh-list">
                        {visible.map((entry) => (
                            <li key={`${entry.query}-${entry.timestamp}`} className="sh-item">
                                <button
                                    className="sh-item-query"
                                    onClick={() => {
                                        onRunQuery(entry.query);
                                        onClose();
                                    }}
                                    title={`Search: ${entry.query}`}
                                >
                                    <span className="sh-item-icon">↩</span>
                                    <span className="sh-item-text">{entry.query}</span>
                                    <span className="sh-item-time">{formatRelative(entry.timestamp)}</span>
                                </button>
                                <button
                                    className="sh-item-remove"
                                    onClick={() => handleRemove(entry.query)}
                                    title="Remove"
                                    aria-label="Remove from history"
                                >
                                    ×
                                </button>
                            </li>
                        ))}
                    </ul>
                )}
            </div>
        </div>
    );
}
