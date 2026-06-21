import { useCallback, useEffect, useRef, useState } from "react";

type Theme = "dark" | "light";
import {
    AutofillSettings,
    CaptureStatus,
    ContextRuntimeStatus,
    MemoryRepairProgress,
    MemoryRepairSummary,
    PRIVACY_ALERTS_EVENT,
    PrivacyAlert,
    StorageHealth,
    StorageReclaimProgress,
    StorageReclaimSummary,
    McpServerStatus,
    deleteAllData,
    deleteOlderThan,
    getBlocklist,
    getMemoryRepairProgress,
    getStorageHealth,
    getStorageReclaimProgress,
    getMcpServerStatus,
    getRetentionDays,
    getAutofillSettings,
    getContextRuntimeStatus,
    pauseCapture,
    resumeCapture,
    setBlocklist,
    setAutofillSettings,
    setRetentionDays,
    startMcpServer,
    stopMcpServer,
    runMemoryRepairBackfill,
    reclaimMemoryStorage,
    getPrivacyAlerts,
    cleanDevBuildCache,
    continuumQualityStatus,
} from "@/shared/ipc/tauri";
import {
    ModelInfo,
    OnboardingState,
    getOnboardingState,
    deleteAiModel,
    downloadModel,
    listAvailableModels,
    refreshAiModels,
    saveOnboardingState,
} from "@/shared/ipc/onboarding";
import {
    CloudIdentity,
    CloudStatus,
    cloudCreateCluster,
    cloudGetIdentity,
    cloudJoinCluster,
    cloudSignOut,
    cloudStatus,
    cloudSyncNow,
} from "@/shared/ipc/cloud";
import { useModelDownloadStatus } from "@/shared/hooks/useModelDownloadStatus";
import { usePolling } from "@/shared/hooks/usePolling";
import { useTauriEvent } from "@/shared/hooks/useTauriEvent";
import { STORAGE_KEYS } from "@/shared/utils/config";
import { formatBytes } from "@/shared/utils/format";
import { Icon } from "@/shared/components/atoms";
import {
    PALETTES,
    applyPalette,
    isPaletteKey,
    listPalettes,
    type PaletteKey,
} from "@/shared/theme/cinematic-palettes";
import {
    WALLPAPERS,
    isWallpaperId,
    listWallpapers,
    type WallpaperId,
} from "@/shared/wallpaper/wallpaper-registry";
import "./ControlPanel.css";
import { PrivacyPanel } from "./PrivacyPanel";

interface ControlPanelProps {
    status: CaptureStatus | null;
    compact?: boolean;
    /** Hide MCP and emphasize core privacy when true (VITE_EVAL_UI build). */
    evalUi?: boolean;
    /** Compatibility: caller may still pass a panel opener; settings no longer renders this section. */
    onOpenPanel?: (panel: PanelKey) => void;
}

type Tab = "settings" | "model" | "privacy";

const DEFAULT_AUTOFILL_SETTINGS: AutofillSettings = {
    enabled: true,
    shortcut: "Alt+F",
    lookback_days: 90,
    auto_inject_threshold: 0.9,
    prefer_typed_injection: true,
    max_candidates: 4,
};

export function ControlPanel({
    status,
    compact: _compact = false,
    evalUi = false,
    onOpenPanel: _onOpenPanel,
}: ControlPanelProps) {
    const [isOpen, setIsOpen] = useState(false);
    const [isAppearanceOpen, setIsAppearanceOpen] = useState(false);
    const [activeTab, setActiveTab] = useState<Tab>("settings");
    const [blocklist, setBlocklistState] = useState<string[]>([]);
    const [privacyAlertCount, setPrivacyAlertCount] = useState(0);
    const [newApp, setNewApp] = useState("");
    const [confirmDelete, setConfirmDelete] = useState(false);
    const [retentionDays, setRetentionDaysState] = useState<number>(7);
    const [retentionBusy, setRetentionBusy] = useState(false);
    const [mcpStatus, setMcpStatus] = useState<McpServerStatus | null>(null);
    const [contextRuntimeStatus, setContextRuntimeStatus] = useState<ContextRuntimeStatus | null>(null);
    const [mcpBusy, setMcpBusy] = useState(false);
    const [copiedMcpLink, setCopiedMcpLink] = useState(false);
    const [profileName, setProfileName] = useState("");
    const [profileDraft, setProfileDraft] = useState("");
    const [profileBusy, setProfileBusy] = useState(false);
    const [profileMsg, setProfileMsg] = useState<string | null>(null);
    const [repairBusy, setRepairBusy] = useState(false);
    const [repairSummary, setRepairSummary] = useState<MemoryRepairSummary | null>(null);
    const [repairError, setRepairError] = useState<string | null>(null);
    const [repairProgress, setRepairProgress] = useState<MemoryRepairProgress | null>(null);
    const [reclaimBusy, setReclaimBusy] = useState(false);
    const [reclaimSummary, setReclaimSummary] = useState<StorageReclaimSummary | null>(null);
    const [reclaimProgress, setReclaimProgress] = useState<StorageReclaimProgress | null>(null);
    const [reclaimError, setReclaimError] = useState<string | null>(null);
    const [storageHealth, setStorageHealth] = useState<StorageHealth | null>(null);
    const [devCacheBusy, setDevCacheBusy] = useState(false);
    const [devCacheError, setDevCacheError] = useState<string | null>(null);
    const [autofillSettings, setAutofillSettingsState] =
        useState<AutofillSettings>(DEFAULT_AUTOFILL_SETTINGS);
    const [savedAutofillSettings, setSavedAutofillSettingsState] =
        useState<AutofillSettings>(DEFAULT_AUTOFILL_SETTINGS);
    const [autofillBusy, setAutofillBusy] = useState(false);
    const [autofillMsg, setAutofillMsg] = useState<string | null>(null);
    const [qualityStatus, setQualityStatus] = useState<{
        stored_count: number;
        dropped_count: number;
        flagged_count: number;
    } | null>(null);
    const prevPrivacyAlertCountRef = useRef(0);

    // Theme state
    const [theme, setTheme] = useState<Theme>(() => {
        return (localStorage.getItem(STORAGE_KEYS.theme) as Theme) || "dark";
    });
    const [paletteKey, setPaletteKey] = useState<PaletteKey>(() => {
        const stored = localStorage.getItem(STORAGE_KEYS.palette);
        return isPaletteKey(stored) ? stored : "matrix";
    });
    const [wallpaperId, setWallpaperId] = useState<WallpaperId>(() => {
        const stored = localStorage.getItem(STORAGE_KEYS.wallpaper);
        return isWallpaperId(stored) ? stored : "aurora";
    });

    // Model tab state
    const [models, setModels] = useState<ModelInfo[]>([]);
    const [modelsLoading, setModelsLoading] = useState(false);
    const [downloadingId, setDownloadingId] = useState<string | null>(null);
    const [modelError, setModelError] = useState<string | null>(null);
    const [confirmDeleteModel, setConfirmDeleteModel] = useState<string | null>(null);
    const [isActivatingModel, setIsActivatingModel] = useState(false);
    const downloadStatus = useModelDownloadStatus();

    // Cloud account state
    const [cloudAccount, setCloudAccount] = useState<CloudStatus | null>(null);
    const [cloudIdentity, setCloudIdentity] = useState<CloudIdentity | null>(null);
    const [cloudSigningOut, setCloudSigningOut] = useState(false);
    const [cloudError, setCloudError] = useState<string | null>(null);
    const [syncingNow, setSyncingNow] = useState(false);
    const [syncNowMsg, setSyncNowMsg] = useState<string | null>(null);
    // Workspace (cluster) create / join
    const [clusterNameDraft, setClusterNameDraft] = useState("");
    const [joinCodeDraft, setJoinCodeDraft] = useState("");
    const [clusterBusy, setClusterBusy] = useState(false);
    const [clusterMsg, setClusterMsg] = useState<string | null>(null);
    const [newJoinCode, setNewJoinCode] = useState<string | null>(null);

    const loadData = useCallback(async () => {
        try {
            if (evalUi) {
                const [bl, ret, onboarding, health, autofill, quality] = await Promise.all([
                    getBlocklist(),
                    getRetentionDays(),
                    getOnboardingState(),
                    getStorageHealth(),
                    getAutofillSettings(),
                    continuumQualityStatus(),
                ]);
                setBlocklistState(bl);
                setRetentionDaysState(ret);
                setStorageHealth(health);
                setQualityStatus(quality);
                const name = onboarding.display_name ?? "";
                setProfileName(name);
                setProfileDraft(name);
                setAutofillSettingsState(autofill);
                setSavedAutofillSettingsState(autofill);
            } else {
                const [bl, ret, mcp, runtimeStatus, onboarding, health, autofill, quality] = await Promise.all([
                    getBlocklist(),
                    getRetentionDays(),
                    getMcpServerStatus(),
                    getContextRuntimeStatus(),
                    getOnboardingState(),
                    getStorageHealth(),
                    getAutofillSettings(),
                    continuumQualityStatus(),
                ]);
                setBlocklistState(bl);
                setRetentionDaysState(ret);
                setMcpStatus(mcp);
                setContextRuntimeStatus(runtimeStatus);
                setStorageHealth(health);
                setQualityStatus(quality);
                const name = onboarding.display_name ?? "";
                setProfileName(name);
                setProfileDraft(name);
                setAutofillSettingsState(autofill);
                setSavedAutofillSettingsState(autofill);
            }
        } catch (err) {
            console.error("Failed to load settings data:", err);
        }
    }, [evalUi]);

    useEffect(() => {
        if (isOpen) {
            void loadData();
        }
    }, [isOpen, loadData]);

    const refreshStorage = useCallback(async (isMounted: () => boolean) => {
        try {
            const health = await getStorageHealth();
            if (isMounted()) {
                setStorageHealth(health);
            }
        } catch {
            // Storage health is informational; keep the previous value on transient failures.
        }
    }, []);
    usePolling(refreshStorage, 15000, isOpen && activeTab === "settings");

    const loadModels = useCallback(async () => {
        setModelsLoading(true);
        try {
            const ms = await listAvailableModels();
            setModels(ms);
        } catch (e) {
            setModelError(String(e));
        } finally {
            setModelsLoading(false);
        }
    }, []);

    useEffect(() => {
        let mounted = true;
        getPrivacyAlerts()
            .then((alerts) => {
                if (mounted) {
                    setPrivacyAlertCount(alerts.length);
                }
            })
            .catch((err) => console.error("Failed fetching alerts:", err));
        return () => {
            mounted = false;
        };
    }, []);
    useTauriEvent<PrivacyAlert[]>(PRIVACY_ALERTS_EVENT, (alerts) =>
        setPrivacyAlertCount(alerts.length)
    );

    useEffect(() => {
        let mounted = true;
        cloudStatus()
            .then((account) => {
                if (!mounted) return;
                setCloudAccount(account);
                if (account.configured && account.signed_in) {
                    cloudGetIdentity()
                        .then((identity) => {
                            if (mounted) setCloudIdentity(identity);
                        })
                        .catch((err) => console.error("Failed to load cloud identity:", err));
                }
            })
            .catch((err) => console.error("Failed to load cloud status:", err));
        return () => {
            mounted = false;
        };
    }, []);

    useEffect(() => {
        const previous = prevPrivacyAlertCountRef.current;
        if (privacyAlertCount > 0 && previous === 0) {
            setIsOpen(true);
            setIsAppearanceOpen(false);
            setActiveTab("privacy");
        }
        prevPrivacyAlertCountRef.current = privacyAlertCount;
    }, [privacyAlertCount]);

    useEffect(() => {
        if (isOpen && activeTab === "model") {
            void loadModels();
        }
    }, [isOpen, activeTab, loadModels]);

    // Close on escape
    useEffect(() => {
        const handleEscape = (e: KeyboardEvent) => {
            if (e.key === "Escape") {
                setIsOpen(false);
                setIsAppearanceOpen(false);
            }
        };
        if (isOpen || isAppearanceOpen) {
            window.addEventListener("keydown", handleEscape);
            return () => window.removeEventListener("keydown", handleEscape);
        }
    }, [isAppearanceOpen, isOpen]);

    // Apply theme to document root.
    // film-paper.css now responds to both "dark"/"film" (dark mode) and
    // "light"/"paper" (light mode) so we can set the label as-is.
    useEffect(() => {
        document.documentElement.setAttribute("data-theme", theme);
        localStorage.setItem(STORAGE_KEYS.theme, theme);
        localStorage.setItem(STORAGE_KEYS.palette, paletteKey);
        localStorage.setItem(STORAGE_KEYS.wallpaper, wallpaperId);
        applyPalette(paletteKey, theme);
    }, [paletteKey, theme, wallpaperId]);

    const selectAppearance = (nextPalette: PaletteKey, nextTheme: Theme, nextWallpaper?: WallpaperId) => {
        setPaletteKey(nextPalette);
        setTheme(nextTheme);
        if (nextWallpaper) setWallpaperId(nextWallpaper);
        window.dispatchEvent(
            new CustomEvent("continuum-appearance-changed", {
                detail: {
                    palette: nextPalette,
                    mode: nextTheme,
                    wallpaper: nextWallpaper ?? wallpaperId,
                },
            })
        );
    };

    const selectWallpaper = (nextWallpaper: WallpaperId) => {
        setWallpaperId(nextWallpaper);
        localStorage.setItem(STORAGE_KEYS.wallpaper, nextWallpaper);
        window.dispatchEvent(
            new CustomEvent("continuum-appearance-changed", {
                detail: { palette: paletteKey, mode: theme, wallpaper: nextWallpaper },
            })
        );
    };

    useEffect(() => {
        if (!downloadingId || downloadStatus.model_id !== downloadingId) {
            return;
        }

        if (downloadStatus.state === "failed" && downloadStatus.error) {
            setModelError(downloadStatus.error);
            setDownloadingId(null);
            void loadModels();
            return;
        }

        if (downloadStatus.state !== "completed" || downloadStatus.error) {
            return;
        }

        let cancelled = false;
        setDownloadingId(null);
        setIsActivatingModel(true);

        void (async () => {
            try {
                const runtime = await refreshAiModels();
                if (!runtime.ai_model_available && !cancelled) {
                    setModelError(
                        `Model download finished, but Continuum still cannot see the files at ${downloadStatus.destination_path ?? "disk"}.`,
                    );
                }
            } catch (refreshError) {
                if (!cancelled) {
                    setModelError(`Model downloaded, but Continuum failed to refresh the runtime: ${String(refreshError)}`);
                }
            } finally {
                if (!cancelled) {
                    setIsActivatingModel(false);
                    void loadModels();
                }
            }
        })();

        return () => {
            cancelled = true;
        };
    }, [downloadStatus.destination_path, downloadStatus.error, downloadStatus.model_id, downloadStatus.state, downloadingId, loadModels]);

    const handleDownloadModel = async (model: ModelInfo) => {
        if (downloadingId) return;
        setModelError(null);

        if (model.download_url === "already_downloaded") {
            setIsActivatingModel(true);
            try {
                const runtime = await refreshAiModels();
                if (!runtime.ai_model_available) {
                    setModelError("The model file should be on disk, but Continuum could not find it in the models folder.");
                }
            } catch (e) {
                setModelError(String(e));
            } finally {
                setIsActivatingModel(false);
                await loadModels();
            }
            return;
        }

        setDownloadingId(model.id);
        try {
            await downloadModel(model.id, model.download_url, model.filename);
        } catch (e) {
            setModelError(String(e));
            setDownloadingId(null);
        }
    };

    const handleDeleteModel = async (model: ModelInfo) => {
        if (confirmDeleteModel !== model.id) {
            setConfirmDeleteModel(model.id);
            return;
        }
        setConfirmDeleteModel(null);
        setModelError(null);
        try {
            await deleteAiModel(model.filename);
            await loadModels();
        } catch (e) {
            setModelError(String(e));
        }
    };

    const handleRetentionChange = async (days: number) => {
        try {
            await setRetentionDays(days);
            setRetentionDaysState(days);
        } catch (e) {
            console.error("Failed to set retention:", e);
        }
    };

    const handleRunRetentionNow = async () => {
        if (retentionDays === 0) return;
        setRetentionBusy(true);
        try {
            await deleteOlderThan(retentionDays);
        } catch (e) {
            console.error("Failed to run retention:", e);
        } finally {
            setRetentionBusy(false);
        }
    };

    const handleToggleCapture = async () => {
        try {
            if (status?.is_paused) {
                await resumeCapture();
            } else {
                await pauseCapture();
            }
        } catch (e) {
            console.error("Failed to toggle capture:", e);
        }
    };

    const handleAddApp = async () => {
        if (!newApp.trim()) return;
        const updated = [...blocklist, newApp.trim()];
        try {
            await setBlocklist(updated);
            setBlocklistState(updated);
            setNewApp("");
        } catch (e) {
            console.error("Failed to update blocklist:", e);
        }
    };

    const handleRemoveApp = async (app: string) => {
        const updated = blocklist.filter((a) => a !== app);
        try {
            await setBlocklist(updated);
            setBlocklistState(updated);
        } catch (e) {
            console.error("Failed to update blocklist:", e);
        }
    };

    const handleDeleteAll = async () => {
        if (!confirmDelete) {
            setConfirmDelete(true);
            return;
        }
        try {
            await deleteAllData();
            setConfirmDelete(false);
        } catch (e) {
            console.error("Failed to delete data:", e);
        }
    };

    const handleRunRepairBackfill = async () => {
        setRepairBusy(true);
        setRepairError(null);
        setRepairSummary(null);
        try {
            const summary = await runMemoryRepairBackfill();
            setRepairSummary(summary);
        } catch (e) {
            setRepairError(String(e));
        } finally {
            setRepairBusy(false);
        }
    };

    const handleReclaimStorage = async () => {
        setReclaimBusy(true);
        setReclaimError(null);
        setReclaimSummary(null);
        setReclaimProgress({
            is_running: true,
            phase: "starting",
            processed: 0,
            total: 0,
            records_rewritten: 0,
            screenshot_paths_cleared: 0,
            screenshot_files_deleted: 0,
            embeddings_refreshed: 0,
            snippet_embeddings_refreshed: 0,
            support_embeddings_refreshed: 0,
            timestamp_ms: Date.now(),
        });
        try {
            const summary = await reclaimMemoryStorage();
            setReclaimSummary(summary);
            try {
                setStorageHealth(await getStorageHealth());
            } catch {
                // Keep the reclaim result visible even if the health refresh fails.
            }
            setReclaimProgress({
                is_running: false,
                phase: "complete",
                processed: summary.records_scanned,
                total: summary.records_scanned,
                records_rewritten: summary.records_rewritten,
                screenshot_paths_cleared: summary.screenshot_paths_cleared,
                screenshot_files_deleted: summary.screenshot_files_deleted,
                embeddings_refreshed: summary.embeddings_refreshed,
                snippet_embeddings_refreshed: summary.snippet_embeddings_refreshed,
                support_embeddings_refreshed: summary.support_embeddings_refreshed,
                timestamp_ms: Date.now(),
            });
        } catch (e) {
            setReclaimError(String(e));
        } finally {
            setReclaimBusy(false);
        }
    };

    const handleCleanDevCache = async () => {
        setDevCacheBusy(true);
        setDevCacheError(null);
        try {
            const health = await cleanDevBuildCache();
            setStorageHealth(health);
        } catch (e) {
            setDevCacheError(String(e));
        } finally {
            setDevCacheBusy(false);
        }
    };

    const pollRepairProgress = useCallback(async (isMounted: () => boolean) => {
        try {
            const progress = await getMemoryRepairProgress();
            if (isMounted()) {
                setRepairProgress(progress);
            }
        } catch {
            // Ignore transient polling failures while repair is running.
        }
    }, []);
    usePolling(pollRepairProgress, 1000, repairBusy);

    const pollReclaimProgress = useCallback(async (isMounted: () => boolean) => {
        try {
            const progress = await getStorageReclaimProgress();
            if (isMounted()) {
                setReclaimProgress(progress);
            }
        } catch {
            // Ignore transient polling errors while reclaim is running.
        }
    }, []);
    usePolling(pollReclaimProgress, 850, reclaimBusy);

    const handleToggleMcpServer = async () => {
        setMcpBusy(true);
        try {
            const updated = mcpStatus?.running ? await stopMcpServer() : await startMcpServer();
            setMcpStatus(updated);
        } catch (e) {
            console.error("Failed to toggle MCP server:", e);
        } finally {
            setMcpBusy(false);
        }
    };

    const handleCopyMcpLink = async () => {
        const mcpLink = mcpStatus?.public_endpoint ?? mcpStatus?.endpoint;
        if (!mcpLink) return;
        try {
            await navigator.clipboard.writeText(mcpLink);
            setCopiedMcpLink(true);
            setTimeout(() => setCopiedMcpLink(false), 1500);
        } catch (e) {
            console.error("Failed to copy MCP endpoint:", e);
        }
    };

    const handleSaveProfile = async () => {
        setProfileBusy(true);
        setProfileMsg(null);
        try {
            const onboarding: OnboardingState = await getOnboardingState();
            const normalized = profileDraft.trim();
            await saveOnboardingState({
                ...onboarding,
                display_name: normalized || null,
            });
            setProfileName(normalized);
            setProfileDraft(normalized);
            window.dispatchEvent(
                new CustomEvent("continuum-profile-updated", {
                    detail: { displayName: normalized || null },
                })
            );
            setProfileMsg("Saved");
        } catch (err) {
            setProfileMsg(`Failed to save: ${String(err)}`);
        } finally {
            setProfileBusy(false);
            window.setTimeout(() => setProfileMsg(null), 1400);
        }
    };

    const handleSaveAutofill = async () => {
        setAutofillBusy(true);
        setAutofillMsg(null);
        try {
            const saved = await setAutofillSettings({
                ...autofillSettings,
                shortcut: autofillSettings.shortcut.trim() || DEFAULT_AUTOFILL_SETTINGS.shortcut,
            });
            setAutofillSettingsState(saved);
            setSavedAutofillSettingsState(saved);
            setAutofillMsg("Saved");
        } catch (err) {
            setAutofillMsg(`Failed to save: ${String(err)}`);
        } finally {
            setAutofillBusy(false);
            window.setTimeout(() => setAutofillMsg(null), 1800);
        }
    };

    const autofillDirty =
        JSON.stringify(autofillSettings) !== JSON.stringify(savedAutofillSettings);

    const handleCloudSignOut = async () => {
        setCloudSigningOut(true);
        setCloudError(null);
        try {
            await cloudSignOut();
            window.location.reload();
        } catch (err) {
            setCloudError(String(err));
            setCloudSigningOut(false);
        }
    };

    const refreshCloudIdentity = async () => {
        try {
            setCloudIdentity(await cloudGetIdentity());
        } catch (err) {
            console.error("Failed to refresh cloud identity:", err);
        }
    };

    const handleSyncNow = async () => {
        if (syncingNow) return;
        setSyncingNow(true);
        setSyncNowMsg(null);
        try {
            const report = await cloudSyncNow();
            const keptPrivate = report.skipped_blocked + report.skipped_local_only;
            const parts = [`Synced ${report.pushed} to the team graph`];
            if (keptPrivate > 0) parts.push(`${keptPrivate} kept private`);
            if (report.failed > 0) parts.push(`${report.failed} failed`);
            setSyncNowMsg(`${parts.join(" · ")}.`);
        } catch (err) {
            setSyncNowMsg(String(err));
        } finally {
            setSyncingNow(false);
        }
    };

    const handleCreateCluster = async () => {
        const name = clusterNameDraft.trim();
        if (!name || clusterBusy) return;
        setClusterBusy(true);
        setClusterMsg(null);
        try {
            const created = await cloudCreateCluster(name);
            setClusterNameDraft("");
            setNewJoinCode(created.join_code);
            setClusterMsg(`Created "${created.name}". Share code ${created.join_code} to invite teammates.`);
            await refreshCloudIdentity();
        } catch (err) {
            setClusterMsg(String(err));
        } finally {
            setClusterBusy(false);
        }
    };

    const handleJoinCluster = async () => {
        const code = joinCodeDraft.trim();
        if (!code || clusterBusy) return;
        setClusterBusy(true);
        setClusterMsg(null);
        try {
            const joined = await cloudJoinCluster(code);
            setJoinCodeDraft("");
            setNewJoinCode(null);
            setClusterMsg(`Joined "${joined.name}" as ${joined.role}.`);
            await refreshCloudIdentity();
        } catch (err) {
            setClusterMsg(String(err));
        } finally {
            setClusterBusy(false);
        }
    };

    return (
        <div className="control-panel-container">
            <div className="control-panel-actions continuum-os-chrome-row">
                <button
                    type="button"
                    className="continuum-os-chrome-btn"
                    onClick={() => {
                        setIsOpen(false);
                        setIsAppearanceOpen(!isAppearanceOpen);
                    }}
                    aria-label="Open appearance"
                    title="Open appearance"
                >
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" aria-hidden>
                        <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" />
                    </svg>
                </button>
                <button
                    type="button"
                    className="continuum-os-chrome-btn control-panel-settings-btn"
                    onClick={() => {
                        setIsAppearanceOpen(false);
                        setIsOpen(!isOpen);
                    }}
                    aria-label={privacyAlertCount > 0 ? `Open settings, ${privacyAlertCount} privacy alert${privacyAlertCount === 1 ? "" : "s"}` : "Open settings"}
                    title={privacyAlertCount > 0 ? `${privacyAlertCount} privacy alert${privacyAlertCount === 1 ? "" : "s"}` : "Open settings"}
                >
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" aria-hidden>
                        <circle cx="12" cy="12" r="3" />
                        <path d="M19.4 15a1.7 1.7 0 0 0 .34 1.86l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.7 1.7 0 0 0-1.86-.34 1.7 1.7 0 0 0-1 1.55V21a2 2 0 0 1-4 0v-.09a1.7 1.7 0 0 0-1-1.55 1.7 1.7 0 0 0-1.86.34l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.7 1.7 0 0 0 .34-1.86 1.7 1.7 0 0 0-1.55-1H3a2 2 0 0 1 0-4h.09a1.7 1.7 0 0 0 1.55-1 1.7 1.7 0 0 0-.34-1.86l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.7 1.7 0 0 0 1.86.34h0a1.7 1.7 0 0 0 1-1.55V3a2 2 0 0 1 4 0v.09a1.7 1.7 0 0 0 1 1.55h0a1.7 1.7 0 0 0 1.86-.34l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.7 1.7 0 0 0-.34 1.86v0a1.7 1.7 0 0 0 1.55 1H21a2 2 0 0 1 0 4h-.09a1.7 1.7 0 0 0-1.55 1Z" />
                    </svg>
                    {privacyAlertCount > 0 && <span className="privacy-badge">{privacyAlertCount}</span>}
                </button>
            </div>

            {(isOpen || isAppearanceOpen) && (
                <div
                    className="panel-backdrop"
                    onClick={() => {
                        setIsOpen(false);
                        setIsAppearanceOpen(false);
                    }}
                />
            )}
            <aside className={`settings-panel ${isOpen ? "open" : ""}`}>
                <header className="panel-header">
                    <div>
                        <h2>Continuum Settings</h2>
                        <p className="panel-subtitle">Private, local, always in your control.</p>
                    </div>
                    <button className="ui-action-btn panel-close" onClick={() => setIsOpen(false)} aria-label="Close">X</button>
                </header>

                <nav className="panel-tabs">
                    <button
                        className={`ui-action-btn tab ${activeTab === "settings" ? "active" : ""}`}
                        onClick={() => setActiveTab("settings")}
                    >
                        Core
                    </button>
                    <button
                        className={`ui-action-btn tab ${activeTab === "model" ? "active" : ""}`}
                        onClick={() => setActiveTab("model")}
                    >
                        Model
                    </button>
                    <button
                        className={`ui-action-btn tab ${activeTab === "privacy" ? "active" : ""}`}
                        onClick={() => setActiveTab("privacy")}
                    >
                        Privacy
                        {privacyAlertCount > 0 && <span className="tab-badge">{privacyAlertCount}</span>}
                    </button>
                </nav>

                <div className="panel-content">
                    {activeTab === "settings" && (
                        <>
                            <section className="panel-section">
                                <h3>Profile</h3>
                                <p className="section-hint">
                                    Continuum uses this name in your greeting.
                                </p>
                                <div className="profile-row">
                                    <input
                                        type="text"
                                        value={profileDraft}
                                        onChange={(event) => setProfileDraft(event.target.value)}
                                        placeholder="Your name"
                                        className="profile-input"
                                        onKeyDown={(event) => {
                                            if (event.key === "Enter") {
                                                void handleSaveProfile();
                                            }
                                        }}
                                    />
                                    <button
                                        className="ui-action-btn btn-secondary"
                                        onClick={() => void handleSaveProfile()}
                                        disabled={profileBusy || profileDraft.trim() === profileName.trim()}
                                    >
                                        {profileBusy ? "..." : "Save"}
                                    </button>
                                </div>
                                {profileMsg && <p className="profile-msg">{profileMsg}</p>}
                            </section>

                            <section className="panel-section">
                                <h3>Cloud account</h3>
                                {cloudAccount === null ? (
                                    <p className="section-hint">Loading…</p>
                                ) : !cloudAccount.configured ? (
                                    <p className="section-hint">Cloud sync not configured</p>
                                ) : cloudAccount.signed_in ? (
                                    <>
                                        <div className="mcp-status-row">
                                            <span className="mcp-pill running">Signed in</span>
                                            <button
                                                className="ui-action-btn btn-secondary"
                                                onClick={() => void handleCloudSignOut()}
                                                disabled={cloudSigningOut}
                                            >
                                                {cloudSigningOut ? "Signing out..." : "Sign out"}
                                            </button>
                                        </div>
                                        <div className="storage-health-line">
                                            <span>{cloudAccount.email ?? "Signed in"}</span>
                                            <span>
                                                Team: {cloudIdentity ? (cloudIdentity.cluster_id ?? "No team yet") : "…"}
                                            </span>
                                        </div>
                                        {cloudIdentity && !cloudIdentity.cluster_id && (
                                            <div style={{ marginTop: 10, display: "flex", flexDirection: "column", gap: 8 }}>
                                                <p className="section-hint">Create a workspace or join one with a code.</p>
                                                <div className="profile-row">
                                                    <input
                                                        type="text"
                                                        value={clusterNameDraft}
                                                        onChange={(event) => setClusterNameDraft(event.target.value)}
                                                        placeholder="Workspace name"
                                                        className="profile-input"
                                                        maxLength={80}
                                                        onKeyDown={(event) => {
                                                            if (event.key === "Enter") void handleCreateCluster();
                                                        }}
                                                    />
                                                    <button
                                                        className="ui-action-btn btn-secondary"
                                                        onClick={() => void handleCreateCluster()}
                                                        disabled={clusterBusy || !clusterNameDraft.trim()}
                                                    >
                                                        {clusterBusy ? "..." : "Create"}
                                                    </button>
                                                </div>
                                                <div className="profile-row">
                                                    <input
                                                        type="text"
                                                        value={joinCodeDraft}
                                                        onChange={(event) => setJoinCodeDraft(event.target.value)}
                                                        placeholder="Join code"
                                                        className="profile-input"
                                                        onKeyDown={(event) => {
                                                            if (event.key === "Enter") void handleJoinCluster();
                                                        }}
                                                    />
                                                    <button
                                                        className="ui-action-btn btn-secondary"
                                                        onClick={() => void handleJoinCluster()}
                                                        disabled={clusterBusy || !joinCodeDraft.trim()}
                                                    >
                                                        {clusterBusy ? "..." : "Join"}
                                                    </button>
                                                </div>
                                            </div>
                                        )}
                                        {cloudAccount.signed_in && (
                                            <div style={{ marginTop: 10, display: "flex", flexDirection: "column", gap: 8 }}>
                                                <div className="profile-row">
                                                    <button
                                                        className="ui-action-btn btn-secondary"
                                                        onClick={() => void handleSyncNow()}
                                                        disabled={syncingNow || !cloudIdentity?.cluster_id}
                                                    >
                                                        {syncingNow ? "Syncing…" : "Sync now"}
                                                    </button>
                                                    <span className="section-hint" style={{ alignSelf: "center" }}>
                                                        {cloudIdentity?.cluster_id
                                                            ? "Push recent memories to your team. Also runs automatically once a day."
                                                            : "Join or create a workspace to enable team sync."}
                                                    </span>
                                                </div>
                                                {syncNowMsg && <p className="section-hint">{syncNowMsg}</p>}
                                            </div>
                                        )}
                                        {newJoinCode && (
                                            <div className="storage-health-line" style={{ marginTop: 8 }}>
                                                <span>Share code</span>
                                                <span style={{ fontFamily: "monospace", letterSpacing: "0.15em" }}>
                                                    {newJoinCode}
                                                </span>
                                            </div>
                                        )}
                                        {clusterMsg && (
                                            <p className="section-hint" style={{ marginTop: 8 }}>{clusterMsg}</p>
                                        )}
                                        {cloudError && (
                                            <p className="section-hint" style={{ marginTop: 8 }}>{cloudError}</p>
                                        )}
                                    </>
                                ) : (
                                    <p className="section-hint">Not signed in</p>
                                )}
                            </section>

                            <section className="panel-section">
                                <h3>Capture Status</h3>
                                <button
                                    className={`ui-action-btn capture-toggle ${status?.is_paused ? "paused" : "active"}`}
                                    onClick={handleToggleCapture}
                                >
                                    {status?.is_paused ? "Resume capture" : "Pause capture"}
                                </button>
                                <CapturePipelineSummary status={status} />
                            </section>

                            {qualityStatus && (
                                <section className="panel-section">
                                    <h3>Memory Quality</h3>
                                    <div className="capture-stats">
                                        <span>Stored: {qualityStatus.stored_count.toLocaleString()}</span>
                                        <span>Dropped: {qualityStatus.dropped_count.toLocaleString()}</span>
                                        {qualityStatus.flagged_count > 0 && (
                                            <span>Flagged: {qualityStatus.flagged_count.toLocaleString()}</span>
                                        )}
                                    </div>
                                </section>
                            )}

                            <section className="panel-section">
                                <h3>Indexing</h3>
                                <p className="section-hint">
                                    Keep a compact rolling memory window.
                                </p>
                                <div className="retention-controls">
                                    <select
                                        value={retentionDays}
                                        onChange={(e) => void handleRetentionChange(Number(e.target.value))}
                                        className="retention-select"
                                    >
                                        <option value={7}>7 days</option>
                                        <option value={30}>30 days</option>
                                        <option value={90}>90 days</option>
                                        <option value={0}>Forever</option>
                                    </select>
                                    {retentionDays > 0 && (
                                        <button
                                            className="ui-action-btn btn-secondary"
                                            onClick={() => void handleRunRetentionNow()}
                                            disabled={retentionBusy}
                                        >
                                            {retentionBusy ? "..." : "Run now"}
                                        </button>
                                    )}
                                </div>
                                {storageHealth && (
                                    <>
                                        <div className="storage-health-line" aria-label="Storage health">
                                            <span>Memory DB {formatBytes(storageHealth.memory_db_bytes)}</span>
                                            <span>Frames {formatBytes(storageHealth.frames_bytes)}</span>
                                            <span>Models {formatBytes(storageHealth.models_bytes)}</span>
                                            <span>Dev cache {formatBytes(storageHealth.dev_build_cache_bytes)}</span>
                                        </div>
                                        {storageHealth.dev_build_cache_bytes > 1024 * 1024 * 1024 && (
                                            <button
                                                className="ui-action-btn btn-secondary"
                                                onClick={() => void handleCleanDevCache()}
                                                disabled={devCacheBusy}
                                            >
                                                {devCacheBusy ? "Cleaning..." : "Clean dev cache"}
                                            </button>
                                        )}
                                        {devCacheError && (
                                            <p className="settings-error">{devCacheError}</p>
                                        )}
                                    </>
                                )}
                            </section>

                            <section className="panel-section">
                                <h3>Screen Auto-Fill</h3>
                                <p className="section-hint">
                                    Use Continuum&apos;s local memory to fill the active field with <strong>⌥F</strong> (Option+F).
                                </p>
                                <div className="autofill-grid">
                                    <label className="autofill-field">
                                        <span className="autofill-label">Mode</span>
                                        <select
                                            value={autofillSettings.enabled ? "enabled" : "disabled"}
                                            onChange={(event) =>
                                                setAutofillSettingsState((current) => ({
                                                    ...current,
                                                    enabled: event.target.value === "enabled",
                                                }))
                                            }
                                            className="retention-select"
                                        >
                                            <option value="enabled">Enabled</option>
                                            <option value="disabled">Disabled</option>
                                        </select>
                                    </label>
                                    <label className="autofill-field">
                                        <span className="autofill-label">Shortcut</span>
                                        <input
                                            type="text"
                                            value={autofillSettings.shortcut}
                                            onChange={(event) =>
                                                setAutofillSettingsState((current) => ({
                                                    ...current,
                                                    shortcut: event.target.value,
                                                }))
                                            }
                                            className="profile-input"
                                            placeholder="Alt+F"
                                        />
                                    </label>
                                    <label className="autofill-field">
                                        <span className="autofill-label">Lookback</span>
                                        <select
                                            value={autofillSettings.lookback_days}
                                            onChange={(event) =>
                                                setAutofillSettingsState((current) => ({
                                                    ...current,
                                                    lookback_days: Number(event.target.value),
                                                }))
                                            }
                                            className="retention-select"
                                        >
                                            <option value={30}>30 days</option>
                                            <option value={60}>60 days</option>
                                            <option value={90}>90 days</option>
                                            <option value={180}>180 days</option>
                                        </select>
                                    </label>
                                    <label className="autofill-field">
                                        <span className="autofill-label">Auto-fill threshold</span>
                                        <select
                                            value={autofillSettings.auto_inject_threshold}
                                            onChange={(event) =>
                                                setAutofillSettingsState((current) => ({
                                                    ...current,
                                                    auto_inject_threshold: Number(event.target.value),
                                                }))
                                            }
                                            className="retention-select"
                                        >
                                            <option value={0.85}>85%</option>
                                            <option value={0.9}>90%</option>
                                            <option value={0.95}>95%</option>
                                            <option value={0.98}>98%</option>
                                        </select>
                                    </label>
                                </div>
                                <label className="autofill-check">
                                    <input
                                        type="checkbox"
                                        checked={autofillSettings.prefer_typed_injection}
                                        onChange={(event) =>
                                            setAutofillSettingsState((current) => ({
                                                ...current,
                                                prefer_typed_injection: event.target.checked,
                                            }))
                                        }
                                    />
                                    Prefer system typing when the target app stays active
                                </label>
                                <label className="autofill-check">
                                    <input
                                        type="checkbox"
                                        checked={autofillSettings.max_candidates > 1}
                                        onChange={(event) =>
                                            setAutofillSettingsState((current) => ({
                                                ...current,
                                                max_candidates: event.target.checked ? 4 : 1,
                                            }))
                                        }
                                    />
                                    Offer quick-pick choices when Continuum finds multiple strong matches
                                </label>
                                <p className="autofill-help">
                                    Shortcut uses the Tauri format: <code>Alt+F</code> = ⌥F, <code>Shift+Alt+F</code> = ⇧⌥F, <code>Super+Shift+F</code> = ⌘⇧F.
                                </p>
                                <div className="profile-row">
                                    <button
                                        className="ui-action-btn btn-secondary"
                                        onClick={() => void handleSaveAutofill()}
                                        disabled={autofillBusy || !autofillDirty}
                                    >
                                        {autofillBusy ? "..." : "Save auto-fill"}
                                    </button>
                                    {autofillMsg && <p className="profile-msg">{autofillMsg}</p>}
                                </div>
                            </section>

                            {!evalUi && (
                                <section className="panel-section">
                                    <h3>MCP Server</h3>
                                    <p className="section-hint">
                                        Connect Continuum to external tools via Model Context Protocol.
                                    </p>
                                    <div className="mcp-status-row">
                                        <span className={`mcp-pill ${mcpStatus?.running ? "running" : "stopped"}`}>
                                            {mcpStatus?.running ? "Running" : "Stopped"}
                                        </span>
                                        <button
                                            className="ui-action-btn btn-secondary"
                                            onClick={() => void handleToggleMcpServer()}
                                            disabled={mcpBusy}
                                        >
                                            {mcpBusy ? "..." : mcpStatus?.running ? "Stop" : "Start"}
                                        </button>
                                    </div>
                                    <div className="mcp-link-row">
                                        <input
                                            className="mcp-link-input"
                                            value={
                                                mcpStatus?.public_endpoint ??
                                                mcpStatus?.endpoint ??
                                                "http://127.0.0.1:8799/mcp"
                                            }
                                            readOnly
                                        />
                                        <button className="ui-action-btn btn-primary" onClick={() => void handleCopyMcpLink()}>
                                            {copiedMcpLink ? "Copied" : "Copy link"}
                                        </button>
                                    </div>
                                    <div className="mcp-link-row">
                                        <input
                                            className="mcp-link-input"
                                            value={`Auth: ${mcpStatus?.auth_mode ?? "disabled for localhost"}`}
                                            readOnly
                                        />
                                    </div>
                                    <div className="mcp-link-row">
                                        <input
                                            className="mcp-link-input"
                                            value={`Runtime: ${contextRuntimeStatus?.status ?? "unknown"} · Project: ${contextRuntimeStatus?.active_project ?? "n/a"} · Pack: ${contextRuntimeStatus?.current_context_pack ?? "none"}`}
                                            readOnly
                                        />
                                    </div>
                                    {mcpStatus?.last_error && <p className="mcp-error">{mcpStatus.last_error}</p>}
                                </section>
                            )}
                        </>
                    )}

                    {activeTab === "model" && (
                        <section className="panel-section">
                            <h3>AI Model</h3>
                            <div className="local-ai-status">
                                <div className="ai-model-row">
                                    <span className="label">Memory model:</span>
                                    <span className="value">Qwen3-VL-2B</span>
                                </div>
                                <div className="ai-model-row">
                                    <span className="label">Search model:</span>
                                    <span className="value">MiniLM-L6-v2 (384-d)</span>
                                </div>
                                <div className="ai-model-row">
                                    <span className="label">Mode:</span>
                                    <span className="value">8 GB Mac optimized</span>
                                </div>
                            </div>
                            <p className="section-hint">
                                {status?.ai_model_available
                                    ? status?.ai_model_loaded
                                        ? "Qwen3-VL-2B is loaded and ready."
                                        : "Qwen3-VL-2B is on disk and will load when needed."
                                    : "Download Qwen3-VL-2B below to enable AI memory synthesis."}
                            </p>
                            <p className="section-hint">
                                Search embeddings: {status
                                    ? status.embedding_degraded
                                        ? `degraded (${status.embedding_backend})`
                                        : status.embedding_backend
                                    : "unknown"}.
                                {status?.embedding_detail ? ` ${status.embedding_detail}` : ""}
                            </p>

                            {modelError && <div className="model-error">{modelError}</div>}

                            {modelsLoading && <p className="section-hint">Loading…</p>}

                            {!modelsLoading && models.map((model) => {
                                const isDownloaded = model.download_url === "already_downloaded";
                                const isDownloading = downloadingId === model.id;
                                const confirmingDelete = confirmDeleteModel === model.id;
                                const shouldShowActivate = isDownloaded && !status?.ai_model_loaded;

                                return (
                                    <div key={model.id} className={`model-row ${isDownloaded ? "downloaded" : ""}`}>
                                        <div className="model-row-info">
                                            <div className="model-row-name">
                                                {model.name}
                                                {isDownloaded && <span className="model-badge-downloaded">Downloaded</span>}
                                                {model.recommended && !isDownloaded && <span className="model-badge-recommended">Recommended</span>}
                                            </div>
                                            <div className="model-row-meta">
                                                {model.size_label} · {model.speed_label} · ~{model.ram_gb} GB RAM
                                                {isDownloaded && <span className="model-status-loaded"> · Loaded & Ready</span>}
                                            </div>
                                            <div className="model-row-specs">
                                                Disk: ~{(model.size_bytes / (1024 * 1024 * 1024)).toFixed(1)} GB · RAM: ~{model.ram_gb} GB · {model.quality_label}
                                            </div>
                                            <div className="model-row-desc">{model.description}</div>
                                        </div>

                                        {isDownloading ? (
                                            <div className="model-dl-progress">
                                                {downloadStatus.state === "downloading" ? (
                                                    <>
                                                        <div className="model-dl-bar-wrap">
                                                            <div className="model-dl-bar-fill" style={{ width: `${downloadStatus.percent.toFixed(1)}%` }} />
                                                        </div>
                                                        <span className="model-dl-pct">
                                                            {formatBytes(downloadStatus.bytes_downloaded)} / {formatBytes(downloadStatus.total_bytes)} ({downloadStatus.percent.toFixed(0)}%)
                                                        </span>
                                                    </>
                                                ) : (
                                                    <span className="model-dl-pct">
                                                        {isActivatingModel
                                                            ? "Loading model…"
                                                            : downloadStatus.state === "finalizing"
                                                                ? "Finalizing…"
                                                                : "Connecting…"}
                                                    </span>
                                                )}
                                            </div>
                                        ) : shouldShowActivate ? (
                                            <button
                                                className="btn-liquid-glass"
                                                onClick={() => void handleDownloadModel(model)}
                                                disabled={isActivatingModel}
                                            >
                                                {isActivatingModel ? "..." : "Load Now"}
                                            </button>
                                        ) : isDownloaded ? (
                                            <button
                                                className={`btn-danger-sm ${confirmingDelete ? "confirm" : ""}`}
                                                onClick={() => void handleDeleteModel(model)}
                                            >
                                                {confirmingDelete ? "Confirm delete" : "Delete"}
                                            </button>
                                        ) : (
                                            <button
                                                className="btn-primary-sm"
                                                onClick={() => void handleDownloadModel(model)}
                                                disabled={!!downloadingId}
                                            >
                                                Download
                                            </button>
                                        )}
                                    </div>
                                );
                            })}

                            {(downloadingId || isActivatingModel) && (
                                <div style={{
                                    marginTop: 16,
                                    background: "rgba(255,255,255,0.04)",
                                    border: "1px solid rgba(255,255,255,0.08)",
                                    borderRadius: 10,
                                    padding: 12,
                                    fontFamily: "inherit",
                                    fontSize: 11,
                                    color: "rgba(255,255,255,0.75)",
                                    maxHeight: 140,
                                    overflowY: "auto"
                                }}>
                                    <div style={{ color: "rgba(255,255,255,0.95)", marginBottom: 8 }}>
                                        Stage: {isActivatingModel ? "activating" : downloadStatus.state}
                                    </div>
                                    {downloadStatus.destination_path && (
                                        <div style={{ marginBottom: 8 }}>{downloadStatus.destination_path}</div>
                                    )}
                                    {downloadStatus.logs.map((line, index) => (
                                        <div key={index} style={{ marginBottom: 4 }}>{line}</div>
                                    ))}
                                </div>
                            )}
                        </section>
                    )}

                    {activeTab === "privacy" && (
                        <>
                            <section className="panel-section">
                                <PrivacyPanel
                                    isVisible={true}
                                    onClose={() => undefined}
                                    onAlertsChange={setPrivacyAlertCount}
                                    onBlocklistChange={setBlocklistState}
                                    embedded={true}
                                />
                            </section>

                            <section className="panel-section">
                                <h3>Blocked Apps & Sites</h3>
                                <p className="section-hint">These apps and websites will not be captured.</p>
                                <div className="blocklist">
                                    {blocklist.length === 0 ? (
                                        <p className="blocklist-empty">No apps or sites blocked</p>
                                    ) : (
                                        blocklist.map((app) => (
                                            <div key={app} className="blocklist-item">
                                                <span>{app}</span>
                                                <button onClick={() => void handleRemoveApp(app)}>x</button>
                                            </div>
                                        ))
                                    )}
                                </div>
                                <div className="add-app-row">
                                    <input
                                        type="text"
                                        placeholder="Add app name or site..."
                                        value={newApp}
                                        onChange={(e) => setNewApp(e.target.value)}
                                        onKeyDown={(e) => e.key === "Enter" && void handleAddApp()}
                                        className="add-app-input"
                                    />
                                    <button onClick={() => void handleAddApp()} className="ui-action-btn btn-primary">Add</button>
                                </div>
                            </section>

                            <section className="panel-section danger-section">
                                <h3>Danger Zone</h3>
                                <p className="section-hint">
                                    One-time repair can merge historical duplicate memories into continuity cards.
                                </p>
                                <button
                                    className="ui-action-btn"
                                    onClick={() => void handleRunRepairBackfill()}
                                    disabled={repairBusy}
                                >
                                    {repairBusy ? "Repairing..." : "Run memory continuity repair (one-time)"}
                                </button>
                                {repairBusy && repairProgress && (
                                    <p className="section-hint" style={{ marginTop: 8 }}>
                                        Progress: {repairProgress.processed.toLocaleString()} / {repairProgress.total.toLocaleString()} ·
                                        phase {repairProgress.phase} · merged {repairProgress.merged_count.toLocaleString()} ·
                                        anchor merges {repairProgress.anchor_merges.toLocaleString()}
                                    </p>
                                )}
                                {repairSummary && (
                                    <p className="section-hint" style={{ marginTop: 8 }}>
                                        Merged {repairSummary.merged_count} duplicates ({repairSummary.total_before} → {repairSummary.total_after} cards),
                                        updated {repairSummary.task_reference_updates} task references,
                                        refreshed {repairSummary.embeddings_refreshed} embeddings,
                                        reclaimed {(repairSummary.chars_reclaimed / 1024).toFixed(1)} KB of raw OCR payload.
                                        {repairSummary.app_merges.length > 0 && (
                                            <>
                                                {" "}
                                                Top merge sources:{" "}
                                                {repairSummary.app_merges
                                                    .slice(0, 8)
                                                    .map((row) => `${row.app_name} (${row.merged})`)
                                                    .join(", ")}
                                                {repairSummary.app_merges.length > 8 ? "…" : "."}
                                            </>
                                        )}
                                    </p>
                                )}
                                {repairError && <p className="section-hint" style={{ marginTop: 8 }}>{repairError}</p>}
                                <button
                                    className="ui-action-btn btn-secondary"
                                    onClick={() => void handleReclaimStorage()}
                                    disabled={reclaimBusy}
                                    style={{ marginTop: 8 }}
                                >
                                    {reclaimBusy ? "Reclaiming..." : "Reclaim storage from old captures"}
                                </button>
                                <p className="section-hint" style={{ marginTop: 8 }}>
                                    Storage reclaim now runs continuity repair first and only proceeds when real embeddings are available.
                                </p>
                                {reclaimBusy && reclaimProgress && (
                                    <div className="reclaim-progress-wrap">
                                        <div className="reclaim-progress-bar-wrap">
                                            <div
                                                className="reclaim-progress-bar-fill"
                                                style={{
                                                    width: `${
                                                        reclaimProgress.total > 0
                                                            ? ((reclaimProgress.processed / reclaimProgress.total) * 100).toFixed(1)
                                                            : "0.0"
                                                    }%`,
                                                }}
                                            />
                                        </div>
                                        <p className="section-hint reclaim-progress-text">
                                            {reclaimProgress.phase} · {reclaimProgress.processed.toLocaleString()} / {reclaimProgress.total.toLocaleString()} ·
                                            rewrote {reclaimProgress.records_rewritten.toLocaleString()} ·
                                            removed {reclaimProgress.screenshot_files_deleted.toLocaleString()} files
                                        </p>
                                    </div>
                                )}
                                {reclaimSummary && (
                                    <p className="section-hint" style={{ marginTop: 8 }}>
                                        Rewrote {reclaimSummary.records_rewritten} / {reclaimSummary.records_scanned} cards,
                                        refreshed {reclaimSummary.embeddings_refreshed + reclaimSummary.snippet_embeddings_refreshed + reclaimSummary.support_embeddings_refreshed} embeddings,
                                        removed {reclaimSummary.screenshot_files_deleted} screenshot files,
                                        reclaimed {(reclaimSummary.bytes_reclaimed / (1024 * 1024)).toFixed(1)} MB ({(reclaimSummary.chars_reclaimed / 1024).toFixed(1)} KB text).
                                    </p>
                                )}
                                {reclaimError && <p className="section-hint" style={{ marginTop: 8 }}>{reclaimError}</p>}
                                <button
                                    className={`ui-action-btn btn-danger ${confirmDelete ? "confirm" : ""}`}
                                    onClick={() => void handleDeleteAll()}
                                >
                                    {confirmDelete ? "Click again to confirm" : "Delete all data"}
                                </button>
                            </section>
                        </>
                    )}
                </div>
            </aside>

            <aside className={`settings-panel ${isAppearanceOpen ? "open" : ""}`}>
                <header className="panel-header">
                    <div>
                        <h2>Appearance</h2>
                        <p className="panel-subtitle">Mode, motion background, and cinematic palette.</p>
                    </div>
                    <button className="ui-action-btn panel-close" onClick={() => setIsAppearanceOpen(false)} aria-label="Close">X</button>
                </header>
                <div className="panel-content">
                    <section className="panel-section">
                        <h3>Appearance</h3>
                        <p className="section-hint">Mode, motion background, and cinematic palette.</p>
                        <div className="theme-choice-row" role="radiogroup" aria-label="Theme selection">
                            <button
                                type="button"
                                className={`continuum-os-glass-btn theme-choice ${theme === "dark" ? "active" : ""}`}
                                onClick={() => selectAppearance(paletteKey, "dark")}
                                aria-pressed={theme === "dark"}
                            >
                                <span className="theme-choice-icon" aria-hidden="true"><Icon name="moon" size={14} /></span>
                                Dark
                            </button>
                            <button
                                type="button"
                                className={`continuum-os-glass-btn theme-choice ${theme === "light" ? "active" : ""}`}
                                onClick={() => selectAppearance(paletteKey, "light")}
                                aria-pressed={theme === "light"}
                            >
                                <span className="theme-choice-icon" aria-hidden="true"><Icon name="sun" size={14} /></span>
                                Light
                            </button>
                        </div>
                        <h4 className="appearance-subheading">Motion background</h4>
                        <p className="section-hint appearance-subhint">
                            Interactive shaders — move the pointer and click empty space to play.
                        </p>
                        <div className="wallpaper-choice-grid" role="listbox" aria-label="Motion background">
                            {listWallpapers().map((key) => {
                                const wp = WALLPAPERS[key];
                                const active = wallpaperId === key;
                                return (
                                    <button
                                        key={key}
                                        type="button"
                                        className={`wallpaper-choice ${active ? "active" : ""}`}
                                        onClick={() => selectWallpaper(key)}
                                        aria-selected={active}
                                    >
                                        <span
                                            className="wallpaper-choice-preview"
                                            style={{ background: wp.preview }}
                                            aria-hidden
                                        />
                                        <span className="wallpaper-choice-copy">
                                            <strong>{wp.name}</strong>
                                            <span>{wp.description}</span>
                                        </span>
                                    </button>
                                );
                            })}
                        </div>
                        <h4 className="appearance-subheading">Cinematic palette</h4>
                        <div className="palette-choice-grid" role="listbox" aria-label="Cinematic palette">
                            {listPalettes().map((key) => {
                                const palette = PALETTES[key];
                                const active = paletteKey === key;
                                return (
                                    <button
                                        key={key}
                                        type="button"
                                        className={`palette-choice ${active ? "active" : ""}`}
                                        onClick={() => {
                                            if (key === "continuumDark") {
                                                selectAppearance(key, "dark");
                                                return;
                                            }
                                            if (key === "continuumLight") {
                                                selectAppearance(key, "light");
                                                return;
                                            }
                                            selectAppearance(key, theme);
                                        }}
                                        aria-selected={active}
                                    >
                                        <span className="palette-choice-copy">
                                            <strong>{palette.name}</strong>
                                            <span>{palette.year} · {palette.director}</span>
                                        </span>
                                        <span className="palette-swatches" aria-hidden="true">
                                            {palette.shades.map((shade, index) => (
                                                <span
                                                    key={`${key}-${shade}-${index}`}
                                                    className="palette-swatch"
                                                    style={{ backgroundColor: shade }}
                                                />
                                            ))}
                                        </span>
                                    </button>
                                );
                            })}
                        </div>
                    </section>
                </div>
            </aside>
        </div>
    );
}

/**
 * Compact "stored vs skipped (with reasons)" breakdown for the Capture
 * Status card. Replaces the old `Frames: N / Dropped: M` line, which only
 * counted successful stores and dedup drops — every other reason a frame
 * could be discarded (surface policy, low signal, noise, grounding, …)
 * was previously invisible to the UI.
 */
function CapturePipelineSummary({ status }: { status: CaptureStatus | null }) {
    const pipeline = status?.pipeline;
    if (!pipeline) {
        return (
            <div className="capture-stats">
                <span>Frames: {status?.frames_captured ?? 0}</span>
                <span>Dropped: {status?.frames_dropped ?? 0}</span>
            </div>
        );
    }
    const reasons: Array<{ key: string; label: string; value: number }> = [
        { key: "self_app", label: "this app (Continuum)", value: pipeline.skipped_self_app ?? 0 },
        { key: "perceptual_dup", label: "dedup (image)", value: pipeline.skipped_perceptual_dup },
        { key: "semantic_dup", label: "dedup (text)", value: pipeline.skipped_semantic_dup },
        { key: "low_signal_text", label: "low signal", value: pipeline.skipped_low_signal_text },
        { key: "noise", label: "noise", value: pipeline.skipped_noise },
        { key: "grounding", label: "grounding", value: pipeline.skipped_grounding },
        { key: "stacked_extraction", label: "extraction", value: pipeline.skipped_stacked_extraction },
        { key: "surface_policy", label: "surface", value: pipeline.skipped_surface_policy },
        { key: "blocklist", label: "blocklist", value: pipeline.skipped_blocklist },
        { key: "visual_novelty", label: "visual novelty", value: pipeline.skipped_visual_novelty },
        { key: "visual_small", label: "visual small", value: pipeline.skipped_visual_small },
        { key: "visual_compose_failed", label: "visual fail", value: pipeline.skipped_visual_compose_failed },
        { key: "ocr_failed", label: "ocr fail", value: pipeline.skipped_ocr_failed },
        { key: "screen_capture_failed", label: "screen fail", value: pipeline.skipped_screen_capture_failed },
    ].filter((r) => r.value > 0);
    const lastSkipLabel = pipeline.last_skip_reason
        ? `Last skip: ${pipeline.last_skip_reason}${pipeline.last_skip_app ? ` (${pipeline.last_skip_app})` : ""}`
        : null;
    const selfSkipHint =
        (pipeline.skipped_self_app ?? 0) > 0 ? (
            <p className="capture-stats-hint">
                Continuum never records its own window. Focus Cursor, Chrome, or another app to build memories
                while this panel is open.
            </p>
        ) : null;
    return (
        <div className="capture-stats capture-stats--pipeline">
            <div className="capture-stats-row">
                <span>Stored: {pipeline.stored_total}</span>
                <span>Skipped: {pipeline.skipped_total}</span>
                <span>Evaluated: {pipeline.evaluated}</span>
            </div>
            {selfSkipHint}
            {reasons.length > 0 && (
                <div className="capture-stats-reasons" title="Why frames were skipped this session">
                    {reasons.map((r) => (
                        <span key={r.key} className="capture-stats-chip">
                            {r.label}: {r.value}
                        </span>
                    ))}
                </div>
            )}
            {lastSkipLabel && <div className="capture-stats-last">{lastSkipLabel}</div>}
        </div>
    );
}
import type { PanelKey } from "@/domains/command-palette/CommandPalette";
