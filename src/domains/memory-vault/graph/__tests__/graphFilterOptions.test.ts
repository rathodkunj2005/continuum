import { describe, it, expect } from "vitest";
import type { GraphView } from "../types";
import { deriveFilterOptions } from "../graphFilterOptions";

const empty: GraphView = { nodes: [], edges: [], clusters: [], communityColors: {} };

describe("deriveFilterOptions", () => {
    it("returns empty options for an empty view", () => {
        const o = deriveFilterOptions(empty);
        expect(o).toEqual({
            nodeTypes: [],
            projects: [],
            topics: [],
            edgeKinds: [],
            confidenceRange: [0, 1],
        });
    });

    it("collects distinct node types from real nodes", () => {
        const view: GraphView = {
            ...empty,
            nodes: [
                {
                    id: "a",
                    raw: { node_type: "Concept", metadata: {} } as never,
                    label: "x",
                    nodeType: "Concept",
                    community: null,
                    connectionCount: 0,
                    size: 8,
                    importance: 0.3,
                },
                {
                    id: "b",
                    raw: { node_type: "Project", metadata: {} } as never,
                    label: "y",
                    nodeType: "Project",
                    community: null,
                    connectionCount: 0,
                    size: 8,
                    importance: 0.3,
                },
                {
                    id: "c",
                    raw: { node_type: "Concept", metadata: {} } as never,
                    label: "z",
                    nodeType: "Concept",
                    community: null,
                    connectionCount: 0,
                    size: 8,
                    importance: 0.3,
                },
            ],
        };
        const o = deriveFilterOptions(view);
        expect(o.nodeTypes.sort()).toEqual(["Concept", "Project"]);
    });

    it("collects projects and topics from node metadata strings", () => {
        const view: GraphView = {
            ...empty,
            nodes: [
                {
                    id: "a",
                    raw: {
                        node_type: "Concept",
                        metadata: { project: "Work / Continuum", topic: "color" },
                    } as never,
                    label: "x",
                    nodeType: "Concept",
                    community: null,
                    connectionCount: 0,
                    size: 8,
                    importance: 0.3,
                },
                {
                    id: "b",
                    raw: { node_type: "Concept", metadata: { project: "Work / Continuum" } } as never,
                    label: "y",
                    nodeType: "Concept",
                    community: null,
                    connectionCount: 0,
                    size: 8,
                    importance: 0.3,
                },
            ],
        };
        const o = deriveFilterOptions(view);
        expect(o.projects).toEqual(["Work / Continuum"]);
        expect(o.topics).toEqual(["color"]);
    });

    it("collects distinct edge kinds from real edges + computes confidence range", () => {
        const view: GraphView = {
            ...empty,
            edges: [
                {
                    id: "e1",
                    raw: {} as never,
                    sourceId: "a",
                    targetId: "b",
                    edgeType: "PartOf",
                    confidence: 0.9,
                    kind: "structural",
                    reasons: [],
                },
                {
                    id: "e2",
                    raw: {} as never,
                    sourceId: "a",
                    targetId: "b",
                    edgeType: "SimilarTo",
                    confidence: 0.5,
                    kind: "semantic",
                    reasons: [],
                },
                {
                    id: "e3",
                    raw: {} as never,
                    sourceId: "b",
                    targetId: "c",
                    edgeType: "PartOf",
                    confidence: 0.9,
                    kind: "structural",
                    reasons: [],
                },
            ],
        };
        const o = deriveFilterOptions(view);
        expect(o.edgeKinds.sort()).toEqual(["semantic", "structural"]);
        expect(o.confidenceRange[0]).toBeCloseTo(0.5);
        expect(o.confidenceRange[1]).toBeCloseTo(0.9);
    });
});
