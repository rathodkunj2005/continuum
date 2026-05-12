// Inspired by CC's ScheduleCronTool / RemoteTriggerTool pattern:
// Named, configurable schedules that execute Tauri API calls on a cron-like timer.
// Configs live in localStorage; a shared scheduler hook fires them in the background.
import { useEffect, useRef, useState } from "react";
import {
    generateDailyBriefing,
    listMemoryCards,
    getStats,
} from "../api/tauri";
import { POLL_INTERVALS, STORAGE_KEYS } from "../lib/config";
import "./AutomationPanel.css";

const STORAGE_KEY = STORAGE_KEYS.automations;

// ── Types ─────────────────────────────────────────────────────────────────────

type AutomationId =
    | "daily-summary"
    | "evening-review"
    | "hourly-context"
    | "weekly-digest";
type AutomationFrequency = "hourly" | "daily" | "weekly";

interface AutomationConfig {
    id: AutomationId;
    enabled: boolean;
    scheduledHour?: number;   // 0-23 for daily/weekly
    scheduledMinute?: number; // 0-59
    scheduledDay?: number;    // 0-6 for weekly (0=Sunday)
    lastRunAt?: number;
    lastRunResult?: string;
}

interface AutomationDefinition {
    id: AutomationId;
    label: string;
    description: string;
    frequency: AutomationFrequency;
    defaultHour?: number;
    defaultDay?: number;
    run: () => Promise<string>;
}

const AUTOMATIONS: AutomationDefinition[] = [
    {
        id: "daily-summary",
        label: "Morning Briefing",
        description: "Generate a daily activity summary every morning and surface it when you open FNDR.",
        frequency: "daily",
        defaultHour: 8,
        run: async () => {
            const text = await generateDailyBriefing("morning");
            return text.slice(0, 120) + (text.length > 120 ? "…" : "");
        },
    },
    {
        id: "evening-review",
        label: "Evening Review",
        description: "End-of-day summary of everything you worked on, ready for tomorrow.",
        frequency: "daily",
        defaultHour: 18,
        run: async () => {
            const text = await generateDailyBriefing("evening");
            return text.slice(0, 120) + (text.length > 120 ? "…" : "");
        },
    },
    {
        id: "hourly-context",
        label: "Hourly Context Snapshot",
        description: "Every hour, cluster recent memories into a focused session summary.",
        frequency: "hourly",
        run: async () => {
            const cards = await listMemoryCards(20);
            const apps = [...new Set(cards.map((c) => c.app_name))].slice(0, 4).join(", ");
            return `Snapshot: ${cards.length} memories across ${apps}`;
        },
    },
    {
        id: "weekly-digest",
        label: "Weekly Digest",
        description: "A deep summary of the week's activity, patterns, and key moments.",
        frequency: "weekly",
        defaultHour: 9,
        defaultDay: 1, // Monday
        run: async () => {
            const stats = await getStats();
            return `Week: ${stats.records_last_7d.toLocaleString()} captures, ${stats.unique_apps} apps, ${stats.current_streak_days}d streak`;
        },
    },
];

// ── Persistence ───────────────────────────────────────────────────────────────

function loadConfigs(): AutomationConfig[] {
    try {
        const raw = localStorage.getItem(STORAGE_KEY);
        if (!raw) return getDefaultConfigs();
        const saved = JSON.parse(raw) as Partial<AutomationConfig>[];
        return AUTOMATIONS.map((def) => {
            const existing = saved.find((s) => s.id === def.id);
            return {
                ...getDefaultConfig(def),
                ...existing,
            };
        });
    } catch {
        return getDefaultConfigs();
    }
}

function getDefaultConfig(def: AutomationDefinition): AutomationConfig {
    return {
        id: def.id,
        enabled: false,
        scheduledHour: def.defaultHour ?? 9,
        scheduledMinute: 0,
        scheduledDay: def.defaultDay ?? 1,
    };
}

function getDefaultConfigs(): AutomationConfig[] {
    return AUTOMATIONS.map(getDefaultConfig);
}

function saveConfigs(configs: AutomationConfig[]): void {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(configs));
}

// ── Scheduler hook (exported for use in App.tsx) ──────────────────────────────

export function useAutomationScheduler(): void {
    const tickRef = useRef<number | null>(null);

    useEffect(() => {
        const tick = async () => {
            const configs = loadConfigs();
            const now = new Date();
            const nowMs = now.getTime();

            for (const config of configs) {
                if (!config.enabled) continue;
                const def = AUTOMATIONS.find((d) => d.id === config.id);
                if (!def) continue;

                let shouldRun = false;
                const lastRun = config.lastRunAt ?? 0;

                if (def.frequency === "hourly") {
                    shouldRun = nowMs - lastRun > 55 * 60_000; // 55 min minimum
                } else if (def.frequency === "daily") {
                    const scheduledToday = new Date(now);
                    scheduledToday.setHours(config.scheduledHour ?? 9, config.scheduledMinute ?? 0, 0, 0);
                    const lastRunDate = lastRun ? new Date(lastRun).toDateString() : "";
                    shouldRun = now >= scheduledToday && lastRunDate !== now.toDateString();
                } else if (def.frequency === "weekly") {
                    const scheduledThisWeek = new Date(now);
                    scheduledThisWeek.setHours(config.scheduledHour ?? 9, config.scheduledMinute ?? 0, 0, 0);
                    const dayDiff = (now.getDay() - (config.scheduledDay ?? 1) + 7) % 7;
                    scheduledThisWeek.setDate(now.getDate() - dayDiff);
                    shouldRun = now >= scheduledThisWeek && nowMs - lastRun > 6 * 24 * 60 * 60_000;
                }

                if (!shouldRun) continue;

                try {
                    const result = await def.run();
                    const updated = configs.map((c) =>
                        c.id === config.id
                            ? { ...c, lastRunAt: nowMs, lastRunResult: result }
                            : c
                    );
                    saveConfigs(updated);
                } catch (err) {
                    console.error(`Automation ${config.id} failed:`, err);
                }
            }
        };

        void tick();
        tickRef.current = window.setInterval(() => void tick(), POLL_INTERVALS.automationsMs);
        return () => {
            if (tickRef.current !== null) window.clearInterval(tickRef.current);
        };
    }, []);
}

// ── Panel ─────────────────────────────────────────────────────────────────────

interface AutomationPanelProps {
    isVisible: boolean;
    onClose: () => void;
}

const DAY_LABELS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

function formatLastRun(ts?: number): string {
    if (!ts) return "Never";
    const diff = Date.now() - ts;
    const m = Math.floor(diff / 60_000);
    if (m < 1) return "Just now";
    if (m < 60) return `${m}m ago`;
    const h = Math.floor(m / 60);
    if (h < 24) return `${h}h ago`;
    return `${Math.floor(h / 24)}d ago`;
}

function formatHour(h: number): string {
    const period = h >= 12 ? "PM" : "AM";
    const base = h % 12 || 12;
    return `${base}:00 ${period}`;
}

function nextRunLabel(config: AutomationConfig, def: AutomationDefinition): string {
    if (!config.enabled) return "Disabled";
    if (def.frequency === "hourly") {
        const lastRun = config.lastRunAt ?? 0;
        const next = lastRun + 55 * 60_000;
        const diff = next - Date.now();
        if (diff <= 0) return "Due now";
        const m = Math.ceil(diff / 60_000);
        return `in ${m}m`;
    }
    if (def.frequency === "daily") {
        const now = new Date();
        const scheduled = new Date(now);
        scheduled.setHours(config.scheduledHour ?? 9, config.scheduledMinute ?? 0, 0, 0);
        if (now < scheduled) {
            const diff = scheduled.getTime() - now.getTime();
            const h = Math.floor(diff / 3_600_000);
            const m = Math.floor((diff % 3_600_000) / 60_000);
            return h > 0 ? `in ${h}h ${m}m` : `in ${m}m`;
        }
        scheduled.setDate(scheduled.getDate() + 1);
        return `Tomorrow ${formatHour(config.scheduledHour ?? 9)}`;
    }
    return `${DAY_LABELS[config.scheduledDay ?? 1]} ${formatHour(config.scheduledHour ?? 9)}`;
}

export function AutomationPanel({ isVisible, onClose }: AutomationPanelProps) {
    const [configs, setConfigs] = useState<AutomationConfig[]>(loadConfigs);
    const [runningId, setRunningId] = useState<AutomationId | null>(null);

    useEffect(() => {
        if (isVisible) setConfigs(loadConfigs());
    }, [isVisible]);

    const update = (id: AutomationId, patch: Partial<AutomationConfig>) => {
        setConfigs((prev) => {
            const next = prev.map((c) => (c.id === id ? { ...c, ...patch } : c));
            saveConfigs(next);
            return next;
        });
    };

    const runNow = async (def: AutomationDefinition) => {
        setRunningId(def.id);
        try {
            const result = await def.run();
            update(def.id, { lastRunAt: Date.now(), lastRunResult: result });
        } catch (err) {
            console.error("Manual run failed:", err);
        } finally {
            setRunningId(null);
        }
    };

    if (!isVisible) return null;

    return (
        <div className="auto-page">
            <header className="auto-header">
                <div>
                    <h2>Automation Center</h2>
                    <p>Scheduled tasks that run in the background — no action needed</p>
                </div>
                <button className="ui-action-btn auto-close-btn" onClick={onClose}>X</button>
            </header>

            <div className="auto-body">
                {AUTOMATIONS.map((def) => {
                    const config = configs.find((c) => c.id === def.id) ?? getDefaultConfig(def);
                    const isRunning = runningId === def.id;

                    return (
                        <div key={def.id} className={`auto-card ${config.enabled ? "enabled" : ""}`}>
                            <div className="auto-card-top">
                                <div className="auto-card-info">
                                    <div className="auto-card-label">{def.label}</div>
                                    <div className="auto-card-desc">{def.description}</div>
                                </div>
                                {/* Toggle — mirrors CC's feature flag toggle pattern */}
                                <button
                                    className={`auto-toggle ${config.enabled ? "on" : "off"}`}
                                    onClick={() => update(def.id, { enabled: !config.enabled })}
                                    aria-label={config.enabled ? "Disable" : "Enable"}
                                    title={config.enabled ? "Disable automation" : "Enable automation"}
                                >
                                    <span className="auto-toggle-knob" />
                                </button>
                            </div>

                            {/* Schedule config (shown when enabled) */}
                            {config.enabled && (
                                <div className="auto-schedule">
                                    {def.frequency === "hourly" && (
                                        <span className="auto-schedule-label">Runs every hour</span>
                                    )}

                                    {def.frequency === "daily" && (
                                        <div className="auto-schedule-row">
                                            <span className="auto-schedule-label">Daily at</span>
                                            <select
                                                className="auto-select"
                                                value={config.scheduledHour ?? def.defaultHour ?? 9}
                                                onChange={(e) =>
                                                    update(def.id, { scheduledHour: Number(e.target.value) })
                                                }
                                            >
                                                {Array.from({ length: 24 }, (_, h) => (
                                                    <option key={h} value={h}>{formatHour(h)}</option>
                                                ))}
                                            </select>
                                        </div>
                                    )}

                                    {def.frequency === "weekly" && (
                                        <div className="auto-schedule-row">
                                            <span className="auto-schedule-label">Every</span>
                                            <select
                                                className="auto-select"
                                                value={config.scheduledDay ?? def.defaultDay ?? 1}
                                                onChange={(e) =>
                                                    update(def.id, { scheduledDay: Number(e.target.value) })
                                                }
                                            >
                                                {DAY_LABELS.map((d, i) => (
                                                    <option key={i} value={i}>{d}</option>
                                                ))}
                                            </select>
                                            <span className="auto-schedule-label">at</span>
                                            <select
                                                className="auto-select"
                                                value={config.scheduledHour ?? def.defaultHour ?? 9}
                                                onChange={(e) =>
                                                    update(def.id, { scheduledHour: Number(e.target.value) })
                                                }
                                            >
                                                {Array.from({ length: 24 }, (_, h) => (
                                                    <option key={h} value={h}>{formatHour(h)}</option>
                                                ))}
                                            </select>
                                        </div>
                                    )}
                                </div>
                            )}

                            {/* Status row */}
                            <div className="auto-card-status">
                                <div className="auto-status-meta">
                                    <span className="auto-status-item">
                                        Last: <strong>{formatLastRun(config.lastRunAt)}</strong>
                                    </span>
                                    {config.enabled && (
                                        <span className="auto-status-item">
                                            Next: <strong>{nextRunLabel(config, def)}</strong>
                                        </span>
                                    )}
                                </div>
                                <button
                                    className="ui-action-btn auto-run-btn"
                                    onClick={() => void runNow(def)}
                                    disabled={isRunning}
                                >
                                    {isRunning ? (
                                        <><span className="auto-spinner" /> Running…</>
                                    ) : "Run now"}
                                </button>
                            </div>

                            {/* Last result */}
                            {config.lastRunResult && (
                                <div className="auto-last-result">
                                    <span className="auto-result-label">Last result:</span>
                                    <span className="auto-result-text">{config.lastRunResult}</span>
                                </div>
                            )}
                        </div>
                    );
                })}

                <p className="auto-footnote">
                    Automations run while FNDR is open. Results are available instantly in the relevant panels.
                </p>
            </div>
        </div>
    );
}
