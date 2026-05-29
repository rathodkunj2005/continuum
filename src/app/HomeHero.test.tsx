import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { HomeHero } from "./HomeHero";

vi.mock("@/shared/ipc/tauri", () => ({
    transcribeVoiceInput: vi.fn(),
}));

vi.mock("@/shared/motion/useReducedMotionSafe", () => ({
    useReducedMotionSafe: () => ({ reduced: true }),
}));

class MockIntersectionObserver {
    observe = vi.fn();
    disconnect = vi.fn();
}

describe("HomeHero", () => {
    beforeEach(() => {
        vi.stubGlobal("IntersectionObserver", MockIntersectionObserver);
    });

    afterEach(() => {
        cleanup();
        vi.unstubAllGlobals();
    });

    it("keeps the landing screen focused on search instead of extra CTA buttons", () => {
        render(
            <HomeHero
                userName="Anurup"
                now={new Date("2026-05-28T22:00:00")}
                greeting="Good Night, Anurup!"
                onHeroSearch={vi.fn()}
            />
        );

        expect(screen.getByRole("search")).toBeInTheDocument();
        expect(screen.queryByRole("button", { name: "Enter the reel" })).not.toBeInTheDocument();
        expect(screen.queryByRole("button", { name: "Open work mode" })).not.toBeInTheDocument();
    });
});
