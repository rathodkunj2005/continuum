import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
    cloudStatus: vi.fn(),
    cloudRequestOtp: vi.fn(),
    cloudVerifyOtp: vi.fn(),
}));

vi.mock("@/shared/ipc/cloud", () => mocks);

import { CloudSignIn } from "./CloudSignIn";

/**
 * Parent that builds a fresh `onSignedIn` reference on every render and can be
 * forced to re-render — mirroring the real callers, e.g. App.tsx's
 * `onSignedIn={() => setCloudGateOk(true)}`.
 */
function Harness() {
    const [, setTick] = useState(0);
    return (
        <div>
            <button onClick={() => setTick((t) => t + 1)}>force-rerender</button>
            <CloudSignIn onSignedIn={() => {}} />
        </div>
    );
}

describe("CloudSignIn", () => {
    beforeEach(() => {
        mocks.cloudStatus.mockReset();
        mocks.cloudRequestOtp.mockReset();
        mocks.cloudVerifyOtp.mockReset();
        mocks.cloudStatus.mockResolvedValue({
            configured: true,
            signed_in: false,
            email: null,
            user_id: null,
        });
        mocks.cloudRequestOtp.mockResolvedValue(undefined);
    });

    afterEach(() => cleanup());

    it("stays on the code step after requesting a code, even when the parent re-renders", async () => {
        render(<Harness />);

        const emailInput = await screen.findByPlaceholderText("you@company.com");
        fireEvent.change(emailInput, { target: { value: "dev@team.io" } });
        fireEvent.click(screen.getByRole("button", { name: /Email me a code/ }));

        // We reach the code-entry step once the OTP request resolves.
        await screen.findByText("Enter your code");
        expect(mocks.cloudRequestOtp).toHaveBeenCalledWith("dev@team.io");

        // A parent re-render hands CloudSignIn a new inline onSignedIn ref.
        // Regression: this used to re-run the mount status check and reset the
        // phase back to "email", bouncing the user off the code screen.
        fireEvent.click(screen.getByRole("button", { name: /force-rerender/ }));

        await waitFor(() => {
            expect(screen.getByText("Enter your code")).toBeInTheDocument();
        });
        expect(screen.queryByPlaceholderText("you@company.com")).not.toBeInTheDocument();
        // The mount check must run exactly once despite the parent re-render.
        expect(mocks.cloudStatus).toHaveBeenCalledTimes(1);
    });

    it("shows the unavailable state when no cloud backend is configured", async () => {
        mocks.cloudStatus.mockResolvedValue({
            configured: false,
            signed_in: false,
            email: null,
            user_id: null,
        });
        render(<CloudSignIn onSignedIn={() => {}} />);
        await screen.findByText(/Cloud sync isn't set up/i);
    });
});
