import { useEffect, useLayoutEffect, useMemo, useState, useCallback, useRef } from "react";
import { AppPanels } from "./AppPanels";
import { isMountedPanelKey, type MountedPanelKey } from "./panels";
import { BiometricLockScreen } from "./BiometricLockScreen";
import { HomeHero } from "./HomeHero";
import { MeetingBriefCard } from "./MeetingBriefCard";
import { ResumeWork } from "./ResumeWork";
import type { AppToast } from "./types";
import { SearchBar } from "@/domains/search/SearchBar";
import { Timeline } from "@/domains/timeline/Timeline";
import { ControlPanel } from "@/domains/workspace/ControlPanel";
import { ModelDownloadBanner } from "@/domains/workspace/ModelDownloadBanner";
import { Onboarding } from "@/domains/workspace/Onboarding";
import { appendToSearchHistory } from "@/domains/workspace/SearchHistoryPanel";
import { type PanelKey } from "@/domains/command-palette/CommandPalette";
import "@/domains/workspace/FocusModePanel.css";

import { useSearch } from "@/shared/hooks/useSearch";
import { usePolling } from "@/shared/hooks/usePolling";
import { useTauriEvent } from "@/shared/hooks/useTauriEvent";
import { POLL_INTERVALS, STORAGE_KEYS, TOAST } from "@/shared/utils/config";
import { createClientId } from "@/shared/utils/id";
import {
    CAPTURE_STATUS_EVENT,
    CaptureStatus,
    OMNIBAR_OPEN_MEMORY_EVENT,
    type FndrNotificationPayload,
    MeetingRecorderStatus,
    MemoryCard,
    getAppNames,
    getMeetingStatus,
    onMeetingStatus,
    onFndrNotification,
    onProactiveSuggestion,
    getStatus,
    getFunGreeting,
} from "@/shared/ipc/tauri";
import { getOnboardingState, type OnboardingState } from "@/shared/ipc/onboarding";
import { EVAL_UI } from "@/shared/utils/eval-ui";
import { SidebarDrawer } from "./SidebarDrawer";
import { ActivityTrace } from "@/shared/components/ActivityTrace";
import "./styles/App.css";

function nextToastId(): string {
    return createClientId("fndr-toast");
}

type SidebarItem =
    | { text: string; key: MountedPanelKey }
    | { text: string; action: "home" | "settings" };

const PRIMARY_SIDEBAR_ITEMS = [
    { action: "home", text: "Home" },
    { key: "ask", text: "Search & Ask FNDR" },
    { key: "memoryCards", text: "Memory Vault" },
    { key: "dailySummary", text: "Daily Brief" },
    { action: "settings", text: "Trust & Settings" },
] as const satisfies readonly SidebarItem[];

const LAB_SIDEBAR_ITEMS = [
    { key: "screenGuide", text: "Screen Guide" },
    { key: "agent", text: "Hermes Agent" },
    { key: "outputs", text: "Daily outputs" },
    { key: "stats", text: "Stats" },
    { key: "todo", text: "To-dos" },
    { key: "wrapped", text: "FNDR Wrapped" },
    { key: "privacyProof", text: "Privacy Activity" },
] as const satisfies readonly SidebarItem[];

/** Visible navigation follows ADR 023. Engine diagnostics is intentionally
 * absent until someone explicitly enables Developer Mode in Settings. */
export function getSidebarGroups(developerMode: boolean) {
    const labs = developerMode
        ? [...LAB_SIDEBAR_ITEMS, { key: "engineMetrics" as const, text: "Engine diagnostics" }]
        : LAB_SIDEBAR_ITEMS;
    return [
        { label: "Navigate", items: PRIMARY_SIDEBAR_ITEMS },
        { label: "Labs (experimental)", items: labs },
    ] as const;
}

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
    const [activePanel, setActivePanel] = useState<MountedPanelKey | null>(null);
    const [memoryVaultFocusId, setMemoryVaultFocusId] = useState<string | null>(null);
    const [showCommandPalette, setShowCommandPalette] = useState(false);
    const [isSettingsOpen, setIsSettingsOpen] = useState(false);
    const [developerMode, setDeveloperMode] = useState(
        () => localStorage.getItem(STORAGE_KEYS.developerMode) === "true",
    );
    const [appToasts, setAppToasts] = useState<AppToast[]>([]);
    const toastTimersRef = useRef<Map<string, number>>(new Map());
    const sidebarButtonRef = useRef<HTMLButtonElement>(null);
    const backgroundLayerRef = useRef<HTMLDivElement>(null);

    const [onboardingDone, setOnboardingDone] = useState<boolean | null>(null);
    const [biometricRequired, setBiometricRequired] = useState<boolean | null>(null);
    const [biometricUnlocked, setBiometricUnlocked] = useState(false);
    const [selectedResult, setSelectedResult] = useState<MemoryCard | null>(null);
    const [isSidebarOpen, setIsSidebarOpen] = useState(false);
    const [deletedMemoryIds, setDeletedMemoryIds] = useState<Set<string>>(new Set());
    const [displayName, setDisplayName] = useState<string | null>(null);
    const [now, setNow] = useState(() => new Date());
    const handleUnlock = useCallback(() => setBiometricUnlocked(true), []);
    const sidebarGroups = useMemo(() => getSidebarGroups(developerMode), [developerMode]);

    const handleDeveloperModeChange = useCallback((enabled: boolean) => {
        localStorage.setItem(STORAGE_KEYS.developerMode, String(enabled));
        setDeveloperMode(enabled);
    }, []);

    const searchAllowed = true;
    const { results, isLoading, error, activityTrace: searchActivityTrace } = useSearch(
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
                setOnboardingDone(s.step === "complete");
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
    const isHomeActive = activePanel === null && isFocusMode;

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

    useEffect(() => {
        let mounted = true;
        getStatus()
            .then((nextStatus) => {
                if (mounted) {
                    setStatus(nextStatus);
                }
            })
            .catch((e) => console.error("Failed to get status:", e));
        return () => {
            mounted = false;
        };
    }, []);
    useTauriEvent<CaptureStatus>(CAPTURE_STATUS_EVENT, setStatus);

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

    // Run a search for a specific app (from Focus Session panel)
    const handleSearchApp = (appName: string) => {
        setAppFilter(appName);
        setQueryDraft("");
        setQuery(" "); // trigger search with only app filter
    };

    // Command Palette panel dispatcher — opens any panel by key
    const handleOpenPanel = useCallback((panel: PanelKey) => {
        if (!isMountedPanelKey(panel)) {
            console.warn(`Ignored unmounted Alpha panel destination: ${panel}`);
            return;
        }
        setShowCommandPalette(false);
        setIsSidebarOpen(false);
        if (panel !== "memoryCards") {
            setMemoryVaultFocusId(null);
        }
        setActivePanel(panel);
    }, []);

    const closeActivePanel = useCallback(() => {
        setActivePanel(null);
        window.setTimeout(() => sidebarButtonRef.current?.focus(), 0);
    }, []);

    const handleOpenMemoryById = useCallback((memoryId: string) => {
        setMemoryVaultFocusId(memoryId);
        setShowCommandPalette(false);
        setIsSidebarOpen(false);
        setActivePanel("memoryCards");
    }, []);

    useTauriEvent<string>(OMNIBAR_OPEN_MEMORY_EVENT, handleOpenMemoryById);

    const goHome = useCallback(() => {
        setActivePanel(null);
        setShowCommandPalette(false);
        setIsSidebarOpen(false);
        setQuery("");
        setQueryDraft("");
        setTimeFilter(null);
        setAppFilter(null);
        window.setTimeout(() => sidebarButtonRef.current?.focus(), 0);
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
            if (toast.targetPanel === "memoryCards" && toast.memoryId) {
                setMemoryVaultFocusId(toast.memoryId);
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
                if (
                    onboardingDone !== true ||
                    (biometricRequired === true && !biometricUnlocked) ||
                    document.getElementById("fndr-settings-panel")
                ) {
                    return;
                }
                e.preventDefault();
                setShowCommandPalette((prev) => !prev);
            }
        };
        window.addEventListener("keydown", handler);
        return () => window.removeEventListener("keydown", handler);
    }, [biometricRequired, biometricUnlocked, onboardingDone]);

    // Proactive suggestion listener — surfaces focus drift alerts and
    // resurfaced memories as toasts.
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
                        });
                        return;
                    }
                    enqueueToast({
                        title: suggestion.task_title ?? "From your memory",
                        body: suggestion.snippet,
                        kind: "proactive_memory",
                        actionLabel: "Open memory",
                        targetPanel: "memoryCards",
                        memoryId: suggestion.memory_id,
                    });
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
                    return { actionLabel: "Review the day", targetPanel: "dailySummary" };
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

    const hasForegroundOverlay = activePanel !== null || showCommandPalette;
    useLayoutEffect(() => {
        const backgroundLayer = backgroundLayerRef.current;
        if (!backgroundLayer) return;

        let settingsWasOpen = false;
        const syncBackgroundIsolation = () => {
            const settingsOpen = document.getElementById("fndr-settings-panel") !== null;
            if (hasForegroundOverlay || settingsOpen) {
                backgroundLayer.setAttribute("inert", "");
            } else {
                backgroundLayer.removeAttribute("inert");
            }

            if (settingsWasOpen && !settingsOpen && !hasForegroundOverlay) {
                backgroundLayer
                    .querySelector<HTMLButtonElement>('[aria-controls="fndr-settings-panel"]')
                    ?.focus();
            }
            settingsWasOpen = settingsOpen;
        };

        syncBackgroundIsolation();
        const foregroundObserver = new MutationObserver(syncBackgroundIsolation);
        foregroundObserver.observe(document.body, { childList: true });
        return () => foregroundObserver.disconnect();
    }, [biometricRequired, biometricUnlocked, hasForegroundOverlay, onboardingDone]);

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
        return <BiometricLockScreen onUnlock={handleUnlock} />;
    }

    return (
        <div className="app">
            <div ref={backgroundLayerRef} className="app-background-layer">
                {!EVAL_UI && (
                    <button
                        ref={sidebarButtonRef}
                        type="button"
                        className="fndr-os-chrome-btn sidebar-toggle"
                        onClick={() => setIsSidebarOpen((prev) => !prev)}
                        aria-label={isSidebarOpen ? "Close sidebar" : "Open sidebar"}
                        aria-expanded={isSidebarOpen}
                        aria-controls="primary-navigation"
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
                        isOpen={isSettingsOpen}
                        onOpenChange={setIsSettingsOpen}
                        developerMode={developerMode}
                        onDeveloperModeChange={handleDeveloperModeChange}
                    />
                </div>

                {status && !status.ai_model_available && <ModelDownloadBanner />}

                {!EVAL_UI && (
                <SidebarDrawer
                    isOpen={isSidebarOpen}
                    onClose={() => {
                        setIsSidebarOpen(false);
                        sidebarButtonRef.current?.focus();
                    }}
                >
                    <div className="sidebar-brand"></div>

                    {sidebarGroups.map((group) => (
                        <div key={group.label} className="sidebar-group">
                            <div className="sidebar-label">{group.label}</div>
                            {group.items.map((item) => {
                                if ("action" in item) {
                                    const isHome = item.action === "home";
                                    return (
                                        <button
                                            key={item.action}
                                            type="button"
                                            className={`sidebar-item ${isHome && isHomeActive ? "active" : ""}`}
                                            aria-current={isHome && isHomeActive ? "page" : undefined}
                                            onClick={() => {
                                                if (isHome) {
                                                    goHome();
                                                } else {
                                                    setIsSettingsOpen(true);
                                                    setIsSidebarOpen(false);
                                                }
                                            }}
                                        >
                                            {item.text}
                                        </button>
                                    );
                                }

                                const { key, text } = item;
                                return (
                                    <button
                                        key={key}
                                        type="button"
                                        className={`sidebar-item ${activePanel === key ? "active" : ""}`}
                                        aria-current={activePanel === key ? "page" : undefined}
                                        onClick={() => {
                                            if (activePanel === key) {
                                                closeActivePanel();
                                            } else {
                                                handleOpenPanel(key);
                                            }
                                        }}
                                    >
                                        {text}
                                    </button>
                                );
                            })}
                        </div>
                    ))}

                    <div className="sidebar-group">
                        <div className="sidebar-label">Commands</div>
                        <button
                            type="button"
                            className="sidebar-item"
                            onClick={() => {
                                sidebarButtonRef.current?.focus();
                                setShowCommandPalette(true);
                                setIsSidebarOpen(false);
                            }}
                        >
                            Command Palette
                            <kbd className="sidebar-item-shortcut">⌘K</kbd>
                        </button>
                    </div>

                    {status?.frames_captured != null && (
                        <div className="sidebar-footer">
                            {status.frames_captured.toLocaleString("en-US")} frames captured
                        </div>
                    )}
                </SidebarDrawer>
            )}

                <main className={`app-main ${isFocusMode ? "search-centered" : "has-active-search"}${isHomeActive && !EVAL_UI ? " has-resume" : ""}`}>
                {isFocusMode ? (
                    <div className="home-hero-stage">
                        <div className="home-hero-fold">
                            <HomeHero
                                userName={displayName}
                                now={now}
                                greeting={homeGreeting}
                                onHeroSearch={(q) => {
                                    setQueryDraft(q);
                                    void handleSearchSubmit(q);
                                }}
                            />
                        </div>
                        {isHomeActive && !EVAL_UI && (
                            <MeetingBriefCard onOpenMemory={handleOpenMemoryById} />
                        )}
                        {isHomeActive && !EVAL_UI && (
                            <ResumeWork
                                onOpenMemory={handleOpenMemoryById}
                                onOpenVault={() => handleOpenPanel("memoryCards")}
                            />
                        )}
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
                                onSetMemoryCardsPanelOpen={(open) => setActivePanel(open ? "memoryCards" : null)}
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
                            {searchActivityTrace && (
                                <div className="search-activity-trace-wrap">
                                    <ActivityTrace trace={searchActivityTrace} />
                                </div>
                            )}
                            {error && <div className="error-banner">{error}</div>}

                            <Timeline
                                results={visibleResults}
                                isLoading={isLoading}
                                query={query}
                                selectedResultId={selectedResult?.id ?? null}
                                onSelectResult={setSelectedResult}
                                evalUi={EVAL_UI}
                            />
                        </section>
                    </div>
                )}
                </main>
            </div>

            {!EVAL_UI && meetingStatus?.is_recording && (
                <div className="recording-consent-banner pending" role="status" aria-live="polite">
                    <strong>Recording Active</strong>
                    <span>{meetingStatus.current_title ?? "Meeting"}</span>
                </div>
            )}

            {!EVAL_UI && (
                <AppPanels
                    activePanel={activePanel}
                    appNames={appNames}
                    appToasts={appToasts}
                    developerMode={developerMode}
                    isCapturePaused={(status?.is_paused ?? false) || (status?.is_incognito ?? false)}
                    isPrivateMode={status?.is_incognito ?? false}
                    query={query}
                    selectedResult={selectedResult}
                    showCommandPalette={showCommandPalette}
                    onClearSearch={() => {
                        setQuery("");
                        setQueryDraft("");
                        setTimeFilter(null);
                        setAppFilter(null);
                    }}
                    onCloseCommandPalette={() => setShowCommandPalette(false)}
                    onClosePanel={closeActivePanel}
                    onDeleteMemory={handleMemoryDeleted}
                    onDismissToast={dismissToast}
                    onGoHome={goHome}
                    onOpenSettings={() => setIsSettingsOpen(true)}
                    onMemoryDeleted={handleMemoryDeleted}
                    onOpenPanel={handleOpenPanel}
                    onRunQuery={handleSearchSubmit}
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
