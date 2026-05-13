import { useEffect, useMemo, useState } from "react";
import {
    MemoryCard,
    MemoryDebugInspector,
    SearchResult,
    deleteMemory,
    findVisuallySimilarMemories,
    getContextRuntimeStatus,
    getMemoryDebugInspector,
    listMemoryCards,
    type GraphNode,
} from "@/shared/ipc/tauri";
import "./MemoryCardsPanel.css";
import { InsightLayers } from "./InsightLayers";
import { KnowledgeGraph } from "./KnowledgeGraph";
import { GRAPH_SIM_MAX_TICKS, useGraph } from "./useGraph";

const VAULT_BROWSE_STORAGE_KEY = "fndr.memoryVault.browseMode";

function readStoredBrowseMode(): "list" | "graph" | "project" {
    try {
        const raw = sessionStorage.getItem(VAULT_BROWSE_STORAGE_KEY);
        if (raw === "list" || raw === "graph" || raw === "project") {
            return raw;
        }
    } catch {
        /* private mode */
    }
    return "graph";
}

interface MemoryCardsPanelProps {
    isVisible: boolean;
    onClose: () => void;
    appNames: string[];
    onMemoryDeleted?: (memoryId: string) => void;
}

const APP_FILTER_ALL = "__all__";
const TIME_FILTER_ALL = "__time_all__";
const PERSPECTIVE_FILTER_ALL = "__perspective_all__";
const MAX_RENDERED_CARDS = 300;

type TimeFilter =
    | typeof TIME_FILTER_ALL
    | "last_hour"
    | "today"
    | "last_24h"
    | "last_7d";

type PerspectiveFilter =
    | typeof PERSPECTIVE_FILTER_ALL
    | "web"
    | "coding"
    | "meetings"
    | "communication"
    | "docs";

const TIME_FILTER_OPTIONS: Array<{ value: TimeFilter; label: string }> = [
    { value: TIME_FILTER_ALL, label: "All history" },
    { value: "last_hour", label: "Last hour" },
    { value: "today", label: "Today" },
    { value: "last_24h", label: "Last 24 hours" },
    { value: "last_7d", label: "Last 7 days" },
];

const PERSPECTIVE_FILTER_OPTIONS: Array<{ value: PerspectiveFilter; label: string }> = [
    { value: PERSPECTIVE_FILTER_ALL, label: "All perspectives" },
    { value: "web", label: "Web pages" },
    { value: "coding", label: "Coding sessions" },
    { value: "meetings", label: "Meetings" },
    { value: "communication", label: "Communication" },
    { value: "docs", label: "Docs & writing" },
];

function normalizeText(value: string | undefined | null): string {
    if (!value) {
        return "";
    }
    return value
        .replace(/[\u0000-\u001f\u007f-\u009f]/g, " ")
        .replace(/\s*Sources:\s*[A-Za-z0-9,\-\s]+\.?$/i, "")
        .replace(/\s+/g, " ")
        .trim();
}

function hasReadableCharacters(value: string): boolean {
    return /[\p{L}\p{N}]/u.test(value);
}

function pickReadable(...candidates: Array<string | undefined | null>): string {
    for (const candidate of candidates) {
        const cleaned = normalizeText(candidate);
        if (cleaned && hasReadableCharacters(cleaned)) {
            return cleaned;
        }
    }
    return "";
}

function fallbackTitle(card: MemoryCard): string {
    const windowTitle = normalizeText(card.window_title);
    const title = normalizeText(card.title);
    const lowerWindow = windowTitle.toLowerCase();
    const lowerApp = card.app_name.toLowerCase();
    const genericWindow = !windowTitle
        || lowerWindow === lowerApp
        || includesAny(lowerWindow, ["new tab", "dashboard", "home", "settings"]);

    if (!genericWindow && (title.endsWith("...") || !title)) {
        return windowTitle;
    }

    return pickReadable(card.title, card.window_title)
        || `Memory in ${card.app_name}`;
}

function fallbackSummary(card: MemoryCard): string {
    const raw = pickReadable(card.summary, card.raw_snippets[0], card.window_title)
        || `Captured context in ${card.app_name}.`;
    return raw
        .replace(/^\s*(then|and then|after that|next)\s*[,:-]?\s+/i, "")
        .replace(/\.\s*(then|and then|after that|next)\s+/gi, ". ")
        .replace(/\s+/g, " ")
        .trim();
}

function includesAny(haystack: string, needles: string[]): boolean {
    return needles.some((needle) => haystack.includes(needle));
}

function getActivityIcon(activityType?: string): string {
    switch (activityType) {
        case "coding": return "💻";
        case "browsing": return "🌐";
        case "communication": return "💬";
        case "docs": return "📝";
        case "design": return "🎨";
        default: return "";
    }
}

function matchesFilters(
    card: MemoryCard,
    timeFilter: TimeFilter,
    perspectiveFilter: PerspectiveFilter
): boolean {
    const now = Date.now();
    const timestamp = Number(card.timestamp) || 0;

    // 1. Time Filtering
    if (timeFilter !== TIME_FILTER_ALL && timestamp > 0) {
        if (timeFilter === "last_hour" && timestamp < now - 60 * 60 * 1000) return false;
        if (timeFilter === "today" && new Date(timestamp).toDateString() !== new Date(now).toDateString()) return false;
        if (timeFilter === "last_24h" && timestamp < now - 24 * 60 * 60 * 1000) return false;
        if (timeFilter === "last_7d" && timestamp < now - 7 * 24 * 60 * 60 * 1000) return false;
    }

    // 2. Perspective Filtering — prefer structured activity_type when present
    if (perspectiveFilter === PERSPECTIVE_FILTER_ALL) {
        return true;
    }

    // Use structured field first for accuracy
    if (card.activity_type && card.activity_type !== "other") {
        if (perspectiveFilter === "coding") return card.activity_type === "coding";
        if (perspectiveFilter === "docs") return card.activity_type === "docs";
        if (perspectiveFilter === "communication") return card.activity_type === "communication";
        if (perspectiveFilter === "web") return card.activity_type === "browsing";
    }

    // Fall back to generic text signals when structured activity_type is absent.
    const text = normalizeText(
        `${card.window_title ?? ""} ${(card.context ?? []).join(" ")} ${card.summary ?? ""}`
    ).toLowerCase();
    const url = (card.url ?? "").toLowerCase();
    const hasAny = (terms: string[]) => terms.some((term) => text.includes(term));

    if (perspectiveFilter === "web") {
        return Boolean(card.url) || /^https?:\/\//i.test(url);
    }

    if (perspectiveFilter === "coding") {
        return hasAny(["code", "debug", "build", "compile", "branch", "commit", "pull request", "repo"]);
    }

    if (perspectiveFilter === "meetings") {
        return hasAny(["meeting", "agenda", "call", "transcript", "attendee", "follow-up"]);
    }

    if (perspectiveFilter === "communication") {
        return hasAny(["message", "email", "chat", "inbox", "reply", "thread"]);
    }

    if (perspectiveFilter === "docs") {
        return hasAny(["doc", "document", "summary", "outline", "spec", "readme", "note", "draft", "pdf"]);
    }

    return true;
}

function isContinuityCard(card: MemoryCard): boolean {
    return Boolean(card.continuity) || card.source_count > 1 || Boolean(card.continuation_of);
}

const MEMORY_BODY_TRUNCATE_CHARS = 360;

function memoryBodyText(card: MemoryCard): string {
    const raw = normalizeText(card.internal_context) || fallbackSummary(card);
    return raw
        .replace(/^Continues from\s+\S+(?::[^\n]*)?\n?/m, "")
        .replace(/^Reopen:\s+\S+\s*\n?/m, "")
        .trim();
}

function cardCopy(
    card: MemoryCard
): {
    title: string;
    summary: string;
    body: string;
    continuity: boolean;
} {
    const title = fallbackTitle(card);
    const continuity = isContinuityCard(card);
    const summary = fallbackSummary(card);
    const body = memoryBodyText(card) || summary;

    return { title, summary, body, continuity };
}

function isHttpUrl(target: string): boolean {
    return /^https?:\/\//i.test(target);
}

function isFileUrl(target: string): boolean {
    return /^file:\/\//i.test(target);
}

async function handleReopen(target: string) {
    if (isHttpUrl(target)) {
        window.open(target, "_blank", "noopener,noreferrer");
        return;
    }
    if (isFileUrl(target)) {
        try {
            const shellModule = await import("@tauri-apps/plugin-shell");
            await shellModule.open(target);
            return;
        } catch (err) {
            console.warn("Shell open failed; copying target to clipboard", err);
        }
    }
    try {
        await navigator.clipboard.writeText(target);
    } catch (err) {
        console.warn("Clipboard write failed", err);
    }
}

function formatDay(timestamp: number): string {
    const date = new Date(timestamp);
    const today = new Date();
    const yesterday = new Date(today);
    yesterday.setDate(yesterday.getDate() - 1);

    if (date.toDateString() === today.toDateString()) {
        return "Today";
    }
    if (date.toDateString() === yesterday.toDateString()) {
        return "Yesterday";
    }
    return date.toLocaleDateString(undefined, {
        weekday: "short",
        month: "short",
        day: "numeric",
    });
}

export function MemoryCardsPanel({ isVisible, onClose, appNames, onMemoryDeleted }: MemoryCardsPanelProps) {
    const [browseMode, setBrowseMode] = useState<"list" | "graph" | "project">(readStoredBrowseMode);
    const [insightProject, setInsightProject] = useState("");
    const [graphNodeFilter, setGraphNodeFilter] = useState("");
    const [selectedGraphNode, setSelectedGraphNode] = useState<GraphNode | null>(null);
    const [graphNodeDetail, setGraphNodeDetail] = useState<GraphNode | null>(null);
    const [graphDetailLoading, setGraphDetailLoading] = useState(false);
    const [memoryInspector, setMemoryInspector] = useState<MemoryDebugInspector | null>(null);
    const [memoryInspectorLoading, setMemoryInspectorLoading] = useState(false);
    const [pathFromId, setPathFromId] = useState<string | null>(null);
    const [pathHighlightIds, setPathHighlightIds] = useState<string[] | null>(null);
    const [hubHighlightIds, setHubHighlightIds] = useState<string[] | null>(null);

    const {
        subgraph,
        loading: graphLoading,
        error: graphError,
        load: loadGraph,
        fetchNodeDetail,
        fetchPath,
        fetchGodNodes,
    } = useGraph();

    const [cards, setCards] = useState<MemoryCard[]>([]);
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [appFilter, setAppFilter] = useState<string>(APP_FILTER_ALL);
    const [timeFilter, setTimeFilter] = useState<TimeFilter>(TIME_FILTER_ALL);
    const [perspectiveFilter, setPerspectiveFilter] = useState<PerspectiveFilter>(PERSPECTIVE_FILTER_ALL);
    const [deletingId, setDeletingId] = useState<string | null>(null);
    const [openDebugIds, setOpenDebugIds] = useState<Set<string>>(new Set());
    const [debugById, setDebugById] = useState<Record<string, MemoryDebugInspector>>({});
    const [debugLoadingId, setDebugLoadingId] = useState<string | null>(null);
    // Image-to-image (CLIP) similar-screens state, keyed by seed card id.
    const [openSimilarIds, setOpenSimilarIds] = useState<Set<string>>(new Set());
    const [similarById, setSimilarById] = useState<Record<string, SearchResult[]>>({});
    const [similarLoadingId, setSimilarLoadingId] = useState<string | null>(null);
    const [similarErrorById, setSimilarErrorById] = useState<Record<string, string>>({});
    const [expandedBodyIds, setExpandedBodyIds] = useState<Set<string>>(new Set());

    useEffect(() => {
        try {
            sessionStorage.setItem(VAULT_BROWSE_STORAGE_KEY, browseMode);
        } catch {
            /* ignore */
        }
    }, [browseMode]);

    const toggleExpandedBody = (memoryId: string) => {
        setExpandedBodyIds((previous) => {
            const next = new Set(previous);
            if (next.has(memoryId)) {
                next.delete(memoryId);
            } else {
                next.add(memoryId);
            }
            return next;
        });
    };

    const selectableApps = useMemo(() => {
        return appNames
            .map((name) => name.trim())
            .filter((name) => name.length > 0)
            .sort((a, b) => a.localeCompare(b));
    }, [appNames]);

    const filteredCards = useMemo(
        () => cards.filter((card) => matchesFilters(card, timeFilter, perspectiveFilter)),
        [cards, timeFilter, perspectiveFilter]
    );

    const uniqueProjects = useMemo(() => {
        const s = new Set<string>();
        for (const c of cards) {
            const p = c.project?.trim();
            if (p && p.toLowerCase() !== "unknown") {
                s.add(p);
            }
        }
        return Array.from(s).sort((a, b) => a.localeCompare(b));
    }, [cards]);

    const [projectPicker, setProjectPicker] = useState("");

    useEffect(() => {
        if (browseMode === "project" && !projectPicker.trim() && uniqueProjects.length > 0) {
            setProjectPicker(uniqueProjects[0]);
        }
    }, [browseMode, projectPicker, uniqueProjects]);

    useEffect(() => {
        if (!isVisible) {
            return;
        }
        let cancelled = false;
        void getContextRuntimeStatus()
            .then((status) => {
                if (cancelled) {
                    return;
                }
                const active = status.active_project?.trim();
                if (active) {
                    setInsightProject((prev) => (prev.trim() === "" ? active : prev));
                }
            })
            .catch(() => {
                /* best-effort default project */
            });
        return () => {
            cancelled = true;
        };
    }, [isVisible]);

    useEffect(() => {
        if (!isVisible) {
            return;
        }
        if (browseMode === "list") {
            void loadGraph({ mode: "full" });
            return;
        }
        if (browseMode === "project") {
            const p = projectPicker.trim() || uniqueProjects[0]?.trim() || "";
            if (!p) {
                return;
            }
            void loadGraph({ mode: "project", projectLabel: p });
            return;
        }
        const label = insightProject.trim();
        void loadGraph(
            label ? { mode: "project", projectLabel: label } : { mode: "full" }
        );
    }, [
        isVisible,
        browseMode,
        insightProject,
        projectPicker,
        uniqueProjects,
        loadGraph,
    ]);

    useEffect(() => {
        if (!isVisible) {
            setSelectedGraphNode(null);
            setGraphNodeDetail(null);
            setMemoryInspector(null);
            setPathFromId(null);
            setPathHighlightIds(null);
            setHubHighlightIds(null);
            setGraphNodeFilter("");
        }
    }, [isVisible]);

    const memoryIdsInGraph = useMemo(() => {
        const next = new Set<string>();
        for (const n of subgraph?.nodes ?? []) {
            for (const mid of n.source_memory_ids ?? []) {
                next.add(mid);
            }
        }
        return next;
    }, [subgraph]);

    const filteredGraphNodes = useMemo(() => {
        const q = graphNodeFilter.trim().toLowerCase();
        const nodes = subgraph?.nodes ?? [];
        if (!q) {
            return nodes;
        }
        return nodes.filter((n) => n.label.toLowerCase().includes(q));
    }, [subgraph, graphNodeFilter]);

    const filteredGraphNodeIds = useMemo(
        () => new Set(filteredGraphNodes.map((n) => n.id)),
        [filteredGraphNodes]
    );

    const filteredGraphEdges = useMemo(
        () =>
            (subgraph?.edges ?? []).filter(
                (e) => filteredGraphNodeIds.has(e.source_id) && filteredGraphNodeIds.has(e.target_id)
            ),
        [subgraph, filteredGraphNodeIds]
    );

    const vizGraphNodes = useMemo(
        () => filteredGraphNodes.map(({ embedding: _emb, ...rest }) => rest),
        [filteredGraphNodes]
    );

    const fullVizGraphNodes = useMemo(
        () => (subgraph?.nodes ?? []).map(({ embedding: _emb, ...rest }) => rest),
        [subgraph]
    );

    const fullGraphEdges = useMemo(() => subgraph?.edges ?? [], [subgraph]);

    const louvainByNodeId = useMemo((): Record<string, number> | null => {
        const m = subgraph?.louvain;
        if (!m || typeof m !== "object") {
            return null;
        }
        return m as Record<string, number>;
    }, [subgraph]);

    const matchingCardForInspector = useMemo(() => {
        if (!memoryInspector) {
            return null;
        }
        return cards.find((c) => c.id === memoryInspector.memory_id) ?? null;
    }, [cards, memoryInspector]);

    useEffect(() => {
        if (!isVisible) {
            return;
        }

        let cancelled = false;
        const selectedApp = appFilter === APP_FILTER_ALL ? null : appFilter;

        setLoading(cards.length === 0);
        setError(null);

        void listMemoryCards(1500, selectedApp)
            .then((items) => {
                if (cancelled) {
                    return;
                }
                setCards(items);
            })
            .catch((err) => {
                if (cancelled) {
                    return;
                }
                // Preserve existing cards if refresh fails so the panel remains usable.
                setError(err instanceof Error ? err.message : "Unable to load memory cards.");
            })
            .finally(() => {
                if (!cancelled) {
                    setLoading(false);
                }
            });

        return () => {
            cancelled = true;
        };
    }, [isVisible, appFilter]);

    const handleDeleteCard = async (memoryId: string) => {
        if (deletingId) {
            return;
        }

        setDeletingId(memoryId);
        try {
            const deleted = await deleteMemory(memoryId);
            if (deleted) {
                setCards((previous) => previous.filter((card) => card.id !== memoryId));
                onMemoryDeleted?.(memoryId);
            }
        } catch (err) {
            setError(err instanceof Error ? err.message : "Unable to delete memory.");
        } finally {
            setDeletingId(null);
        }
    };

    const handleToggleDebug = async (memoryId: string) => {
        const isOpen = openDebugIds.has(memoryId);
        if (isOpen) {
            setOpenDebugIds((previous) => {
                const next = new Set(previous);
                next.delete(memoryId);
                return next;
            });
            return;
        }
        if (!debugById[memoryId]) {
            setDebugLoadingId(memoryId);
            try {
                const debug = await getMemoryDebugInspector(memoryId);
                setDebugById((previous) => ({
                    ...previous,
                    [memoryId]: debug,
                }));
            } catch (err) {
                setError(err instanceof Error ? err.message : "Unable to load memory debug details.");
            } finally {
                setDebugLoadingId(null);
            }
        }
        setOpenDebugIds((previous) => {
            const next = new Set(previous);
            next.add(memoryId);
            return next;
        });
    };

    const handleToggleVisuallySimilar = async (memoryId: string) => {
        const isOpen = openSimilarIds.has(memoryId);
        if (isOpen) {
            setOpenSimilarIds((previous) => {
                const next = new Set(previous);
                next.delete(memoryId);
                return next;
            });
            return;
        }
        if (similarById[memoryId] === undefined) {
            setSimilarLoadingId(memoryId);
            try {
                const hits = await findVisuallySimilarMemories({
                    seedMemoryId: memoryId,
                    limit: 6,
                });
                setSimilarById((previous) => ({
                    ...previous,
                    [memoryId]: hits,
                }));
                setSimilarErrorById((previous) => {
                    const next = { ...previous };
                    delete next[memoryId];
                    return next;
                });
            } catch (err) {
                setSimilarErrorById((previous) => ({
                    ...previous,
                    [memoryId]:
                        err instanceof Error
                            ? err.message
                            : "Unable to load visually similar memories.",
                }));
            } finally {
                setSimilarLoadingId(null);
            }
        }
        setOpenSimilarIds((previous) => {
            const next = new Set(previous);
            next.add(memoryId);
            return next;
        });
    };

    const handleGraphNodeClick = async (node: GraphNode) => {
        setSelectedGraphNode(node);
        setGraphDetailLoading(true);
        setMemoryInspector(null);
        let detail: GraphNode = node;
        try {
            const fresh = await fetchNodeDetail(node.id);
            if (fresh) {
                detail = fresh;
            }
            setGraphNodeDetail(detail);
        } finally {
            setGraphDetailLoading(false);
        }
        const firstMem = detail.source_memory_ids?.[0];
        if (firstMem) {
            setBrowseMode("list");
            requestAnimationFrame(() => {
                document.getElementById(`memory-card-${firstMem}`)?.scrollIntoView({
                    behavior: "smooth",
                    block: "center",
                });
            });
        }
        if (!firstMem) {
            return;
        }
        setMemoryInspectorLoading(true);
        try {
            const insp = await getMemoryDebugInspector(firstMem);
            setMemoryInspector(insp);
        } catch {
            setMemoryInspector(null);
        } finally {
            setMemoryInspectorLoading(false);
        }
    };

    const handleCloseGraphDetail = () => {
        setSelectedGraphNode(null);
        setGraphNodeDetail(null);
        setMemoryInspector(null);
    };

    const handleGraphReload = () => {
        if (browseMode === "project") {
            const p = projectPicker.trim() || uniqueProjects[0]?.trim() || "";
            if (p) {
                void loadGraph({ mode: "project", projectLabel: p });
            }
            return;
        }
        const label = insightProject.trim();
        void loadGraph(label ? { mode: "project", projectLabel: label } : { mode: "full" });
    };

    const handleFindPathToSelected = async () => {
        if (!pathFromId || !selectedGraphNode || pathFromId === selectedGraphNode.id) {
            return;
        }
        const dto = await fetchPath(pathFromId, selectedGraphNode.id);
        setPathHighlightIds(dto?.nodes ?? null);
    };

    const handleToggleHubs = async () => {
        if (hubHighlightIds?.length) {
            setHubHighlightIds(null);
            return;
        }
        const g = await fetchGodNodes(16);
        if (!g?.nodes?.length) {
            return;
        }
        const ids = g.nodes.map((entry) => (Array.isArray(entry) ? entry[0] : String(entry)));
        setHubHighlightIds(ids);
    };

    if (!isVisible) {
        return null;
    }

    return (
        <div className="memory-cards-panel">
            <div className="memory-cards-header">
                <div className="memory-cards-heading">
                    <h2>Memory Vault</h2>
                    <p>Global graph, every memory, and project views — local to this Mac.</p>
                </div>
                <button className="ui-action-btn memory-cards-close-btn" onClick={onClose}>X</button>
            </div>

            <div className="memory-cards-toolbar">
                <div className="memory-cards-toolbar-top">
                    <div className="memory-cards-view-tabs" role="tablist" aria-label="Vault view">
                        <button
                            type="button"
                            role="tab"
                            aria-selected={browseMode === "graph"}
                            className={`memory-cards-tab${browseMode === "graph" ? " memory-cards-tab--active" : ""}`}
                            onClick={() => setBrowseMode("graph")}
                        >
                            Global graph
                        </button>
                        <button
                            type="button"
                            role="tab"
                            aria-selected={browseMode === "list"}
                            className={`memory-cards-tab${browseMode === "list" ? " memory-cards-tab--active" : ""}`}
                            onClick={() => setBrowseMode("list")}
                        >
                            All memories
                        </button>
                        <button
                            type="button"
                            role="tab"
                            aria-selected={browseMode === "project"}
                            className={`memory-cards-tab${browseMode === "project" ? " memory-cards-tab--active" : ""}`}
                            onClick={() => setBrowseMode("project")}
                        >
                            By project
                        </button>
                    </div>
                    {browseMode === "list" && (
                        <div className="memory-cards-count">{filteredCards.length} cards</div>
                    )}
                    {(browseMode === "graph" || browseMode === "project") && (
                        <div className="memory-cards-count">
                            {(subgraph?.nodes?.length ?? 0)} nodes · {(subgraph?.edges?.length ?? 0)} links
                        </div>
                    )}
                </div>

                {browseMode === "list" && (
                <div className="memory-cards-filters">
                    <label className="memory-cards-filter">
                        Universe
                        <div className="memory-cards-filter-control">
                            <select
                                value={appFilter}
                                onChange={(event) => setAppFilter(event.target.value)}
                            >
                                <option value={APP_FILTER_ALL}>All Apps</option>
                                {selectableApps.map((name) => (
                                    <option key={name} value={name}>
                                        {name}
                                    </option>
                                ))}
                            </select>
                            <svg className="memory-cards-filter-arrow" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
                                <path d="M6 9l6 6 6-6" />
                            </svg>
                        </div>
                    </label>

                    <label className="memory-cards-filter">
                        History
                        <div className="memory-cards-filter-control">
                            <select
                                value={timeFilter}
                                onChange={(event) => setTimeFilter(event.target.value as TimeFilter)}
                            >
                                {TIME_FILTER_OPTIONS.map((option) => (
                                    <option key={option.value} value={option.value}>
                                        {option.label}
                                    </option>
                                ))}
                            </select>
                            <svg className="memory-cards-filter-arrow" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
                                <path d="M6 9l6 6 6-6" />
                            </svg>
                        </div>
                    </label>

                    <label className="memory-cards-filter">
                        Perspective
                        <div className="memory-cards-filter-control">
                            <select
                                value={perspectiveFilter}
                                onChange={(event) => setPerspectiveFilter(event.target.value as PerspectiveFilter)}
                            >
                                {PERSPECTIVE_FILTER_OPTIONS.map((option) => (
                                    <option key={option.value} value={option.value}>
                                        {option.label}
                                    </option>
                                ))}
                            </select>
                            <svg className="memory-cards-filter-arrow" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
                                <path d="M6 9l6 6 6-6" />
                            </svg>
                        </div>
                    </label>
                </div>
                )}

                {(browseMode === "graph" || browseMode === "project") && (
                    <div className="memory-graph-chrome" aria-label="Graph scope">
                        {browseMode === "project" && (
                            <label className="memory-graph-project-field">
                                <span>Memory project</span>
                                <select
                                    value={projectPicker}
                                    onChange={(e) => setProjectPicker(e.target.value)}
                                >
                                    {uniqueProjects.map((p) => (
                                        <option key={p} value={p}>
                                            {p}
                                        </option>
                                    ))}
                                </select>
                            </label>
                        )}
                        {browseMode === "graph" && (
                        <label className="memory-graph-project-field">
                            <span>Insight project</span>
                            <input
                                type="text"
                                value={insightProject}
                                onChange={(e) => setInsightProject(e.target.value)}
                                placeholder="Blank = full graph"
                                spellCheck={false}
                            />
                        </label>
                        )}
                        <label className="memory-graph-filter-field">
                            <span>Filter labels</span>
                            <input
                                type="search"
                                value={graphNodeFilter}
                                onChange={(e) => setGraphNodeFilter(e.target.value)}
                                placeholder="Substring match"
                            />
                        </label>
                        <div className="memory-graph-chrome-actions">
                            <button type="button" className="ui-action-btn" onClick={() => handleGraphReload()}>
                                Reload
                            </button>
                            <button type="button" className="ui-action-btn" onClick={() => void handleToggleHubs()}>
                                {hubHighlightIds?.length ? "Clear hubs" : "Hub nodes"}
                            </button>
                            {pathHighlightIds && pathHighlightIds.length > 0 && (
                                <button
                                    type="button"
                                    className="ui-action-btn"
                                    onClick={() => setPathHighlightIds(null)}
                                >
                                    Clear path
                                </button>
                            )}
                        </div>
                    </div>
                )}
            </div>

            <div
                className={`memory-cards-body${
                    browseMode === "graph" || browseMode === "project" ? " memory-cards-body--graph" : ""
                }${browseMode === "list" ? " memory-cards-body--vault-list" : ""}`}
            >
                {browseMode === "list" && (
                <>
                <section className="memory-vault-global-graph" aria-label="Global memory graph">
                    {subgraph?.cluster_0_name ? (
                        <div className="memory-vault-cluster-legend" title="Louvain community 0 label">
                            {subgraph.cluster_0_name}
                        </div>
                    ) : null}
                    {graphError && (
                        <div className="memory-cards-inline-error" role="alert">
                            {graphError}
                        </div>
                    )}
                    {graphLoading && (subgraph?.nodes?.length ?? 0) === 0 && !graphError && (
                        <div className="memory-vault-graph-strip-loading">
                            <div className="thinking-loader thinking-loader-lg" aria-hidden="true" />
                            <p>Loading global graph…</p>
                        </div>
                    )}
                    {(subgraph?.nodes?.length ?? 0) > 0 && (
                        <KnowledgeGraph
                            height={220}
                            maxSimulationTicks={220}
                            nodes={fullVizGraphNodes}
                            edges={fullGraphEdges}
                            louvainByNodeId={louvainByNodeId}
                            onNodeClick={(n) => void handleGraphNodeClick(n)}
                            selectedNodeId={selectedGraphNode?.id ?? null}
                            pathNodeIds={pathHighlightIds}
                            highlightNodeIds={hubHighlightIds}
                        />
                    )}
                </section>
                {loading && cards.length === 0 && (
                    <div className="memory-cards-state">
                        <div className="thinking-loader thinking-loader-lg" aria-hidden="true" />
                        <p>Loading memory cards...</p>
                    </div>
                )}

                {error && filteredCards.length > 0 && (
                    <div className="memory-cards-inline-error">
                        {error}
                    </div>
                )}

                {error && filteredCards.length === 0 && (
                    <div className="memory-cards-state">
                        <p>{error}</p>
                    </div>
                )}

                {!loading && !error && filteredCards.length === 0 && (
                    <div className="memory-cards-state">
                        <p>No memory cards yet for this filter.</p>
                    </div>
                )}

                {filteredCards.length > 0 && (
                    <div className="memory-cards-stream">
                        {filteredCards.slice(0, MAX_RENDERED_CARDS).map((card) => {
                            const { body } = cardCopy(card);
                            const expanded = expandedBodyIds.has(card.id);
                            const truncated = body.length > MEMORY_BODY_TRUNCATE_CHARS;
                            const displayBody = !expanded && truncated
                                ? `${body.slice(0, MEMORY_BODY_TRUNCATE_CHARS).trimEnd()}…`
                                : body;
                            const reopenTarget = card.reopen_target?.trim();
                            const continuationId = card.continuation_of?.trim();

                            return (
                                <article
                                    key={card.id}
                                    id={`memory-card-${card.id}`}
                                    className="result-card memory-browse-card"
                                >
                                    <div className="result-meta memory-browse-meta">
                                        <div className="memory-browse-meta-main">
                                            <span className="result-app">
                                                {getActivityIcon(card.activity_type) && (
                                                    <span className="memory-activity-icon" title={card.activity_type}>
                                                        {getActivityIcon(card.activity_type)}
                                                    </span>
                                                )}
                                                {card.app_name}
                                            </span>
                                            <span className="result-time">
                                                {formatDay(card.timestamp)} ·{" "}
                                                {new Date(card.timestamp).toLocaleTimeString(undefined, {
                                                    hour: "2-digit",
                                                    minute: "2-digit",
                                                })}
                                            </span>
                                            {card.source_count > 1 && (
                                                <span className="memory-source-count" title={`Composed from ${card.source_count} captures`}>
                                                    {card.source_count} captures
                                                </span>
                                            )}
                                            {card.session_duration_mins !== undefined && card.session_duration_mins > 0 && (
                                                <span className="memory-duration" title="Session duration">
                                                    {card.session_duration_mins}m
                                                </span>
                                            )}
                                            {card.timeline_action_class &&
                                                card.timeline_action_class !== "other" && (
                                                    <span
                                                        className="memory-action-class-chip"
                                                        title="Timeline action (content-derived)"
                                                    >
                                                        {card.timeline_action_class}
                                                    </span>
                                                )}
                                            {(memoryIdsInGraph.has(card.id) || (card.insight_kg_node_count ?? 0) > 0) && (
                                                <span
                                                    className="memory-graph-badge"
                                                    title="Referenced by at least one insight graph entity"
                                                >
                                                    Graph
                                                </span>
                                            )}
                                        </div>
                                        <div className="memory-card-actions">
                                            <button
                                                className="ui-action-btn memory-delete-btn"
                                                onClick={(e) => { e.stopPropagation(); void handleToggleDebug(card.id); }}
                                                disabled={debugLoadingId === card.id}
                                                aria-label="Toggle memory debug details"
                                                title="Inspect memory debug data"
                                            >
                                                {debugLoadingId === card.id ? "Loading..." : openDebugIds.has(card.id) ? "Hide Debug" : "Debug"}
                                            </button>
                                            <button
                                                className="ui-action-btn memory-delete-btn"
                                                onClick={(e) => { e.stopPropagation(); void handleToggleVisuallySimilar(card.id); }}
                                                disabled={similarLoadingId === card.id}
                                                aria-label="Find visually similar screens"
                                                title="Find visually similar screens (CLIP image embedding)"
                                            >
                                                {similarLoadingId === card.id
                                                    ? "Loading..."
                                                    : openSimilarIds.has(card.id)
                                                        ? "Hide similar"
                                                        : "Find similar"}
                                            </button>
                                            <button
                                                className="ui-action-btn memory-delete-btn"
                                                onClick={(e) => { e.stopPropagation(); void handleDeleteCard(card.id); }}
                                                disabled={deletingId === card.id}
                                                aria-label="Delete memory card"
                                                title="Delete this memory"
                                            >
                                                {deletingId === card.id ? "Deleting..." : "Delete"}
                                            </button>
                                        </div>
                                    </div>
                                    <div className="memory-browse-content">
                                        <div className="memory-browse-summary memory-browse-summary-primary">
                                            {displayBody}
                                            {truncated && (
                                                <button
                                                    type="button"
                                                    className="memory-body-expand"
                                                    onClick={() => toggleExpandedBody(card.id)}
                                                >
                                                    {expanded ? "Show less" : "Show more"}
                                                </button>
                                            )}
                                        </div>
                                        {(reopenTarget || continuationId) && (
                                            <div className="memory-browse-affordances">
                                                {reopenTarget && (
                                                    <button
                                                        type="button"
                                                        className="memory-reopen-anchor"
                                                        onClick={() => { void handleReopen(reopenTarget); }}
                                                        title={reopenTarget}
                                                    >
                                                        Reopen
                                                    </button>
                                                )}
                                                {continuationId && (
                                                    <span
                                                        className="memory-continuation-chip"
                                                        title={`Continues from ${continuationId}`}
                                                    >
                                                        Continues from earlier capture
                                                    </span>
                                                )}
                                            </div>
                                        )}
                                        {card.files_touched && card.files_touched.length > 0 && (
                                            <div className="memory-browse-files">
                                                {card.files_touched.slice(0, 4).map((f) => (
                                                    <span key={f} className="memory-file-chip" title={f}>{f}</span>
                                                ))}
                                            </div>
                                        )}
                                        <InsightLayers
                                            card={card}
                                            evalUi={openDebugIds.has(card.id)}
                                        />
                                        {openDebugIds.has(card.id) && (
                                            <div className="memory-debug-drawer">
                                                <pre>
{JSON.stringify(debugById[card.id] ?? { memory_id: card.id, status: "loading" }, null, 2)}
                                                </pre>
                                            </div>
                                        )}
                                        {openSimilarIds.has(card.id) && (
                                            <div className="memory-similar-drawer">
                                                <div className="memory-similar-heading">
                                                    Visually similar screens
                                                </div>
                                                {similarErrorById[card.id] && (
                                                    <p
                                                        className="memory-similar-empty"
                                                        role="alert"
                                                    >
                                                        {similarErrorById[card.id]}
                                                    </p>
                                                )}
                                                {!similarErrorById[card.id]
                                                    && (similarById[card.id]?.length ?? 0) === 0 && (
                                                        <p className="memory-similar-empty">
                                                            No visually similar screens yet. Older
                                                            captures may pre-date the CLIP image
                                                            embedding wiring.
                                                        </p>
                                                    )}
                                                {!similarErrorById[card.id]
                                                    && (similarById[card.id]?.length ?? 0) > 0 && (
                                                        <ul className="memory-similar-list">
                                                            {similarById[card.id]!.map((hit) => (
                                                                <li
                                                                    key={hit.id}
                                                                    className="memory-similar-item"
                                                                >
                                                                    <div className="memory-similar-meta">
                                                                        <span className="memory-similar-app">
                                                                            {hit.app_name}
                                                                        </span>
                                                                        <span className="memory-similar-time">
                                                                            {new Date(hit.timestamp).toLocaleString()}
                                                                        </span>
                                                                        <span className="memory-similar-score">
                                                                            {(hit.score * 100).toFixed(0)}%
                                                                        </span>
                                                                    </div>
                                                                    <div className="memory-similar-title">
                                                                        {hit.window_title || hit.snippet || hit.text.slice(0, 120)}
                                                                    </div>
                                                                </li>
                                                            ))}
                                                        </ul>
                                                    )}
                                            </div>
                                        )}
                                    </div>
                                </article>
                            );
                        })}
                    </div>
                )}
                </>
                )}

                {(browseMode === "graph" || browseMode === "project") && (
                    <div className="memory-graph-layout">
                        {graphError && (
                            <div className="memory-cards-inline-error" role="alert">
                                {graphError}
                            </div>
                        )}
                        {graphLoading && (subgraph?.nodes?.length ?? 0) === 0 && !graphError && (
                            <div className="memory-cards-state">
                                <div className="thinking-loader thinking-loader-lg" aria-hidden="true" />
                                <p>Loading knowledge graph...</p>
                            </div>
                        )}
                        {!graphLoading && !graphError && (subgraph?.nodes?.length ?? 0) === 0 && (
                            <div className="memory-cards-state" role="status">
                                <p>No graph nodes yet for this scope.</p>
                                <p className="memory-graph-empty-hint">
                                    Try clearing the project field for the full graph, or capture more context so entities can be extracted.
                                </p>
                            </div>
                        )}
                        {(subgraph?.nodes?.length ?? 0) > 0 && (
                            <div className="memory-graph-stage">
                                <KnowledgeGraph
                                    height={420}
                                    maxSimulationTicks={GRAPH_SIM_MAX_TICKS}
                                    nodes={vizGraphNodes}
                                    edges={filteredGraphEdges}
                                    louvainByNodeId={louvainByNodeId}
                                    onNodeClick={(n) => void handleGraphNodeClick(n)}
                                    selectedNodeId={selectedGraphNode?.id ?? null}
                                    pathNodeIds={pathHighlightIds}
                                    highlightNodeIds={hubHighlightIds}
                                />
                                {selectedGraphNode && (
                                    <aside className="memory-graph-detail" aria-label="Graph node detail">
                                        <div className="memory-graph-detail-header">
                                            <h3>{graphNodeDetail?.label ?? selectedGraphNode.label}</h3>
                                            <button
                                                type="button"
                                                className="ui-action-btn memory-graph-detail-close"
                                                onClick={handleCloseGraphDetail}
                                                aria-label="Close node detail"
                                            >
                                                Close
                                            </button>
                                        </div>
                                        {graphDetailLoading && <p className="memory-graph-detail-muted">Loading node…</p>}
                                        {!graphDetailLoading && graphNodeDetail && (
                                            <dl className="memory-graph-detail-dl">
                                                <div><dt>Type</dt><dd>{graphNodeDetail.node_type}</dd></div>
                                                <div><dt>Confidence</dt><dd>{graphNodeDetail.confidence.toFixed(2)}</dd></div>
                                                {graphNodeDetail.stale !== undefined && (
                                                    <div><dt>Stale</dt><dd>{graphNodeDetail.stale ? "yes" : "no"}</dd></div>
                                                )}
                                                <div>
                                                    <dt>Source memories</dt>
                                                    <dd>
                                                        {(graphNodeDetail.source_memory_ids ?? []).join(", ")
                                                            || "None"}
                                                    </dd>
                                                </div>
                                            </dl>
                                        )}
                                        <div className="memory-graph-detail-actions">
                                            <button
                                                type="button"
                                                className="ui-action-btn"
                                                onClick={() => setPathFromId(graphNodeDetail?.id ?? selectedGraphNode.id)}
                                            >
                                                Set path start
                                            </button>
                                            {pathFromId
                                                && pathFromId !== (graphNodeDetail?.id ?? selectedGraphNode.id) && (
                                                <button
                                                    type="button"
                                                    className="ui-action-btn"
                                                    onClick={() => void handleFindPathToSelected()}
                                                >
                                                    Find path here
                                                </button>
                                            )}
                                        </div>
                                        <div className="memory-graph-memory-block">
                                            <h4>Primary memory</h4>
                                            {memoryInspectorLoading && (
                                                <p className="memory-graph-detail-muted">Loading memory…</p>
                                            )}
                                            {!memoryInspectorLoading && matchingCardForInspector && (
                                                <InsightLayers card={matchingCardForInspector} evalUi={false} />
                                            )}
                                            {!memoryInspectorLoading && memoryInspector && !matchingCardForInspector && (
                                                <p className="memory-graph-memory-context">
                                                    {memoryInspector.memory_context}
                                                </p>
                                            )}
                                            {!memoryInspectorLoading && !memoryInspector && (
                                                <p className="memory-graph-detail-muted">
                                                    No linked memory payload for this node.
                                                </p>
                                            )}
                                        </div>
                                    </aside>
                                )}
                            </div>
                        )}
                    </div>
                )}
            </div>
        </div>
    );
}
