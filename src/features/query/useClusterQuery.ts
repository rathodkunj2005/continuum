import { useCallback, useRef, useState } from "react";
import { type ClusterAnswer, cloudQueryCluster } from "@/shared/ipc/cloud";

// Cluster Q&A state for the omnibar's "Cluster" surface. The desktop asks the
// shared team graph (`query-synthesize` Edge Function via the `cloud_query_cluster`
// command); the answer carries inline [Name@HH:MM] citations plus the teammate
// nodes it cited. This is the cloud twin of the on-device `continuumAnswer` ask.

export type ClusterQueryState =
    | { status: "idle" }
    | { status: "asking" }
    | { status: "answer"; answer: ClusterAnswer }
    | { status: "error"; message: string };

export interface UseClusterQuery {
    state: ClusterQueryState;
    /** Ask the cluster. No-op on empty input; cancels any in-flight ask. */
    ask: (query: string) => void;
    /** Return to the idle (input) state, dropping any answer or error. */
    reset: () => void;
}

export function useClusterQuery(): UseClusterQuery {
    const [state, setState] = useState<ClusterQueryState>({ status: "idle" });
    // Monotonic guard so a stale/cancelled request can't overwrite a newer one.
    const seq = useRef(0);

    const ask = useCallback((query: string) => {
        const trimmed = query.trim();
        if (!trimmed) {
            return;
        }
        const current = ++seq.current;
        setState({ status: "asking" });
        cloudQueryCluster(trimmed)
            .then((answer) => {
                if (seq.current !== current) {
                    return;
                }
                setState({ status: "answer", answer });
            })
            .catch((err) => {
                if (seq.current !== current) {
                    return;
                }
                // The Rust command rejects with a user-facing string.
                const message =
                    typeof err === "string"
                        ? err
                        : "Couldn't reach the cluster.";
                setState({ status: "error", message });
            });
    }, []);

    const reset = useCallback(() => {
        seq.current += 1;
        setState({ status: "idle" });
    }, []);

    return { state, ask, reset };
}
