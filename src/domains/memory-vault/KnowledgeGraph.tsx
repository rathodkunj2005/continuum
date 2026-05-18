import { useEffect, useMemo, useRef, useState } from "react";
import type { InsightGraphEdge, InsightGraphNode } from "@/shared/ipc/tauri";
import { buildGraphView } from "./graph/graphDataBuilder";
import { buildLegend } from "./graph/graphLegendBuilder";
import {
    EMPTY_FILTERS,
    applyFilters,
    type GraphFilterState,
} from "./graph/graphFilters";
import { deriveFilterOptions } from "./graph/graphFilterOptions";
import type { GraphNodeView } from "./graph/types";
import { KnowledgeGraphCanvas, type KnowledgeGraphCanvasHandle } from "./KnowledgeGraphCanvas";
import { KnowledgeGraphSidePanel } from "./KnowledgeGraphSidePanel";
import { KnowledgeGraphLegend } from "./KnowledgeGraphLegend";
import { KnowledgeGraphTopBar } from "./KnowledgeGraphTopBar";
import { KnowledgeGraphZoomControls } from "./KnowledgeGraphZoomControls";
import { GRAPH_SIM_MAX_TICKS } from "./useGraph";
import "./KnowledgeGraph.css";

export interface KnowledgeGraphProps {
    nodes: InsightGraphNode[];
    edges: InsightGraphEdge[];
    /** Numeric height in pixels, or a CSS string like "100%". Defaults to 480. */
    height?: number | string;
    onNodeClick?: (node: InsightGraphNode) => void;
    selectedNodeId?: string | null;
    pathNodeIds?: readonly string[] | null;
    highlightNodeIds?: readonly string[] | null;
    /** Optional Louvain map from caller (back-compat with existing MemoryCardsPanel callsites). */
    louvainByNodeId?: Record<string, number> | null;
    maxSimulationTicks?: number;
    /** Hierarchical layout is no longer supported; this prop is accepted for back-compat and ignored. */
    layoutMode?: "hierarchical" | "force";
    /** When true, mount the vertical right-side memory card. Default: true. */
    showSidePanel?: boolean;
    /** When true, mount the top filter bar. Default: true. */
    showFilters?: boolean;
    /** When true, mount the right-side legend. Default: true. */
    showLegend?: boolean;
    /** When true, mount the bottom-right zoom controls. Default: true. */
    showZoomControls?: boolean;
    /** When true, render the loading scrim above the canvas. */
    loading?: boolean;
    /** When set, render the error scrim with this message. */
    errorMessage?: string | null;
}

export function KnowledgeGraph({
    nodes,
    edges,
    height = 480,
    onNodeClick,
    selectedNodeId = null,
    pathNodeIds = null,
    highlightNodeIds = null,
    louvainByNodeId = null,
    maxSimulationTicks = GRAPH_SIM_MAX_TICKS,
    showSidePanel = true,
    showFilters = true,
    showLegend = true,
    showZoomControls = true,
    loading = false,
    errorMessage = null,
}: KnowledgeGraphProps) {
    const fullView = useMemo(
        () =>
            buildGraphView({
                nodes,
                edges,
                louvain: louvainByNodeId ?? undefined,
            }),
        [nodes, edges, louvainByNodeId],
    );

    const filterOptions = useMemo(() => deriveFilterOptions(fullView), [fullView]);
    const [filterState, setFilterState] = useState<GraphFilterState>(EMPTY_FILTERS);

    const view = useMemo(() => applyFilters(fullView, filterState), [fullView, filterState]);

    const nodeIndex = useMemo(() => new Map(view.nodes.map((n) => [n.id, n])), [view.nodes]);
    const legendRows = useMemo(() => buildLegend(view), [view]);

    const [hoveredId, setHoveredId] = useState<string | null>(null);
    const [internalSelectedId, setInternalSelectedId] = useState<string | null>(null);
    const effectiveSelectedId = selectedNodeId ?? internalSelectedId;

    // Drop internal selection when an external selection change replaces it.
    useEffect(() => {
        if (selectedNodeId !== undefined && selectedNodeId !== null) {
            setInternalSelectedId(null);
        }
    }, [selectedNodeId]);

    const neighborhoodIds = useMemo(() => {
        const focus = hoveredId ?? effectiveSelectedId;
        if (!focus) return new Set<string>();
        const out = new Set<string>([focus]);
        for (const e of view.edges) {
            if (e.sourceId === focus) out.add(e.targetId);
            if (e.targetId === focus) out.add(e.sourceId);
        }
        return out;
    }, [hoveredId, effectiveSelectedId, view.edges]);

    const pathSet = useMemo(() => new Set(pathNodeIds ?? []), [pathNodeIds]);
    const hubSet = useMemo(() => new Set(highlightNodeIds ?? []), [highlightNodeIds]);

    const selectedNode = effectiveSelectedId ? nodeIndex.get(effectiveSelectedId) ?? null : null;
    const incidentEdges = useMemo(() => {
        if (!effectiveSelectedId) return [];
        return view.edges.filter(
            (e) => e.sourceId === effectiveSelectedId || e.targetId === effectiveSelectedId,
        );
    }, [view.edges, effectiveSelectedId]);

    const handleSelect = (n: GraphNodeView) => {
        setInternalSelectedId(n.id);
        onNodeClick?.(n.raw);
    };

    const canvasRef = useRef<KnowledgeGraphCanvasHandle | null>(null);
    const shellRef = useRef<HTMLDivElement | null>(null);
    const [legendCollapsed, setLegendCollapsed] = useState(false);

    // Keyboard shortcuts, scoped to the graph shell so they don't collide with global hotkeys.
    useEffect(() => {
        const el = shellRef.current;
        if (!el) return;
        const handler = (ev: KeyboardEvent) => {
            const target = ev.target as HTMLElement | null;
            const tag = target?.tagName ?? "";
            if (tag === "INPUT" || tag === "TEXTAREA" || target?.isContentEditable) return;
            switch (ev.key) {
                case "+":
                case "=":
                    canvasRef.current?.zoomIn();
                    ev.preventDefault();
                    break;
                case "-":
                case "_":
                    canvasRef.current?.zoomOut();
                    ev.preventDefault();
                    break;
                case "0":
                    canvasRef.current?.reset();
                    ev.preventDefault();
                    break;
                case "f":
                case "F":
                    canvasRef.current?.fit();
                    ev.preventDefault();
                    break;
                case "Escape":
                    setInternalSelectedId(null);
                    break;
            }
        };
        el.addEventListener("keydown", handler);
        return () => el.removeEventListener("keydown", handler);
    }, []);

    return (
        <div
            ref={shellRef}
            className={`knowledge-graph-shell${showSidePanel ? "" : " knowledge-graph-shell-bare"}`}
            data-empty={view.nodes.length === 0 ? "true" : undefined}
            style={{ height }}
            tabIndex={0}
        >
            <div className="knowledge-graph-canvas-wrap film-grain">
                {showFilters && (
                    <KnowledgeGraphTopBar
                        options={filterOptions}
                        filters={filterState}
                        onChange={setFilterState}
                        nodeCount={view.nodes.length}
                        edgeCount={view.edges.length}
                    />
                )}
                {showLegend && (
                    <KnowledgeGraphLegend
                        rows={legendRows}
                        collapsed={legendCollapsed}
                        onToggle={() => setLegendCollapsed((v) => !v)}
                    />
                )}
                <KnowledgeGraphCanvas
                    ref={canvasRef}
                    view={view}
                    width={0}
                    height={typeof height === "number" ? height : 0}
                    selectedId={effectiveSelectedId}
                    hoveredId={hoveredId}
                    neighborhoodIds={neighborhoodIds}
                    pathNodeIds={pathSet}
                    hubNodeIds={hubSet}
                    maxTicks={maxSimulationTicks}
                    onHover={setHoveredId}
                    onSelect={handleSelect}
                />
                {showZoomControls && <KnowledgeGraphZoomControls handle={canvasRef} />}
                {loading && (
                    <div
                        className="kg-state-scrim kg-state-loading"
                        role="status"
                        aria-live="polite"
                    >
                        <span className="kg-state-line" />
                        <span className="kg-state-line" />
                        <span className="kg-state-line" />
                        <p className="kg-state-text">developing…</p>
                    </div>
                )}
                {errorMessage && !loading && (
                    <div className="kg-state-scrim kg-state-error" role="alert">
                        <p className="kg-state-text">{errorMessage}</p>
                    </div>
                )}
            </div>
            {showSidePanel && (
                <KnowledgeGraphSidePanel
                    selected={selectedNode}
                    incidentEdges={incidentEdges}
                    nodeIndex={nodeIndex}
                    onSelectNode={handleSelect}
                />
            )}
        </div>
    );
}
