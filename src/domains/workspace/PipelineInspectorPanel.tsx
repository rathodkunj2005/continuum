import { useEffect, useMemo, useState } from "react";
import {
    CaptureStatus,
    MemoryCard,
    SearchResult,
    Stats,
    getStatus,
    getStats,
    listMemoryCards,
    searchMemoryCards,
    searchRawResults,
} from "../../shared/ipc/tauri";
import "./PipelineInspectorPanel.css";

interface PipelineInspectorPanelProps {
    isVisible: boolean;
    onClose: () => void;
    currentQuery?: string;
    timeFilter?: string | null;
    appFilter?: string | null;
}

function formatWhen(timestamp: number): string {
    if (!timestamp || Number.isNaN(timestamp)) {
        return "n/a";
    }
    return new Date(timestamp).toLocaleString();
}

function percent(numerator: number, denominator: number): number {
    if (!denominator || denominator <= 0) {
        return 0;
    }
    return Math.max(0, Math.min(100, (numerator / denominator) * 100));
}

function formatScore(score: number | undefined): string {
    if (typeof score !== "number" || Number.isNaN(score)) {
        return "0.00";
    }
    return score.toFixed(2);
}

function tokenizeQuery(value: string): string[] {
    return value
        .toLowerCase()
        .split(/[^a-z0-9]+/g)
        .filter((token) => token.length > 1);
}

function shortSnippet(value: string, maxLen = 120): string {
    const normalized = value.replace(/\s+/g, " ").trim();
    if (normalized.length <= maxLen) {
        return normalized;
    }
    return `${normalized.slice(0, maxLen - 3)}...`;
}

export function PipelineInspectorPanel({
    isVisible,
    onClose,
    currentQuery = "",
    timeFilter = null,
    appFilter = null,
}: PipelineInspectorPanelProps) {
    const [status, setStatus] = useState<CaptureStatus | null>(null);
    const [stats, setStats] = useState<Stats | null>(null);
    const [recentCards, setRecentCards] = useState<MemoryCard[]>([]);
    const [traceQuery, setTraceQuery] = useState("");
    const [rawResults, setRawResults] = useState<SearchResult[]>([]);
    const [finalCards, setFinalCards] = useState<MemoryCard[]>([]);
    const [loading, setLoading] = useState(false);
    const [runningTrace, setRunningTrace] = useState(false);
    const [lastTraceMs, setLastTraceMs] = useState<number | null>(null);
    const [error, setError] = useState<string | null>(null);

    const latestCard = recentCards[0] ?? null;
    const queryTokens = useMemo(() => tokenizeQuery(traceQuery), [traceQuery]);
    const topApps = useMemo(() => (stats?.apps ?? []).slice(0, 4), [stats]);

    const dropRate = useMemo(() => {
        if (!status) {
            return 0;
        }
        const total = status.frames_captured + status.frames_dropped;
        return percent(status.frames_dropped, total);
    }, [status]);

    const coverageRate = useMemo(() => {
        if (!stats) {
            return 0;
        }
        return percent(stats.records_with_clean_text, stats.total_records);
    }, [stats]);

    const runBaselineLoad = async () => {
        const [nextStatus, nextStats, cards] = await Promise.all([
            getStatus(),
            getStats(),
            listMemoryCards(24, null),
        ]);
        setStatus(nextStatus);
        setStats(nextStats);
        setRecentCards(cards);
    };

    const runTrace = async (queryValue: string) => {
        const normalized = queryValue.trim();
        if (!normalized) {
            setRawResults([]);
            setFinalCards([]);
            setLastTraceMs(null);
            return;
        }

        setRunningTrace(true);
        const startedAt = performance.now();
        try {
            const [raw, cards] = await Promise.all([
                searchRawResults(normalized, timeFilter ?? undefined, appFilter ?? undefined, 18),
                searchMemoryCards(normalized, timeFilter ?? undefined, appFilter ?? undefined, 8),
            ]);
            setRawResults(raw);
            setFinalCards(cards);
            setLastTraceMs(Math.round(performance.now() - startedAt));
        } finally {
            setRunningTrace(false);
        }
    };

    useEffect(() => {
        if (!isVisible) {
            return;
        }
        const suggested = currentQuery.trim();
        if (suggested && !traceQuery.trim()) {
            setTraceQuery(suggested);
        }

        let cancelled = false;
        setLoading(true);
        setError(null);
        void runBaselineLoad()
            .then(async () => {
                if (cancelled) {
                    return;
                }
                const queryToRun = suggested || traceQuery;
                if (queryToRun.trim()) {
                    await runTrace(queryToRun);
                }
            })
            .catch((err) => {
                if (!cancelled) {
                    setError(String(err));
                }
            })
            .finally(() => {
                if (!cancelled) {
                    setLoading(false);
                }
            });

        return () => {
            cancelled = true;
        };
    }, [isVisible]);

    if (!isVisible) {
        return null;
    }

    return (
        <div className="pipeline-panel">
            <header className="pipeline-header">
                <div>
                    <h2>Pipeline Inspector</h2>
                    <p>Real capture → index → retrieval trace with live internals.</p>
                </div>
                <button className="ui-action-btn pipeline-close-btn" onClick={onClose}>X</button>
            </header>

            <section className="pipeline-query-bar">
                <input
                    value={traceQuery}
                    onChange={(event) => setTraceQuery(event.target.value)}
                    placeholder='Try: "spotify", "startup evaluation full points", "last time I watched cricket"'
                />
                <button
                    className="ui-action-btn pipeline-run-btn"
                    onClick={() => void runTrace(traceQuery)}
                    disabled={runningTrace}
                >
                    {runningTrace ? "Tracing..." : "Trace Query"}
                </button>
            </section>

            <section className="pipeline-chip-row">
                <span className="pipeline-chip">
                    Time filter: {timeFilter ?? "all"}
                </span>
                <span className="pipeline-chip">
                    App filter: {appFilter ?? "all"}
                </span>
                <span className="pipeline-chip">
                    Embeddings: {status?.embedding_backend ?? "unknown"}
                    {status?.embedding_degraded ? " (degraded)" : ""}
                </span>
                <span className="pipeline-chip">
                    Trace latency: {lastTraceMs !== null ? `${lastTraceMs}ms` : "n/a"}
                </span>
            </section>

            {error && <div className="pipeline-error">{error}</div>}

            <div className="pipeline-body">
                <section className="pipeline-diagram-card">
                    <h3>System Flow Graph</h3>
                    <svg viewBox="0 0 980 210" className="pipeline-flow-svg" role="img" aria-label="FNDR pipeline flow">
                        <defs>
                            <linearGradient id="pipeEdge" x1="0%" y1="0%" x2="100%" y2="0%">
                                <stop offset="0%" stopColor="rgba(199,206,255,0.55)" />
                                <stop offset="100%" stopColor="rgba(120,171,255,0.85)" />
                            </linearGradient>
                        </defs>
                        <rect x="20" y="40" width="150" height="120" rx="14" />
                        <rect x="210" y="40" width="150" height="120" rx="14" />
                        <rect x="400" y="40" width="150" height="120" rx="14" />
                        <rect x="590" y="40" width="150" height="120" rx="14" />
                        <rect x="780" y="40" width="180" height="120" rx="14" />

                        <line x1="170" y1="100" x2="210" y2="100" />
                        <line x1="360" y1="100" x2="400" y2="100" />
                        <line x1="550" y1="100" x2="590" y2="100" />
                        <line x1="740" y1="100" x2="780" y2="100" />

                        <text x="35" y="72">Capture</text>
                        <text x="35" y="98">Frames: {status?.frames_captured ?? 0}</text>
                        <text x="35" y="122">Dropped: {status?.frames_dropped ?? 0}</text>
                        <text x="35" y="146">Drop rate: {dropRate.toFixed(1)}%</text>

                        <text x="225" y="72">OCR + Cleanup</text>
                        <text x="225" y="98">Avg OCR: {((stats?.avg_ocr_confidence ?? 0) * 100).toFixed(1)}%</text>
                        <text x="225" y="122">Noise: {(stats?.avg_noise_score ?? 0).toFixed(2)}</text>
                        <text x="225" y="146">Clean text: {coverageRate.toFixed(1)}%</text>

                        <text x="415" y="72">Chunk + Embed</text>
                        <text x="415" y="98">Backend: {status?.embedding_backend ?? "n/a"}</text>
                        <text x="415" y="122">LLM snippets: {stats?.llm_count ?? 0}</text>
                        <text x="415" y="146">Fallback: {stats?.fallback_count ?? 0}</text>

                        <text x="605" y="72">Lance Store</text>
                        <text x="605" y="98">Records: {stats?.total_records ?? 0}</text>
                        <text x="605" y="122">Apps: {stats?.unique_apps ?? 0}</text>
                        <text x="605" y="146">Domains: {stats?.unique_domains ?? 0}</text>

                        <text x="795" y="72">Hybrid + Cards</text>
                        <text x="795" y="98">Raw hits: {rawResults.length}</text>
                        <text x="795" y="122">Cards: {finalCards.length}</text>
                        <text x="795" y="146">Top card: {formatScore(finalCards[0]?.score)}</text>
                    </svg>
                </section>

                <section className="pipeline-metrics-grid">
                    <article>
                        <h4>Capture Health</h4>
                        <p>{status?.is_paused ? "Paused" : "Running"}</p>
                        <div className="pipeline-meter">
                            <span style={{ width: `${100 - dropRate}%` }} />
                        </div>
                    </article>
                    <article>
                        <h4>OCR Quality</h4>
                        <p>{((stats?.avg_ocr_confidence ?? 0) * 100).toFixed(1)}%</p>
                        <div className="pipeline-meter">
                            <span style={{ width: `${Math.max(0, Math.min(100, (stats?.avg_ocr_confidence ?? 0) * 100))}%` }} />
                        </div>
                    </article>
                    <article>
                        <h4>Noise Pressure</h4>
                        <p>{(stats?.avg_noise_score ?? 0).toFixed(2)}</p>
                        <div className="pipeline-meter">
                            <span style={{ width: `${Math.max(0, Math.min(100, (1 - (stats?.avg_noise_score ?? 0)) * 100))}%` }} />
                        </div>
                    </article>
                    <article>
                        <h4>Index Volume</h4>
                        <p>{stats?.total_records ?? 0} records</p>
                        <div className="pipeline-meter">
                            <span style={{ width: `${Math.min(100, ((stats?.records_last_24h ?? 0) / Math.max(1, stats?.total_records ?? 1)) * 100)}%` }} />
                        </div>
                    </article>
                </section>

                <section className="pipeline-two-col">
                    <article className="pipeline-panel-card">
                        <h3>Latest Captured Memory</h3>
                        {latestCard ? (
                            <div className="pipeline-latest-card">
                                <div className="pipeline-kv"><span>App</span><strong>{latestCard.app_name}</strong></div>
                                <div className="pipeline-kv"><span>Window</span><strong>{shortSnippet(latestCard.window_title, 80)}</strong></div>
                                <div className="pipeline-kv"><span>Timestamp</span><strong>{formatWhen(latestCard.timestamp)}</strong></div>
                                <div className="pipeline-kv"><span>Confidence</span><strong>{formatScore(latestCard.confidence)}</strong></div>
                                <p>{shortSnippet(latestCard.summary || latestCard.raw_snippets?.[0] || "", 220)}</p>
                            </div>
                        ) : (
                            <div className="pipeline-empty">No captured memory available yet.</div>
                        )}
                    </article>

                    <article className="pipeline-panel-card">
                        <h3>Query Signal Breakdown</h3>
                        <div className="pipeline-token-wrap">
                            {queryTokens.length > 0 ? queryTokens.map((token) => (
                                <span className="pipeline-token" key={token}>{token}</span>
                            )) : <span className="pipeline-empty-inline">No query tokens yet.</span>}
                        </div>
                        <div className="pipeline-app-bars">
                            {topApps.length > 0 ? topApps.map((app) => (
                                <div className="pipeline-app-bar" key={app.name}>
                                    <label>{app.name}</label>
                                    <div><span style={{ width: `${percent(app.count, Math.max(1, stats?.total_records ?? 1))}%` }} /></div>
                                    <em>{app.count}</em>
                                </div>
                            )) : <div className="pipeline-empty-inline">No app distribution yet.</div>}
                        </div>
                    </article>
                </section>

                <section className="pipeline-two-col">
                    <article className="pipeline-panel-card">
                        <h3>Raw Retrieval (Before Memory Cards)</h3>
                        <ul className="pipeline-list">
                            {rawResults.slice(0, 8).map((result) => (
                                <li key={result.id}>
                                    <div className="pipeline-list-head">
                                        <strong>{result.app_name}</strong>
                                        <span>{formatScore(result.score)}</span>
                                    </div>
                                    <div className="pipeline-meter compact">
                                        <span style={{ width: `${Math.max(0, Math.min(100, (result.score ?? 0) * 100))}%` }} />
                                    </div>
                                    <p>{shortSnippet(result.snippet || result.text || result.window_title, 140)}</p>
                                </li>
                            ))}
                            {!rawResults.length && <li className="pipeline-empty">Run a query trace to inspect raw hits.</li>}
                        </ul>
                    </article>

                    <article className="pipeline-panel-card">
                        <h3>Final Memory Cards (After Grouping)</h3>
                        <ul className="pipeline-list">
                            {finalCards.slice(0, 8).map((card) => (
                                <li key={card.id}>
                                    <div className="pipeline-list-head">
                                        <strong>{card.app_name}</strong>
                                        <span>{formatScore(card.score)}</span>
                                    </div>
                                    <div className="pipeline-meter compact">
                                        <span style={{ width: `${Math.max(0, Math.min(100, (card.score ?? 0) * 100))}%` }} />
                                    </div>
                                    <p>{shortSnippet(card.summary || card.title, 140)}</p>
                                    <small>
                                        Evidence IDs: {card.evidence_ids?.length ?? 0} · Source count: {card.source_count}
                                    </small>
                                </li>
                            ))}
                            {!finalCards.length && <li className="pipeline-empty">No final cards generated yet.</li>}
                        </ul>
                    </article>
                </section>
            </div>

            {loading && <div className="pipeline-loading">Loading inspector context...</div>}
        </div>
    );
}
