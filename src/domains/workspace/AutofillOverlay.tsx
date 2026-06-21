import { type FormEvent, useEffect, useMemo, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
    type AutofillCandidate,
    type AutofillOverlayPayload,
    type AutofillResolution,
    type FieldContext,
    dismissAutofill,
    injectText,
    resolveAutofill,
    setAutofillOverlayReady,
    takePendingAutofillPayload,
} from "@/shared/ipc/tauri";

const SUCCESS_TOAST_MS = 900;
const ERROR_TOAST_MS = 2200;

type Phase =
    | { kind: "idle" }
    | {
        kind: "searching";
        label: string;
        appName: string;
        windowTitle: string;
        contextHint: string;
    }
    | {
        kind: "manual";
        appName: string;
        windowTitle: string;
        contextHint: string;
        message?: string;
    }
    | {
        kind: "preview";
        label: string;
        resolution: AutofillResolution;
        selectedIndex: number;
        appName: string;
        windowTitle: string;
        contextHint: string;
    }
    | { kind: "injecting"; label: string; candidate: AutofillCandidate }
    | { kind: "done"; label: string; candidate: AutofillCandidate }
    | { kind: "error"; message: string };

function normalizePhrase(input: string): string {
    return input
        .toLowerCase()
        .replace(/[^a-z0-9#]+/g, " ")
        .trim()
        .replace(/\s+/g, " ");
}

function confidenceLabel(confidence: number): string {
    return `${Math.round(confidence * 100)}% match`;
}

function confidenceTone(confidence: number): "high" | "medium" | "low" {
    if (confidence >= 0.94) return "high";
    if (confidence >= 0.84) return "medium";
    return "low";
}

function timeAgo(timestampMs: number): string {
    const delta = Date.now() - timestampMs;
    const days = Math.floor(delta / 86_400_000);
    if (days <= 0) return "today";
    if (days === 1) return "yesterday";
    if (days < 7) return `${days}d ago`;
    if (days < 30) return `${Math.floor(days / 7)}w ago`;
    return `${Math.floor(days / 30)}mo ago`;
}

function labelFromContext(context: FieldContext): string {
    return context.label || context.inferred_label || context.placeholder || "";
}

function contextHintFromContext(context: FieldContext): string {
    return context.screen_context || context.window_title || context.app_name || "";
}

function isEditableTarget(target: EventTarget | null): boolean {
    if (!(target instanceof HTMLElement)) {
        return false;
    }
    return (
        target.tagName === "INPUT"
        || target.tagName === "TEXTAREA"
        || target.isContentEditable
    );
}

function isErrorPayload(payload: AutofillOverlayPayload): payload is { error: string } {
    return "error" in payload;
}

function isScanningPayload(
    payload: AutofillOverlayPayload,
): payload is { scanning: true; message?: string } {
    return "scanning" in payload;
}

export function AutofillOverlay() {
    const [phase, setPhase] = useState<Phase>({ kind: "idle" });
    const [query, setQuery] = useState("");
    const [overlayVisible, setOverlayVisible] = useState(false);
    const contextRef = useRef<FieldContext | null>(null);
    const queryInputRef = useRef<HTMLInputElement>(null);
    const dismissTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
    const resolveTokenRef = useRef(0);

    function clearDismissTimer() {
        if (dismissTimer.current) {
            clearTimeout(dismissTimer.current);
            dismissTimer.current = null;
        }
    }

    function hideOverlay() {
        void dismissAutofill().catch(() => {});
    }

    function resetAndHide() {
        clearDismissTimer();
        resolveTokenRef.current += 1;
        setOverlayVisible(false);
        setPhase({ kind: "idle" });
        setQuery("");
        hideOverlay();
    }

    function scheduleDismiss(delayMs: number) {
        clearDismissTimer();
        dismissTimer.current = setTimeout(() => {
            resetAndHide();
        }, delayMs);
    }

    function showManual(context: FieldContext, message?: string) {
        setOverlayVisible(true);
        setPhase({
            kind: "manual",
            appName: context.app_name,
            windowTitle: context.window_title,
            contextHint: contextHintFromContext(context),
            message,
        });
    }

    async function acceptCandidate(label: string, candidate: AutofillCandidate) {
        clearDismissTimer();
        resolveTokenRef.current += 1;
        setOverlayVisible(true);

        try {
            setPhase({ kind: "injecting", label, candidate });
            await injectText(candidate.value);
            setPhase({ kind: "done", label, candidate });
            scheduleDismiss(SUCCESS_TOAST_MS);
        } catch (error) {
            setPhase({ kind: "error", message: String(error) });
            scheduleDismiss(ERROR_TOAST_MS);
        }
    }

    async function runResolution(context: FieldContext, nextQuery?: string) {
        const pendingLabel = (nextQuery ?? query ?? labelFromContext(context)).trim();
        if (!pendingLabel) {
            showManual(context, "No field label was detected yet. Type what you want Continuum to find.");
            return;
        }

        clearDismissTimer();
        setQuery(pendingLabel);
        setOverlayVisible(true);

        const token = resolveTokenRef.current + 1;
        resolveTokenRef.current = token;

        setPhase({
            kind: "searching",
            label: pendingLabel,
            appName: context.app_name,
            windowTitle: context.window_title,
            contextHint: contextHintFromContext(context),
        });

        try {
            const resolution = await resolveAutofill(context, pendingLabel);
            if (token !== resolveTokenRef.current) {
                return;
            }

            const label = resolution.query || pendingLabel;
            setQuery(label);

            if (resolution.candidates.length === 0) {
                showManual(context, `No strong matches for "${pendingLabel}" yet. Refine the search and press Enter again.`);
                return;
            }

            const topCandidate = resolution.candidates[0];
            if (
                !resolution.requires_confirmation
                && topCandidate.confidence >= resolution.auto_inject_threshold
            ) {
                await acceptCandidate(label, topCandidate);
                return;
            }

            setPhase({
                kind: "preview",
                label,
                resolution,
                selectedIndex: 0,
                appName: context.app_name,
                windowTitle: context.window_title,
                contextHint: contextHintFromContext(context),
            });
        } catch (error) {
            if (token !== resolveTokenRef.current) {
                return;
            }
            setPhase({ kind: "error", message: String(error) });
            scheduleDismiss(ERROR_TOAST_MS);
        }
    }

    async function syncPendingPayload(showFallback = false) {
        setOverlayVisible(true);
        try {
            const pending = await takePendingAutofillPayload();
            if (pending) {
                await handlePayload(pending);
                return true;
            }
        } catch {
            // Keep the overlay visible even if the payload fetch fails once.
        }

        if (showFallback) {
            setPhase((current) =>
                current.kind === "idle"
                    ? {
                        kind: "searching",
                        label: "Preparing autofill",
                        appName: "Continuum",
                        windowTitle: "",
                        contextHint: "",
                    }
                    : current,
            );
        }

        return false;
    }

    async function handlePayload(payload: AutofillOverlayPayload) {
        clearDismissTimer();
        setOverlayVisible(true);

        if (isScanningPayload(payload)) {
            setPhase((current) =>
                current.kind === "idle"
                    ? {
                        kind: "searching",
                        label: payload.message || "Searching memories",
                        appName: "Continuum",
                        windowTitle: "",
                        contextHint: "",
                    }
                    : current,
            );
            void syncPendingPayload(false);
            return;
        }

        if (isErrorPayload(payload)) {
            setPhase({ kind: "error", message: payload.error });
            scheduleDismiss(ERROR_TOAST_MS);
            return;
        }

        const context = payload;
        contextRef.current = context;

        const seededQuery = labelFromContext(context);
        setQuery(seededQuery);

        if (!seededQuery && !context.screen_context.trim()) {
            showManual(context);
            return;
        }

        await runResolution(context, seededQuery || undefined);
    }

    useEffect(() => {
        let unlisten: UnlistenFn | null = null;
        let isMounted = true;

        void setAutofillOverlayReady(true).then((pending) => {
            if (isMounted && pending) {
                void handlePayload(pending);
            }
        });

        listen<AutofillOverlayPayload>("autofill-triggered", (event) => {
            void handlePayload(event.payload);
        }).then((fn) => {
            unlisten = fn;
        });

        function handleWindowVisible() {
            const visible = document.visibilityState === "visible" || document.hasFocus();
            setOverlayVisible(visible);
            if (visible) {
                void syncPendingPayload(true);
            }
        }

        window.addEventListener("focus", handleWindowVisible);
        document.addEventListener("visibilitychange", handleWindowVisible);

        return () => {
            isMounted = false;
            clearDismissTimer();
            resolveTokenRef.current += 1;
            unlisten?.();
            window.removeEventListener("focus", handleWindowVisible);
            document.removeEventListener("visibilitychange", handleWindowVisible);
            void setAutofillOverlayReady(false).catch(() => {});
        };
    }, []);

    // Only auto-focus the search input in manual mode. In preview mode the input
    // being focused would intercept ArrowUp/Down/1-9 keyboard shortcuts (isEditableTarget
    // guard in the key handler ignores those keys when an input is focused).
    useEffect(() => {
        if (phase.kind !== "manual") {
            return;
        }

        const timer = window.setTimeout(() => {
            queryInputRef.current?.focus();
            queryInputRef.current?.select();
        }, 40);

        return () => window.clearTimeout(timer);
    }, [phase.kind]);

    const selectedCandidate =
        phase.kind === "preview"
            ? phase.resolution.candidates[phase.selectedIndex] ?? phase.resolution.candidates[0]
            : null;

    const queryMatchesSelection = useMemo(() => {
        if (phase.kind !== "preview") {
            return false;
        }
        return normalizePhrase(query) === normalizePhrase(phase.label);
    }, [phase, query]);

    useEffect(() => {
        function handleKey(event: KeyboardEvent) {
            if (phase.kind === "idle") {
                return;
            }

            if (event.key === "Escape") {
                event.preventDefault();
                resetAndHide();
                return;
            }

            if (phase.kind !== "preview" || isEditableTarget(event.target)) {
                return;
            }

            if (event.key === "Enter") {
                event.preventDefault();
                const candidate = phase.resolution.candidates[phase.selectedIndex];
                if (candidate) {
                    void acceptCandidate(phase.label, candidate);
                }
                return;
            }

            if (event.key === "ArrowDown") {
                event.preventDefault();
                setPhase((current) =>
                    current.kind !== "preview"
                        ? current
                        : {
                            ...current,
                            selectedIndex: Math.min(
                                current.selectedIndex + 1,
                                current.resolution.candidates.length - 1,
                            ),
                        },
                );
                return;
            }

            if (event.key === "ArrowUp") {
                event.preventDefault();
                setPhase((current) =>
                    current.kind !== "preview"
                        ? current
                        : {
                            ...current,
                            selectedIndex: Math.max(current.selectedIndex - 1, 0),
                        },
                );
                return;
            }

            if (/^[1-9]$/.test(event.key)) {
                const index = Number(event.key) - 1;
                const candidate = phase.resolution.candidates[index];
                if (candidate) {
                    event.preventDefault();
                    void acceptCandidate(phase.label, candidate);
                }
            }
        }

        window.addEventListener("keydown", handleKey);
        return () => window.removeEventListener("keydown", handleKey);
    }, [phase]);

    const showBootstrapSurface = phase.kind === "idle" && overlayVisible;

    if (phase.kind === "idle" && !showBootstrapSurface) {
        return null;
    }

    const showSearchSurface =
        showBootstrapSurface
        || phase.kind === "searching"
        || phase.kind === "manual"
        || phase.kind === "preview";
    const searchButtonLabel =
        phase.kind === "preview" && queryMatchesSelection
            ? "Insert"
            : phase.kind === "searching" || showBootstrapSurface
                ? "Searching..."
                : "Search";
    const contextAppName =
        phase.kind === "manual" || phase.kind === "preview" || phase.kind === "searching"
            ? phase.appName
            : contextRef.current?.app_name ?? "Continuum";
    const contextWindowTitle =
        phase.kind === "manual" || phase.kind === "preview" || phase.kind === "searching"
            ? phase.windowTitle
            : contextRef.current?.window_title ?? "";
    const contextHint =
        phase.kind === "manual" || phase.kind === "preview" || phase.kind === "searching"
            ? phase.contextHint
            : contextRef.current?.screen_context ?? "";

    async function submitQuery(event: FormEvent) {
        event.preventDefault();
        const context = contextRef.current;
        if (!context) {
            return;
        }

        const nextQuery = query.trim();
        if (phase.kind === "preview" && selectedCandidate && queryMatchesSelection) {
            await acceptCandidate(phase.label, selectedCandidate);
            return;
        }

        await runResolution(context, nextQuery);
    }

    return (
        <div className="af-overlay" role="dialog" aria-modal="false" aria-label="Continuum Autofill">
            {showSearchSurface && (
                <div className={`af-card af-main-card ${phase.kind}`}>
                    <div className="af-header">
                        <div className="af-brand">
                            <span className="af-brand-mark">Continuum</span>
                            <span className="af-brand-state">
                                {phase.kind === "preview"
                                    ? "Smart Fill"
                                    : phase.kind === "manual"
                                        ? "Search Memory"
                                        : "Searching"}
                            </span>
                        </div>
                        <button
                            className="af-close"
                            onClick={resetAndHide}
                            aria-label="Dismiss"
                            type="button"
                        >
                            ×
                        </button>
                    </div>

                    <form className="af-search-row" onSubmit={(event) => void submitQuery(event)}>
                        <input
                            ref={queryInputRef}
                            className="af-search-input"
                            value={query}
                            onChange={(event) => setQuery(event.target.value)}
                            onKeyDown={(event) => {
                                if (event.key === "Escape") {
                                    event.preventDefault();
                                    resetAndHide();
                                }
                            }}
                            placeholder="Search for policy number, EIN, member ID..."
                            autoComplete="off"
                            spellCheck={false}
                            disabled={showBootstrapSurface}
                        />
                        <button
                            className="af-search-btn"
                            type="submit"
                            disabled={phase.kind === "searching" || showBootstrapSurface}
                        >
                            {searchButtonLabel}
                        </button>
                    </form>

                    <div className="af-context-row">
                        <span className="af-context-app">{contextAppName || "Continuum"}</span>
                        <span className="af-context-window">{contextWindowTitle}</span>
                    </div>

                    {(showBootstrapSurface || phase.kind === "searching") && (
                        <>
                            <div className="af-searching-panel">
                                <span className="af-spinner" aria-hidden />
                                <div className="af-searching-copy">
                                    <span className="af-searching-title">Searching memories</span>
                                    <span className="af-searching-value">
                                        {showBootstrapSurface
                                            ? "Preparing the focused field context"
                                            : phase.kind === "searching"
                                                ? phase.label
                                                : "Using visible form context"}
                                    </span>
                                </div>
                            </div>
                            {contextHint && (
                                <div className="af-context-box">
                                    <span className="af-context-label">Visible form context</span>
                                    <span className="af-context-text">{contextHint}</span>
                                </div>
                            )}
                            <div className="af-footer-hint">
                                Continuum is using the active field plus nearby screen context to rank matches.
                            </div>
                        </>
                    )}

                    {phase.kind === "manual" && (
                        <>
                            <div className="af-empty-state">
                                <span className="af-empty-title">Search memory for this field</span>
                                <span className="af-empty-copy">
                                    Continuum needs a better search phrase for this form input. Edit the query and press Enter.
                                </span>
                            </div>
                            {phase.message && (
                                <div className="af-banner af-banner-soft">{phase.message}</div>
                            )}
                            {contextHint && (
                                <div className="af-context-box">
                                    <span className="af-context-label">Visible form context</span>
                                    <span className="af-context-text">{contextHint}</span>
                                </div>
                            )}
                            <div className="af-footer-hint">Enter searches again. Esc closes.</div>
                        </>
                    )}

                    {phase.kind === "preview" && selectedCandidate && (
                        <>
                            <div className="af-selection-card">
                                <div className="af-selection-top">
                                    <span className="af-field-chip">{phase.label}</span>
                                    <span
                                        className={`af-confidence-badge ${confidenceTone(selectedCandidate.confidence)}`}
                                    >
                                        {confidenceLabel(selectedCandidate.confidence)}
                                    </span>
                                </div>
                                <span className="af-selection-value">{selectedCandidate.value}</span>
                                <span className="af-selection-reason">
                                    {selectedCandidate.match_reason}
                                </span>
                                <div className="af-selection-source">
                                    <span>
                                        {selectedCandidate.source_window_title || selectedCandidate.source_app}
                                    </span>
                                    <span>{timeAgo(selectedCandidate.timestamp)}</span>
                                </div>
                            </div>

                            {selectedCandidate.confidence < phase.resolution.auto_inject_threshold && (
                                <div className="af-banner af-banner-warn">
                                    Context was strong enough to rank this memory first, but Continuum wants confirmation before inserting it.
                                </div>
                            )}

                            {selectedCandidate.source_snippet && (
                                <div className="af-context-box">
                                    <span className="af-context-label">Why this memory</span>
                                    <span className="af-context-text">{selectedCandidate.source_snippet}</span>
                                </div>
                            )}

                            {phase.resolution.candidates.length > 1 && (
                                <div className="af-candidate-list">
                                    {phase.resolution.candidates.map((candidate, index) => (
                                        <button
                                            key={`${candidate.memory_id}-${candidate.value}-${index}`}
                                            type="button"
                                            className={`af-candidate ${index === phase.selectedIndex ? "selected" : ""}`}
                                            onClick={() =>
                                                setPhase((current) =>
                                                    current.kind !== "preview"
                                                        ? current
                                                        : { ...current, selectedIndex: index },
                                                )
                                            }
                                        >
                                            <span className="af-candidate-rank">{index + 1}</span>
                                            <div className="af-candidate-copy">
                                                <span className="af-candidate-value">{candidate.value}</span>
                                                <span className="af-candidate-meta">
                                                    {confidenceLabel(candidate.confidence)} · {timeAgo(candidate.timestamp)}
                                                </span>
                                            </div>
                                        </button>
                                    ))}
                                </div>
                            )}

                            <div className="af-actions">
                                <button
                                    type="button"
                                    className="af-primary"
                                    onClick={() => void acceptCandidate(phase.label, selectedCandidate)}
                                >
                                    Insert Selected
                                    <kbd>↵</kbd>
                                </button>
                                <button type="button" className="af-secondary" onClick={resetAndHide}>
                                    Dismiss
                                    <kbd>Esc</kbd>
                                </button>
                            </div>
                            <div className="af-footer-hint">
                                Press Enter to insert the selected value. Up, Down, or 1-9 switches candidates.
                            </div>
                        </>
                    )}
                </div>
            )}

            {(phase.kind === "injecting" || phase.kind === "done" || phase.kind === "error") && (
                <div className={`af-card af-inline-card ${phase.kind}`}>
                    {phase.kind === "injecting" ? (
                        <span className="af-spinner" aria-hidden />
                    ) : (
                        <span className={`af-inline-icon ${phase.kind}`}>
                            {phase.kind === "done" ? "✓" : "!"}
                        </span>
                    )}
                    <div className="af-inline-copy">
                        <span className="af-inline-label">
                            {phase.kind === "injecting" && "Inserting into active field"}
                            {phase.kind === "done" && "Filled field"}
                            {phase.kind === "error" && "Auto-fill hit an issue"}
                        </span>
                        <span className="af-inline-value">
                            {phase.kind === "injecting" && phase.candidate.value}
                            {phase.kind === "done"
                                && `${phase.label} from ${phase.candidate.source_window_title || phase.candidate.source_app}`}
                            {phase.kind === "error" && phase.message}
                        </span>
                    </div>
                    <button className="af-close" onClick={resetAndHide} aria-label="Dismiss" type="button">
                        ×
                    </button>
                </div>
            )}

            <style>{`
                .af-overlay {
                    position: fixed;
                    inset: 0;
                    display: flex;
                    align-items: stretch;
                    justify-content: stretch;
                    padding: 0;
                    pointer-events: none;
                    background:
                        radial-gradient(circle at top right, rgba(255, 194, 110, 0.14), transparent 28%),
                        linear-gradient(155deg, rgba(27, 21, 16, 0.985), rgba(14, 11, 9, 0.985));
                    font-family: "SF Pro Text", "Avenir Next", "Helvetica Neue", system-ui, sans-serif;
                    -webkit-font-smoothing: antialiased;
                }

                .af-card {
                    pointer-events: all;
                    color: rgba(250, 246, 239, 0.94);
                    background: transparent;
                    animation: af-slide-in 0.18s cubic-bezier(0.25, 1, 0.5, 1) both;
                }

                .af-main-card {
                    display: flex;
                    flex-direction: column;
                    gap: 12px;
                    width: 100%;
                    min-height: 100%;
                    padding: 18px 18px 16px;
                    overflow-y: auto;
                    border: 1px solid rgba(255, 196, 122, 0.14);
                    box-shadow:
                        inset 0 1px 0 rgba(255, 255, 255, 0.04),
                        0 18px 44px rgba(0, 0, 0, 0.28);
                }

                .af-inline-card {
                    display: flex;
                    align-items: center;
                    gap: 10px;
                    width: min(448px, calc(100vw - 24px));
                    height: auto;
                    padding: 12px 14px;
                    margin: auto 12px 12px auto;
                    border-radius: 22px;
                    border: 1px solid rgba(255, 196, 122, 0.18);
                    background:
                        radial-gradient(circle at top right, rgba(255, 194, 110, 0.14), transparent 30%),
                        linear-gradient(155deg, rgba(27, 21, 16, 0.96), rgba(14, 11, 9, 0.96));
                    box-shadow:
                        0 22px 56px rgba(0, 0, 0, 0.52),
                        inset 0 1px 0 rgba(255, 255, 255, 0.04);
                }

                .af-inline-card.done {
                    border-color: rgba(104, 212, 140, 0.24);
                    background:
                        radial-gradient(circle at top right, rgba(104, 212, 140, 0.10), transparent 30%),
                        linear-gradient(155deg, rgba(18, 28, 20, 0.96), rgba(12, 15, 12, 0.96));
                }

                .af-inline-card.error {
                    border-color: rgba(255, 141, 117, 0.24);
                    background:
                        radial-gradient(circle at top right, rgba(255, 141, 117, 0.10), transparent 30%),
                        linear-gradient(155deg, rgba(35, 21, 18, 0.96), rgba(20, 11, 10, 0.96));
                }

                .af-header {
                    display: flex;
                    align-items: center;
                    justify-content: space-between;
                    gap: 10px;
                }

                .af-brand {
                    display: flex;
                    flex-direction: column;
                    gap: 2px;
                }

                .af-brand-mark {
                    font-size: 10px;
                    font-weight: 900;
                    letter-spacing: 0.12em;
                    text-transform: uppercase;
                    color: #fff;
                    background: linear-gradient(135deg, #ffc47a, #f97316);
                    padding: 3px 8px;
                    border-radius: 8px;
                    width: fit-content;
                    margin-bottom: 2px;
                    box-shadow: 0 4px 12px rgba(249, 115, 22, 0.2);
                }

                .af-brand-state {
                    font-size: 12px;
                    color: rgba(250, 246, 239, 0.56);
                }

                .af-close {
                    width: 30px;
                    height: 30px;
                    border-radius: 999px;
                    border: 1px solid rgba(255, 255, 255, 0.10);
                    background: rgba(255, 255, 255, 0.04);
                    color: rgba(250, 246, 239, 0.62);
                    font-size: 18px;
                    line-height: 1;
                    cursor: pointer;
                }

                .af-close:hover {
                    background: rgba(255, 255, 255, 0.08);
                    color: rgba(250, 246, 239, 0.92);
                }

                .af-search-row {
                    display: grid;
                    grid-template-columns: 1fr auto;
                    gap: 8px;
                }

                .af-search-input {
                    width: 100%;
                    min-width: 0;
                    border-radius: 14px;
                    border: 1px solid rgba(255, 255, 255, 0.10);
                    background:
                        linear-gradient(180deg, rgba(255, 255, 255, 0.07), rgba(255, 255, 255, 0.03));
                    padding: 12px 14px;
                    color: rgba(250, 246, 239, 0.96);
                    font-size: 14px;
                    outline: none;
                    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.04);
                    caret-color: rgba(120, 205, 255, 0.96);
                }

                .af-search-input:focus {
                    border-color: rgba(120, 205, 255, 0.42);
                    box-shadow:
                        0 0 0 3px rgba(120, 205, 255, 0.12),
                        inset 0 1px 0 rgba(255, 255, 255, 0.06);
                }

                .af-search-input::placeholder {
                    color: rgba(250, 246, 239, 0.30);
                }

                .af-search-input:disabled {
                    opacity: 0.72;
                    cursor: default;
                }

                .af-search-btn,
                .af-primary,
                .af-secondary {
                    border: 1px solid transparent;
                    border-radius: 14px;
                    font-family: inherit;
                    font-size: 13px;
                    font-weight: 600;
                    cursor: pointer;
                    transition: transform 0.12s ease, background 0.12s ease, border-color 0.12s ease;
                }

                .af-search-btn {
                    padding: 0 14px;
                    background: linear-gradient(180deg, rgba(255, 197, 112, 0.24), rgba(255, 161, 67, 0.14));
                    border-color: rgba(255, 197, 112, 0.26);
                    color: rgba(255, 220, 170, 0.98);
                }

                .af-search-btn:disabled {
                    cursor: default;
                    opacity: 0.7;
                    transform: none;
                }

                .af-search-btn:hover:not(:disabled),
                .af-primary:hover,
                .af-secondary:hover {
                    transform: translateY(-1px);
                }

                .af-context-row {
                    display: flex;
                    align-items: center;
                    gap: 8px;
                    min-width: 0;
                    color: rgba(250, 246, 239, 0.40);
                    font-size: 11px;
                }

                .af-context-app {
                    flex-shrink: 0;
                    padding: 3px 8px;
                    border-radius: 999px;
                    background: rgba(255, 255, 255, 0.06);
                    border: 1px solid rgba(255, 255, 255, 0.08);
                }

                .af-context-window {
                    overflow: hidden;
                    text-overflow: ellipsis;
                    white-space: nowrap;
                }

                .af-searching-panel,
                .af-empty-state {
                    display: flex;
                    flex-direction: column;
                    gap: 6px;
                }

                .af-searching-panel {
                    flex-direction: row;
                    align-items: center;
                    gap: 12px;
                    padding: 4px 0;
                }

                .af-searching-copy {
                    display: flex;
                    flex-direction: column;
                    gap: 3px;
                    min-width: 0;
                }

                .af-searching-title,
                .af-empty-title {
                    font-size: 16px;
                    font-weight: 700;
                    color: rgba(250, 246, 239, 0.96);
                }

                .af-searching-value,
                .af-empty-copy {
                    font-size: 12px;
                    line-height: 1.45;
                    color: rgba(250, 246, 239, 0.58);
                }

                .af-banner {
                    border-radius: 14px;
                    padding: 10px 12px;
                    font-size: 12px;
                    line-height: 1.45;
                }

                .af-banner-soft {
                    border: 1px solid rgba(120, 205, 255, 0.16);
                    background: rgba(120, 205, 255, 0.08);
                    color: rgba(194, 234, 255, 0.92);
                }

                .af-banner-warn {
                    border: 1px solid rgba(255, 194, 110, 0.20);
                    background: rgba(255, 194, 110, 0.09);
                    color: rgba(255, 221, 170, 0.94);
                }

                .af-context-box {
                    display: flex;
                    flex-direction: column;
                    gap: 5px;
                    padding: 12px;
                    border-radius: 16px;
                    border: 1px solid rgba(255, 255, 255, 0.08);
                    background: rgba(255, 255, 255, 0.04);
                }

                .af-context-label {
                    font-size: 10px;
                    font-weight: 700;
                    letter-spacing: 0.10em;
                    text-transform: uppercase;
                    color: rgba(250, 246, 239, 0.34);
                }

                .af-context-text {
                    font-size: 12px;
                    line-height: 1.5;
                    color: rgba(250, 246, 239, 0.62);
                    white-space: pre-line;
                }

                .af-selection-card {
                    display: flex;
                    flex-direction: column;
                    gap: 8px;
                    padding: 14px;
                    border-radius: 18px;
                    border: 1px solid rgba(255, 196, 122, 0.14);
                    background:
                        radial-gradient(circle at top left, rgba(255, 188, 92, 0.12), transparent 35%),
                        rgba(255, 255, 255, 0.04);
                }

                .af-selection-top {
                    display: flex;
                    align-items: center;
                    gap: 8px;
                    justify-content: space-between;
                }

                .af-field-chip {
                    min-width: 0;
                    max-width: 60%;
                    overflow: hidden;
                    text-overflow: ellipsis;
                    white-space: nowrap;
                    border-radius: 999px;
                    padding: 4px 10px;
                    background: rgba(255, 255, 255, 0.07);
                    border: 1px solid rgba(255, 255, 255, 0.10);
                    font-size: 11px;
                    color: rgba(250, 246, 239, 0.66);
                }

                .af-confidence-badge {
                    border-radius: 999px;
                    padding: 4px 10px;
                    font-size: 11px;
                    font-weight: 700;
                    flex-shrink: 0;
                }

                .af-confidence-badge.high {
                    background: rgba(104, 212, 140, 0.14);
                    color: rgba(145, 239, 174, 0.96);
                }

                .af-confidence-badge.medium {
                    background: rgba(255, 194, 110, 0.14);
                    color: rgba(255, 220, 166, 0.96);
                }

                .af-confidence-badge.low {
                    background: rgba(255, 140, 110, 0.14);
                    color: rgba(255, 188, 165, 0.96);
                }

                .af-selection-value {
                    font-size: 24px;
                    line-height: 1.15;
                    letter-spacing: -0.02em;
                    font-weight: 800;
                    color: rgba(255, 244, 226, 0.98);
                    word-break: break-word;
                }

                .af-selection-reason {
                    font-size: 12px;
                    line-height: 1.45;
                    color: rgba(250, 246, 239, 0.58);
                }

                .af-selection-source {
                    display: flex;
                    align-items: center;
                    justify-content: space-between;
                    gap: 10px;
                    font-size: 11px;
                    color: rgba(250, 246, 239, 0.38);
                }

                .af-candidate-list {
                    display: flex;
                    flex-direction: column;
                    gap: 8px;
                    max-height: 188px;
                    overflow-y: auto;
                }

                .af-candidate {
                    display: grid;
                    grid-template-columns: 28px 1fr;
                    gap: 10px;
                    align-items: start;
                    width: 100%;
                    padding: 10px;
                    border-radius: 16px;
                    border: 1px solid rgba(255, 255, 255, 0.08);
                    background: rgba(255, 255, 255, 0.03);
                    color: inherit;
                    text-align: left;
                }

                .af-candidate.selected {
                    border-color: rgba(120, 205, 255, 0.28);
                    background: rgba(120, 205, 255, 0.09);
                }

                .af-candidate-rank {
                    width: 28px;
                    height: 28px;
                    border-radius: 999px;
                    display: inline-flex;
                    align-items: center;
                    justify-content: center;
                    background: rgba(255, 255, 255, 0.08);
                    color: rgba(250, 246, 239, 0.58);
                    font-size: 11px;
                    font-weight: 700;
                }

                .af-candidate.selected .af-candidate-rank {
                    background: rgba(120, 205, 255, 0.18);
                    color: rgba(194, 234, 255, 0.96);
                }

                .af-candidate-copy {
                    display: flex;
                    flex-direction: column;
                    gap: 4px;
                    min-width: 0;
                }

                .af-candidate-value {
                    font-size: 13px;
                    font-weight: 700;
                    color: rgba(250, 246, 239, 0.92);
                    word-break: break-word;
                }

                .af-candidate-meta {
                    font-size: 11px;
                    color: rgba(250, 246, 239, 0.42);
                }

                .af-actions {
                    display: grid;
                    grid-template-columns: 1fr 1fr;
                    gap: 8px;
                }

                .af-primary,
                .af-secondary {
                    display: inline-flex;
                    align-items: center;
                    justify-content: center;
                    gap: 6px;
                    padding: 12px 14px;
                }

                .af-primary {
                    background: linear-gradient(180deg, rgba(255, 194, 110, 0.24), rgba(255, 161, 67, 0.14));
                    border-color: rgba(255, 194, 110, 0.24);
                    color: rgba(255, 231, 196, 0.98);
                }

                .af-secondary {
                    background: rgba(255, 255, 255, 0.05);
                    border-color: rgba(255, 255, 255, 0.10);
                    color: rgba(250, 246, 239, 0.72);
                }

                .af-primary kbd,
                .af-secondary kbd {
                    font-size: 11px;
                    opacity: 0.7;
                }

                .af-footer-hint {
                    font-size: 11px;
                    color: rgba(250, 246, 239, 0.34);
                }

                .af-spinner {
                    width: 18px;
                    height: 18px;
                    border-radius: 999px;
                    border: 2px solid rgba(255, 255, 255, 0.14);
                    border-top-color: rgba(255, 226, 183, 0.92);
                    animation: af-spin 0.7s linear infinite;
                    flex-shrink: 0;
                }

                .af-inline-icon {
                    width: 22px;
                    height: 22px;
                    border-radius: 999px;
                    display: inline-flex;
                    align-items: center;
                    justify-content: center;
                    font-size: 12px;
                    font-weight: 800;
                    flex-shrink: 0;
                }

                .af-inline-icon.done {
                    background: rgba(104, 212, 140, 0.16);
                    color: rgba(145, 239, 174, 0.96);
                }

                .af-inline-icon.error {
                    background: rgba(255, 140, 110, 0.16);
                    color: rgba(255, 194, 182, 0.96);
                }

                .af-inline-copy {
                    display: flex;
                    flex-direction: column;
                    gap: 2px;
                    min-width: 0;
                    flex: 1;
                }

                .af-inline-label {
                    font-size: 11px;
                    color: rgba(250, 246, 239, 0.40);
                }

                .af-inline-value {
                    font-size: 13px;
                    color: rgba(250, 246, 239, 0.88);
                    line-height: 1.4;
                    word-break: break-word;
                }

                @keyframes af-slide-in {
                    from {
                        opacity: 0;
                        transform: translateY(10px) scale(0.98);
                    }
                    to {
                        opacity: 1;
                        transform: translateY(0) scale(1);
                    }
                }

                @keyframes af-spin {
                    to {
                        transform: rotate(360deg);
                    }
                }
            `}</style>
        </div>
    );
}
