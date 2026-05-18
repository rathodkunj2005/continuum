import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { render, screen, act, cleanup } from "@testing-library/react";

// Stub the heavy shells so this test exercises only the mode-switch logic.
// Both stubs must be declared before importing AppShell because of
// vi.mock hoisting.
vi.mock("./WorkModeShell", () => ({
    WorkModeShell: () => <div data-testid="work-shell" />,
    default: () => <div data-testid="work-shell" />,
}));
vi.mock("./ScrollModeShell", () => ({
    ScrollModeShell: () => <div data-testid="scroll-shell" />,
    default: () => <div data-testid="scroll-shell" />,
}));

import { AppShell } from "./AppShell";
import { STORAGE_KEYS } from "@/shared/utils/config";

describe("AppShell", () => {
    beforeEach(() => {
        localStorage.clear();
    });

    afterEach(() => {
        cleanup();
    });

    it("renders WorkModeShell by default when no mode is stored", () => {
        render(<AppShell />);
        expect(screen.getByTestId("work-shell")).toBeInTheDocument();
        expect(screen.queryByTestId("scroll-shell")).not.toBeInTheDocument();
    });

    it("renders ScrollModeShell when localStorage has immersive mode", () => {
        localStorage.setItem(STORAGE_KEYS.appMode, "immersive");
        render(<AppShell />);
        expect(screen.getByTestId("scroll-shell")).toBeInTheDocument();
        expect(screen.queryByTestId("work-shell")).not.toBeInTheDocument();
    });

    it("toggles modes when ⌘. is pressed and persists to localStorage", () => {
        render(<AppShell />);
        expect(screen.getByTestId("work-shell")).toBeInTheDocument();

        act(() => {
            window.dispatchEvent(new KeyboardEvent("keydown", { key: ".", metaKey: true }));
        });

        expect(screen.getByTestId("scroll-shell")).toBeInTheDocument();
        expect(localStorage.getItem(STORAGE_KEYS.appMode)).toBe("immersive");

        act(() => {
            window.dispatchEvent(new KeyboardEvent("keydown", { key: ".", ctrlKey: true }));
        });

        expect(screen.getByTestId("work-shell")).toBeInTheDocument();
        expect(localStorage.getItem(STORAGE_KEYS.appMode)).toBe("work");
    });

    it("ignores ⌘. while focused inside an input", () => {
        render(
            <>
                <input data-testid="input" />
                <AppShell />
            </>
        );
        const input = screen.getByTestId("input") as HTMLInputElement;
        input.focus();

        act(() => {
            input.dispatchEvent(
                new KeyboardEvent("keydown", { key: ".", metaKey: true, bubbles: true })
            );
        });

        // Mode unchanged.
        expect(screen.getByTestId("work-shell")).toBeInTheDocument();
        expect(localStorage.getItem(STORAGE_KEYS.appMode)).toBeNull();
    });
});
