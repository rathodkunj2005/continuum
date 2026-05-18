import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { render, screen, cleanup } from "@testing-library/react";

vi.mock("./WorkModeShell", () => ({
    WorkModeShell: () => <div data-testid="work-shell" />,
    default: () => <div data-testid="work-shell" />,
}));

vi.mock("@/shared/components/MotionWallpaper", () => ({
    MotionWallpaper: () => <canvas data-testid="motion-wallpaper" />,
}));

import { AppShell } from "./AppShell";

describe("AppShell", () => {
    beforeEach(() => {
        localStorage.clear();
    });

    afterEach(() => {
        cleanup();
    });

    it("renders the main shell and motion wallpaper layer", () => {
        render(<AppShell />);
        expect(screen.getByTestId("work-shell")).toBeInTheDocument();
        expect(screen.getByTestId("motion-wallpaper")).toBeInTheDocument();
    });
});
