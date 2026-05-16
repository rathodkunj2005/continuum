import { useCallback, useEffect, useRef, useState } from "react";
import { open as shellOpen } from "@tauri-apps/plugin-shell";
import { usePolling } from "@/shared/hooks/usePolling";
import { createClientId } from "@/shared/utils/id";
import {
    type AgentStatus,
    type AgentMode,
    type AgentRunResponse,
    type AgentAuditRecord,
    type AgentEvalCase,
    type AgentSkillCandidate,
    type RetrievalExplanation,
    type RetrievalFeedbackRating,
    type ContextPack,
    type ContextRuntimeStatus,
    type HermesBridgeStatus,
    getAgentStatus,
    getContextRuntimeStatus,
    getHermesBridgeStatus,
    installHermesBridge,
    explainAgentRetrieval,
    getAgentAuditRun,
    listRecentContextPacks,
    listAgentAuditRuns,
    proposeEvalFromRun,
    proposeSkillFromRun,
    quickSetupOllama,
    rateAgentResult,
    runAgentRequest,
    saveHermesSetup,
    sendDirectChat,
    sendHermesMessage,
    startHermesGateway,
    stopAgent,
    stopHermesGateway,
    syncHermesBridgeContext,
    fndrSubscribe,
    fndrUnsubscribe,
    onContextDelta,
    type ContextDelta,
} from "@/shared/ipc/tauri";
import "./AgentPanel.css";

interface AgentPanelProps {
    isVisible: boolean;
    onClose: () => void;
}

type AgentView = "overview" | "hermes";
type HermesProviderKind = "ollama" | "codex" | "openrouter" | "custom";

interface HermesUiMessage {
    role: "user" | "assistant";
    content: string;
}

const HERMES_DOCS_URL = "https://hermes-agent.nousresearch.com/docs/";
const OLLAMA_DOWNLOAD_URL = "https://ollama.com/download";
const DEFAULT_OLLAMA_BASE_URL = "http://127.0.0.1:11434/v1";

function nextConversationId(): string {
    return createClientId("fndr-hermes");
}

function isProviderKind(value: string | null | undefined): value is HermesProviderKind {
    return value === "ollama" || value === "codex" || value === "openrouter" || value === "custom";
}

function inferInitialProvider(hermes: HermesBridgeStatus | null): HermesProviderKind {
    if (isProviderKind(hermes?.provider_kind)) {
        return hermes.provider_kind;
    }
    if (hermes?.ollama_installed && hermes?.ollama_reachable) {
        return "ollama";
    }
    if (hermes?.codex_logged_in) {
        return "codex";
    }
    return "openrouter";
}

function defaultModelForProvider(
    provider: HermesProviderKind,
    hermes: HermesBridgeStatus | null
): string {
    if (provider === "ollama") {
        return hermes?.ollama_models[0] ?? "llama3.2:latest";
    }
    if (provider === "codex") {
        return "gpt-5.3-codex";
    }
    if (provider === "custom") {
        return "gpt-4.1-mini";
    }
    return "openai/gpt-5-mini";
}

function defaultBaseUrlForProvider(
    provider: HermesProviderKind,
    hermes: HermesBridgeStatus | null
): string {
    if (provider === "ollama") {
        return hermes?.ollama_base_url ?? DEFAULT_OLLAMA_BASE_URL;
    }
    if (provider === "custom") {
        return hermes?.provider_kind === "custom" ? hermes.base_url ?? "" : "";
    }
    return "";
}

async function openExternalUrl(url: string): Promise<void> {
    try {
        await shellOpen(url);
        return;
    } catch {
        // ignore
    }
    window.open(url, "_blank", "noopener,noreferrer");
}

function getReadinessStep(hermes: HermesBridgeStatus | null): number {
    if (!hermes) return 0;
    if (!hermes.installed) return hermes.direct_ollama_ready ? 4 : 0;
    if (!hermes.configured) return 1;
    if (hermes.api_server_ready) return 4;
    if (hermes.gateway_running) return 3;
    return 2;
}

function formatTimestamp(timestamp: number | null): string {
    if (!timestamp) return "Not synced";
    return new Date(timestamp).toLocaleString(undefined, {
        month: "short",
        day: "numeric",
        hour: "numeric",
        minute: "2-digit",
    });
}

export function AgentPanel({ isVisible, onClose }: AgentPanelProps) {
    const [activeView, setActiveView] = useState<AgentView>("overview");
    const [status, setStatus] = useState<AgentStatus | null>(null);
    const [hermes, setHermes] = useState<HermesBridgeStatus | null>(null);
    const [runtimeStatus, setRuntimeStatus] = useState<ContextRuntimeStatus | null>(null);
    const [recentPacks, setRecentPacks] = useState<ContextPack[]>([]);
    const [busyAction, setBusyAction] = useState<string | null>(null);
    const [hermesError, setHermesError] = useState<string | null>(null);
    const [providerKind, setProviderKind] = useState<HermesProviderKind>("openrouter");
    const [modelName, setModelName] = useState("openai/gpt-5-mini");
    const [apiKey, setApiKey] = useState("");
    const [baseUrl, setBaseUrl] = useState("");
    const [messages, setMessages] = useState<HermesUiMessage[]>([]);
    const [agentGoal, setAgentGoal] = useState("");
    const [agentMode, setAgentMode] = useState<AgentMode>("ask");
    const [agentRun, setAgentRun] = useState<AgentRunResponse | null>(null);
    const [agentRunError, setAgentRunError] = useState<string | null>(null);
    const [auditRuns, setAuditRuns] = useState<AgentAuditRecord[]>([]);
    const [selectedAudit, setSelectedAudit] = useState<AgentAuditRecord | null>(null);
    const [retrievalExplanation, setRetrievalExplanation] = useState<RetrievalExplanation | null>(null);
    const [agentDraftSkill, setAgentDraftSkill] = useState<AgentSkillCandidate | null>(null);
    const [agentDraftEval, setAgentDraftEval] = useState<AgentEvalCase | null>(null);
    const [agentInspectError, setAgentInspectError] = useState<string | null>(null);
    const [draft, setDraft] = useState("");
    const [conversationId, setConversationId] = useState(() => nextConversationId());
    const [hasSeededForm, setHasSeededForm] = useState(false);
    const [setupExpanded, setSetupExpanded] = useState(false);
    const [lastDelta, setLastDelta] = useState<ContextDelta | null>(null);
    const chatBottomRef = useRef<HTMLDivElement>(null);
    const chatInputRef = useRef<HTMLTextAreaElement>(null);

    const loadAgentWorkspace = useCallback(async (isMounted: () => boolean) => {
        try {
            const [agentStatus, hermesStatus, runtime, packs, runs] = await Promise.all([
                getAgentStatus(),
                getHermesBridgeStatus(),
                getContextRuntimeStatus(),
                listRecentContextPacks(2),
                listAgentAuditRuns(8),
            ]);
            if (isMounted()) {
                setStatus(agentStatus);
                setHermes(hermesStatus);
                setRuntimeStatus(runtime);
                setRecentPacks(packs);
                setAuditRuns(runs);
            }
        } catch (err) {
            console.error("Failed to load agent workspace:", err);
        }
    }, []);
    usePolling(loadAgentWorkspace, 4000, isVisible);

    useEffect(() => {
        if (!hermes || hasSeededForm) return;
        const nextProvider = inferInitialProvider(hermes);
        setProviderKind(nextProvider);
        setModelName(hermes.model_name ?? defaultModelForProvider(nextProvider, hermes));
        setBaseUrl(hermes.base_url ?? defaultBaseUrlForProvider(nextProvider, hermes));
        setHasSeededForm(true);
        if (!hermes.configured) {
            setSetupExpanded(true);
        }
    }, [hasSeededForm, hermes]);

    useEffect(() => {
        chatBottomRef.current?.scrollIntoView({ behavior: "smooth" });
    }, [messages, busyAction]);

    const fullAgentConfigured = !!hermes?.installed && !!hermes?.configured;
    const fullAgentReady = !!hermes?.api_server_ready;
    const localFallbackReady = !fullAgentConfigured && !!hermes?.direct_ollama_ready;
    const isHermesReady = fullAgentConfigured || localFallbackReady;

    useEffect(() => {
        if (activeView === "hermes" && isHermesReady) {
            window.setTimeout(() => chatInputRef.current?.focus(), 80);
        }
    }, [activeView, isHermesReady]);

    useEffect(() => {
        if (!isVisible) return;

        let unlisten: (() => void) | null = null;

        const setup = async () => {
            try {
                await fndrSubscribe(conversationId);
                unlisten = await onContextDelta((delta) => {
                    setLastDelta(delta);
                });
            } catch (err) {
                console.error("Failed to subscribe to context runtime:", err);
            }
        };

        setup();

        return () => {
            fndrUnsubscribe(conversationId).catch(() => {});
            if (unlisten) unlisten();
        };
    }, [isVisible, conversationId]);

    useEffect(() => {
        if (!isVisible) {
            setHermesError(null);
            setBusyAction(null);
            setActiveView("overview");
            setMessages([]);
            setDraft("");
            setConversationId(nextConversationId());
            setApiKey("");
            setHasSeededForm(false);
            setSetupExpanded(false);
            setSelectedAudit(null);
            setRetrievalExplanation(null);
            setAgentDraftSkill(null);
            setAgentDraftEval(null);
            setAgentInspectError(null);
        }
    }, [isVisible]);

    if (!isVisible) return null;

    const handleChooseProvider = (nextProvider: HermesProviderKind) => {
        setProviderKind(nextProvider);
        setHermesError(null);
        setModelName(
            hermes?.provider_kind === nextProvider && hermes.model_name
                ? hermes.model_name
                : defaultModelForProvider(nextProvider, hermes)
        );
        setBaseUrl(
            hermes?.provider_kind === nextProvider && hermes.base_url
                ? hermes.base_url
                : defaultBaseUrlForProvider(nextProvider, hermes)
        );
        if (nextProvider !== "openrouter" && nextProvider !== "custom") {
            setApiKey("");
        }
    };

    const runHermesAction = async (
        action: string,
        fn: () => Promise<HermesBridgeStatus>
    ) => {
        setBusyAction(action);
        setHermesError(null);
        try {
            const next = await fn();
            setHermes(next);
        } catch (err) {
            setHermesError(err instanceof Error ? err.message : String(err));
        } finally {
            setBusyAction(null);
        }
    };

    const handleSaveSetup = async () => {
        await runHermesAction("setup", () =>
            saveHermesSetup({
                provider_kind: providerKind,
                model_name: modelName.trim(),
                api_key:
                    providerKind === "openrouter" || providerKind === "custom"
                        ? apiKey
                        : null,
                base_url:
                    providerKind === "custom" || providerKind === "ollama"
                        ? baseUrl.trim()
                        : null,
            })
        );
        setApiKey("");
        setSetupExpanded(false);
    };

    const handleSend = async () => {
        const input = draft.trim();
        if (!input || busyAction === "send") return;

        const nextMessages = [...messages, { role: "user" as const, content: input }];
        setMessages(nextMessages);
        setDraft("");
        setBusyAction("send");
        setHermesError(null);

        try {
            let replyContent: string;

            if (fullAgentConfigured) {
                const reply = await sendHermesMessage(conversationId, input);
                replyContent = reply.content;
            } else if (hermes?.direct_ollama_ready) {
                // Local fallback mode — available before the full Hermes runtime is online.
                const history = messages.map(m => ({ role: m.role, content: m.content }));
                replyContent = await sendDirectChat(history, input);
            } else {
                throw new Error("Enable the FNDR agent runtime or connect a local Ollama model first.");
            }

            setMessages([...nextMessages, { role: "assistant", content: replyContent }]);
        } catch (err) {
            const message = err instanceof Error ? err.message : String(err);
            setHermesError(message);
            setMessages([
                ...nextMessages,
                { role: "assistant", content: `Error: ${message}` },
            ]);
        } finally {
            setBusyAction(null);
            window.setTimeout(() => chatInputRef.current?.focus(), 50);
        }
    };

    const handleRunAgentMode = async () => {
        const userGoal = agentGoal.trim();
        if (!userGoal || busyAction === "agent-run") return;

        setBusyAction("agent-run");
        setAgentRunError(null);
        try {
            const response = await runAgentRequest({
                user_goal: userGoal,
                mode: agentMode,
                window_minutes: 30,
                include_raw_evidence: false,
                budget_tokens: agentMode === "ask" ? 900 : 1400,
            });
            setAgentRun(response);
            const [runs, detail, explanation] = await Promise.all([
                listAgentAuditRuns(8),
                getAgentAuditRun(response.run_id),
                explainAgentRetrieval({ run_id: response.run_id }),
            ]);
            setAuditRuns(runs);
            setSelectedAudit(detail);
            setRetrievalExplanation(explanation);
            setAgentDraftSkill(null);
            setAgentDraftEval(null);
        } catch (err) {
            setAgentRunError(err instanceof Error ? err.message : String(err));
        } finally {
            setBusyAction(null);
        }
    };

    const handleSelectAuditRun = async (runId: string) => {
        setBusyAction("audit-detail");
        setAgentInspectError(null);
        try {
            const [detail, explanation] = await Promise.all([
                getAgentAuditRun(runId),
                explainAgentRetrieval({ run_id: runId }),
            ]);
            setSelectedAudit(detail);
            setRetrievalExplanation(explanation);
            setAgentDraftSkill(null);
            setAgentDraftEval(null);
        } catch (err) {
            setAgentInspectError(err instanceof Error ? err.message : String(err));
        } finally {
            setBusyAction(null);
        }
    };

    const handleRateResult = async (
        runId: string,
        rating: RetrievalFeedbackRating,
        memoryId?: string
    ) => {
        setBusyAction("feedback");
        setAgentInspectError(null);
        try {
            await rateAgentResult({ run_id: runId, memory_id: memoryId ?? null, rating });
            const [runs, detail] = await Promise.all([
                listAgentAuditRuns(8),
                getAgentAuditRun(runId),
            ]);
            setAuditRuns(runs);
            setSelectedAudit(detail);
        } catch (err) {
            setAgentInspectError(err instanceof Error ? err.message : String(err));
        } finally {
            setBusyAction(null);
        }
    };

    const handleProposeSkill = async (runId: string) => {
        setBusyAction("skill");
        setAgentInspectError(null);
        try {
            setAgentDraftSkill(await proposeSkillFromRun(runId));
        } catch (err) {
            setAgentInspectError(err instanceof Error ? err.message : String(err));
        } finally {
            setBusyAction(null);
        }
    };

    const handleProposeEval = async (runId: string) => {
        setBusyAction("eval");
        setAgentInspectError(null);
        try {
            setAgentDraftEval(await proposeEvalFromRun(runId));
        } catch (err) {
            setAgentInspectError(err instanceof Error ? err.message : String(err));
        } finally {
            setBusyAction(null);
        }
    };

    const readinessStep = getReadinessStep(hermes);
    const currentProviderLabel =
        isProviderKind(hermes?.provider_kind) ? hermes.provider_kind.toUpperCase() : providerKind.toUpperCase();
    const showBaseUrlField = providerKind === "custom" || providerKind === "ollama";
    const showApiKeyField = providerKind === "openrouter" || providerKind === "custom";
    const canSaveSetup = (() => {
        if (busyAction !== null || !modelName.trim()) return false;
        if (providerKind === "openrouter") return apiKey.trim().length > 0;
        if (providerKind === "custom") return baseUrl.trim().length > 0;
        if (providerKind === "ollama") return !!hermes?.ollama_installed;
        return !!hermes?.codex_logged_in;
    })();

    // Show quick-connect banner when Ollama is running with models but not yet configured
    // Works regardless of whether hermes CLI is installed
    const showOllamaBanner =
        hermes !== null &&
        hermes.ollama_reachable &&
        hermes.ollama_models.length > 0 &&
        !hermes.configured;

    const gatewayStatusClass = fullAgentReady || localFallbackReady
        ? "ap-dot-ready"
        : hermes?.gateway_running || fullAgentConfigured
            ? "ap-dot-starting"
            : "ap-dot-off";

    return (
        <div className="ap-root">
            {/* Header */}
            <header className="ap-header">
                <div className="ap-header-left">
                    <div className={`ap-header-dot ${gatewayStatusClass}`} />
                    <span className="ap-header-title">FNDR Agent</span>
                    {hermes?.configured && (
                        <span className="ap-header-badge">
                            {hermes.model_name ?? currentProviderLabel}
                        </span>
                    )}
                </div>
                <button className="ap-close-btn" onClick={onClose} aria-label="Close">
                    <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
                        <path d="M1 1L13 13M13 1L1 13" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
                    </svg>
                </button>
            </header>

            <div className="ap-layout">
                {/* Sidebar */}
                <nav className="ap-sidebar">
                    <button
                        className={`ap-nav-item ${activeView === "overview" ? "active" : ""}`}
                        onClick={() => setActiveView("overview")}
                    >
                        <svg className="ap-nav-icon" viewBox="0 0 16 16" fill="none">
                            <rect x="1" y="1" width="6" height="6" rx="1.5" stroke="currentColor" strokeWidth="1.2" />
                            <rect x="9" y="1" width="6" height="6" rx="1.5" stroke="currentColor" strokeWidth="1.2" />
                            <rect x="1" y="9" width="6" height="6" rx="1.5" stroke="currentColor" strokeWidth="1.2" />
                            <rect x="9" y="9" width="6" height="6" rx="1.5" stroke="currentColor" strokeWidth="1.2" />
                        </svg>
                        <span>Overview</span>
                    </button>
                    <button
                        className={`ap-nav-item ${activeView === "hermes" ? "active" : ""}`}
                        onClick={() => setActiveView("hermes")}
                    >
                        <svg className="ap-nav-icon" viewBox="0 0 16 16" fill="none">
                            <circle cx="8" cy="8" r="3" stroke="currentColor" strokeWidth="1.2" />
                            <path d="M8 1v2M8 13v2M1 8h2M13 8h2M3.05 3.05l1.41 1.41M11.54 11.54l1.41 1.41M3.05 12.95l1.41-1.41M11.54 4.46l1.41-1.41" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
                        </svg>
                        <span>FNDR Agent</span>
                        {readinessStep === 4 && (
                            <span className="ap-nav-ready-dot" />
                        )}
                    </button>
                </nav>

                {/* Content */}
                <main className="ap-content">
                    {activeView === "overview" ? (
                        <OverviewView
                            status={status}
                            hermes={hermes}
                            runtimeStatus={runtimeStatus}
                            latestPack={recentPacks[0] ?? null}
                            lastDelta={lastDelta}
                            readinessStep={readinessStep}
                            agentGoal={agentGoal}
                            agentMode={agentMode}
                            agentRun={agentRun}
                            agentRunError={agentRunError}
                            auditRuns={auditRuns}
                            selectedAudit={selectedAudit}
                            retrievalExplanation={retrievalExplanation}
                            agentDraftSkill={agentDraftSkill}
                            agentDraftEval={agentDraftEval}
                            agentInspectError={agentInspectError}
                            agentBusy={busyAction === "agent-run"}
                            inspectBusy={busyAction === "audit-detail" || busyAction === "feedback" || busyAction === "skill" || busyAction === "eval"}
                            onAgentGoalChange={setAgentGoal}
                            onAgentModeChange={setAgentMode}
                            onRunAgentMode={handleRunAgentMode}
                            onSelectAuditRun={handleSelectAuditRun}
                            onRateResult={handleRateResult}
                            onProposeSkill={handleProposeSkill}
                            onProposeEval={handleProposeEval}
                            onStop={() => stopAgent().then(setStatus).catch(console.error)}
                            onOpenHermes={() => setActiveView("hermes")}
                        />
                    ) : (
                        <HermesView
                            hermes={hermes}
                            busyAction={busyAction}
                            hermesError={hermesError}
                            providerKind={providerKind}
                            modelName={modelName}
                            apiKey={apiKey}
                            baseUrl={baseUrl}
                            messages={messages}
                            draft={draft}
                            conversationId={conversationId}
                            hasSeededForm={hasSeededForm}
                            setupExpanded={setupExpanded}
                            isHermesReady={isHermesReady}
                            readinessStep={readinessStep}
                            showOllamaBanner={showOllamaBanner}
                            showBaseUrlField={showBaseUrlField}
                            showApiKeyField={showApiKeyField}
                            canSaveSetup={canSaveSetup}
                            currentProviderLabel={currentProviderLabel}
                            chatBottomRef={chatBottomRef}
                            chatInputRef={chatInputRef}
                            onChooseProvider={handleChooseProvider}
                            onModelNameChange={setModelName}
                            onApiKeyChange={setApiKey}
                            onBaseUrlChange={setBaseUrl}
                            onSaveSetup={handleSaveSetup}
                            onSetupExpanded={setSetupExpanded}
                            onDraftChange={setDraft}
                            onSend={handleSend}
                            onResetConversation={() => {
                                setMessages([]);
                                setConversationId(nextConversationId());
                            }}
                            onInstall={() => runHermesAction("install", installHermesBridge)}
                            onQuickSetupOllama={() => runHermesAction("quick-ollama", quickSetupOllama)}
                            onStart={() => runHermesAction("start", startHermesGateway)}
                            onStop={() => runHermesAction("stop", stopHermesGateway)}
                            onSync={() => runHermesAction("sync", syncHermesBridgeContext)}
                        />
                    )}
                </main>
            </div>
        </div>
    );
}

// ─── Overview View ───────────────────────────────────────────────────────────

interface OverviewViewProps {
    status: AgentStatus | null;
    hermes: HermesBridgeStatus | null;
    runtimeStatus: ContextRuntimeStatus | null;
    latestPack: ContextPack | null;
    lastDelta: ContextDelta | null;
    readinessStep: number;
    agentGoal: string;
    agentMode: AgentMode;
    agentRun: AgentRunResponse | null;
    agentRunError: string | null;
    auditRuns: AgentAuditRecord[];
    selectedAudit: AgentAuditRecord | null;
    retrievalExplanation: RetrievalExplanation | null;
    agentDraftSkill: AgentSkillCandidate | null;
    agentDraftEval: AgentEvalCase | null;
    agentInspectError: string | null;
    agentBusy: boolean;
    inspectBusy: boolean;
    onAgentGoalChange: (value: string) => void;
    onAgentModeChange: (value: AgentMode) => void;
    onRunAgentMode: () => void;
    onSelectAuditRun: (runId: string) => void;
    onRateResult: (runId: string, rating: RetrievalFeedbackRating, memoryId?: string) => void;
    onProposeSkill: (runId: string) => void;
    onProposeEval: (runId: string) => void;
    onStop: () => void;
    onOpenHermes: () => void;
}

function OverviewView({
    status,
    hermes,
    runtimeStatus,
    latestPack,
    lastDelta,
    readinessStep,
    agentGoal,
    agentMode,
    agentRun,
    agentRunError,
    auditRuns,
    selectedAudit,
    retrievalExplanation,
    agentDraftSkill,
    agentDraftEval,
    agentInspectError,
    agentBusy,
    inspectBusy,
    onAgentGoalChange,
    onAgentModeChange,
    onRunAgentMode,
    onSelectAuditRun,
    onRateResult,
    onProposeSkill,
    onProposeEval,
    onStop,
    onOpenHermes,
}: OverviewViewProps) {
    const isRunning = status?.status === "running";
    const fullAgentReady = !!hermes?.api_server_ready;
    const localFallbackReady = !hermes?.installed && !!hermes?.direct_ollama_ready;
    const runtimeLabel = localFallbackReady ? "Local chat" : "Agent runtime";
    const runtimeValue = localFallbackReady
        ? "Ready"
        : readinessStep === 4
            ? "Ready"
            : readinessStep === 3
                ? "Starting"
                : readinessStep >= 2
                    ? "Stopped"
                    : "Not installed";
    const runtimeDetail = localFallbackReady ? (hermes?.model_name ?? "Ollama") : (hermes?.api_url ?? "");
    const runtimeDotClass = localFallbackReady || fullAgentReady
        ? "ap-dot-ready"
        : readinessStep >= 2
            ? "ap-dot-starting"
            : "ap-dot-off";

    return (
        <div className="ap-section-stack">
            <div className="ap-card ap-chat-card">
                <div className="ap-chat-header">
                    <div>
                        <div className="ap-card-title">Agent command</div>
                        <div className="ap-card-subtitle">Local memory context · read-only by default</div>
                    </div>
                    <select
                        className="ap-mode-select"
                        value={agentMode}
                        onChange={(event) => onAgentModeChange(event.target.value as AgentMode)}
                    >
                        <option value="ask">Ask only</option>
                        <option value="plan">Plan</option>
                        <option value="act">Act with approval</option>
                        <option value="learn">Learn from workflow</option>
                    </select>
                </div>
                <div className="ap-agent-command-row">
                    <textarea
                        className="ap-chat-textarea"
                        value={agentGoal}
                        onChange={(event) => onAgentGoalChange(event.target.value)}
                        onKeyDown={(event) => {
                            if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
                                onRunAgentMode();
                            }
                        }}
                        placeholder="What do you want FNDR Agent to help with?"
                    />
                    <button
                        className="ap-btn ap-btn-primary"
                        disabled={agentBusy || !agentGoal.trim()}
                        onClick={onRunAgentMode}
                    >
                        {agentBusy ? "Building..." : agentMode === "ask" ? "Ask" : "Build"}
                    </button>
                </div>
                {(agentRun || agentRunError) && (
                    <div className="ap-agent-result">
                        {agentRunError ? (
                            <div className="ap-error-box">{agentRunError}</div>
                        ) : agentRun ? (
                            <>
                                <div className="ap-agent-answer">{agentRun.answer}</div>
                                <div className="ap-agent-evidence-grid">
                                    <MetricCard
                                        label="Memories used"
                                        value={String(agentRun.context_pack.relevant_memories.length)}
                                        detail={agentRun.context_pack.source_context_pack_id}
                                        dotClass={agentRun.context_pack.relevant_memories.length > 0 ? "ap-dot-ready" : "ap-dot-starting"}
                                    />
                                    <MetricCard
                                        label="Confidence"
                                        value={agentRun.context_pack.confidence.toFixed(2)}
                                        detail={`${agentRun.context_pack.token_budget.used} tokens`}
                                        dotClass={agentRun.context_pack.confidence > 0.5 ? "ap-dot-ready" : "ap-dot-starting"}
                                    />
                                    <MetricCard
                                        label="Policy"
                                        value={agentRun.context_pack.privacy_scope.read_only ? "Read-only" : "Approval gated"}
                                        detail={`${agentRun.blocked_actions.length} blocked`}
                                        dotClass="ap-dot-ready"
                                    />
                                </div>
                                {agentRun.context_pack.relevant_memories.length > 0 && (
                                    <div className="ap-evidence-list">
                                        {agentRun.context_pack.relevant_memories.slice(0, 4).map((memory) => (
                                            <div className="ap-evidence-item" key={memory.memory_id}>
                                                <div className="ap-evidence-title">{memory.title}</div>
                                                <div className="ap-evidence-meta">
                                                    {memory.app_name} · {new Date(memory.timestamp).toLocaleString()}
                                                </div>
                                                <div className="ap-evidence-summary">{memory.match_reason}</div>
                                            </div>
                                        ))}
                                    </div>
                                )}
                                {agentRun.context_pack.disallowed_context.length > 0 && (
                                    <div className="ap-redaction-line">
                                        {agentRun.context_pack.disallowed_context.length} context item(s) were dropped or redacted before reaching the agent.
                                    </div>
                                )}
                            </>
                        ) : null}
                    </div>
                )}
            </div>

            <div className="ap-card">
                <div className="ap-card-title">Recent agent runs</div>
                {auditRuns.length === 0 ? (
                    <p className="ap-card-body">No agent audit runs recorded yet.</p>
                ) : (
                    <div className="ap-run-list">
                        {auditRuns.map((run) => (
                            <button
                                key={run.run_id}
                                className={`ap-run-row ${selectedAudit?.run_id === run.run_id ? "active" : ""}`}
                                disabled={inspectBusy}
                                onClick={() => onSelectAuditRun(run.run_id)}
                            >
                                <span>{run.user_goal || "Untitled run"}</span>
                                <small>{run.mode} · {run.result_status} · {new Date(run.created_at).toLocaleString()}</small>
                            </button>
                        ))}
                    </div>
                )}
            </div>

            {(selectedAudit || retrievalExplanation || agentInspectError) && (
                <div className="ap-card">
                    <div className="ap-card-title">Run detail</div>
                    {agentInspectError && <div className="ap-error-box">{agentInspectError}</div>}
                    {selectedAudit && (
                        <>
                            <div className="ap-agent-evidence-grid">
                                <MetricCard
                                    label="Mode"
                                    value={selectedAudit.mode}
                                    detail={selectedAudit.result_status}
                                    dotClass={selectedAudit.result_status === "failed" ? "ap-dot-off" : "ap-dot-ready"}
                                />
                                <MetricCard
                                    label="Approvals"
                                    value={selectedAudit.approvals_required.length > 0 ? "Required" : "None"}
                                    detail={`${selectedAudit.tools_blocked.length} blocked`}
                                    dotClass={selectedAudit.approvals_required.length > 0 ? "ap-dot-starting" : "ap-dot-ready"}
                                />
                                <MetricCard
                                    label="Feedback"
                                    value={String(selectedAudit.feedback.length)}
                                    detail={selectedAudit.context_pack_id ?? "no context pack"}
                                    dotClass="ap-dot-ready"
                                />
                            </div>
                            <div className="ap-inline-actions">
                                {(["useful", "irrelevant", "wrong", "stale", "missing_context"] as RetrievalFeedbackRating[]).map((rating) => (
                                    <button
                                        key={rating}
                                        className="ap-btn"
                                        disabled={inspectBusy}
                                        onClick={() => onRateResult(selectedAudit.run_id, rating)}
                                    >
                                        {rating.replace("_", " ")}
                                    </button>
                                ))}
                            </div>
                            <div className="ap-inline-actions">
                                <button className="ap-btn" disabled={inspectBusy} onClick={() => onProposeSkill(selectedAudit.run_id)}>
                                    Propose skill from this run
                                </button>
                                <button className="ap-btn" disabled={inspectBusy} onClick={() => onProposeEval(selectedAudit.run_id)}>
                                    Propose eval from this run
                                </button>
                            </div>
                        </>
                    )}
                    {retrievalExplanation && (
                        <div className="ap-inspect-section">
                            <div className="ap-card-subtitle">Why this context?</div>
                            {retrievalExplanation.selected_memories.slice(0, 6).map((memory) => (
                                <div className="ap-evidence-item" key={memory.memory_id}>
                                    <div className="ap-evidence-title">{memory.title}</div>
                                    <div className="ap-evidence-meta">{memory.memory_id} · confidence {memory.confidence.toFixed(2)}</div>
                                    <div className="ap-evidence-summary">{memory.matched_reason}</div>
                                    <div className="ap-evidence-summary">{memory.keyword_match}</div>
                                </div>
                            ))}
                            {(retrievalExplanation.dropped_context.length > 0 || retrievalExplanation.redacted_context.length > 0) && (
                                <div className="ap-redaction-line">
                                    Dropped {retrievalExplanation.dropped_context.length}; redacted {retrievalExplanation.redacted_context.length}.
                                </div>
                            )}
                            <div className="ap-card-body">
                                {retrievalExplanation.limitations.join(" ")}
                            </div>
                        </div>
                    )}
                    {agentDraftSkill && (
                        <div className="ap-draft-box">
                            <div className="ap-card-subtitle">Skill draft</div>
                            <strong>{agentDraftSkill.name}</strong>
                            <p>{agentDraftSkill.when_to_use}</p>
                        </div>
                    )}
                    {agentDraftEval && (
                        <div className="ap-draft-box">
                            <div className="ap-card-subtitle">Eval draft</div>
                            <strong>{agentDraftEval.workflow_name}</strong>
                            <p>{agentDraftEval.expected_outcome}</p>
                        </div>
                    )}
                </div>
            )}

            {/* Status node */}
            <div className="ap-overview-hero">
                <div className="ap-overview-node-ring">
                    <div className={`ap-overview-node ${isRunning ? "running" : ""}`}>
                        <svg width="28" height="28" viewBox="0 0 28 28" fill="none">
                            <path d="M14 4C8.48 4 4 8.48 4 14s4.48 10 10 10 10-4.48 10-10S19.52 4 14 4zm-2 14.5v-9l7 4.5-7 4.5z" fill="currentColor" />
                        </svg>
                    </div>
                </div>
                <div className="ap-overview-hero-text">
                    <div className="ap-overview-status-label">
                        {isRunning ? "Agent running" : "Agent idle"}
                    </div>
                    <h3>{status?.task_title ?? "No active task"}</h3>
                    <p>{status?.last_message ?? "Start a task from the command palette or open the FNDR Agent."}</p>
                </div>
                <div className="ap-overview-hero-actions">
                    {isRunning && (
                        <button className="ap-btn ap-btn-danger" onClick={onStop}>
                            Stop task
                        </button>
                    )}
                    <button className="ap-btn" onClick={onOpenHermes}>
                        Open Agent
                    </button>
                </div>
            </div>

            {/* System metrics */}
            <div className="ap-metrics-row">
                <MetricCard
                    label={runtimeLabel}
                    value={runtimeValue}
                    detail={runtimeDetail}
                    dotClass={runtimeDotClass}
                />
                <MetricCard
                    label="Provider"
                    value={hermes?.configured ? (hermes.provider_kind?.toUpperCase() ?? "—") : "Not configured"}
                    detail={hermes?.model_name ?? ""}
                    dotClass={hermes?.configured ? "ap-dot-ready" : "ap-dot-off"}
                />
                <MetricCard
                    label="Context sync"
                    value={hermes?.context_ready ? "Synced" : "Pending"}
                    detail={formatTimestamp(hermes?.last_synced_at ?? null)}
                    dotClass={hermes?.context_ready ? "ap-dot-ready" : "ap-dot-off"}
                />
                <MetricCard
                    label="Runtime pack"
                    value={runtimeStatus?.status ?? "Unknown"}
                    detail={latestPack ? `${latestPack.tokens_used}/${latestPack.budget_tokens} tokens` : "No pack yet"}
                    dotClass={runtimeStatus?.mcp_running ? "ap-dot-ready" : "ap-dot-off"}
                />
            </div>

            {/* Live Context Delta */}
            {lastDelta && (lastDelta.new_events.length > 0 || lastDelta.new_items.length > 0) && (
                <section className="ap-card ap-delta-card">
                    <div className="ap-card-title">
                        Live Context Updates
                        <span className="ap-delta-live-dot" />
                    </div>
                    <div className="ap-memory-list">
                        {lastDelta.new_events.map((event) => (
                            <div key={event.id} className="ap-memory-row">
                                <div className="ap-memory-app">{event.activity_type.toUpperCase()}</div>
                                <div className="ap-memory-title">{event.title}</div>
                                <div className="ap-memory-summary">{event.summary}</div>
                            </div>
                        ))}
                        {lastDelta.new_items.map((item, i) => (
                            <div key={`item-${i}`} className="ap-memory-row">
                                <div className="ap-memory-app">NEW ITEM</div>
                                <div className="ap-memory-title">{item}</div>
                            </div>
                        ))}
                    </div>
                </section>
            )}

            {/* Recent memories */}
            {(hermes?.recent_memories?.length ?? 0) > 0 && (
                <section className="ap-card">
                    <div className="ap-card-title">Recent FNDR context</div>
                    <div className="ap-memory-list">
                        {hermes!.recent_memories.map((m, i) => (
                            <div key={i} className="ap-memory-row">
                                <div className="ap-memory-app">{m.app_name}</div>
                                <div className="ap-memory-title">{m.title}</div>
                                <div className="ap-memory-summary">{m.summary}</div>
                            </div>
                        ))}
                    </div>
                </section>
            )}

            {latestPack && (
                <section className="ap-card">
                    <div className="ap-card-title">Latest context pack</div>
                    <div className="ap-memory-list">
                        <div className="ap-memory-row">
                            <div className="ap-memory-app">{latestPack.project ?? "General"}</div>
                            <div className="ap-memory-title">{latestPack.active_goal ?? latestPack.summary}</div>
                            <div className="ap-memory-summary">
                                Included {latestPack.included.length} items, excluded {latestPack.excluded.length}, used {latestPack.tokens_used}/{latestPack.budget_tokens} tokens.
                            </div>
                        </div>
                    </div>
                </section>
            )}
        </div>
    );
}

// ─── Hermes View ─────────────────────────────────────────────────────────────

interface HermesViewProps {
    hermes: HermesBridgeStatus | null;
    busyAction: string | null;
    hermesError: string | null;
    providerKind: HermesProviderKind;
    modelName: string;
    apiKey: string;
    baseUrl: string;
    messages: HermesUiMessage[];
    draft: string;
    conversationId: string;
    hasSeededForm: boolean;
    setupExpanded: boolean;
    isHermesReady: boolean;
    readinessStep: number;
    showOllamaBanner: boolean;
    showBaseUrlField: boolean;
    showApiKeyField: boolean;
    canSaveSetup: boolean;
    currentProviderLabel: string;
    chatBottomRef: React.RefObject<HTMLDivElement>;
    chatInputRef: React.RefObject<HTMLTextAreaElement>;
    onChooseProvider: (p: HermesProviderKind) => void;
    onModelNameChange: (v: string) => void;
    onApiKeyChange: (v: string) => void;
    onBaseUrlChange: (v: string) => void;
    onSaveSetup: () => void;
    onSetupExpanded: (v: boolean) => void;
    onDraftChange: (v: string) => void;
    onSend: () => void;
    onResetConversation: () => void;
    onInstall: () => void;
    onQuickSetupOllama: () => void;
    onStart: () => void;
    onStop: () => void;
    onSync: () => void;
}

function HermesView(props: HermesViewProps) {
    const {
        hermes, busyAction, hermesError, providerKind, modelName, apiKey, baseUrl,
        messages, draft, setupExpanded, isHermesReady, readinessStep, showOllamaBanner,
        showBaseUrlField, showApiKeyField, canSaveSetup,
        chatBottomRef, chatInputRef,
        onChooseProvider, onModelNameChange, onApiKeyChange, onBaseUrlChange,
        onSaveSetup, onSetupExpanded, onDraftChange, onSend, onResetConversation,
        onInstall, onQuickSetupOllama, onStart, onStop, onSync,
    } = props;

    const busy = busyAction !== null;
    const fullAgentConfigured = !!hermes?.installed && !!hermes?.configured;
    const fullAgentReady = !!hermes?.api_server_ready;
    const localFallbackReady = !fullAgentConfigured && !!hermes?.direct_ollama_ready;
    const showInstallCard =
        hermes !== null &&
        !hermes.installed &&
        (!localFallbackReady || hermes.bundled_repo_available);
    const installTitle = hermes?.bundled_repo_available
        ? "Enable full FNDR agent"
        : "Install Hermes";
    const installBody = hermes?.bundled_repo_available
        ? localFallbackReady
            ? "FNDR can already do quick local chat through Ollama, but enabling the bundled Hermes runtime unlocks the full native agent experience: tools, longer-lived conversations, and Hermes-style behavior inside FNDR."
            : "FNDR found the vendored hermes-agent clone in this repo. Enabling it prepares a private runtime inside FNDR so the agent behaves like a built-in feature instead of a separately installed CLI."
        : "FNDR will run the official Hermes installer to set up the local agent runtime.";
    const installButtonLabel = hermes?.bundled_repo_available ? "Enable Agent" : "Install Hermes";
    const emptyStateTitle = fullAgentReady
        ? "FNDR Agent is ready"
        : fullAgentConfigured
            ? "FNDR Agent will start on first message"
        : localFallbackReady
            ? "Local chat is ready"
            : "Start the agent runtime";
    const emptyStateBody = fullAgentReady
        ? "Ask about your FNDR memories, draft something, or run a multi-step task."
        : fullAgentConfigured
            ? "Send a message and FNDR will launch the bundled Hermes runtime automatically."
        : localFallbackReady
            ? "You can chat through Ollama right now. Enable the full FNDR agent runtime for richer Hermes behavior."
            : "Bring the FNDR agent online above, then chat here.";
    const chatTitle = fullAgentConfigured ? "Agent chat" : "Local chat";
    const assistantLabel = "Agent";

    return (
        <div className="ap-section-stack">
            {/* Step progress */}
            <div className="ap-steps-bar">
                {["Install", "Configure", "Start", "Chat"].map((label, i) => (
                    <div key={label} className={`ap-step ${readinessStep > i ? "done" : readinessStep === i ? "active" : ""}`}>
                        <div className="ap-step-dot">
                            {readinessStep > i ? (
                                <svg width="10" height="10" viewBox="0 0 10 10">
                                    <path d="M1.5 5L4 7.5 8.5 2.5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" fill="none" />
                                </svg>
                            ) : (
                                <span>{i + 1}</span>
                            )}
                        </div>
                        <span className="ap-step-label">{label}</span>
                        {i < 3 && <div className="ap-step-line" />}
                    </div>
                ))}
            </div>

            {/* Ollama quick-connect banner */}
            {showOllamaBanner && (
                <div className="ap-ollama-banner">
                    <div className="ap-ollama-banner-left">
                        <div className="ap-ollama-pulse" />
                        <div>
                            <div className="ap-ollama-banner-title">Ollama detected</div>
                            <div className="ap-ollama-banner-sub">
                                {hermes!.ollama_models.length} model{hermes!.ollama_models.length !== 1 ? "s" : ""} available — {hermes!.ollama_models[0]}
                            </div>
                        </div>
                    </div>
                    <button
                        className="ap-btn ap-btn-primary"
                        onClick={onQuickSetupOllama}
                        disabled={busy}
                    >
                        {busyAction === "quick-ollama" ? "Connecting..." : "Connect Ollama"}
                    </button>
                </div>
            )}

            {/* Metrics */}
            <div className="ap-metrics-row">
                <MetricCard
                    label="Gateway"
                    value={hermes?.gateway_running ? "Running" : "Stopped"}
                    detail={hermes?.api_url ?? "—"}
                    dotClass={fullAgentReady ? "ap-dot-ready" : hermes?.gateway_running ? "ap-dot-starting" : "ap-dot-off"}
                />
                <MetricCard
                    label="Context"
                    value={hermes?.context_ready ? "Ready" : "Pending sync"}
                    detail={formatTimestamp(hermes?.last_synced_at ?? null)}
                    dotClass={hermes?.context_ready ? "ap-dot-ready" : "ap-dot-off"}
                />
                <MetricCard
                    label="Provider"
                    value={hermes?.configured ? (hermes.provider_kind?.toUpperCase() ?? "—") : "Not set"}
                    detail={hermes?.model_name ?? "No model configured"}
                    dotClass={hermes?.configured ? "ap-dot-ready" : "ap-dot-off"}
                />
            </div>

            {/* Install / enable step */}
            {showInstallCard && (
                <section className="ap-card">
                    <div className="ap-card-title">{installTitle}</div>
                    <p className="ap-card-body">
                        {installBody}
                    </p>
                    {!hermes?.bundled_repo_available && (
                        <div className="ap-terminal-line">
                            <span className="ap-terminal-prompt">$</span>
                            <span>{hermes?.install_command ?? "curl -fsSL https://hermes-agent.nousresearch.com/install.sh | bash"}</span>
                        </div>
                    )}
                    <div className="ap-inline-actions">
                        <button
                            className="ap-btn ap-btn-primary"
                            onClick={onInstall}
                            disabled={busy}
                        >
                            {busyAction === "install" ? "Preparing..." : installButtonLabel}
                        </button>
                        <button
                            className="ap-btn"
                            onClick={() => void openExternalUrl(HERMES_DOCS_URL)}
                        >
                            Docs
                        </button>
                    </div>
                </section>
            )}

            {/* Configure step */}
            {(hermes?.installed || hermes?.bundled_repo_available) && (
                <section className="ap-card">
                    <button
                        className="ap-card-collapsible-header"
                        onClick={() => onSetupExpanded(!setupExpanded)}
                    >
                        <div>
                            <div className="ap-card-title">Configure provider</div>
                            {hermes.configured && !setupExpanded && (
                                <div className="ap-card-subtitle">
                                    {hermes.provider_kind?.toUpperCase()} · {hermes.model_name}
                                </div>
                            )}
                        </div>
                        <svg
                            className={`ap-chevron ${setupExpanded ? "open" : ""}`}
                            width="14" height="14" viewBox="0 0 14 14" fill="none"
                        >
                            <path d="M3 5L7 9L11 5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
                        </svg>
                    </button>

                    {setupExpanded && (
                        <div className="ap-setup-body">
                            {/* Provider tabs */}
                            <div className="ap-provider-tabs">
                                {(["ollama", "codex", "openrouter", "custom"] as HermesProviderKind[]).map((option) => (
                                    <button
                                        key={option}
                                        className={`ap-provider-tab ${providerKind === option ? "active" : ""}`}
                                        onClick={() => onChooseProvider(option)}
                                    >
                                        <span className="ap-provider-tab-name">{providerTabLabel(option)}</span>
                                        <span className="ap-provider-tab-status">
                                            {providerTabStatus(option, hermes)}
                                        </span>
                                    </button>
                                ))}
                            </div>

                            {/* Provider note */}
                            <div className="ap-provider-note">
                                {providerDetailNote(providerKind, hermes)}
                                {providerKind === "ollama" && !hermes.ollama_installed && (
                                    <button
                                        className="ap-link-btn"
                                        onClick={() => void openExternalUrl(OLLAMA_DOWNLOAD_URL)}
                                    >
                                        Get Ollama →
                                    </button>
                                )}
                            </div>

                            {/* Form fields */}
                            <div className="ap-form-grid">
                                <label className="ap-field">
                                    <span>Model</span>
                                    {providerKind === "ollama" && (hermes.ollama_models.length ?? 0) > 0 ? (
                                        <select
                                            value={modelName}
                                            onChange={(e) => onModelNameChange(e.target.value)}
                                            disabled={busy}
                                        >
                                            {hermes.ollama_models.map((m) => (
                                                <option key={m} value={m}>{m}</option>
                                            ))}
                                        </select>
                                    ) : (
                                        <input
                                            value={modelName}
                                            onChange={(e) => onModelNameChange(e.target.value)}
                                            placeholder={defaultModelForProvider(providerKind, hermes)}
                                            disabled={busy}
                                        />
                                    )}
                                </label>

                                {showBaseUrlField && (
                                    <label className="ap-field">
                                        <span>{providerKind === "ollama" ? "Ollama URL" : "Base URL"}</span>
                                        <input
                                            value={baseUrl}
                                            onChange={(e) => onBaseUrlChange(e.target.value)}
                                            placeholder={providerKind === "ollama" ? DEFAULT_OLLAMA_BASE_URL : "http://localhost:8000/v1"}
                                            disabled={busy}
                                        />
                                    </label>
                                )}

                                {showApiKeyField && (
                                    <label className="ap-field ap-field-wide">
                                        <span>
                                            {providerKind === "openrouter" ? "OpenRouter API key" : "Endpoint API key (optional)"}
                                        </span>
                                        <input
                                            type="password"
                                            value={apiKey}
                                            onChange={(e) => onApiKeyChange(e.target.value)}
                                            placeholder={
                                                providerKind === "openrouter"
                                                    ? "sk-or-..."
                                                    : "Leave empty if not required"
                                            }
                                            disabled={busy}
                                        />
                                    </label>
                                )}
                            </div>

                            <div className="ap-inline-actions">
                                <button
                                    className="ap-btn ap-btn-primary"
                                    onClick={onSaveSetup}
                                    disabled={!canSaveSetup}
                                >
                                    {busyAction === "setup" ? "Saving..." : "Save configuration"}
                                </button>
                                <button
                                    className="ap-btn"
                                    onClick={onSync}
                                    disabled={busy || !hermes.configured}
                                >
                                    {busyAction === "sync" ? "Syncing..." : "Sync context"}
                                </button>
                            </div>
                        </div>
                    )}
                </section>
            )}

            {/* Gateway controls */}
            {hermes?.installed && hermes.configured && (
                <section className="ap-card ap-gateway-card">
                    <div className="ap-gateway-header">
                        <div className="ap-gateway-status">
                            <div className={`ap-gateway-dot ${fullAgentReady ? "ready" : hermes.gateway_running ? "starting" : "off"}`} />
                            <span className="ap-gateway-label">
                                {fullAgentReady ? "Agent runtime online" : hermes.gateway_running ? "Starting up..." : "Agent runtime offline"}
                            </span>
                            <span className="ap-gateway-url">{hermes.api_url}</span>
                        </div>
                        <div className="ap-inline-actions">
                            <button
                                className="ap-btn ap-btn-primary"
                                onClick={onStart}
                                disabled={busy || fullAgentReady}
                            >
                                {busyAction === "start" ? "Starting..." : "Start"}
                            </button>
                            <button
                                className="ap-btn"
                                onClick={onStop}
                                disabled={busy || !hermes.gateway_running}
                            >
                                {busyAction === "stop" ? "Stopping..." : "Stop"}
                            </button>
                            <button className="ap-btn" onClick={onSync} disabled={busy}>
                                {busyAction === "sync" ? "Syncing..." : "Sync"}
                            </button>
                        </div>
                    </div>
                </section>
            )}

            {/* Chat — available in local fallback mode or full agent mode */}
            {hermes?.configured && (hermes.installed || hermes.direct_ollama_ready) && (
                <section className="ap-card ap-chat-card">
                    <div className="ap-chat-header">
                        <div className="ap-card-title">{chatTitle}</div>
                        <button
                            className="ap-ghost-btn"
                            onClick={onResetConversation}
                            disabled={busyAction === "send"}
                        >
                            New conversation
                        </button>
                    </div>

                    <div className="ap-chat-messages" role="log">
                        {messages.length === 0 && (
                            <div className="ap-chat-empty">
                                <div className="ap-chat-empty-icon">
                                    <svg width="32" height="32" viewBox="0 0 32 32" fill="none">
                                        <circle cx="16" cy="16" r="12" stroke="currentColor" strokeWidth="1.5" opacity="0.4" />
                                        <circle cx="16" cy="16" r="4" fill="currentColor" opacity="0.3" />
                                    </svg>
                                </div>
                                <div className="ap-chat-empty-title">
                                    {emptyStateTitle}
                                </div>
                                <p>{emptyStateBody}</p>
                            </div>
                        )}

                        {messages.map((msg, i) => (
                            <div key={i} className={`ap-chat-row ap-chat-${msg.role}`}>
                                <div className="ap-chat-role">
                                    {msg.role === "user" ? "You" : assistantLabel}
                                </div>
                                <div className="ap-chat-bubble">
                                    {msg.content}
                                </div>
                            </div>
                        ))}

                        {busyAction === "send" && (
                            <div className="ap-chat-row ap-chat-assistant">
                                <div className="ap-chat-role">{assistantLabel}</div>
                                <div className="ap-chat-bubble ap-chat-thinking">
                                    <span /><span /><span />
                                </div>
                            </div>
                        )}
                        <div ref={chatBottomRef} />
                    </div>

                    <div className="ap-chat-input-area">
                        <textarea
                            ref={chatInputRef}
                            className="ap-chat-textarea"
                            placeholder={
                                isHermesReady
                                    ? fullAgentConfigured
                                        ? "Ask the FNDR agent..."
                                        : "Ask local chat..."
                                    : "Set up the agent runtime to send messages"
                            }
                            value={draft}
                            onChange={(e) => onDraftChange(e.target.value)}
                            onKeyDown={(e) => {
                                if (e.key === "Enter" && !e.shiftKey) {
                                    e.preventDefault();
                                    void onSend();
                                }
                            }}
                            rows={1}
                            disabled={!isHermesReady || busyAction === "send"}
                        />
                        <button
                            className="ap-chat-send-btn"
                            onClick={onSend}
                            disabled={!isHermesReady || !draft.trim() || busyAction === "send"}
                            aria-label="Send"
                        >
                            <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
                                <path d="M14 2L2 7.5L7 8.5M14 2L9 14L7 8.5M14 2L7 8.5" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" />
                            </svg>
                        </button>
                    </div>
                </section>
            )}

            {/* Error */}
            {(hermesError || hermes?.last_error) && (
                <div className="ap-error-banner">
                    <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
                        <circle cx="7" cy="7" r="6" stroke="currentColor" strokeWidth="1.2" />
                        <path d="M7 4v3M7 9.5v.5" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
                    </svg>
                    {hermesError ?? hermes?.last_error}
                </div>
            )}
        </div>
    );
}

// ─── Shared sub-components ────────────────────────────────────────────────────

function MetricCard({ label, value, detail, dotClass }: {
    label: string;
    value: string;
    detail: string;
    dotClass: string;
}) {
    return (
        <div className="ap-metric-card">
            <div className="ap-metric-header">
                <div className={`ap-dot ${dotClass}`} />
                <span className="ap-metric-label">{label}</span>
            </div>
            <div className="ap-metric-value">{value}</div>
            {detail && <div className="ap-metric-detail">{detail}</div>}
        </div>
    );
}

// ─── Provider helpers ─────────────────────────────────────────────────────────

function providerTabLabel(p: HermesProviderKind): string {
    switch (p) {
        case "ollama": return "Ollama";
        case "codex": return "Codex";
        case "openrouter": return "OpenRouter";
        case "custom": return "Custom";
    }
}

function providerTabStatus(p: HermesProviderKind, hermes: HermesBridgeStatus | null): string {
    if (!hermes) return "Checking...";
    switch (p) {
        case "ollama":
            if (!hermes.ollama_installed) return "Not installed";
            if (!hermes.ollama_reachable) return "Not running";
            return hermes.ollama_models.length > 0
                ? `${hermes.ollama_models.length} model${hermes.ollama_models.length !== 1 ? "s" : ""}`
                : "No models";
        case "codex":
            if (!hermes.codex_cli_installed) return "Not found";
            return hermes.codex_logged_in ? "Authenticated" : "Not signed in";
        case "openrouter":
            return "API key required";
        case "custom":
            return "Bring your endpoint";
    }
}

function providerDetailNote(p: HermesProviderKind, hermes: HermesBridgeStatus | null): string {
    switch (p) {
        case "ollama":
            if (!hermes?.ollama_installed) return "Install Ollama to run the FNDR agent fully locally. No API key needed.";
            if (!hermes.ollama_reachable) return "Ollama is installed but not running. Open Ollama or run `ollama serve`.";
            if (hermes.ollama_models.length === 0) return "Ollama is running but has no models. Pull one: `ollama pull llama3.2`";
            return `Running ${hermes.ollama_models.length} local model${hermes.ollama_models.length !== 1 ? "s" : ""}. FNDR can chat locally right away, and the full bundled agent runtime can layer on top of the same Ollama setup.`;
        case "codex":
            return hermes?.codex_logged_in
                ? "FNDR detected your local Codex auth. No extra API key is needed inside FNDR."
                : "Sign in to Codex on this Mac first, then return here.";
        case "openrouter":
            return "Access frontier models through OpenRouter. FNDR stores the key in its contained agent runtime.";
        case "custom":
            return "Point Hermes at any OpenAI-compatible endpoint — self-hosted or private.";
    }
}
