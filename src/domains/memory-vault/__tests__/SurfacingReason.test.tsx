import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { SurfacingReason } from "../SurfacingReason";

describe("SurfacingReason", () => {
    it("renders the headline and routes", () => {
        render(
            <SurfacingReason
                reason={{
                    headline: "Reached via SameTaskAs from planner seed",
                    routes: ["vector", "graph(1-hop via SameTaskAs:planner)"],
                    graph_path: [
                        { from_label: "planner seed", edge: "SameTaskAs", to_label: "planner" },
                    ],
                    anchor_terms_hit: ["planner"],
                    recency_boost: 0.0,
                }}
            />,
        );
        expect(screen.getByTestId("continuum-surfacing-reason")).toBeTruthy();
        expect(screen.getByText(/Reached via SameTaskAs/)).toBeTruthy();
    });

    it("renders nothing when the headline is missing", () => {
        const { container } = render(
            <SurfacingReason
                reason={{
                    headline: "",
                    routes: [],
                }}
            />,
        );
        expect(container.textContent).toBe("");
    });
});
