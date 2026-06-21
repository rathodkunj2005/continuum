import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { MemoryCardsPanel } from "./MemoryCardsPanel";
import { listMemoryCards } from "@/shared/ipc/tauri";
import type { MemoryCard } from "@/shared/ipc/tauri";

vi.mock("@/shared/ipc/tauri", () => ({
    deleteMemory: vi.fn(),
    listMemoryCards: vi.fn(),
    getFullGraph: vi.fn().mockResolvedValue({
        nodes: [],
        edges: [],
        louvain: {},
        cluster_0_name: "",
    }),
    getGraphForProject: vi.fn().mockResolvedValue({
        nodes: [],
        edges: [],
        louvain: {},
        cluster_0_name: "",
    }),
    getContextRuntimeStatus: vi.fn().mockResolvedValue({
        status: "idle",
        mcp_running: false,
        active_project: null,
        current_context_pack: null,
        recent_pack_count: 0,
        activity_event_count: 0,
        decision_count: 0,
        failed_writes: 0,
        last_error: null,
        latest_pack_summary: null,
        latest_pack_tokens_used: 0,
    }),
}));

function card(index: number): MemoryCard {
    return {
        id: `memory-${index}`,
        title: `Memory ${index}`,
        summary: `Worked through memory loading issue ${index}.`,
        action: "Reviewed memory loading",
        context: ["Continuum"],
        timestamp: Date.now() - index,
        app_name: "VS Code",
        window_title: `Memory ${index}`,
        score: 1,
        source_count: 1,
        raw_snippets: [`Worked through memory loading issue ${index}.`],
    };
}

afterEach(() => {
    cleanup();
    vi.clearAllMocks();
});

describe("MemoryCardsPanel", () => {
    it("requests the full all-app browse limit and renders returned cards", async () => {
        vi.mocked(listMemoryCards).mockResolvedValue(
            Array.from({ length: 1500 }, (_, index) => card(index))
        );

        render(
            <MemoryCardsPanel
                isVisible={true}
                onClose={() => {}}
                appNames={["VS Code"]}
                feature="vault"
            />
        );

        await waitFor(() => {
            expect(listMemoryCards).toHaveBeenCalledWith(1500, null);
        });
        expect(await screen.findByText("1500 cards")).toBeInTheDocument();
    });
});
