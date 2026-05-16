import { describe, it, expect } from "vitest";
import type { InsightGraphSubgraph } from "@/shared/ipc/tauri";
import { buildGraphView } from "../graphDataBuilder";

const baseNode = {
    confidence: 1,
    source_memory_ids: [],
    embedding: null,
    created_at: "2026-05-16T00:00:00Z",
    updated_at: "2026-05-16T00:00:00Z",
    stale: false,
    metadata: {},
};

describe("buildGraphView", () => {
    it("returns an empty view for an empty subgraph", () => {
        const view = buildGraphView({ nodes: [], edges: [] });
        expect(view.nodes).toEqual([]);
        expect(view.edges).toEqual([]);
        expect(view.clusters).toEqual([]);
        expect(view.communityColors).toEqual({});
    });

    it("builds a single-node view with null community", () => {
        const sub: InsightGraphSubgraph = {
            nodes: [{ ...baseNode, id: "n1", node_type: "Concept", label: "A" }],
            edges: [],
        };
        const view = buildGraphView(sub);
        expect(view.nodes).toHaveLength(1);
        expect(view.nodes[0].community).toBeNull();
        expect(view.nodes[0].connectionCount).toBe(0);
        expect(view.clusters).toEqual([]);
    });

    it("groups nodes into clusters using the louvain map", () => {
        const sub: InsightGraphSubgraph = {
            nodes: [
                { ...baseNode, id: "n1", node_type: "Concept", label: "A" },
                { ...baseNode, id: "n2", node_type: "Concept", label: "B" },
                { ...baseNode, id: "n3", node_type: "Concept", label: "C" },
            ],
            edges: [],
            louvain: { n1: 0, n2: 0, n3: 1 },
            cluster_0_name: "primary",
        };
        const view = buildGraphView(sub);
        expect(view.clusters).toHaveLength(2);
        const c0 = view.clusters.find((c) => c.id === 0)!;
        const c1 = view.clusters.find((c) => c.id === 1)!;
        expect(c0.nodeIds.sort()).toEqual(["n1", "n2"]);
        expect(c1.nodeIds).toEqual(["n3"]);
        expect(c0.label).toBe("primary");
        expect(c1.label).toBeNull();
        expect(view.communityColors[0]).toBeDefined();
        expect(view.communityColors[1]).toBeDefined();
    });

    it("counts edges per node and grows size monotonically", () => {
        const sub: InsightGraphSubgraph = {
            nodes: [
                { ...baseNode, id: "hub", node_type: "Concept", label: "Hub" },
                { ...baseNode, id: "leaf", node_type: "Concept", label: "Leaf" },
                { ...baseNode, id: "leaf2", node_type: "Concept", label: "Leaf 2" },
            ],
            edges: [
                {
                    id: "e1",
                    source_id: "hub",
                    target_id: "leaf",
                    edge_type: "PartOf",
                    confidence: 0.9,
                    conflict_flag: false,
                    created_at: "x",
                    metadata: {},
                },
                {
                    id: "e2",
                    source_id: "hub",
                    target_id: "leaf2",
                    edge_type: "PartOf",
                    confidence: 0.9,
                    conflict_flag: false,
                    created_at: "x",
                    metadata: {},
                },
            ],
        };
        const view = buildGraphView(sub);
        const hub = view.nodes.find((n) => n.id === "hub")!;
        const leaf = view.nodes.find((n) => n.id === "leaf")!;
        expect(hub.connectionCount).toBe(2);
        expect(leaf.connectionCount).toBe(1);
        expect(hub.size).toBeGreaterThan(leaf.size);
        expect(hub.size).toBeLessThanOrEqual(18);
    });

    it("drops edges whose endpoints are not in the node set", () => {
        const sub: InsightGraphSubgraph = {
            nodes: [{ ...baseNode, id: "n1", node_type: "Concept", label: "A" }],
            edges: [
                {
                    id: "e1",
                    source_id: "n1",
                    target_id: "missing",
                    edge_type: "PartOf",
                    confidence: 1,
                    conflict_flag: false,
                    created_at: "x",
                    metadata: {},
                },
            ],
        };
        const view = buildGraphView(sub);
        expect(view.edges).toEqual([]);
    });

    it("attaches RelationshipReason[] to each surviving edge", () => {
        const sub: InsightGraphSubgraph = {
            nodes: [
                { ...baseNode, id: "n1", node_type: "Concept", label: "A" },
                { ...baseNode, id: "n2", node_type: "Concept", label: "B" },
            ],
            edges: [
                {
                    id: "e1",
                    source_id: "n1",
                    target_id: "n2",
                    edge_type: "PartOf",
                    confidence: 0.9,
                    conflict_flag: false,
                    created_at: "x",
                    metadata: {},
                },
            ],
        };
        const view = buildGraphView(sub);
        expect(view.edges).toHaveLength(1);
        expect(view.edges[0].reasons.length).toBeGreaterThan(0);
        expect(view.edges[0].reasons[0].text).toBe("part of B");
    });
});
