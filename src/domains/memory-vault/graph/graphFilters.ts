import type { GraphView } from "./types";

export interface GraphFilterState {
    nodeTypes: ReadonlySet<string> | null;
    projects: ReadonlySet<string> | null;
    topics: ReadonlySet<string> | null;
    minConfidence: number;
    edgeKinds: ReadonlySet<string> | null;
}

export const EMPTY_FILTERS: GraphFilterState = {
    nodeTypes: null,
    projects: null,
    topics: null,
    minConfidence: 0,
    edgeKinds: null,
};

/** Returns a new view with nodes/edges that pass all active filters. Identity when no filters active. */
export function applyFilters(view: GraphView, filters: GraphFilterState): GraphView {
    const noActiveFilters =
        filters.nodeTypes === null &&
        filters.projects === null &&
        filters.topics === null &&
        filters.edgeKinds === null &&
        filters.minConfidence <= 0;
    if (noActiveFilters) return view;

    const nodes = view.nodes.filter((n) => {
        if (filters.nodeTypes && !filters.nodeTypes.has(n.nodeType)) return false;
        const md =
            n.raw.metadata && typeof n.raw.metadata === "object"
                ? (n.raw.metadata as Record<string, unknown>)
                : null;
        if (filters.projects) {
            const project = md?.project;
            if (typeof project !== "string" || !filters.projects.has(project)) return false;
        }
        if (filters.topics) {
            const topic = md?.topic;
            if (typeof topic !== "string" || !filters.topics.has(topic)) return false;
        }
        return true;
    });

    const keepIds = new Set(nodes.map((n) => n.id));
    const edges = view.edges.filter((e) => {
        if (!keepIds.has(e.sourceId) || !keepIds.has(e.targetId)) return false;
        if (filters.edgeKinds && !filters.edgeKinds.has(e.kind)) return false;
        if (e.confidence < filters.minConfidence) return false;
        return true;
    });

    const remainingCommunities = new Set(
        nodes.map((n) => n.community).filter((c): c is number => c !== null),
    );
    const clusters = view.clusters.filter((c) => remainingCommunities.has(c.id));

    return { nodes, edges, clusters, communityColors: view.communityColors };
}
