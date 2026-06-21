import { invoke } from "@tauri-apps/api/core";

// Cloud sign-in (Supabase email OTP). The session is owned by the Rust backend
// and stored in the OS keychain — the webview never holds the tokens.

export interface CloudStatus {
    /** Backend configured in this build (SUPABASE_URL + anon key present). */
    configured: boolean;
    /** A valid session is stored. */
    signed_in: boolean;
    email: string | null;
    user_id: string | null;
}

export interface CloudIdentity {
    user_id: string;
    email: string | null;
    /** Cluster the user's shared observations sync into (null until they join one). */
    cluster_id: string | null;
    letta_agent_id: string | null;
}

export async function cloudStatus(): Promise<CloudStatus> {
    return invoke<CloudStatus>("cloud_status");
}

/** Email a one-time sign-in code. */
export async function cloudRequestOtp(email: string): Promise<void> {
    return invoke("cloud_request_otp", { email });
}

/** Verify the emailed code; on success the session is persisted server-side. */
export async function cloudVerifyOtp(email: string, code: string): Promise<CloudStatus> {
    return invoke<CloudStatus>("cloud_verify_otp", { email, code });
}

export async function cloudSignOut(): Promise<void> {
    return invoke("cloud_sign_out");
}

export async function cloudGetIdentity(): Promise<CloudIdentity> {
    return invoke<CloudIdentity>("cloud_get_identity");
}

/** Cluster sharing policy set by the manager (serialized snake_case from Rust). */
export type ClusterSharePolicy = "disabled" | "members" | "opt_in";

/** Snapshot of the outbound team-graph sync pipeline. */
export interface CloudSyncStatus {
    configured: boolean;
    signed_in: boolean;
    /** Cluster the user's shared observations sync into (null until joined). */
    cluster_id: string | null;
    /** Manager-controlled sharing policy; `disabled` withholds all sharing. */
    policy: ClusterSharePolicy;
    /** Observations queued for push (offline buffer depth). */
    queue_depth: number;
    /** Counters since launch. */
    synced: number;
    deduped: number;
    blocked: number;
    local_only: number;
    /** SHARED_ANON but withheld by the cluster policy. */
    withheld: number;
    failed: number;
    /** Unix ms of the last successful push, or 0. */
    last_synced_at_ms: number;
    last_error: string | null;
}

/** A teammate node that informed a cluster answer. */
export interface Citation {
    user: string;
    concept: string;
    app: string;
    topic: string;
    /** ISO-8601 capture time, if present. */
    timestamp: string | null;
    node_id: string;
}

/** A grounded, citation-aware answer from the cluster. */
export interface ClusterAnswer {
    answer: string;
    citations: Citation[];
    node_ids: string[];
}

/** Status of the outbound team-graph sync pipeline. Cheap; safe to poll. */
export async function cloudSyncStatus(): Promise<CloudSyncStatus> {
    return invoke<CloudSyncStatus>("cloud_sync_status");
}

/**
 * Ask the user's cluster a question via the `query-synthesize` Edge Function.
 * Returns a grounded answer plus the teammate nodes it cited.
 */
export async function cloudQueryCluster(query: string): Promise<ClusterAnswer> {
    return invoke<ClusterAnswer>("cloud_query_cluster", { query });
}
