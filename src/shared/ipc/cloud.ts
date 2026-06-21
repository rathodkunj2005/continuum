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

/**
 * Sign in by pasting a Supabase magic link (or its token) — for projects whose
 * email template sends a link instead of a 6-digit code. The session is
 * persisted in the OS keychain on success.
 */
export async function cloudVerifyMagicLink(link: string): Promise<CloudStatus> {
    return invoke<CloudStatus>("cloud_verify_magic_link", { link });
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

/** Result of a manual / daily "sync now" run. */
export interface ManualSyncReport {
    considered: number;
    pushed: number;
    skipped_blocked: number;
    skipped_local_only: number;
    skipped_empty: number;
    skipped_duplicate: number;
    failed: number;
}

/** Status of the outbound team-graph sync pipeline. Cheap; safe to poll. */
export async function cloudSyncStatus(): Promise<CloudSyncStatus> {
    return invoke<CloudSyncStatus>("cloud_sync_status");
}

/**
 * Manually push recent local memories (last 7 days) to the team graph now.
 * Explicit user action: bypasses the cluster policy gate but keeps the safety
 * floor (BLOCKED / LOCAL_ONLY content never leaves the device).
 */
export async function cloudSyncNow(): Promise<ManualSyncReport> {
    return invoke<ManualSyncReport>("cloud_sync_now");
}

/**
 * Ask the user's cluster a question via the `query-synthesize` Edge Function.
 * Returns a grounded answer plus the teammate nodes it cited.
 */
export async function cloudQueryCluster(query: string): Promise<ClusterAnswer> {
    return invoke<ClusterAnswer>("cloud_query_cluster", { query });
}

/** A workspace the user belongs to. `join_code` is only present after creating. */
export interface ClusterMembership {
    cluster_id: string;
    name: string;
    role: string;
    join_code: string | null;
}

/** Create a workspace; the caller becomes its admin and gets a shareable code. */
export async function cloudCreateCluster(name: string): Promise<ClusterMembership> {
    return invoke<ClusterMembership>("cloud_create_cluster", { name });
}

/** Join a workspace with a code shared by its admin. */
export async function cloudJoinCluster(joinCode: string): Promise<ClusterMembership> {
    return invoke<ClusterMembership>("cloud_join_cluster", { joinCode });
}
