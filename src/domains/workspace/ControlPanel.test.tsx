import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { ControlPanel } from "./ControlPanel";

vi.mock("@/shared/ipc/tauri", () => ({
    PRIVACY_ALERTS_EVENT: "privacy://alerts",
    CAPTURE_STATUS_EVENT: "capture://status",
    deleteAllData: vi.fn(),
    deleteOlderThan: vi.fn(),
    getBlocklist: vi.fn().mockResolvedValue([]),
    getAutofillSettings: vi.fn().mockResolvedValue({
        enabled: true,
        shortcut: "Alt+F",
        lookback_days: 90,
        auto_inject_threshold: 0.9,
        prefer_typed_injection: true,
        max_candidates: 4,
    }),
    getContextRuntimeStatus: vi.fn().mockResolvedValue({
        status: "healthy",
        active_project: null,
        current_context_pack_id: null,
        latest_pack_summary: "",
        tokens_used: 0,
        last_generated_at: null,
        recent_pack_count: 0,
        activity_event_count: 0,
        decision_count: 0,
        runtime_tables_ready: true,
    }),
    getMemoryRepairProgress: vi.fn(),
    getStorageHealth: vi.fn().mockResolvedValue({
        memory_db_bytes: 1024,
        frames_bytes: 0,
        models_bytes: 2048,
        dev_build_cache_bytes: 0,
        runtime_total_bytes: 3072,
        measured_at_ms: 0,
    }),
    getStorageReclaimProgress: vi.fn(),
    getMcpServerStatus: vi.fn().mockResolvedValue({
        running: false,
        host: "127.0.0.1",
        port: 8799,
        endpoint: "http://127.0.0.1:8799/mcp",
        require_auth: false,
        auth_mode: "disabled for localhost",
        last_error: null,
    }),
    getPrivacyAlerts: vi.fn().mockResolvedValue([]),
    getRetentionDays: vi.fn().mockResolvedValue(7),
    pauseCapture: vi.fn(),
    resumeCapture: vi.fn(),
    reclaimMemoryStorage: vi.fn(),
    runMemoryRepairBackfill: vi.fn(),
    setBlocklist: vi.fn(),
    setAutofillSettings: vi.fn(),
    setRetentionDays: vi.fn(),
    startMcpServer: vi.fn(),
    stopMcpServer: vi.fn(),
}));

vi.mock("@/shared/ipc/onboarding", () => ({
    deleteAiModel: vi.fn(),
    downloadModel: vi.fn(),
    getModelDownloadStatus: vi.fn().mockResolvedValue({
        state: "idle",
        model_id: null,
        filename: null,
        download_url: null,
        destination_path: null,
        temp_path: null,
        bytes_downloaded: 0,
        total_bytes: 0,
        percent: 0,
        done: false,
        error: null,
        logs: [],
        updated_at_ms: 0,
    }),
    getOnboardingState: vi.fn().mockResolvedValue({
        step: "complete",
        model_downloaded: true,
        display_name: null,
        biometric_enabled: false,
    }),
    listAvailableModels: vi.fn().mockResolvedValue([]),
    onDownloadStatus: vi.fn().mockResolvedValue(() => {}),
    refreshAiModels: vi.fn(),
    saveOnboardingState: vi.fn(),
}));

afterEach(() => {
    cleanup();
    vi.clearAllMocks();
});

describe("ControlPanel", () => {
    it("exposes privacy alerts inside settings privacy", async () => {
        render(<ControlPanel status={null} compact={true} />);

        const settingsButton = screen.getByRole("button", { name: /open settings/i });
        expect(settingsButton).toBeInTheDocument();

        fireEvent.click(settingsButton);
        fireEvent.click(screen.getByRole("button", { name: /privacy/i }));

        expect(await screen.findByText(/no active privacy alerts/i)).toBeInTheDocument();
    });
});
