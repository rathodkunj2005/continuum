import { describe, it, expect } from "vitest";
import type { GraphView } from "../types";
import { buildLegend } from "../graphLegendBuilder";

const emptyView: GraphView = { nodes: [], edges: [], clusters: [], communityColors: {} };

describe("buildLegend", () => {
    it("returns no rows for an empty view", () => {
        expect(buildLegend(emptyView)).toEqual([]);
    });

    it("only emits edge-kind rows for kinds actually present", () => {
        const view: GraphView = {
            ...emptyView,
            edges: [
                {
                    id: "e",
                    raw: {} as never,
                    sourceId: "a",
                    targetId: "b",
                    edgeType: "PartOf",
                    confidence: 1,
                    kind: "structural",
                    reasons: [],
                },
            ],
        };
        const legend = buildLegend(view);
        const kinds = legend.filter((r) => r.kind === "edge-kind").map((r) => r.label);
        expect(kinds).toEqual(["structural"]);
    });

    it("emits one community row per cluster, using the assigned color", () => {
        const view: GraphView = {
            ...emptyView,
            clusters: [
                { id: 0, nodeIds: ["a"], label: "primary" },
                { id: 1, nodeIds: ["b"], label: null },
            ],
            communityColors: { 0: "hsl(30 58% 52%)", 1: "hsl(77 58% 52%)" },
        };
        const legend = buildLegend(view);
        const communities = legend.filter((r) => r.kind === "community");
        expect(communities).toHaveLength(2);
        expect(communities[0].swatch.color).toBe("hsl(30 58% 52%)");
        expect(communities[0].label).toBe("primary");
        expect(communities[1].label).toMatch(/^community 1$/);
    });

    it("emits an importance encoding row when any node has connections", () => {
        const view: GraphView = {
            ...emptyView,
            nodes: [
                {
                    id: "n",
                    raw: {} as never,
                    label: "x",
                    nodeType: "Concept",
                    community: null,
                    connectionCount: 3,
                    size: 12,
                    importance: 0.5,
                },
            ],
        };
        const legend = buildLegend(view);
        expect(legend.some((r) => r.kind === "encoding" && /size/i.test(r.label))).toBe(true);
    });
});
