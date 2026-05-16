import type { GraphLegendRow, GraphView } from "./types";

export function buildLegend(view: GraphView): GraphLegendRow[] {
    const rows: GraphLegendRow[] = [];

    for (const cluster of view.clusters) {
        const color = view.communityColors[cluster.id] ?? "var(--cp-accent-muted)";
        rows.push({
            kind: "community",
            label: cluster.label ?? `community ${cluster.id}`,
            swatch: { color, shape: "dot" },
        });
    }

    const seenEdgeKinds = new Set<string>();
    for (const edge of view.edges) {
        if (seenEdgeKinds.has(edge.kind)) continue;
        seenEdgeKinds.add(edge.kind);
        rows.push({
            kind: "edge-kind",
            label: edge.kind,
            swatch: {
                color: "var(--cp-accent)",
                shape:
                    edge.kind === "semantic"
                        ? "dash"
                        : edge.kind === "reference"
                          ? "dot-dot"
                          : edge.kind === "temporal"
                            ? "arrow"
                            : "dot",
            },
        });
    }

    const hasConnections = view.nodes.some((n) => n.connectionCount > 0);
    if (hasConnections) {
        rows.push({
            kind: "encoding",
            label: "size · connection count",
            swatch: { color: "var(--cp-accent)", shape: "ring" },
        });
    }

    return rows;
}
