import { useCallback, useState } from "react";
import { getTimeTracking, AppTimeEntry, TimeTrackingResult } from "@/shared/ipc/tauri";
import { usePolling } from "@/shared/hooks/usePolling";
import "./TimeTrackingPanel.css";

interface TimeTrackingPanelProps {
    isVisible: boolean;
    onClose: () => void;
    onSearchApp: (appName: string) => void;
}

function fmt(minutes: number): string {
    if (minutes < 1) return "<1m";
    if (minutes < 60) return `${minutes}m`;
    const h = Math.floor(minutes / 60);
    const m = minutes % 60;
    return m > 0 ? `${h}h ${m}m` : `${h}h`;
}

// Teal-to-purple palette ordered by index
const PALETTE = [
    "rgba(80, 200, 180, 0.75)",
    "rgba(100, 160, 230, 0.75)",
    "rgba(160, 120, 230, 0.75)",
    "rgba(220, 120, 160, 0.75)",
    "rgba(230, 160, 80, 0.75)",
    "rgba(140, 210, 100, 0.75)",
];

function AppBar({ entry, totalMinutes, rank }: { entry: AppTimeEntry; totalMinutes: number; rank: number }) {
    const pct = totalMinutes > 0 ? Math.round((entry.duration_minutes / totalMinutes) * 100) : 0;
    const color = PALETTE[rank % PALETTE.length];

    return (
        <div className="tt-app-row">
            <div className="tt-app-meta">
                <span className="tt-app-name">{entry.app_name}</span>
                <span className="tt-app-time">{fmt(entry.duration_minutes)}</span>
            </div>
            <div className="tt-bar-track">
                <div
                    className="tt-bar-fill"
                    style={{ width: `${pct}%`, background: color }}
                />
            </div>
            <div className="tt-app-sub">
                {pct}% · {entry.capture_count} snapshots
            </div>
        </div>
    );
}

function DonutRing({ breakdown, totalMinutes }: { breakdown: AppTimeEntry[]; totalMinutes: number }) {
    const r = 54;
    const cx = 64;
    const cy = 64;
    const circ = 2 * Math.PI * r;

    let offset = 0;
    const slices = breakdown.slice(0, 6).map((e, i) => {
        const pct = totalMinutes > 0 ? e.duration_minutes / totalMinutes : 0;
        const dash = circ * pct;
        const gap = circ - dash;
        const slice = (
            <circle
                key={e.app_name}
                cx={cx}
                cy={cy}
                r={r}
                fill="none"
                stroke={PALETTE[i % PALETTE.length]}
                strokeWidth={14}
                strokeDasharray={`${dash} ${gap}`}
                strokeDashoffset={-offset}
                style={{ transition: "stroke-dasharray 0.6s ease" }}
            />
        );
        offset += dash + 2; // 2px gap between slices
        return slice;
    });

    return (
        <svg className="tt-donut" viewBox="0 0 128 128" aria-hidden="true">
            <circle cx={cx} cy={cy} r={r} fill="none" stroke="rgba(255,255,255,0.06)" strokeWidth={14} />
            {/* Rotate so first slice starts at top */}
            <g transform={`rotate(-90 ${cx} ${cy})`}>{slices}</g>
            <text x={cx} y={cy - 6} textAnchor="middle" className="tt-donut-label-top">
                {fmt(totalMinutes)}
            </text>
            <text x={cx} y={cx + 10} textAnchor="middle" className="tt-donut-label-sub">
                today
            </text>
        </svg>
    );
}

export function TimeTrackingPanel({ isVisible, onClose, onSearchApp }: TimeTrackingPanelProps) {
    const [result, setResult] = useState<TimeTrackingResult | null>(null);
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);

    const load = useCallback(async (isMounted: () => boolean) => {
            setLoading(true);
            setError(null);
            try {
                const data = await getTimeTracking();
                if (isMounted()) setResult(data);
            } catch (err) {
                if (isMounted()) setError(err instanceof Error ? err.message : "Failed to load time tracking.");
            } finally {
                if (isMounted()) setLoading(false);
            }
    }, []);
    usePolling(load, 5 * 60_000, isVisible);

    if (!isVisible) return null;

    const breakdown = result?.breakdown ?? [];
    const totalMinutes = breakdown.reduce((s, e) => s + e.duration_minutes, 0);

    return (
        <div className="tt-page">
            <header className="tt-header">
                <div>
                    <h2>Time Tracking</h2>
                    <p>Today's screen time, derived from memory captures</p>
                </div>
                <button className="ui-action-btn tt-close-btn" onClick={onClose}>X</button>
            </header>

            <div className="tt-body">
                {loading && !result && (
                    <div className="tt-state">
                        <div className="thinking-loader thinking-loader-md" aria-hidden="true" />
                        <p>Computing today's activity…</p>
                    </div>
                )}

                {error && (
                    <div className="tt-state">
                        <p className="tt-error">{error}</p>
                    </div>
                )}

                {result && breakdown.length === 0 && (
                    <div className="tt-state">
                        <p>No captures yet today.</p>
                    </div>
                )}

                {result && breakdown.length > 0 && (
                    <>
                        {/* Donut summary */}
                        <div className="tt-summary">
                            <DonutRing breakdown={breakdown} totalMinutes={totalMinutes} />
                            <div className="tt-legend">
                                {breakdown.slice(0, 6).map((e, i) => (
                                    <div key={e.app_name} className="tt-legend-row">
                                        <span
                                            className="tt-legend-dot"
                                            style={{ background: PALETTE[i % PALETTE.length] }}
                                        />
                                        <span className="tt-legend-name">{e.app_name}</span>
                                        <span className="tt-legend-time">{fmt(e.duration_minutes)}</span>
                                    </div>
                                ))}
                                {breakdown.length > 6 && (
                                    <div className="tt-legend-more">+{breakdown.length - 6} more apps</div>
                                )}
                            </div>
                        </div>

                        <div className="tt-section-label">By application</div>

                        {/* Bar list */}
                        <div className="tt-list">
                            {breakdown.map((entry, i) => (
                                <button
                                    key={entry.app_name}
                                    className="tt-app-btn"
                                    onClick={() => {
                                        onSearchApp(entry.app_name);
                                        onClose();
                                    }}
                                    title={`Search all ${entry.app_name} memories`}
                                >
                                    <AppBar entry={entry} totalMinutes={totalMinutes} rank={i} />
                                </button>
                            ))}
                        </div>

                        <p className="tt-footnote">
                            {result.total_captures} total snapshots · click any app to search its memories
                        </p>
                    </>
                )}
            </div>
        </div>
    );
}
