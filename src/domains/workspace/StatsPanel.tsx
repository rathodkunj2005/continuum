import { CSSProperties, useCallback, useMemo, useRef, useState } from "react";
import { Stats, getStats } from "@/shared/ipc/tauri";
import { usePolling } from "@/shared/hooks/usePolling";
import "./StatsPanel.css";

interface StatsPanelProps {
    isVisible: boolean;
    onClose: () => void;
}

type CardId = "metrics" | "insights" | "ranks" | "composition" | "rhythms";
const ALL_CARDS: CardId[] = ["metrics", "insights", "ranks", "composition", "rhythms"];

interface HourEntry {
    hour: number;
    count: number;
}

interface DonutSlice {
    label: string;
    value: number;
    percent: number;
    path: string;
    shade: string;
}

const DONUT_SHADES = ["#f6f6f6", "#cdcdcd", "#969696", "#585858", "#262626"];

function formatPercent(value: number) {
    return `${value.toFixed(1)}%`;
}

function formatHourLabel(hour: number) {
    const period = hour >= 12 ? "PM" : "AM";
    const base = hour % 12 || 12;
    return `${base}${period}`;
}

function formatTimestamp(ts: number | null) {
    if (!ts) return "—";
    return new Date(ts).toLocaleString(undefined, {
        month: "short",
        day: "numeric",
        year: "numeric",
        hour: "numeric",
        minute: "2-digit",
    });
}

function normalizeHourlyDistribution(entries: { hour: number; count: number }[]): HourEntry[] {
    const map = new Map(entries.map((entry) => [entry.hour, entry.count]));
    return Array.from({ length: 24 }, (_, hour) => ({
        hour,
        count: map.get(hour) ?? 0,
    }));
}

function buildCaptureNarrative(stats: Stats, hourly: HourEntry[]): string[] {
    const topApp = stats.apps[0];
    const topDomain = stats.top_domains[0];
    const peak = [...hourly].sort((a, b) => b.count - a.count)[0];
    const switchTempo =
        stats.app_switch_rate_per_hour >= 12
            ? "rapid"
            : stats.app_switch_rate_per_hour >= 6
                ? "steady"
                : "deep-focus";

    return [
        topApp
            ? `${topApp.name} is your center of gravity at ${formatPercent((topApp.count / Math.max(stats.total_records, 1)) * 100)} of captures.`
            : "Your capture graph is still warming up.",
        peak && peak.count > 0
            ? `${formatHourLabel(peak.hour)} is your hotspot with ${peak.count.toLocaleString()} captures.`
            : "No meaningful hourly hotspot yet.",
        `Context switching tempo is ${switchTempo} (${stats.app_switch_rate_per_hour.toFixed(1)} switches/hour).`,
        topDomain
            ? `Most revisited domain: ${topDomain.domain} with ${topDomain.count.toLocaleString()} memories.`
            : "Domain intelligence will appear as soon as URL captures accumulate.",
        `Signal quality: OCR ${formatPercent(stats.avg_ocr_confidence * 100)}, noise ${stats.avg_noise_score.toFixed(2)}.`,
        `You logged ${stats.today_count.toLocaleString()} captures today with ${stats.records_last_hour.toLocaleString()} in the last hour.`,
    ];
}

function polarToCartesian(cx: number, cy: number, radius: number, angle: number) {
    const radians = ((angle - 90) * Math.PI) / 180;
    return {
        x: cx + radius * Math.cos(radians),
        y: cy + radius * Math.sin(radians),
    };
}

function describeDonutSlice(
    cx: number,
    cy: number,
    outerRadius: number,
    innerRadius: number,
    startAngle: number,
    endAngle: number
) {
    const safeEnd = Math.min(endAngle, startAngle + 359.999);
    const largeArc = safeEnd - startAngle > 180 ? 1 : 0;

    const outerStart = polarToCartesian(cx, cy, outerRadius, startAngle);
    const outerEnd = polarToCartesian(cx, cy, outerRadius, safeEnd);
    const innerStart = polarToCartesian(cx, cy, innerRadius, startAngle);
    const innerEnd = polarToCartesian(cx, cy, innerRadius, safeEnd);

    return [
        `M ${outerStart.x} ${outerStart.y}`,
        `A ${outerRadius} ${outerRadius} 0 ${largeArc} 1 ${outerEnd.x} ${outerEnd.y}`,
        `L ${innerEnd.x} ${innerEnd.y}`,
        `A ${innerRadius} ${innerRadius} 0 ${largeArc} 0 ${innerStart.x} ${innerStart.y}`,
        "Z",
    ].join(" ");
}

function buildDonutSlices(raw: Array<{ label: string; value: number }>): DonutSlice[] {
    const total = raw.reduce((sum, slice) => sum + Math.max(0, slice.value), 0);
    if (total <= 0) {
        return [];
    }

    let cursor = 0;
    return raw
        .filter((slice) => slice.value > 0)
        .map((slice, index) => {
            const start = cursor;
            const sweep = (slice.value / total) * 360;
            cursor += sweep;
            return {
                label: slice.label,
                value: slice.value,
                percent: (slice.value / total) * 100,
                path: describeDonutSlice(80, 80, 72, 44, start, cursor),
                shade: DONUT_SHADES[index % DONUT_SHADES.length],
            };
        });
}

function buildSparkline(values: number[]) {
    const width = 420;
    const height = 120;
    const top = 12;
    const bottom = 100;
    const step = width / Math.max(values.length - 1, 1);
    const maxValue = Math.max(...values, 1);

    const points = values.map((value, index) => {
        const x = step * index;
        const y = bottom - (value / maxValue) * (bottom - top);
        return { x, y };
    });

    const linePath = points
        .map((point, index) => `${index === 0 ? "M" : "L"} ${point.x.toFixed(2)} ${point.y.toFixed(2)}`)
        .join(" ");
    const areaPath = `${linePath} L ${width} ${height} L 0 ${height} Z`;
    return { linePath, areaPath, width, height };
}

export function StatsPanel({ isVisible, onClose }: StatsPanelProps) {
    const [stats, setStats] = useState<Stats | null>(null);
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const hasLoadedStatsRef = useRef(false);

    const [viewMode, setViewMode] = useState<"stacked" | "grid">("grid");
    const [deckOrder, setDeckOrder] = useState<CardId[]>(ALL_CARDS);

    const loadStats = useCallback(async (isMounted: () => boolean) => {
        const showLoading = !hasLoadedStatsRef.current;
        if (showLoading) {
            setLoading(true);
        }
        setError(null);
        try {
            const snapshot = await getStats();
            if (isMounted()) {
                hasLoadedStatsRef.current = true;
                setStats(snapshot);
            }
        } catch (err) {
            if (isMounted()) {
                setError(err instanceof Error ? err.message : "Unable to load stats.");
            }
        } finally {
            if (isMounted()) {
                setLoading(false);
            }
        }
    }, []);
    usePolling(loadStats, 15_000, isVisible);

    const hourlySeries = useMemo(
        () => normalizeHourlyDistribution(stats?.hourly_distribution ?? []),
        [stats?.hourly_distribution]
    );
    const maxHourly = useMemo(
        () => Math.max(...hourlySeries.map((entry) => entry.count), 1),
        [hourlySeries]
    );
    const sparkline = useMemo(
        () => buildSparkline(hourlySeries.map((entry) => entry.count)),
        [hourlySeries]
    );

    const quickStats = useMemo(() => {
        if (!stats) return [];
        const records = Math.max(stats.total_records, 1);
        return [
            { label: "Total captures", value: stats.total_records.toLocaleString() },
            { label: "Today", value: stats.today_count.toLocaleString() },
            { label: "Last hour", value: stats.records_last_hour.toLocaleString() },
            { label: "Capture streak", value: `${stats.current_streak_days}d` },
            { label: "Apps in play", value: stats.unique_apps.toLocaleString() },
            { label: "Context switches", value: stats.app_switches.toLocaleString() },
            { label: "Switches / hour", value: stats.app_switch_rate_per_hour.toFixed(1) },
            { label: "URL coverage", value: formatPercent((stats.records_with_url / records) * 100) },
            {
                label: "Screenshot coverage",
                value: formatPercent((stats.records_with_screenshot / records) * 100),
            },
            { label: "OCR confidence", value: formatPercent(stats.avg_ocr_confidence * 100) },
            { label: "Noise score", value: stats.avg_noise_score.toFixed(2) },
            { label: "Longest gap", value: `${stats.longest_gap_minutes.toLocaleString()}m` },
        ];
    }, [stats]);

    const narrativeLines = useMemo(() => {
        if (!stats) return [];
        return buildCaptureNarrative(stats, hourlySeries);
    }, [stats, hourlySeries]);

    const daypartSorted = useMemo(() => {
        if (!stats) return [];
        return [...stats.daypart_distribution].sort((a, b) => b.count - a.count);
    }, [stats]);
    const daypartSlices = useMemo(
        () => buildDonutSlices(daypartSorted.map((entry) => ({ label: entry.daypart, value: entry.count }))),
        [daypartSorted]
    );

    const weekdaySorted = useMemo(() => {
        if (!stats) return [];
        return [...stats.weekday_distribution].sort((a, b) => b.count - a.count);
    }, [stats]);

    const handleCardClick = (id: CardId) => {
        if (viewMode !== "stacked") return;
        setDeckOrder((prev) => {
            if (prev[0] === id) {
                return [...prev.slice(1), id];
            }
            return [id, ...prev.filter((card) => card !== id)];
        });
    };

    const renderDonut = (
        title: string,
        subtitle: string,
        slices: DonutSlice[],
        total: number
    ) => (
        <div className="stats-donut-panel">
            <div className="stats-chart-head">
                <h4>{title}</h4>
                <span>{subtitle}</span>
            </div>
            {total <= 0 || slices.length === 0 ? (
                <p className="stats-page-empty">No distribution available yet.</p>
            ) : (
                <div className="stats-donut-shell">
                    <div className="stats-donut-stage">
                        <svg viewBox="0 0 160 160" className="stats-donut-svg" role="img" aria-label={title}>
                            {slices.map((slice) => (
                                <path key={slice.label} d={slice.path} style={{ fill: slice.shade }} />
                            ))}
                        </svg>
                        <div className="stats-donut-center">
                            <strong>{total.toLocaleString()}</strong>
                            <span>Total</span>
                        </div>
                    </div>
                    <div className="stats-donut-legend">
                        {slices.map((slice) => (
                            <div key={slice.label} className="stats-donut-legend-row">
                                <span className="stats-donut-swatch" style={{ backgroundColor: slice.shade }} />
                                <span>{slice.label}</span>
                                <strong>{slice.percent.toFixed(1)}%</strong>
                            </div>
                        ))}
                    </div>
                </div>
            )}
        </div>
    );

    const renderCardContent = (id: CardId) => {
        if (!stats) return null;

        if (id === "metrics") {
            return (
                <div className="stats-card-scroller">
                    <h3>Live Pulse Board</h3>
                    <div className="stats-page-grid">
                        {quickStats.map((item) => (
                            <div key={item.label} className="stats-page-card stats-data-card">
                                <span className="stats-page-value">{item.value}</span>
                                <span className="stats-page-label">{item.label}</span>
                            </div>
                        ))}
                    </div>
                    <div className="stats-meter-row">
                        <div className="stats-meter-card">
                            <span>Focus share</span>
                            <strong>{formatPercent(stats.focus_app_share_pct)}</strong>
                            <div className="stats-meter-track">
                                <div
                                    className="stats-meter-fill"
                                    style={{ width: `${Math.min(100, Math.max(0, stats.focus_app_share_pct))}%` }}
                                />
                            </div>
                        </div>
                        <div className="stats-meter-card">
                            <span>Low-confidence OCR</span>
                            <strong>{stats.low_confidence_records.toLocaleString()}</strong>
                            <div className="stats-meter-track">
                                <div
                                    className="stats-meter-fill"
                                    style={{
                                        width: `${Math.min(
                                            100,
                                            (stats.low_confidence_records / Math.max(stats.total_records, 1)) * 100
                                        )}%`,
                                    }}
                                />
                            </div>
                        </div>
                        <div className="stats-meter-card">
                            <span>High-noise captures</span>
                            <strong>{stats.high_noise_records.toLocaleString()}</strong>
                            <div className="stats-meter-track">
                                <div
                                    className="stats-meter-fill"
                                    style={{
                                        width: `${Math.min(
                                            100,
                                            (stats.high_noise_records / Math.max(stats.total_records, 1)) * 100
                                        )}%`,
                                    }}
                                />
                            </div>
                        </div>
                    </div>
                </div>
            );
        }

        if (id === "insights") {
            return (
                <div className="stats-card-scroller">
                    <h3>Intelligence Brief</h3>
                    <div className="stats-intel-strip">
                        {narrativeLines.map((line, index) => (
                            <p key={line}>
                                <span>{String(index + 1).padStart(2, "0")}</span>
                                {line}
                            </p>
                        ))}
                    </div>
                    <div className="stats-wave-panel">
                        <div className="stats-chart-head">
                            <h4>Daily Rhythm Wave</h4>
                            <span>Capture cadence by hour</span>
                        </div>
                        <svg viewBox={`0 0 ${sparkline.width} ${sparkline.height}`} className="stats-wave-svg" role="img" aria-label="Daily rhythm wave">
                            <path d={sparkline.areaPath} className="stats-wave-area" />
                            <path d={sparkline.linePath} className="stats-wave-line" />
                        </svg>
                    </div>
                </div>
            );
        }

        if (id === "ranks") {
            return (
                <div className="stats-card-scroller stats-two-column">
                    <div>
                        <div className="stats-chart-head">
                            <h4>Top Apps</h4>
                            <span>Where your time was captured</span>
                        </div>
                        <div className="stats-page-rank-list">
                            {stats.apps.length === 0 && <p className="stats-page-empty">No app activity yet.</p>}
                            {stats.apps.map((app) => {
                                const max = Math.max(stats.apps[0]?.count ?? 1, 1);
                                const width = (app.count / max) * 100;
                                return (
                                    <div key={app.name} className="stats-page-rank-row">
                                        <div className="stats-page-rank-meta">
                                            <span>{app.name}</span>
                                            <span>{app.count.toLocaleString()}</span>
                                        </div>
                                        <div className="stats-page-rank-bar">
                                            <span style={{ width: `${width}%` }} />
                                        </div>
                                    </div>
                                );
                            })}
                        </div>
                    </div>
                    <div>
                        <div className="stats-chart-head">
                            <h4>Top Domains</h4>
                            <span>Most captured web neighborhoods</span>
                        </div>
                        <div className="stats-page-rank-list">
                            {stats.top_domains.length === 0 && <p className="stats-page-empty">No domain activity yet.</p>}
                            {stats.top_domains.map((domain) => {
                                const max = Math.max(stats.top_domains[0]?.count ?? 1, 1);
                                const width = (domain.count / max) * 100;
                                return (
                                    <div key={domain.domain} className="stats-page-rank-row">
                                        <div className="stats-page-rank-meta">
                                            <span>{domain.domain}</span>
                                            <span>{domain.count.toLocaleString()}</span>
                                        </div>
                                        <div className="stats-page-rank-bar">
                                            <span style={{ width: `${width}%` }} />
                                        </div>
                                    </div>
                                );
                            })}
                        </div>
                    </div>
                </div>
            );
        }

        if (id === "composition") {
            const daypartTotal = daypartSlices.reduce((sum, slice) => sum + slice.value, 0);
            return (
                <div className="stats-card-scroller stats-composition-grid">
                    {renderDonut("Daypart Distribution", "When your captures cluster during the day", daypartSlices, daypartTotal)}
                </div>
            );
        }

        return (
            <div className="stats-card-scroller">
                <h3>Capture Timeline</h3>
                <div className="stats-chart-head">
                    <h4>Hourly Distribution</h4>
                    <span>Integrated into your timeline, right up top</span>
                </div>
                <div className="stats-hourly-racks" aria-label="Hourly capture distribution bars">
                    {hourlySeries.map((entry) => {
                        const ratio = entry.count / maxHourly;
                        const height = 18 + ratio * 68;
                        return (
                            <div key={entry.hour} className="stats-hour-rack" title={`${formatHourLabel(entry.hour)} · ${entry.count.toLocaleString()} captures`}>
                                <span className="stats-hour-bar" style={{ height: `${height}px` }} />
                            </div>
                        );
                    })}
                </div>
                <div className="stats-hour-labels">
                    <span>12AM</span>
                    <span>6AM</span>
                    <span>12PM</span>
                    <span>6PM</span>
                    <span>11PM</span>
                </div>

                <div className="stats-page-meta-grid">
                    <div className="stats-page-meta-row">
                        <span>First capture</span>
                        <strong>{formatTimestamp(stats.first_capture_ts)}</strong>
                    </div>
                    <div className="stats-page-meta-row">
                        <span>Most recent capture</span>
                        <strong>{formatTimestamp(stats.last_capture_ts)}</strong>
                    </div>
                    <div className="stats-page-meta-row">
                        <span>Busiest day</span>
                        <strong>{stats.busiest_day ? `${stats.busiest_day.day} (${stats.busiest_day.count.toLocaleString()})` : "—"}</strong>
                    </div>
                    <div className="stats-page-meta-row">
                        <span>Quietest day</span>
                        <strong>{stats.quietest_day ? `${stats.quietest_day.day} (${stats.quietest_day.count.toLocaleString()})` : "—"}</strong>
                    </div>
                </div>

                <div className="stats-weekday-bars">
                    <div className="stats-chart-head">
                        <h4>Weekday Momentum</h4>
                        <span>Workweek capture bar graph</span>
                    </div>
                    {weekdaySorted.map((entry) => {
                        const max = Math.max(weekdaySorted[0]?.count ?? 1, 1);
                        return (
                            <div key={entry.weekday} className="stats-weekday-row">
                                <span>{entry.weekday}</span>
                                <div className="stats-weekday-track">
                                    <div
                                        className="stats-weekday-fill"
                                        style={{ width: `${(entry.count / max) * 100}%` }}
                                    />
                                </div>
                                <strong>{entry.count.toLocaleString()}</strong>
                            </div>
                        );
                    })}
                </div>
            </div>
        );
    };

    if (!isVisible) {
        return null;
    }

    return (
        <div className="stats-page">
            <header className="stats-page-header">
                <div>
                    <h2>FNDR Stats Intelligence</h2>
                    <p>Behavioral telemetry with context, cadence, and signal quality in one view.</p>
                </div>
                <div className="stats-page-actions">
                    <button
                        className="ui-action-btn stats-layout-btn"
                        onClick={() => setViewMode((value) => (value === "stacked" ? "grid" : "stacked"))}
                    >
                        {viewMode === "stacked" ? "Lay Out All" : "Stack Cards"}
                    </button>
                    <button className="ui-action-btn stats-close-btn" onClick={onClose}>X</button>
                </div>
            </header>

            <div className="stats-page-body">
                {loading && (
                    <div className="stats-page-state">
                        <div className="thinking-loader thinking-loader-lg" aria-hidden="true" />
                        <p>Loading stats...</p>
                    </div>
                )}

                {!loading && error && (
                    <div className="stats-page-state">
                        <p>{error}</p>
                    </div>
                )}

                {!loading && !error && stats && (
                    <div className={`stats-deck-container is-${viewMode}`}>
                        {ALL_CARDS.map((id) => {
                            const stackIndex = deckOrder.indexOf(id);
                            return (
                                <div
                                    key={id}
                                    className={`stats-playing-card ${stackIndex === 0 ? "is-top" : ""} card-${id}`}
                                    style={{ "--stack-index": stackIndex } as CSSProperties}
                                    onClick={() => handleCardClick(id)}
                                    role="button"
                                    tabIndex={0}
                                >
                                    <div className={`stats-card-bg bg-${id}`} />
                                    <div className="stats-card-content">{renderCardContent(id)}</div>
                                </div>
                            );
                        })}
                    </div>
                )}
            </div>
        </div>
    );
}
