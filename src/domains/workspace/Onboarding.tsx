import { useState, useEffect, useCallback, useRef } from "react";
import {
    OnboardingState,
    OnboardingStep,
    ModelInfo,
    getOnboardingState,
    saveOnboardingState,
    requestBiometricAuth,
    checkPermissions,
    openSystemSettings,
    listAvailableModels,
    downloadModel,
    refreshAiModels,
} from "@/shared/ipc/onboarding";
import { useModelDownloadStatus } from "@/shared/hooks/useModelDownloadStatus";
import { usePolling } from "@/shared/hooks/usePolling";
import { formatBytes } from "@/shared/utils/format";
import "./Onboarding.css";

// ── Helper: step index for progress dots ─────────────────────────────────
const STEPS: OnboardingStep[] = [
    "welcome",
    "biometrics",
    "privacy_promise",
    "model_download",
    "permissions",
];

const DEFAULT_ONBOARDING_STATE: OnboardingState = {
    step: "welcome",
    biometric_enabled: false,
    screen_permission: false,
    accessibility_permission: false,
    model_downloaded: false,
    model_id: null,
    display_name: null,
};

function stepIndex(s: OnboardingStep) {
    return STEPS.indexOf(s);
}

// ── StepDots ──────────────────────────────────────────────────────────────
function StepDots({ current }: { current: OnboardingStep }) {
    const ci = stepIndex(current);
    return (
        <div className="ob-step-dots">
            {STEPS.map((s, i) => (
                <div
                    key={s}
                    className={`ob-step-dot ${i === ci ? "active" : i < ci ? "done" : ""}`}
                />
            ))}
        </div>
    );
}

// ── Step 1: Welcome ───────────────────────────────────────────────────────
function StepWelcome({
    state,
    onSave,
}: {
    state: OnboardingState;
    onSave: (s: OnboardingState) => void;
}) {
    const [displayName, setDisplayName] = useState(state.display_name ?? "");

    function handleNext() {
        onSave({
            ...state,
            display_name: displayName.trim() || null,
            step: "biometrics",
        });
    }

    return (
        <>
            <span className="ob-icon">⌘</span>
            <h1 className="ob-title">Your memory, on your Mac.</h1>
            <p className="ob-subtitle">
                FNDR remembers what you&apos;ve worked on so you don&apos;t have to.
                Synthesize meetings and track tasks — all instantly.
                <br /><br />
                Everything runs on your computer. Nothing leaves it. Ever.
            </p>
            <label className="ob-name-label" htmlFor="ob-display-name">
                What should FNDR call you?
            </label>
            <input
                id="ob-display-name"
                className="ob-name-input"
                type="text"
                value={displayName}
                placeholder="Your name (optional)"
                onChange={(event) => setDisplayName(event.target.value)}
                onKeyDown={(event) => {
                    if (event.key === "Enter") {
                        handleNext();
                    }
                }}
            />
            <button id="ob-get-started" className="ob-btn-primary" onClick={handleNext}>
                Get Started
            </button>
            <button className="ob-btn-ghost" onClick={handleNext}>
                Skip for now
            </button>
        </>
    );
}

// ── Step 2: Biometrics ────────────────────────────────────────────────────
function StepBiometrics({ state, onSave }: { state: OnboardingState; onSave: (s: OnboardingState) => void }) {
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);

    async function handleEnable() {
        setLoading(true);
        setError(null);
        try {
            const ok = await requestBiometricAuth("Unlock FNDR — your private screen history");
            if (ok) {
                const next = { ...state, step: "privacy_promise" as OnboardingStep, biometric_enabled: true };
                onSave(next);
            } else {
                setError("Authentication failed. Please try again.");
            }
        } catch {
            setError("Touch ID is not available. We'll use your Mac login password.");
        }
        setLoading(false);
    }

    function handleSkip() {
        onSave({ ...state, step: "privacy_promise", biometric_enabled: false });
    }

    return (
        <>
            <span className="ob-icon">🔐</span>
            <h1 className="ob-title">Lock FNDR with Touch ID</h1>
            <p className="ob-subtitle">
                FNDR stores everything you see on screen.
                Before we start, let's make sure only you can open it.
            </p>
            {error && <div className="ob-error-box">{error}</div>}
            <button id="ob-enable-touchid" className="ob-btn-primary" onClick={handleEnable} disabled={loading}>
                {loading ? "Authenticating…" : "Enable Touch ID Lock"}
            </button>
            <button className="ob-btn-ghost" onClick={handleSkip}>
                Skip for now
            </button>
        </>
    );
}

// ── Step 3: Privacy Promise ───────────────────────────────────────────────
function StepPrivacyPromise({ state, onSave }: { state: OnboardingState; onSave: (s: OnboardingState) => void }) {
    return (
        <>
            <span className="ob-icon">🔒</span>
            <h1 className="ob-title">What FNDR sees (and doesn't share)</h1>
            <div className="ob-privacy-list">
                {[
                    {
                        icon: "✅",
                        title: "What FNDR stores",
                        body: "Text, window metadata, and snapshots of your screen. This is indexed into a local LanceDB store on your Mac.",
                    },
                    {
                        icon: "🌐",
                        title: "Nothing leaves your Mac",
                        body: "No servers. No cloud. Local Qwen3-VL and Whisper models process everything offline.",
                    },
                    {
                        icon: "🎭",
                        title: "Automatic privacy",
                        body: "Password managers and banking apps are automatically skipped using perceptual deduplication and blocklists.",
                    },
                    {
                        icon: "🗑",
                        title: "You're in control",
                        body: "Delete any memory, clear your history, or wipe the entire local database in one tap.",
                    },
                ].map(({ icon, title, body }) => (
                    <div className="ob-privacy-item" key={title}>
                        <span className="ob-privacy-icon">{icon}</span>
                        <div className="ob-privacy-text">
                            <strong>{title}</strong>
                            <span>{body}</span>
                        </div>
                    </div>
                ))}
            </div>
            <button
                id="ob-accept-privacy"
                className="ob-btn-primary"
                onClick={() => onSave({ ...state, step: "model_download" })}
            >
                I&apos;m in — Continue
            </button>
            <button
                className="ob-btn-ghost"
                onClick={() => onSave({ ...state, step: "model_download" })}
            >
                Skip for now
            </button>
        </>
    );
}

// ── Step 4: Permissions ───────────────────────────────────────────────────
function StepPermissions({ state, onSave }: { state: OnboardingState; onSave: (s: OnboardingState) => void }) {
    const [perms, setPerms] = useState({ screen_recording: false, accessibility: false, microphone: false });

    const refresh = useCallback(async () => {
        try {
            const p = await checkPermissions();
            setPerms(p);
        } catch {/* ignore */}
    }, []);

    usePolling(refresh, 2500);

    async function openSettings(pane: Parameters<typeof openSystemSettings>[0]) {
        await openSystemSettings(pane);
    }

    function handleContinue() {
        onSave({
            ...state,
            step: "complete",
            screen_permission: perms.screen_recording,
            accessibility_permission: perms.accessibility,
        });
    }

    const canContinue = perms.screen_recording;

    return (
        <>
            <span className="ob-icon">🛡️</span>
            <h1 className="ob-title">Grant a few permissions</h1>
            <p className="ob-subtitle">FNDR needs permission to see your screen. Everything stays local.</p>

            {[
                {
                    key: "screen_recording" as const,
                    icon: "🖥",
                    label: "Screen Recording",
                    desc: "Required — captures snapshots locally",
                    pane: "screen-recording" as const,
                },
                {
                    key: "accessibility" as const,
                    icon: "🔡",
                    label: "Accessibility",
                    desc: "Optional — reads window titles for better search",
                    pane: "accessibility" as const,
                },
                {
                    key: "microphone" as const,
                    icon: "🎙",
                    label: "Microphone",
                    desc: "Optional — for meeting transcription, voice search, and voice control",
                    pane: "microphone" as const,
                },
            ].map(({ key, icon, label, desc, pane }) => (
                <div className={`ob-permission-row ${perms[key] ? "granted" : ""}`} key={key}>
                    <div className="ob-permission-left">
                        <span className="ob-permission-icon">{icon}</span>
                        <div>
                            <div className="ob-permission-label">{label}</div>
                            <div className="ob-permission-desc">{desc}</div>
                        </div>
                    </div>
                    {perms[key] ? (
                        <span className="ob-permission-badge">✅</span>
                    ) : (
                        <button
                            id={`ob-perm-${pane}`}
                            className="ob-permission-btn"
                            onClick={() => openSettings(pane)}
                        >
                            Grant
                        </button>
                    )}
                </div>
            ))}

            <button
                id="ob-continue-permissions"
                className="ob-btn-primary"
                style={{ marginTop: 20 }}
                onClick={handleContinue}
                disabled={!canContinue}
                title={canContinue ? undefined : "Screen Recording is required to continue"}
            >
                {canContinue ? "Open FNDR →" : "Grant Screen Recording to continue"}
            </button>
            <button
                className="ob-btn-ghost"
                onClick={handleContinue}
            >
                Skip for now
            </button>
        </>
    );
}

// ── Step 5: Model Download ────────────────────────────────────────────────
function StepModelDownload({ state, onSave }: { state: OnboardingState; onSave: (s: OnboardingState) => void }) {
    const [models, setModels] = useState<ModelInfo[]>([]);
    const [selected, setSelected] = useState<ModelInfo | null>(null);
    const [error, setError] = useState<string | null>(null);
    const [pendingModelId, setPendingModelId] = useState<string | null>(null);
    const [isActivatingModel, setIsActivatingModel] = useState(false);
    const downloadStatus = useModelDownloadStatus();

    async function activateModel(modelId: string) {
        const runtime = await refreshAiModels();
        if (!runtime.ai_model_available) {
            throw new Error(`FNDR could not find the local model files for ${modelId}.`);
        }
        return runtime;
    }

    useEffect(() => {
        listAvailableModels()
            .then((ms) => {
                setModels(ms);
                const preferred = ms.find((m) => m.recommended) ?? ms[0];
                setSelected(preferred ?? null);
            })
            .catch((e) => setError(`Failed to load models: ${String(e)}`));
    }, []);

    useEffect(() => {
        if (!pendingModelId || downloadStatus.model_id !== pendingModelId) {
            return;
        }

        if (downloadStatus.state === "failed" && downloadStatus.error) {
            setError(downloadStatus.error);
            setPendingModelId(null);
            return;
        }

        if (downloadStatus.state !== "completed" || downloadStatus.error) {
            return;
        }

        let cancelled = false;
        const completedModelId = downloadStatus.model_id ?? pendingModelId;
        setPendingModelId(null);
        setIsActivatingModel(true);
        setError(null);

        void (async () => {
            try {
                await activateModel(completedModelId);
                if (!cancelled) {
                    onSave({
                        ...state,
                        step: "permissions",
                        model_downloaded: true,
                        model_id: completedModelId,
                    });
                }
            } catch (refreshError) {
                if (!cancelled) {
                    setError(`Model download finished, but FNDR could not activate it: ${String(refreshError)}`);
                }
            } finally {
                if (!cancelled) {
                    setIsActivatingModel(false);
                }
            }
        })();

        return () => {
            cancelled = true;
        };
    }, [downloadStatus.error, downloadStatus.model_id, downloadStatus.state, onSave, pendingModelId, state]);

    const activeDownloadStatus =
        pendingModelId && downloadStatus.model_id === pendingModelId ? downloadStatus : null;
    const isDownloading =
        isActivatingModel ||
        (activeDownloadStatus !== null &&
            ["preparing", "downloading", "finalizing"].includes(activeDownloadStatus.state));

    // Auto-scroll logs to bottom
    const logsEndRef = useRef<HTMLDivElement>(null);
    useEffect(() => {
        if (logsEndRef.current && activeDownloadStatus) {
            logsEndRef.current.scrollIntoView({ behavior: "smooth" });
        }
    }, [activeDownloadStatus]);

    async function handleDownload() {
        if (!selected) return;
        setError(null);
        if (selected.download_url === "already_downloaded") {
            setIsActivatingModel(true);
            try {
                await activateModel(selected.id);
                onSave({ ...state, step: "permissions", model_downloaded: true, model_id: selected.id });
            } catch (refreshError) {
                setError(`FNDR found the model on disk, but could not activate it: ${String(refreshError)}`);
            } finally {
                setIsActivatingModel(false);
            }
            return;
        }
        setPendingModelId(selected.id);
        try {
            await downloadModel(selected.id, selected.download_url, selected.filename);
        } catch (e: unknown) {
            setError(String(e));
            setPendingModelId(null);
        }
    }

    const alreadyDownloaded = selected?.download_url === "already_downloaded";
    const activeModelName =
        models.find((model) => model.id === activeDownloadStatus?.model_id)?.name ?? selected?.name;

    return (
        <>
            <span className="ob-icon">🧠</span>
            <h1 className="ob-title">Select your local AI model</h1>
            <p className="ob-subtitle">
                Choose the &apos;brain&apos; for your FNDR. Qwen3-VL (4B) is recommended for best-in-class 
                summaries, memory Q&amp;A, and screen understanding.
                <br /><br />
                Optional helpers for transcription and TTS are loaded only when needed.
            </p>

            {!isDownloading && (
                <div className="ob-model-cards">
                    {models.map((m) => (
                        <button
                            key={m.id}
                            id={`ob-model-${m.id}`}
                            className={`ob-model-card ${selected?.id === m.id ? "selected" : ""} ${m.download_url === "already_downloaded" ? "already-downloaded" : ""}`}
                            onClick={() => setSelected(m)}
                        >
                            {m.recommended && <span className="ob-model-badge">Recommended</span>}
                            {m.download_url === "already_downloaded" && (
                                <span className="ob-model-badge downloaded">Already on Disk</span>
                            )}
                            <div className="ob-model-name">{m.name}</div>
                            <div className="ob-model-desc">{m.description}</div>
                            <div className="ob-model-meta">
                                <span>💾 {m.size_label}</span>
                                <span>⚡ {m.speed_label}</span>
                                <span>🧠 ~{m.ram_gb} GB RAM</span>
                            </div>
                        </button>
                    ))}
                </div>
            )}

            {!isDownloading && (
                <div className="ob-privacy-list" style={{ marginBottom: 24 }}>
                    {[
                        {
                            icon: "✨",
                            title: "Multi-modal Intelligence",
                            body: "Qwen3-VL powers the core experience, enabling search by screen content and natural language synthesis.",
                        },
                        {
                            icon: "🎙",
                            title: "Local Meeting Recording",
                            body: "Whisper GGUF models are used for automatic meeting detection and privacy-first transcription.",
                        },
                        {
                            icon: "🕸",
                            title: "Tasks",
                            body: "FNDR extracts reminders and converts them into local tasks automatically.",
                        },
                    ].map(({ icon, title, body }) => (
                        <div className="ob-privacy-item" key={title}>
                            <span className="ob-privacy-icon">{icon}</span>
                            <div className="ob-privacy-text">
                                <strong>{title}</strong>
                                <span>{body}</span>
                            </div>
                        </div>
                    ))}
                </div>
            )}

            {isDownloading && activeDownloadStatus?.state === "downloading" && (
                <div style={{ marginBottom: 24 }}>
                    <div className="ob-download-info">
                        <div className="ob-download-title">Downloading {activeModelName}…</div>
                        <div className="ob-download-subtitle">
                            {formatBytes(activeDownloadStatus.bytes_downloaded)} / {formatBytes(activeDownloadStatus.total_bytes)}
                        </div>
                    </div>
                    <div className="ob-progress-bar-wrap">
                        <div
                            className="ob-progress-bar-fill"
                            style={{ width: `${activeDownloadStatus.percent.toFixed(1)}%` }}
                        />
                    </div>
                    <div className="ob-progress-label">{activeDownloadStatus.percent.toFixed(0)}%</div>
                </div>
            )}

            {isDownloading && (!activeDownloadStatus || activeDownloadStatus.state !== "downloading") && (
                <div style={{ marginBottom: 24, padding: "24px 0", textAlign: "center" }}>
                    <span className="ob-icon pulse" style={{ display: "inline-block", fontSize: 24, marginBottom: 12 }}>⚙️</span>
                    <div className="ob-download-title">
                        {isActivatingModel
                            ? "Loading model into FNDR..."
                            : activeDownloadStatus?.state === "finalizing"
                                ? "Finalizing model file..."
                                : "Preparing Download..."}
                    </div>
                    <div className="ob-download-subtitle">
                        {activeDownloadStatus?.destination_path
                            ? activeDownloadStatus.destination_path
                            : "Connecting to huggingface.co"}
                    </div>
                </div>
            )}

            {isDownloading && (
                <div className="ob-download-logs" style={{
                    background: "rgba(0,0,0,0.2)",
                    borderRadius: 8,
                    padding: 12,
                    fontSize: 11,
                    fontFamily: "inherit",
                    color: "rgba(255,255,255,0.7)",
                    height: 120,
                    overflowY: "auto",
                    marginBottom: 24,
                    textAlign: "left"
                }}>
                    <div style={{ color: "var(--accent)" }}>
                        [Stage: {activeDownloadStatus?.state ?? (isActivatingModel ? "activating" : "pending")} | Logs: {activeDownloadStatus?.logs.length ?? 0}]
                    </div>
                    {activeDownloadStatus?.logs.map((L, i) => (
                        <div key={i} style={{ marginBottom: 4 }}>{L}</div>
                    ))}
                    <div ref={logsEndRef} />
                </div>
            )}

            {error && <div className="ob-error-box">{error}</div>}

            {!isDownloading && (
                <>
                    <button
                        id="ob-download-model"
                        className="ob-btn-primary"
                        onClick={handleDownload}
                        disabled={!selected}
                    >
                        {alreadyDownloaded
                            ? `Use ${selected?.name}`
                            : `Download ${selected?.name ?? ""} · ${selected?.size_label ?? ""}`}
                    </button>
                    <button
                        className="ob-btn-ghost"
                        onClick={() => onSave({ ...state, step: "permissions" })}
                    >
                        Skip for now
                    </button>
                </>
            )}

            {isDownloading && (
                <button
                    className="ob-btn-ghost"
                    onClick={() => onSave({ ...state, step: "permissions" })}
                >
                    Skip for now
                </button>
            )}
        </>
    );
}

// ── Root Onboarding Component ─────────────────────────────────────────────
interface OnboardingProps {
    onComplete: (state: OnboardingState) => void;
}

export function Onboarding({ onComplete }: OnboardingProps) {
    const [state, setState] = useState<OnboardingState | null>(null);

    useEffect(() => {
        getOnboardingState()
            .then(setState)
            .catch(() => setState(DEFAULT_ONBOARDING_STATE));
    }, []);

    const save = useCallback(
        async (next: OnboardingState) => {
            setState(next);
            await saveOnboardingState(next);
            if (next.step === "complete") {
                onComplete(next);
            }
        },
        [onComplete]
    );

    if (!state) return null;

    return (
        <div className="onboarding-overlay">
            <div className="ob-card">
                {state.step !== "welcome" && state.step !== "complete" && (
                    <StepDots current={state.step === "indexing_started" ? "permissions" : state.step} />
                )}

                {state.step === "welcome" && <StepWelcome state={state} onSave={save} />}
                {state.step === "biometrics" && <StepBiometrics state={state} onSave={save} />}
                {state.step === "privacy_promise" && <StepPrivacyPromise state={state} onSave={save} />}
                {state.step === "model_download" && <StepModelDownload state={state} onSave={save} />}
                {(state.step === "permissions" || state.step === "indexing_started") && (
                    <StepPermissions state={state} onSave={save} />
                )}
            </div>
        </div>
    );
}
