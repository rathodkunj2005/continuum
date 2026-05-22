import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
    companionGetStatus: vi.fn(),
    companionListDevices: vi.fn(),
    companionRevokeDevice: vi.fn(),
    companionStartPairing: vi.fn(),
}));

vi.mock("@/shared/ipc/tauri", () => mocks);

import { CompanionDevicesPanel } from "../CompanionDevicesPanel";

const runningStatus = {
    running: true,
    host: "127.0.0.1",
    port: 47812,
    tls: true,
    base_url: "https://127.0.0.1:47812",
    mac_name: "Test Mac",
    last_error: null,
};

describe("CompanionDevicesPanel", () => {
    beforeEach(() => {
        mocks.companionGetStatus.mockReset();
        mocks.companionListDevices.mockReset();
        mocks.companionRevokeDevice.mockReset();
        mocks.companionStartPairing.mockReset();

        mocks.companionGetStatus.mockResolvedValue(runningStatus);
        mocks.companionListDevices.mockResolvedValue([]);
    });

    afterEach(() => {
        cleanup();
    });

    it("renders the running server endpoint", async () => {
        render(<CompanionDevicesPanel pollIntervalMs={0} />);
        await screen.findByText(/Listening on 127\.0\.0\.1:47812 \(TLS\)/);
    });

    it("shows pair code returned by companion_start_pairing", async () => {
        mocks.companionStartPairing.mockResolvedValueOnce({
            pairing_code: "123456",
            qr_payload: "{\"v\":1}",
            expires_at_ms: Date.now() + 300_000,
            host: "127.0.0.1",
            port: 47812,
            cert_fingerprint_sha256: null,
        });

        render(<CompanionDevicesPanel pollIntervalMs={0} />);
        await screen.findByRole("button", { name: /Pair a device/ });
        fireEvent.click(screen.getByRole("button", { name: /Pair a device/ }));

        await waitFor(() => {
            expect(screen.getByTestId("pending-pair")).toBeInTheDocument();
        });
        // Formatted as "123 456".
        expect(screen.getByLabelText("pairing code")).toHaveTextContent("123 456");
    });

    it("lists paired devices and supports revoke", async () => {
        mocks.companionListDevices.mockResolvedValueOnce([
            {
                device_id: "dev_iphone_123",
                device_name: "Anurup's iPhone",
                device_type: "iphone",
                paired_at_ms: Date.now() - 1_000_000,
                last_seen_at_ms: Date.now() - 60_000,
                revoked_at_ms: null,
                app_version: "0.1.0",
            },
        ]);
        // After revoke the second list call reflects the revocation.
        mocks.companionRevokeDevice.mockResolvedValueOnce(true);
        mocks.companionListDevices.mockResolvedValueOnce([
            {
                device_id: "dev_iphone_123",
                device_name: "Anurup's iPhone",
                device_type: "iphone",
                paired_at_ms: Date.now() - 1_000_000,
                last_seen_at_ms: Date.now() - 60_000,
                revoked_at_ms: Date.now(),
                app_version: "0.1.0",
            },
        ]);

        render(<CompanionDevicesPanel pollIntervalMs={0} />);
        await screen.findByText("Anurup's iPhone");

        fireEvent.click(screen.getByRole("button", { name: /Revoke/ }));
        await waitFor(() => {
            expect(mocks.companionRevokeDevice).toHaveBeenCalledWith("dev_iphone_123");
        });
        await waitFor(() => {
            expect(screen.getByText("revoked")).toBeInTheDocument();
        });
    });

    it("surfaces backend errors", async () => {
        mocks.companionGetStatus.mockRejectedValueOnce(new Error("boom"));
        render(<CompanionDevicesPanel pollIntervalMs={0} />);
        await screen.findByTestId("companion-error");
        expect(screen.getByTestId("companion-error")).toHaveTextContent("boom");
    });

    it("disables Pair when the server is not running", async () => {
        mocks.companionGetStatus.mockResolvedValueOnce({
            ...runningStatus,
            running: false,
        });
        render(<CompanionDevicesPanel pollIntervalMs={0} />);
        const button = await screen.findByRole("button", { name: /Pair a device/ });
        expect(button).toBeDisabled();
    });
});
