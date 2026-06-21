import { afterEach, describe, expect, it, vi } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ClusterAnswer } from "@/shared/ipc/cloud";
import { useClusterQuery } from "./useClusterQuery";

const { cloudQueryCluster } = vi.hoisted(() => ({
    cloudQueryCluster: vi.fn(),
}));

vi.mock("@/shared/ipc/cloud", () => ({
    cloudQueryCluster,
}));

const answer: ClusterAnswer = {
    answer: "The team moved to OTP [Luke@14:32].",
    citations: [
        {
            user: "Luke",
            concept: "magic-link OTP",
            app: "VS Code",
            topic: "auth",
            timestamp: "2026-06-20T14:32:00Z",
            node_id: "n1",
        },
    ],
    node_ids: ["n1"],
};

afterEach(() => {
    vi.clearAllMocks();
});

describe("useClusterQuery", () => {
    it("starts idle", () => {
        const { result } = renderHook(() => useClusterQuery());
        expect(result.current.state).toEqual({ status: "idle" });
    });

    it("ignores empty input", () => {
        const { result } = renderHook(() => useClusterQuery());
        act(() => result.current.ask("   "));
        expect(cloudQueryCluster).not.toHaveBeenCalled();
        expect(result.current.state.status).toBe("idle");
    });

    it("transitions asking → answer on success", async () => {
        cloudQueryCluster.mockResolvedValueOnce(answer);
        const { result } = renderHook(() => useClusterQuery());
        act(() => result.current.ask("what did the team decide?"));
        expect(result.current.state.status).toBe("asking");
        await waitFor(() =>
            expect(result.current.state.status).toBe("answer")
        );
        expect(result.current.state).toEqual({ status: "answer", answer });
        expect(cloudQueryCluster).toHaveBeenCalledWith(
            "what did the team decide?"
        );
    });

    it("surfaces a rejected string as an error message", async () => {
        cloudQueryCluster.mockRejectedValueOnce("You haven't joined a cluster yet.");
        const { result } = renderHook(() => useClusterQuery());
        act(() => result.current.ask("anything"));
        await waitFor(() => expect(result.current.state.status).toBe("error"));
        expect(result.current.state).toEqual({
            status: "error",
            message: "You haven't joined a cluster yet.",
        });
    });

    it("drops a stale in-flight result when reset", async () => {
        let resolve: (a: ClusterAnswer) => void = () => {};
        cloudQueryCluster.mockReturnValueOnce(
            new Promise<ClusterAnswer>((r) => {
                resolve = r;
            })
        );
        const { result } = renderHook(() => useClusterQuery());
        act(() => result.current.ask("slow question"));
        act(() => result.current.reset());
        expect(result.current.state).toEqual({ status: "idle" });
        await act(async () => {
            resolve(answer);
        });
        // The cancelled request must not overwrite the idle state.
        expect(result.current.state).toEqual({ status: "idle" });
    });
});
