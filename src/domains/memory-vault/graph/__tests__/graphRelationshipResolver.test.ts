import { describe, it, expect } from "vitest";
import type { InsightGraphEdge, InsightGraphNode } from "@/shared/ipc/tauri";
import { explainEdge, edgeKindFor } from "../graphRelationshipResolver";

function mkNode(over: Partial<InsightGraphNode> = {}): InsightGraphNode {
    return {
        id: "n1",
        node_type: "Concept",
        label: "Halation",
        confidence: 1,
        source_memory_ids: [],
        embedding: null,
        created_at: "2026-05-16T00:00:00Z",
        updated_at: "2026-05-16T00:00:00Z",
        stale: false,
        metadata: {},
        ...over,
    };
}
function mkEdge(over: Partial<InsightGraphEdge> = {}): InsightGraphEdge {
    return {
        id: "e1",
        source_id: "n1",
        target_id: "n2",
        edge_type: "PartOf",
        confidence: 0.9,
        conflict_flag: false,
        created_at: "2026-05-16T00:00:00Z",
        metadata: {},
        ...over,
    };
}

describe("edgeKindFor", () => {
    it.each([
        ["PartOf", "structural"],
        ["Contains", "structural"],
        ["DependsOn", "structural"],
        ["Imports", "structural"],
        ["Extends", "structural"],
        ["Implements", "structural"],
        ["UsedIn", "structural"],
        ["CreatedBy", "structural"],
        ["SimilarTo", "semantic"],
        ["MentionedIn", "reference"],
        ["AppliesTo", "reference"],
        ["PrecededBy", "temporal"],
        ["FollowedBy", "temporal"],
        ["Causes", "temporal"],
        ["TriggeredBy", "temporal"],
        ["Contradicts", "conflict"],
        ["Supersedes", "conflict"],
        ["Resolves", "conflict"],
        ["Questions", "conflict"],
        ["UnknownNewEdgeType", "reference"], // fallback bucket
    ] as const)("classifies %s as %s", (edgeType, kind) => {
        expect(edgeKindFor(edgeType)).toBe(kind);
    });
});

describe("explainEdge", () => {
    it("returns the PartOf reason with the target label", () => {
        const source = mkNode({ id: "n1", label: "Halation" });
        const target = mkNode({ id: "n2", label: "Aperture notes" });
        const reasons = explainEdge(mkEdge({ edge_type: "PartOf" }), source, target);
        expect(reasons[0]).toEqual({
            text: "part of Aperture notes",
            tone: "neutral",
        });
    });

    it("appends shared project from both nodes' metadata", () => {
        const source = mkNode({ id: "n1", label: "A", metadata: { project: "Work / Continuum" } });
        const target = mkNode({ id: "n2", label: "B", metadata: { project: "Work / Continuum" } });
        const reasons = explainEdge(mkEdge({ edge_type: "PartOf" }), source, target);
        expect(reasons.map((r) => r.text)).toContain("shared project · Work / Continuum");
    });

    it("appends shared topic from both nodes' metadata", () => {
        const source = mkNode({ id: "n1", label: "A", metadata: { topic: "color theory" } });
        const target = mkNode({ id: "n2", label: "B", metadata: { topic: "color theory" } });
        const reasons = explainEdge(mkEdge({ edge_type: "PartOf" }), source, target);
        expect(reasons.map((r) => r.text)).toContain("shared topic · color theory");
    });

    it("uses amber tone for SimilarTo and appends confidence", () => {
        const source = mkNode({ id: "n1" });
        const target = mkNode({ id: "n2", label: "Memory B" });
        const reasons = explainEdge(
            mkEdge({ edge_type: "SimilarTo", confidence: 0.42 }),
            source,
            target,
        );
        expect(reasons[0].tone).toBe("amber");
        expect(reasons[0].text).toMatch(/semantic similarity · confidence 0\.42/);
    });

    it("uses alarm tone for Contradicts", () => {
        const reasons = explainEdge(
            mkEdge({ edge_type: "Contradicts" }),
            mkNode({ id: "n1" }),
            mkNode({ id: "n2", label: "Counter-evidence" }),
        );
        expect(reasons[0].tone).toBe("alarm");
        expect(reasons[0].text).toMatch(/contradicts Counter-evidence/);
    });

    it("appends '· low confidence' when edge confidence < 0.7", () => {
        const reasons = explainEdge(
            mkEdge({ edge_type: "DependsOn", confidence: 0.4 }),
            mkNode({ id: "n1" }),
            mkNode({ id: "n2", label: "Foundation" }),
        );
        expect(reasons[0].text).toMatch(/· low confidence$/);
    });

    it("falls back to a humanized form for unknown edge types", () => {
        const reasons = explainEdge(
            mkEdge({ edge_type: "MentionsObliquely" }),
            mkNode({ id: "n1" }),
            mkNode({ id: "n2", label: "Target" }),
        );
        expect(reasons[0].text).toMatch(/mentions obliquely/i);
    });

    it("returns a temporal reason for FollowedBy", () => {
        const reasons = explainEdge(
            mkEdge({ edge_type: "FollowedBy" }),
            mkNode({ id: "n1", label: "Earlier" }),
            mkNode({ id: "n2", label: "Later" }),
        );
        expect(reasons[0].text).toMatch(/^temporal · follows Later/);
    });
});
