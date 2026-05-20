import { useEffect, useMemo, useState, useCallback, useRef } from "react";
import { AppPanels } from "./AppPanels";
import { BiometricLockScreen } from "./BiometricLockScreen";
import { HomeHero } from "./HomeHero";
import type { AppToast } from "./types";
import { SearchBar } from "@/domains/search/SearchBar";
import { Timeline } from "@/domains/timeline/Timeline";
import { ControlPanel } from "@/domains/workspace/ControlPanel";
import { ModelDownloadBanner } from "@/domains/workspace/ModelDownloadBanner";
import { Onboarding } from "@/domains/workspace/Onboarding";
import { appendToSearchHistory } from "@/domains/workspace/SearchHistoryPanel";
import { type PanelKey } from "@/domains/command-palette/CommandPalette";
import { useAutomationScheduler } from "@/domains/workspace/AutomationPanel";
import "@/domains/workspace/FocusModePanel.css";

import { useSearch } from "@/shared/hooks/useSearch";
import { usePolling } from "@/shared/hooks/usePolling";
import { POLL_INTERVALS, TOAST } from "@/shared/utils/config";
import { createClientId } from "@/shared/utils/id";
import {
    CaptureStatus,
    type FndrNotificationPayload,
    MeetingRecorderStatus,
    MemoryCard,
    deleteMemory,
    getAppNames,
    getMeetingStatus,
    onMeetingStatus,
    onFndrNotification,
    onProactiveSuggestion,
    getStatus,
    getFunGreeting,
} from "@/shared/ipc/tauri";
import { getOnboardingState, saveOnboardingState, type OnboardingState } from "@/shared/ipc/onboarding";
import { EVAL_UI } from "@/shared/utils/eval-ui";
import "./styles/App.css";

function nextToastId(): string {
    return createClientId("fndr-toast");
}

const SIDEBAR_GROUPS = [
    {
        label: "Features",
        items: [
            { key: "memoryCards", text: "Memory Vault" },
            { key: "knowledgeGraph", text: "Knowledge Graph" },
            { key: "engineMetrics", text: "Engine metrics" },
            { key: "glassesImport", text: "Glasses photo import" },
            { key: "stats", text: "Stats" },
            { key: "todo", text: "To Do" },
            { key: "meeting", text: "Meetings" },
            { key: "dailySummary", text: "Daily Summary" },
            { key: "agent", text: "Agent" },
            { key: "pipeline", text: "Pipeline Inspector" },
        ],
    },
    {
        label: "Smart",
        items: [
            { key: "focusSession", text: "Focus Session" },
            { key: "quickSkills", text: "Quick Skills" },
            { key: "searchHistory", text: "Search History" },
            { key: "automation", text: "Automation" },
            { key: "research", text: "Research" },
            { key: "timeTracking", text: "Time Tracking" },
            { key: "focusMode", text: "Focus Mode" },
        ],
    },
] as const satisfies ReadonlyArray<{
    label: string;
    items: ReadonlyArray<{ key: PanelKey; text: string }>;
}>;

function App() {
    const [queryDraft, setQueryDraft] = useState("");
    const [query, setQuery] = useState("");
    const [timeFilter, setTimeFilter] = useState<string | null>(null);
    const [appFilter, setAppFilter] = useState<string | null>(null);
    const [appNames, setAppNames] = useState<string[]>([]);
    const [status, setStatus] = useState<CaptureStatus | null>(null);
    const [meetingStatus, setMeetingStatus] = useState<MeetingRecorderStatus | null>(null);
    // Single active-panel state — only one full-screen panel can be open at a time.
    // CommandPalette is kept separate because it layers on top of the current panel.
    const [activePanel, setActivePanel] = useState<PanelKey | null>(null);
    const [researchSeedMemory, setResearchSeedMemory] = useState<MemoryCard | null>(null);
    const [memoryVaultFocusId, setMemoryVaultFocusId] = useState<string | null>(null);
    const [showCommandPalette, setShowCommandPalette] = useState(false);
    const [appToasts, setAppToasts] = useState<AppToast[]>([]);
    const toastTimersRef = useRef<Map<string, number>>(new Map());

    // Background automation scheduler — fires Tauri calls on configured schedules
    useAutomationScheduler();
    const [onboardingDone, setOnboardingDone] = useState<boolean | null>(null);
    const [biometricRequired, setBiometricRequired] = useState<boolean | null>(null);
    const [biometricUnlocked, setBiometricUnlocked] = useState(false);
    const [selectedResult, setSelectedResult] = useState<MemoryCard | null>(null);
    const [isSidebarOpen, setIsSidebarOpen] = useState(false);
    const [deletedMemoryIds, setDeletedMemoryIds] = useState<Set<string>>(new Set());
    const [displayName, setDisplayName] = useState<string | null>(null);
    const [now, setNow] = useState(() => new Date());
    const handleUnlock = useCallback(() => setBiometricUnlocked(true), []);
    const handleDisableBiometricLock = useCallback(async () => {
        try {
            const current = await getOnboardingState();
            await saveOnboardingState({
                ...current,
                biometric_enabled: false,
            });
        } catch (err) {
            console.error("Failed to disable biometric lock:", err);
        } finally {
            setBiometricRequired(false);
            setBiometricUnlocked(true);
        }
    }, []);

    const searchAllowed = true;
    const { results, isLoading, error } = useSearch(
        searchAllowed ? query : "",
        timeFilter,
        appFilter
    );
    const visibleResults = useMemo(
        () => results.filter((item) => !deletedMemoryIds.has(item.id)),
        [results, deletedMemoryIds]
    );

    useEffect(() => {
        getOnboardingState()
            .then((s) => {
                setOnboardingDone(s.step === "complete" && s.model_downloaded);
                setDisplayName(s.display_name ?? null);
                setBiometricRequired(s.biometric_enabled === true);
            })
            .catch(() => {
                setOnboardingDone(false);
                setDisplayName(null);
                setBiometricRequired(false);
            });
    }, []);

    const isFocusMode = !query.trim();

    const [homeGreeting, setHomeGreeting] = useState("");

    // Fetch the fun animated greeting anytime they log in or the name changes
    useEffect(() => {
        getFunGreeting(displayName).then(setHomeGreeting).catch(() => {
            setHomeGreeting("Welcome back to FNDR.");
        });
    }, [displayName]);

    const loadAppNames = useCallback(async (isMounted: () => boolean) => {
        try {
            const names = await getAppNames();
            if (isMounted()) {
                setAppNames(names);
            }
        } catch {
            if (isMounted()) {
                setAppNames([]);
            }
        }
    }, []);
    usePolling(loadAppNames, POLL_INTERVALS.appNamesMs);

    const fetchStatus = useCallback(async (isMounted: () => boolean) => {
        try {
            const nextStatus = await getStatus();
            if (isMounted()) {
                setStatus(nextStatus);
            }
        } catch (e) {
            console.error("Failed to get status:", e);
        }
    }, []);
    usePolling(fetchStatus, POLL_INTERVALS.captureStatusMs);

    useEffect(() => {
        let mounted = true;
        let unlisten: (() => void) | null = null;

        const fetchMeeting = async () => {
            try {
                const nextStatus = await getMeetingStatus();
                if (!mounted) return;
                setMeetingStatus(nextStatus);
            } catch {
                // Ignore transient meeting status failures while runtime starts.
            }
        };

        const subscribe = async () => {
            try {
                unlisten = await onMeetingStatus((nextStatus) => {
                    if (!mounted) return;
                    setMeetingStatus(nextStatus);
                });
            } catch {
                // Ignore listener registration errors; manual refresh paths remain available.
            }
        };

        void fetchMeeting();
        void subscribe();

        return () => {
            mounted = false;
            if (unlisten) {
                unlisten();
            }
        };
    }, []);

    const refreshNow = useCallback(() => setNow(new Date()), []);
    usePolling(refreshNow, POLL_INTERVALS.clockTickMs);

    useEffect(() => {
        const handleProfileUpdated = (event: Event) => {
            const customEvent = event as CustomEvent<{ displayName: string | null }>;
            setDisplayName(customEvent.detail?.displayName ?? null);
        };
        window.addEventListener("fndr-profile-updated", handleProfileUpdated as EventListener);
        return () =>
            window.removeEventListener("fndr-profile-updated", handleProfileUpdated as EventListener);
    }, []);

    useEffect(() => {
        if (query.trim().length > 0) {
            return;
        }
        setTimeFilter(null);
        setAppFilter(null);
    }, [query]);

    const handleSearchSubmit = async (nextValue?: string) => {
        const source = nextValue ?? queryDraft;
        const normalized = source
            .replace(/\r?\n/g, " ")
            .replace(/\s+/g, " ")
            .trim();

        setQuery(normalized);
        if (normalized) appendToSearchHistory(normalized);
        if (typeof nextValue === "string") {
            setQueryDraft(nextValue);
        }
    };

    // Run a Quick Skill: set query + optional time filter, then submit
    const handleRunSkill = (skillQuery: string, timeFilter?: string) => {
        if (timeFilter) setTimeFilter(timeFilter);
        setQueryDraft(skillQuery);
        setQuery(skillQuery);
        if (skillQuery) appendToSearchHistory(skillQuery);
    };

    // Run a search for a specific app (from Focus Session panel)
    const handleSearchApp = (appName: string) => {
        setAppFilter(appName);
        setQueryDraft("");
        setQuery(" "); // trigger search with only app filter
    };

    // Command Palette panel dispatcher — opens any panel by key
    const handleOpenPanel = useCallback((panel: PanelKey) => {
        setShowCommandPalette(false);
        setIsSidebarOpen(false);
        if (panel !== "memoryCards") {
            setMemoryVaultFocusId(null);
        }
        setActivePanel(panel);
    }, []);

    const handleOpenMemoryById = useCallback((memoryId: string) => {
        setMemoryVaultFocusId(memoryId);
        setShowCommandPalette(false);
        setIsSidebarOpen(false);
        setActivePanel("memoryCards");
    }, []);

    // Research trigger — opens Research panel seeded with a memory
    const handleResearchMemory = useCallback((memory: MemoryCard) => {
        setResearchSeedMemory(memory);
        setActivePanel("research");
        setShowCommandPalette(false);
    }, []);

    const dismissToast = useCallback((toastId: string) => {
        const timer = toastTimersRef.current.get(toastId);
        if (timer !== undefined) {
            window.clearTimeout(timer);
            toastTimersRef.current.delete(toastId);
        }
        setAppToasts((previous) => previous.filter((toast) => toast.id !== toastId));
    }, []);

    const enqueueToast = useCallback(
        (toast: Omit<AppToast, "id">, durationMs = TOAST.defaultDurationMs) => {
            const id = nextToastId();
            const nextToast: AppToast = { id, ...toast };
            setAppToasts((previous) => [nextToast, ...previous].slice(0, TOAST.stackLimit));
            const timer = window.setTimeout(() => {
                dismissToast(id);
            }, durationMs);
            toastTimersRef.current.set(id, timer);
        },
        [dismissToast]
    );

    const handleToastAction = useCallback(
        (toast: AppToast) => {
            dismissToast(toast.id);
            setShowCommandPalette(false);
            setIsSidebarOpen(false);
            if (toast.targetPanel !== "research") {
                setResearchSeedMemory(null);
            }
            if (toast.targetPanel) {
                setActivePanel(toast.targetPanel);
            }
        },
        [dismissToast]
    );

    // Global Cmd+K / Ctrl+K listener
    useEffect(() => {
        const handler = (e: KeyboardEvent) => {
            if ((e.metaKey || e.ctrlKey) && (e.key === "k" || e.key === "K")) {
                e.preventDefault();
                setShowCommandPalette((prev) => !prev);
            }
        };
        window.addEventListener("keydown", handler);
        return () => window.removeEventListener("keydown", handler);
    }, []);

    // Proactive suggestion listener — surfaces focus drift alerts as a toast.
    // Uses async inner function so the unlisten handle is guaranteed to be
    // assigned before any cleanup can run, avoiding a listener leak.
    useEffect(() => {
        let unlisten: (() => void) | null = null;
        let mounted = true;

        const subscribe = async () => {
            try {
                const fn = await onProactiveSuggestion((suggestion) => {
                    if (suggestion.memory_id === "focus_drift") {
                        enqueueToast({
                            title: "Focus Drift Detected",
                            body: suggestion.snippet,
                            kind: "focus_drift",
                            actionLabel: "View Focus Mode",
                            targetPanel: "focusMode",
                        });
                    }
                });
                if (mounted) {
                    unlisten = fn;
                } else {
                    fn(); // component already unmounted — immediately release
                }
            } catch {
                // non-fatal; proactive surface is best-effort
            }
        };

        void subscribe();
        return () => {
            mounted = false;
            if (unlisten) unlisten();
        };
    }, [enqueueToast]);

    useEffect(() => {
        let unlisten: (() => void) | null = null;
        let mounted = true;

        const actionForNotification = (
            notification: FndrNotificationPayload
        ): Pick<AppToast, "actionLabel" | "targetPanel"> => {
            switch (notification.kind) {
                case "briefing":
                    return { actionLabel: "Open Daily Summary", targetPanel: "dailySummary" };
                case "stale_tasks":
                    return { actionLabel: "Review Tasks", targetPanel: "todo" };
                case "context_switch":
                    return { actionLabel: "Open Focus Mode", targetPanel: "focusMode" };
                default:
                    return {};
            }
        };

        const subscribe = async () => {
            try {
                const fn = await onFndrNotification((notification) => {
                    const action = actionForNotification(notification);
                    enqueueToast({
                        title: notification.title,
                        body: notification.body,
                        kind: notification.kind,
                        actionLabel: action.actionLabel,
                        targetPanel: action.targetPanel,
                    });
                });
                if (mounted) {
                    unlisten = fn;
                } else {
                    fn();
                }
            } catch (error) {
                console.error("Failed to subscribe to FNDR notifications:", error);
            }
        };

        void subscribe();
        return () => {
            mounted = false;
            if (unlisten) unlisten();
        };
    }, [enqueueToast]);

    useEffect(() => {
        return () => {
            for (const timer of toastTimersRef.current.values()) {
                window.clearTimeout(timer);
            }
            toastTimersRef.current.clear();
        };
    }, []);

    useEffect(() => {
        if (!visibleResults.length) {
            setSelectedResult(null);
            return;
        }

        setSelectedResult((previous) => {
            if (!previous) {
                return visibleResults[0];
            }

            const stillVisible = visibleResults.find((item) => item.id === previous.id);
            return stillVisible ?? visibleResults[0];
        });
    }, [visibleResults]);

    const handleMemoryDeleted = (memoryId: string) => {
        setDeletedMemoryIds((previous) => {
            const next = new Set(previous);
            next.add(memoryId);
            return next;
        });
    };

    const handleDeleteMemory = async (memoryId: string) => {
        try {
            const deleted = await deleteMemory(memoryId);
            if (!deleted) {
                return;
            }
            handleMemoryDeleted(memoryId);
        } catch (err) {
            console.error("Failed to delete memory:", err);
        }
    };

    if (onboardingDone === null || biometricRequired === null) {
        return null;
    }

    if (!onboardingDone) {
        return (
            <Onboarding
                onComplete={(next: OnboardingState) => {
                    setOnboardingDone(true);
                    setDisplayName(next.display_name ?? null);
                    setBiometricRequired(next.biometric_enabled === true);
                    // If they just enabled biometrics, mark as already unlocked for this session
                    if (next.biometric_enabled) {
                        setBiometricUnlocked(true);
                    }
                }}
            />
        );
    }

    if (biometricRequired && !biometricUnlocked) {
        return (
            <BiometricLockScreen
                onUnlock={handleUnlock}
                onDisableBiometricLock={handleDisableBiometricLock}
            />
        );
    }

    return (
        <div className="app film-grain">
            {!EVAL_UI && (
                <button
                    type="button"
                    className="fndr-os-chrome-btn sidebar-toggle"
                    onClick={() => setIsSidebarOpen((prev) => !prev)}
                    aria-label={isSidebarOpen ? "Close sidebar" : "Open sidebar"}
                >
                    {isSidebarOpen ? (
                        <span aria-hidden="true">×</span>
                    ) : (
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" aria-hidden>
                            <path d="M4 7h16M4 12h16M4 17h16" strokeLinecap="round" />
                        </svg>
                    )}
                </button>
            )}

            <div className="top-right-control">
                <ControlPanel
                    status={status}
                    compact={true}
                    evalUi={EVAL_UI}
                    onOpenPanel={(panel) => {
                        setIsSidebarOpen(false);
                        setActivePanel(panel);
                    }}
                />
            </div>

            {!EVAL_UI && meetingStatus?.is_recording && (
                <div className="recording-consent-banner pending">
                    <strong>Recording Active</strong>
                    <span>{meetingStatus.current_title ?? "Meeting"}</span>
                </div>
            )}

            {status && !status.ai_model_available && <ModelDownloadBanner />}

            {!EVAL_UI && isSidebarOpen && (
                <button
                    className="sidebar-scrim"
                    onClick={() => setIsSidebarOpen(false)}
                    aria-label="Close sidebar overlay"
                />
            )}

            {!EVAL_UI && (
                <aside className={`left-sidebar ${isSidebarOpen ? "open" : ""}`}>
                    <div className="sidebar-brand"></div>

                    {SIDEBAR_GROUPS.map((group) => (
                        <div key={group.label} className="sidebar-group sidebar-actions">
                            <div className="sidebar-label">{group.label}</div>
                            {group.items.map(({ key, text }) => (
                                <button
                                    key={key}
                                    className={`ui-action-btn ${activePanel === key ? "active" : ""}`}
                                    onClick={() => {
                                        if (key === "research") setResearchSeedMemory(null);
                                        setActivePanel(activePanel === key ? null : key);
                                        setIsSidebarOpen(false);
                                    }}
                                >
                                    {text}
                                </button>
                            ))}
                        </div>
                    ))}

                    <div className="sidebar-group sidebar-actions">
                        <div className="sidebar-label">Commands</div>
                        <button
                            className="ui-action-btn"
                            onClick={() => {
                                setShowCommandPalette(true);
                                setIsSidebarOpen(false);
                            }}
                        >
                            Cmd+K Palette
                        </button>
                    </div>

                    <div className="sidebar-reel-footer">
                        <div className="sidebar-reel-meta">
                            <span className="sidebar-reel-date">
                                {new Date().toLocaleDateString("en-US", { month: "short", day: "numeric" }).toUpperCase()}
                            </span>
                            {status?.frames_captured != null && (
                                <span className="sidebar-reel-frames">
                                    {String(status.frames_captured).padStart(4, "0")} FR
                                </span>
                            )}
                        </div>
                        <div className="sidebar-reel-strip" aria-hidden="true">
                            <div className="sidebar-reel-inner" />
                        </div>
                    </div>
                </aside>
            )}

            <main className={`app-main ${isFocusMode ? "search-centered" : ""}`}>
                {isFocusMode ? (
                    <div className="home-hero-stage">
                        <HomeHero
                            userName={displayName}
                            now={now}
                            greeting={homeGreeting}
                            onHeroSearch={(q) => {
                                setQueryDraft(q);
                                void handleSearchSubmit(q);
                            }}
                            onEnterReel={() => { /* handled by search submit in focus mode */ }}
                            onEnterWorkMode={() => setActivePanel("focusMode")}
                        />
                        <section className={`search-shell ${query.trim() ? "is-active" : ""}`}>
                            <SearchBar
                                value={queryDraft}
                                submittedValue={query}
                                onChange={setQueryDraft}
                                onSubmit={(v) => void handleSearchSubmit(v)}
                                timeFilter={timeFilter}
                                onTimeFilterChange={setTimeFilter}
                                appFilter={appFilter}
                                onAppFilterChange={setAppFilter}
                                onSetMeetingPanelOpen={(open) => setActivePanel(open ? "meeting" : null)}
                                onSetMemoryCardsPanelOpen={(open) => setActivePanel(open ? "memoryCards" : null)}
                                onSetKnowledgeGraphPanelOpen={(open) => setActivePanel(open ? "knowledgeGraph" : null)}
                                appNames={appNames}
                                resultCount={visibleResults.length}
                                searchResults={visibleResults}
                                disabled={!searchAllowed}
                            />
                        </section>
                    </div>
                ) : (
                    <section className={`search-shell ${query.trim() ? "is-active" : ""}`}>
                        <SearchBar
                                value={queryDraft}
                                submittedValue={query}
                                onChange={setQueryDraft}
                                onSubmit={(v) => void handleSearchSubmit(v)}
                                timeFilter={timeFilter}
                                onTimeFilterChange={setTimeFilter}
                                appFilter={appFilter}
                                onAppFilterChange={setAppFilter}
                                onSetMeetingPanelOpen={(open) => setActivePanel(open ? "meeting" : null)}
                                onSetMemoryCardsPanelOpen={(open) => setActivePanel(open ? "memoryCards" : null)}
                                onSetKnowledgeGraphPanelOpen={(open) => setActivePanel(open ? "knowledgeGraph" : null)}
                            appNames={appNames}
                            resultCount={visibleResults.length}
                            searchResults={visibleResults}
                            disabled={!searchAllowed}
                        />
                    </section>
                )}

                {!isFocusMode && (
                    <div className="main-layout">
                        <section className="main-column">
                            {error && <div className="error-banner">{error}</div>}

                            <Timeline
                                results={visibleResults}
                                isLoading={isLoading}
                                query={query}
                                selectedResultId={selectedResult?.id ?? null}
                                onSelectResult={setSelectedResult}
                                onDeleteMemory={(memoryId) => void handleDeleteMemory(memoryId)}
                                evalUi={EVAL_UI}
                            />
                        </section>
                    </div>
                )}
            </main>

            {!EVAL_UI && (
                <AppPanels
                    activePanel={activePanel}
                    appFilter={appFilter}
                    appNames={appNames}
                    appToasts={appToasts}
                    isCapturing={status?.is_capturing ?? false}
                    query={query}
                    researchSeedMemory={researchSeedMemory}
                    selectedResult={selectedResult}
                    showCommandPalette={showCommandPalette}
                    timeFilter={timeFilter}
                    onClearSearch={() => {
                        setQuery("");
                        setQueryDraft("");
                        setTimeFilter(null);
                        setAppFilter(null);
                    }}
                    onCloseCommandPalette={() => setShowCommandPalette(false)}
                    onClosePanel={() => setActivePanel(null)}
                    onDeleteMemory={handleMemoryDeleted}
                    onDismissToast={dismissToast}
                    onMemoryDeleted={handleMemoryDeleted}
                    onOpenPanel={handleOpenPanel}
                    onResearchMemory={handleResearchMemory}
                    onRunQuery={handleSearchSubmit}
                    onRunSkill={handleRunSkill}
                    onSearchApp={handleSearchApp}
                    onToastAction={handleToastAction}
                    memoryVaultFocusId={memoryVaultFocusId}
                    onOpenMemoryById={handleOpenMemoryById}
                />
            )}

        </div>
    );
}

export default App;
