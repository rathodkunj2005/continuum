import * as d3 from "d3";
import type { GraphCluster, GraphEdgeView, GraphNodeView } from "./types";

export interface LayoutSimNode extends d3.SimulationNodeDatum {
    id: string;
    size: number;
    community: number | null;
    view: GraphNodeView;
}

export interface LayoutSimLink extends d3.SimulationLinkDatum<LayoutSimNode> {
    id: string;
    confidence: number;
    view: GraphEdgeView;
}

export interface LayoutConfig {
    width: number;
    height: number;
    /** Maximum tick count before settling. */
    maxTicks: number;
}

/** Build a pre-configured (but unstarted) force simulation. */
export function buildSimulation(
    nodes: LayoutSimNode[],
    links: LayoutSimLink[],
    clusters: GraphCluster[],
    config: LayoutConfig,
): d3.Simulation<LayoutSimNode, LayoutSimLink> {
    const { width, height } = config;

    const sim = d3
        .forceSimulation<LayoutSimNode>(nodes)
        .force(
            "link",
            d3
                .forceLink<LayoutSimNode, LayoutSimLink>(links)
                .id((d) => d.id)
                .distance(96)
                .strength((d) => {
                    const a = (d.source as LayoutSimNode).community;
                    const b = (d.target as LayoutSimNode).community;
                    if (a !== null && a === b) return 0.55;
                    return 0.25;
                }),
        )
        .force("charge", d3.forceManyBody<LayoutSimNode>().strength(-160))
        .force("center", d3.forceCenter(width / 2, height / 2))
        .force(
            "collision",
            d3.forceCollide<LayoutSimNode>().radius((d) => d.size + 8),
        );

    if (clusters.length > 0) {
        const ringR = Math.min(width, height) * 0.34;
        const target = new Map<number, { x: number; y: number }>();
        clusters.forEach((c, i) => {
            const angle = (i / clusters.length) * Math.PI * 2 - Math.PI / 2;
            target.set(c.id, {
                x: width / 2 + ringR * Math.cos(angle),
                y: height / 2 + ringR * Math.sin(angle),
            });
        });

        sim.force(
            "clusterX",
            d3
                .forceX<LayoutSimNode>((d) => {
                    if (d.community === null) return width / 2;
                    return target.get(d.community)?.x ?? width / 2;
                })
                .strength(0.22),
        ).force(
            "clusterY",
            d3
                .forceY<LayoutSimNode>((d) => {
                    if (d.community === null) return height / 2;
                    return target.get(d.community)?.y ?? height / 2;
                })
                .strength(0.22),
        );
    }

    return sim;
}
