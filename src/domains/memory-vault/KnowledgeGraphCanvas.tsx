import { useEffect, useMemo, useRef } from "react";
import * as d3 from "d3";
import type { GraphNodeView, GraphView } from "./graph/types";
import {
    buildSimulation,
    type LayoutSimLink,
    type LayoutSimNode,
} from "./graph/graphLayoutEngine";

export interface KnowledgeGraphCanvasProps {
    view: GraphView;
    width: number;
    height: number;
    selectedId: string | null;
    hoveredId: string | null;
    neighborhoodIds: ReadonlySet<string>;
    pathNodeIds: ReadonlySet<string>;
    hubNodeIds: ReadonlySet<string>;
    maxTicks: number;
    onHover: (id: string | null) => void;
    onSelect: (node: GraphNodeView) => void;
}

export function KnowledgeGraphCanvas({
    view,
    width,
    height,
    selectedId,
    hoveredId,
    neighborhoodIds,
    pathNodeIds,
    hubNodeIds,
    maxTicks,
    onHover,
    onSelect,
}: KnowledgeGraphCanvasProps) {
    const svgRef = useRef<SVGSVGElement | null>(null);

    const simNodes = useMemo<LayoutSimNode[]>(
        () =>
            view.nodes.map((n) => ({
                id: n.id,
                size: n.size,
                community: n.community,
                view: n,
            })),
        [view.nodes],
    );

    const simLinks = useMemo<LayoutSimLink[]>(() => {
        const ids = new Set(simNodes.map((n) => n.id));
        return view.edges
            .filter((e) => ids.has(e.sourceId) && ids.has(e.targetId))
            .map((e) => ({
                id: e.id,
                source: e.sourceId,
                target: e.targetId,
                confidence: e.confidence,
                view: e,
            }));
    }, [view.edges, simNodes]);

    // Build & run simulation once per view; render into SVG.
    useEffect(() => {
        const svg = svgRef.current;
        if (!svg) return;
        svg.innerHTML = "";

        const actualWidth = svg.clientWidth || width || 800;

        const root = d3.select(svg);
        const gRoot = root.append("g").attr("class", "kg-canvas-root");

        const zoom = d3
            .zoom<SVGSVGElement, unknown>()
            .scaleExtent([0.35, 4])
            .on("zoom", (event) => {
                gRoot.attr("transform", event.transform.toString());
            });
        root.call(zoom);

        if (simNodes.length === 0) {
            gRoot
                .append("text")
                .attr("x", actualWidth / 2)
                .attr("y", height / 2)
                .attr("text-anchor", "middle")
                .attr("class", "kg-empty")
                .text("nothing to develop yet.");
            return;
        }

        const sim = buildSimulation(simNodes, simLinks, view.clusters, {
            width: actualWidth,
            height,
            maxTicks,
        });

        const linkSel = gRoot
            .append("g")
            .attr("class", "kg-edges")
            .selectAll<SVGLineElement, LayoutSimLink>("line")
            .data(simLinks, (d) => d.id)
            .join("line")
            .attr("class", (d) => `kg-edge kg-edge-${d.view.kind}`)
            .attr("data-edge-id", (d) => d.id)
            .attr("stroke-width", (d) => 0.4 + d.confidence * 1.6);

        const drag = d3
            .drag<SVGGElement, LayoutSimNode>()
            .on("start", (event, d) => {
                if (!event.active) sim.alphaTarget(0.25).restart();
                d.fx = d.x;
                d.fy = d.y;
            })
            .on("drag", (event, d) => {
                d.fx = event.x;
                d.fy = event.y;
            })
            .on("end", (event, d) => {
                if (!event.active) sim.alphaTarget(0);
                d.fx = null;
                d.fy = null;
            });

        const nodeSel = gRoot
            .append("g")
            .attr("class", "kg-nodes")
            .selectAll<SVGGElement, LayoutSimNode>("g")
            .data(simNodes, (d) => d.id)
            .join("g")
            .attr("class", "kg-node")
            .attr("data-node-id", (d) => d.id)
            .style("cursor", "pointer")
            .on("mouseenter", (_e, d) => onHover(d.id))
            .on("mouseleave", () => onHover(null))
            .on("click", (_e, d) => onSelect(d.view))
            .call(drag);

        nodeSel
            .append("circle")
            .attr("class", "kg-node-halo")
            .attr("r", (d) => d.size + 10);

        nodeSel
            .append("circle")
            .attr("class", "kg-node-core")
            .attr("r", (d) => d.size)
            .attr("fill", (d) =>
                d.community !== null
                    ? view.communityColors[d.community] ?? "var(--cp-accent)"
                    : "var(--cp-accent-muted)",
            );

        let ticks = 0;
        sim.on("tick", () => {
            ticks += 1;
            linkSel
                .attr("x1", (d) => (d.source as LayoutSimNode).x ?? 0)
                .attr("y1", (d) => (d.source as LayoutSimNode).y ?? 0)
                .attr("x2", (d) => (d.target as LayoutSimNode).x ?? 0)
                .attr("y2", (d) => (d.target as LayoutSimNode).y ?? 0);
            nodeSel.attr("transform", (d) => `translate(${d.x ?? 0},${d.y ?? 0})`);
            if (ticks >= maxTicks) {
                sim.alphaTarget(0);
                sim.stop();
            }
        });

        return () => {
            sim.stop();
            sim.on("tick", null);
            root.on(".zoom", null);
        };
    }, [
        simNodes,
        simLinks,
        view.clusters,
        view.communityColors,
        width,
        height,
        maxTicks,
        onHover,
        onSelect,
    ]);

    // Apply dim/highlight classes whenever selection / hover / neighborhood changes (no relayout).
    useEffect(() => {
        const svg = svgRef.current;
        if (!svg) return;
        const isDimming = hoveredId !== null || selectedId !== null;
        const focusSet = new Set<string>(neighborhoodIds);
        if (selectedId) focusSet.add(selectedId);
        if (hoveredId) focusSet.add(hoveredId);

        d3.select(svg)
            .selectAll<SVGGElement, LayoutSimNode>("g.kg-node")
            .attr("data-state", (d) => {
                if (!isDimming) return "idle";
                if (d.id === selectedId) return "selected";
                if (d.id === hoveredId) return "hovered";
                if (focusSet.has(d.id)) return "neighbor";
                return "dimmed";
            })
            .classed("kg-node-path", (d) => pathNodeIds.has(d.id))
            .classed("kg-node-hub", (d) => hubNodeIds.has(d.id));

        d3.select(svg)
            .selectAll<SVGLineElement, LayoutSimLink>("line.kg-edge")
            .attr("data-state", (d) => {
                if (!isDimming) return "idle";
                const sId = (d.source as LayoutSimNode).id;
                const tId = (d.target as LayoutSimNode).id;
                if (focusSet.has(sId) && focusSet.has(tId)) return "active";
                return "dimmed";
            });
    }, [selectedId, hoveredId, neighborhoodIds, pathNodeIds, hubNodeIds]);

    return (
        <svg
            ref={svgRef}
            className="kg-canvas"
            width="100%"
            height={height}
            role="img"
            aria-label="Knowledge graph"
        />
    );
}
