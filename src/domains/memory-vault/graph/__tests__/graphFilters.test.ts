import { describe, it, expect } from "vitest";
import type { GraphView } from "../types";
import { EMPTY_FILTERS, applyFilters } from "../graphFilters";

const sample: GraphView = {
    nodes: [
        {
            id: "a",
            raw: { metadata: { project: "P1" } } as never,
            label: "A",
            nodeType: "Concept",
            community: 0,
            connectionCount: 1,
            size: 8,
            importance: 0.5,
        },
        {
            id: "b",
            raw: { metadata: { project: "P2" } } as never,
            label: "B",
            nodeType: "Project",
            community: 1,
            connectionCount: 1,
            size: 8,
            importance: 0.5,
        },
    ],
    edges: [
        {
            id: "e1",
            raw: {} as never,
            sourceId: "a",
            targetId: "b",
            edgeType: "PartOf",
            confidence: 0.5,
            kind: "structural",
            reasons: [],
        },
    ],
    clusters: [
        { id: 0, nodeIds: ["a"], label: null },
        { id: 1, nodeIds: ["b"], label: null },
    ],
    communityColors: { 0: "x", 1: "y" },
};

describe("applyFilters", () => {
    it("returns the identical view when no filters are active", () => {
        expect(applyFilters(sample, EMPTY_FILTERS)).toBe(sample);
    });

    it("filters by nodeType and prunes orphan edges", () => {
        const out = applyFilters(sample, { ...EMPTY_FILTERS, nodeTypes: new Set(["Concept"]) });
        expect(out.nodes.map((n) => n.id)).toEqual(["a"]);
        expect(out.edges).toEqual([]);
        expect(out.clusters.map((c) => c.id)).toEqual([0]);
    });

    it("filters by minConfidence on edges", () => {
        const out = applyFilters(sample, { ...EMPTY_FILTERS, minConfidence: 0.9 });
        expect(out.edges).toEqual([]);
        expect(out.nodes.map((n) => n.id).sort()).toEqual(["a", "b"]);
    });

    it("filters by project metadata", () => {
        const out = applyFilters(sample, { ...EMPTY_FILTERS, projects: new Set(["P2"]) });
        expect(out.nodes.map((n) => n.id)).toEqual(["b"]);
    });

    it("filters by edgeKind", () => {
        const out = applyFilters(sample, { ...EMPTY_FILTERS, edgeKinds: new Set(["semantic"]) });
        expect(out.edges).toEqual([]);
        expect(out.nodes.map((n) => n.id).sort()).toEqual(["a", "b"]);
    });

    it("filters by topic metadata", () => {
        const view: GraphView = {
            ...sample,
            nodes: [
                {
                    ...sample.nodes[0],
                    raw: { metadata: { topic: "color" } } as never,
                },
                {
                    ...sample.nodes[1],
                    raw: { metadata: { topic: "archive" } } as never,
                },
            ],
        };
        const out = applyFilters(view, { ...EMPTY_FILTERS, topics: new Set(["archive"]) });
        expect(out.nodes.map((n) => n.id)).toEqual(["b"]);
    });
});
