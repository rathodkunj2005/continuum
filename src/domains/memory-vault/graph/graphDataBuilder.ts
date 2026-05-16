import type { InsightGraphSubgraph } from "@/shared/ipc/tauri";
import type { GraphCluster, GraphEdgeView, GraphNodeView, GraphView } from "./types";
import { assignCommunityColors } from "./graphPalette";
import { edgeKindFor, explainEdge } from "./graphRelationshipResolver";

const MIN_RADIUS = 6;
const MAX_RADIUS = 18;
const BASE_RADIUS = 8;
const RADIUS_SCALE = 3;

const MAX_LABEL_LEN = 60;
function truncateLabel(label: string): string {
    if (label.length <= MAX_LABEL_LEN) return label;
    return `${label.slice(0, MAX_LABEL_LEN - 1).trimEnd()}…`;
}

function clamp(n: number, lo: number, hi: number): number {
    return Math.min(hi, Math.max(lo, n));
}

export function buildGraphView(subgraph: InsightGraphSubgraph): GraphView {
    const nodeIds = new Set(subgraph.nodes.map((n) => n.id));
    const connectionCounts = new Map<string, number>();
    for (const e of subgraph.edges) {
        if (nodeIds.has(e.source_id) && nodeIds.has(e.target_id)) {
            connectionCounts.set(e.source_id, (connectionCounts.get(e.source_id) ?? 0) + 1);
            connectionCounts.set(e.target_id, (connectionCounts.get(e.target_id) ?? 0) + 1);
        }
    }

    const louvain = subgraph.louvain ?? {};
    const nodes: GraphNodeView[] = subgraph.nodes.map((raw) => {
        const connectionCount = connectionCounts.get(raw.id) ?? 0;
        const size = clamp(
            BASE_RADIUS + Math.log2(connectionCount + 1) * RADIUS_SCALE,
            MIN_RADIUS,
            MAX_RADIUS,
        );
        const community = raw.id in louvain ? louvain[raw.id] : null;
        const importance = clamp(
            raw.confidence * (Math.log2(connectionCount + 1) / 4 + 0.25),
            0,
            1,
        );
        return {
            id: raw.id,
            raw,
            label: truncateLabel(raw.label),
            nodeType: raw.node_type,
            community,
            connectionCount,
            size,
            importance,
        };
    });

    const nodeIndex = new Map(subgraph.nodes.map((n) => [n.id, n]));
    const edges: GraphEdgeView[] = subgraph.edges
        .filter((e) => nodeIds.has(e.source_id) && nodeIds.has(e.target_id))
        .map((e) => {
            const source = nodeIndex.get(e.source_id)!;
            const target = nodeIndex.get(e.target_id)!;
            return {
                id: e.id,
                raw: e,
                sourceId: e.source_id,
                targetId: e.target_id,
                edgeType: e.edge_type,
                confidence: e.confidence,
                kind: edgeKindFor(e.edge_type),
                reasons: explainEdge(e, source, target),
            };
        });

    const clusterMap = new Map<number, string[]>();
    for (const n of nodes) {
        if (n.community === null) continue;
        const list = clusterMap.get(n.community) ?? [];
        list.push(n.id);
        clusterMap.set(n.community, list);
    }
    const clusters: GraphCluster[] = Array.from(clusterMap.entries())
        .map(([id, ids]) => ({
            id,
            nodeIds: ids,
            label: id === 0 && subgraph.cluster_0_name ? subgraph.cluster_0_name : null,
        }))
        .sort((a, b) => a.id - b.id);

    const communityColors = assignCommunityColors(clusters.map((c) => c.id));

    return { nodes, edges, clusters, communityColors };
}
