import { useEffect, useMemo, useState } from "react";
import {
    backfillGraphFromExistingMemories,
    MemoryCard,
    MemoryDebugInspector,
    SearchResult,
    deleteMemory,
    findVisuallySimilarMemories,
    getMemoryDebugInspector,
    listMemoryCards,
    reopenMemory,
    type GraphNode,
} from "@/shared/ipc/tauri";
import "./MemoryCardsPanel.css";
import { InsightLayers } from "./InsightLayers";
import { KnowledgeGraph } from "./KnowledgeGraph";
import { GRAPH_SIM_MAX_TICKS, useGraph } from "./useGraph";
import { MemoryCard as MemoryCardComponent } from "./MemoryCard";
import { ExpandedMemoryCard } from "./ExpandedMemoryCard";
import { KnowledgeGraph3D } from "@/features/graph/components";

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
    feature?: "mixed" | "vault" | "graph";
    focusMemoryId?: string | null;
    onOpenMemoryById?: (memoryId: string) => void;
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

async function handleReopen(memoryId: string) {
    try {
        const opened = await reopenMemory(memoryId);
        if (!opened) {
            console.warn("No reopen target available for memory", memoryId);
        }
    } catch (err) {
        console.warn("Reopen command failed", err);
    }
}

export function MemoryCardsPanel({
    isVisible,
    onClose,
    appNames,
    onMemoryDeleted,
    feature = "mixed",
    focusMemoryId = null,
    onOpenMemoryById,
}: MemoryCardsPanelProps) {
    const [browseMode, setBrowseMode] = useState<"list" | "graph" | "project">(() => {
        if (feature === "vault") {
            return "list";
        }
        if (feature === "graph") {
            const stored = readStoredBrowseMode();
            return stored === "list" ? "graph" : stored;
        }
        return readStoredBrowseMode();
    });
    const [use3DGraph, setUse3DGraph] = useState(false);
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
    } = useGraph();

    const [cards, setCards] = useState<MemoryCard[]>([]);
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [appFilter, setAppFilter] = useState<string>(APP_FILTER_ALL);
    const [timeFilter, setTimeFilter] = useState<TimeFilter>(TIME_FILTER_ALL);
    const [perspectiveFilter, setPerspectiveFilter] = useState<PerspectiveFilter>(PERSPECTIVE_FILTER_ALL);
    const [deletingId, setDeletingId] = useState<string | null>(null);
    const [openDebugIds, setOpenDebugIds] = useState<Set<string>>(new Set());
    const [debugById, setDebugById] = useState<Record<string, MemoryDebugInspector | null>>({});
    const [, setDebugLoadingId] = useState<string | null>(null);
    // Image-to-image (CLIP) similar-screens state, keyed by seed card id.
    const [openSimilarIds, setOpenSimilarIds] = useState<Set<string>>(new Set());
    const [similarById, setSimilarById] = useState<Record<string, SearchResult[]>>({});
    const [, setSimilarLoadingId] = useState<string | null>(null);
    const [similarErrorById, setSimilarErrorById] = useState<Record<string, string>>({});
    /** Currently-expanded card id (one modal at a time). */
    const [openExpandedId, setOpenExpandedId] = useState<string | null>(null);

    const isVaultFeature = feature === "vault";
    const isGraphFeature = feature === "graph";
    const showListSurface = !isGraphFeature && browseMode === "list";
    const showGraphSurface = !isVaultFeature && (browseMode === "graph" || browseMode === "project");
    const showEmbeddedGraphStrip = feature === "mixed" && browseMode === "list";

    useEffect(() => {
        if (feature === "vault" && browseMode !== "list") {
            setBrowseMode("list");
            return;
        }
        if (feature === "graph" && browseMode === "list") {
            setBrowseMode("graph");
        }
    }, [feature, browseMode]);

    useEffect(() => {
        try {
            if (feature === "vault") {
                return;
            }
            sessionStorage.setItem(VAULT_BROWSE_STORAGE_KEY, browseMode);
        } catch {
            /* ignore */
        }
    }, [browseMode, feature]);

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


    useEffect(() => {
        if (!isVisible) {
            return;
        }
        if (isVaultFeature) {
            return;
        }
        void loadGraph({ mode: "full" });
    }, [isVisible, loadGraph, isVaultFeature]);

    useEffect(() => {
        if (!isVisible) {
            setSelectedGraphNode(null);
            setGraphNodeDetail(null);
            setMemoryInspector(null);
            setPathFromId(null);
            setPathHighlightIds(null);
            setHubHighlightIds(null);
        }
    }, [isVisible]);

    const vizGraphNodes = useMemo(
        () => (subgraph?.nodes ?? []).map(({ embedding: _emb, ...rest }) => rest),
        [subgraph]
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

    useEffect(() => {
        if (!isVisible || !focusMemoryId) {
            return;
        }
        if (!showListSurface) {
            return;
        }
        const frame = requestAnimationFrame(() => {
            document.getElementById(`memory-card-${focusMemoryId}`)?.scrollIntoView({
                behavior: "smooth",
                block: "center",
            });
        });
        return () => cancelAnimationFrame(frame);
    }, [isVisible, focusMemoryId, cards, showListSurface]);

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
            } catch {
                // Debug details are optional — don't surface as a panel-level error.
                // The expanded modal will simply show no debug section.
                setDebugById((previous) => ({ ...previous, [memoryId]: null }));
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

    const handleFindPathToSelected = async () => {
        if (!pathFromId || !selectedGraphNode || pathFromId === selectedGraphNode.id) {
            return;
        }
        const dto = await fetchPath(pathFromId, selectedGraphNode.id);
        setPathHighlightIds(dto?.nodes ?? null);
    };

    const handleGraphBackfill = async () => {
        await backfillGraphFromExistingMemories(2500);
        void loadGraph({ mode: "full" });
    };

    if (!isVisible) {
        return null;
    }

    return (
        <div className="memory-cards-panel">
            <div className="memory-cards-header">
                <div className="memory-cards-heading">
                    <h2>{isGraphFeature ? "Knowledge Graph" : "Memory Vault"}</h2>
                    <p>
                        {isGraphFeature
                            ? "Hierarchical memory graph with project, session, memory, and entity links."
                            : "All captured memories in one browseable vault."}
                    </p>
                </div>
                <button className="ui-action-btn memory-cards-close-btn" onClick={onClose}>X</button>
            </div>

            <div className="memory-cards-toolbar">
                <div className="memory-cards-toolbar-top">
                    {isVaultFeature ? null : (
                        <div className="memory-cards-view-tabs" role="tablist" aria-label="Graph view">
                            <button
                                type="button"
                                role="tab"
                                aria-selected={browseMode === "graph"}
                                className={`ui-action-btn memory-cards-tab${browseMode === "graph" ? " memory-cards-tab--active" : ""}`}
                                onClick={() => setBrowseMode("graph")}
                            >
                                Global graph
                            </button>
                            <button
                                type="button"
                                role="tab"
                                aria-selected={browseMode === "project"}
                                className={`ui-action-btn memory-cards-tab${browseMode === "project" ? " memory-cards-tab--active" : ""}`}
                                onClick={() => setBrowseMode("project")}
                            >
                                By project
                            </button>
                        </div>
                    )}
                    {showListSurface && (
                        <div className="memory-cards-count">{filteredCards.length} cards</div>
                    )}
                    {showGraphSurface && (
                        <div className="memory-cards-count">
                            {(subgraph?.nodes?.length ?? 0)} nodes · {(subgraph?.edges?.length ?? 0)} links
                        </div>
                    )}
                </div>

                {showListSurface && (
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

                {showGraphSurface && (
                    <div className="memory-graph-chrome" aria-label="Graph scope">
                    </div>
                )}
            </div>

            <div
                className={`memory-cards-body${
                    showGraphSurface ? " memory-cards-body--graph" : ""
                }${showListSurface ? " memory-cards-body--vault-list" : ""}`}
            >
                {showListSurface && (
                <>
                {showEmbeddedGraphStrip && (
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
                            showSidePanel={false}
                            showFilters={false}
                            showLegend={false}
                            showZoomControls={false}
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
                )}
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
                        {filteredCards.slice(0, MAX_RENDERED_CARDS).map((card) => (
                            <MemoryCardComponent
                                key={card.id}
                                card={card}
                                variant="compact"
                                onOpen={(c) => {
                                    setOpenExpandedId(c.id);
                                    void handleToggleDebug(c.id);
                                }}
                                threadCountHint={card.insight_kg_node_count}
                            />
                        ))}
                    </div>
                )}
                </>
                )}

                {showGraphSurface && (
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
                                <button type="button" className="ui-action-btn" onClick={() => void handleGraphBackfill()}>
                                    Build graph from existing memories
                                </button>
                            </div>
                        )}
                        {(subgraph?.nodes?.length ?? 0) > 0 && (
                            <div className="memory-graph-stage" style={{ position: "relative" }}>
                                {/* 2D/3D toggle button */}
                                <div style={{ position: "absolute", top: 10, right: 10, zIndex: 20 }}>
                                    <button
                                        type="button"
                                        className="ui-action-btn"
                                        onClick={() => setUse3DGraph(!use3DGraph)}
                                        style={{
                                            backgroundColor: use3DGraph ? "#0066cc" : "transparent",
                                            border: "1px solid #444",
                                            padding: "6px 12px",
                                            fontSize: "12px",
                                            cursor: "pointer",
                                        }}
                                    >
                                        {use3DGraph ? "📊 2D" : "🎨 3D"}
                                    </button>
                                </div>

                                {!use3DGraph ? (
                                    <KnowledgeGraph
                                        height="100%"
                                        maxSimulationTicks={GRAPH_SIM_MAX_TICKS}
                                        nodes={vizGraphNodes}
                                        edges={subgraph?.edges ?? []}
                                        louvainByNodeId={louvainByNodeId}
                                        onNodeClick={(n) => void handleGraphNodeClick(n)}
                                        selectedNodeId={selectedGraphNode?.id ?? null}
                                        pathNodeIds={pathHighlightIds}
                                        highlightNodeIds={hubHighlightIds}
                                        showLegend={false}
                                        showSidePanel={false}
                                    />
                                ) : (
                                    <KnowledgeGraph3D
                                        onClose={() => setUse3DGraph(false)}
                                    />
                                )}
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
                                            {graphNodeDetail?.source_memory_ids?.[0] && onOpenMemoryById && (
                                                <button
                                                    type="button"
                                                    className="ui-action-btn"
                                                    onClick={() => onOpenMemoryById(graphNodeDetail.source_memory_ids[0])}
                                                >
                                                    Open linked memory
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

            {(() => {
                if (!openExpandedId) return null;
                const expandedCard = cards.find((c) => c.id === openExpandedId);
                if (!expandedCard) return null;
                const debugOpen = openDebugIds.has(expandedCard.id);
                const similarOpen = openSimilarIds.has(expandedCard.id);
                const debugSlot = debugOpen ? (
                    <div className="memory-debug-drawer">
                        <pre>
{JSON.stringify(
    debugById[expandedCard.id] ?? { memory_id: expandedCard.id, status: "loading" },
    null,
    2,
)}
                        </pre>
                    </div>
                ) : null;
                const similarSlot = similarOpen ? (
                    <div className="memory-similar-drawer">
                        <div className="memory-similar-heading">Visually similar screens</div>
                        {similarErrorById[expandedCard.id] && (
                            <p className="memory-similar-empty" role="alert">
                                {similarErrorById[expandedCard.id]}
                            </p>
                        )}
                        {!similarErrorById[expandedCard.id]
                            && (similarById[expandedCard.id]?.length ?? 0) === 0 && (
                                <p className="memory-similar-empty">
                                    No visually similar screens yet. Older captures may pre-date
                                    the CLIP image embedding wiring.
                                </p>
                            )}
                        {!similarErrorById[expandedCard.id]
                            && (similarById[expandedCard.id]?.length ?? 0) > 0 && (
                                <ul className="memory-similar-list">
                                    {similarById[expandedCard.id]!.map((hit) => (
                                        <li key={hit.id} className="memory-similar-item">
                                            <div className="memory-similar-meta">
                                                <span className="memory-similar-app">{hit.app_name}</span>
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
                ) : null;
                return (
                    <ExpandedMemoryCard
                        card={expandedCard}
                        insightsSlot={
                            <>
                                <InsightLayers card={expandedCard} evalUi={debugOpen} />
                                <div className="fndr-emc-extra-actions">
                                    <button
                                        type="button"
                                        className="ui-action-btn"
                                        onClick={() => void handleToggleVisuallySimilar(expandedCard.id)}
                                    >
                                        {similarOpen ? "Hide similar" : "Find similar screens"}
                                    </button>
                                </div>
                            </>
                        }
                        debugSlot={debugSlot}
                        similarSlot={similarSlot}
                        onClose={() => setOpenExpandedId(null)}
                        onDelete={(id) => {
                            void handleDeleteCard(id);
                            setOpenExpandedId(null);
                        }}
                        onReopen={(c) => void handleReopen(c.id)}
                    />
                );
            })()}
        </div>
    );
}
