import { forwardRef, useEffect, useImperativeHandle, useMemo, useRef } from "react";
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

export interface KnowledgeGraphCanvasHandle {
    zoomIn: () => void;
    zoomOut: () => void;
    reset: () => void;
    fit: () => void;
}

export const KnowledgeGraphCanvas = forwardRef<
    KnowledgeGraphCanvasHandle,
    KnowledgeGraphCanvasProps
>(function KnowledgeGraphCanvas(
    {
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
    },
    ref,
) {
    const svgRef = useRef<SVGSVGElement | null>(null);
    const zoomRef = useRef<d3.ZoomBehavior<SVGSVGElement, unknown> | null>(null);

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
        zoomRef.current = zoom;

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

        // Add ambient dust layer (30 fixed background circles)
        const dustG = gRoot.append("g").attr("class", "kg-dust");
        const centerX = (actualWidth || 800) / 2;
        const centerY = height / 2;
        const dustRadius = Math.min(centerX, centerY) * 1.1;
        for (let i = 0; i < 30; i++) {
            const angle = (Math.PI * 2 * i) / 30 + Math.sin(i * 1.7) * 0.4;
            const r = dustRadius * (0.2 + (Math.sin(i * 0.9 + 1.3) * 0.5 + 0.5) * 0.8);
            dustG
                .append("circle")
                .attr("cx", centerX + Math.cos(angle) * r)
                .attr("cy", centerY + Math.sin(angle) * r)
                .attr("r", 0.6)
                .attr("fill", "var(--fg)")
                .style("opacity", String(0.05 + (i % 7) * 0.02));
        }

        const nodesG = gRoot.append("g").attr("class", "kg-nodes");
        // Disable drift animation when graph is large (performance)
        if (simNodes.length > 500) {
            nodesG.attr("data-perf-reduced", "true");
        }

        const nodeSel = nodesG
            .selectAll<SVGGElement, LayoutSimNode>("g")
            .data(simNodes, (d) => d.id)
            .join("g")
            .attr("class", (d) => `kg-node kg-drift-${d.id.charCodeAt(0) % 5}`)
            .attr("data-node-id", (d) => d.id)
            .style("cursor", "pointer")
            .style("animation-delay", (d) => `${(d.id.charCodeAt(0) % 6) * 0.5}s`)
            .on("mouseenter", (_e, d) => onHover(d.id))
            .on("mouseleave", () => onHover(null))
            .on("click", (_e, d) => onSelect(d.view))
            .call(drag);

        // Outer halation ring (large, faint)
        nodeSel
            .append("circle")
            .attr("class", "kg-node-halo kg-node-halo-outer")
            .attr("r", (d) => d.size + 14)
            .attr("fill", "var(--accent)")
            .style("opacity", "0")
            .style("transition", "opacity var(--film-dur-fast) var(--film-ease-shutter)");

        // Inner halation ring
        nodeSel
            .append("circle")
            .attr("class", "kg-node-halo")
            .attr("r", (d) => d.size + 8);

        nodeSel
            .append("circle")
            .attr("class", "kg-node-core")
            .attr("r", (d) => d.size)
            .attr("fill", (d) =>
                d.community !== null
                    ? view.communityColors[d.community] ?? "var(--accent)"
                    : "var(--accent-2)",
            );

        // Label: show for weight≥3 or hovered/selected (CSS toggles visibility)
        nodeSel
            .append("text")
            .attr("class", "kg-node-label")
            .attr("text-anchor", "middle")
            .attr("y", (d) => d.size + 14)
            .attr("fill", "var(--fg-2)")
            .style("font", `11px var(--font-mono)`)
            .style("text-transform", "lowercase")
            .style("pointer-events", "none")
            .attr("data-weight-high", (d) => ((d.view.connectionCount ?? 0) >= 3 ? "true" : "false"))
            .text((d) => {
                const label = d.view.label ?? "";
                return label.length > 16 ? label.slice(0, 15) + "…" : label;
            });

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

    useImperativeHandle(
        ref,
        () => ({
            zoomIn: () => {
                const svg = svgRef.current;
                const zoom = zoomRef.current;
                if (!svg || !zoom) return;
                d3.select(svg).transition().duration(280).call(zoom.scaleBy, 1.4);
            },
            zoomOut: () => {
                const svg = svgRef.current;
                const zoom = zoomRef.current;
                if (!svg || !zoom) return;
                d3.select(svg).transition().duration(280).call(zoom.scaleBy, 1 / 1.4);
            },
            reset: () => {
                const svg = svgRef.current;
                const zoom = zoomRef.current;
                if (!svg || !zoom) return;
                d3.select(svg).transition().duration(420).call(zoom.transform, d3.zoomIdentity);
            },
            fit: () => {
                const svg = svgRef.current;
                const zoom = zoomRef.current;
                if (!svg || !zoom) return;
                const g = svg.querySelector("g.kg-canvas-root") as SVGGraphicsElement | null;
                if (!g) return;
                let bbox: DOMRect;
                try {
                    bbox = g.getBBox() as unknown as DOMRect;
                } catch {
                    return;
                }
                if (!bbox || bbox.width <= 0 || bbox.height <= 0) return;
                const w = svg.clientWidth || width || 800;
                const h = svg.clientHeight || height || 480;
                const pad = 32;
                const scale = Math.min(
                    (w - pad * 2) / bbox.width,
                    (h - pad * 2) / bbox.height,
                    4,
                );
                const tx = w / 2 - scale * (bbox.x + bbox.width / 2);
                const ty = h / 2 - scale * (bbox.y + bbox.height / 2);
                d3.select(svg)
                    .transition()
                    .duration(560)
                    .call(zoom.transform, d3.zoomIdentity.translate(tx, ty).scale(scale));
            },
        }),
        [width, height],
    );

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
});
