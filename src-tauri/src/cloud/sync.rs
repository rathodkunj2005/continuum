//! Outbound sync — the core pipe from on-device capture to the team graph.
//!
//! For every flushed memory the desktop derives a [`Descriptor`], classifies it
//! (`BLOCKED` / `LOCAL_ONLY` / `SHARED_ANON`), applies the cluster/manager
//! policy gate, scrubs it, dedups it (L1), and queues it. A background worker
//! drains the queue by POSTing to the `agent-sync` Edge Function, which embeds
//! (OpenAI, server-side — so no model key lives on the desktop), inserts a
//! `semantic_nodes` row, and broadcasts it to the cluster.
//!
//! The queue is checkpointed to the local [`StateStore`] so observations
//! captured while offline survive a restart and flush on reconnect.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};

use crate::cloud::descriptor::Descriptor;
use crate::cloud::share_policy::{
    allows_graph_push, classify, scrub, ClassifyCtx, ClusterSharePolicy, ShareDecision,
};
use crate::cloud::{self, CloudConfig};
use crate::storage::StateStore;
use crate::AppState;

/// StateStore key under which the pending queue is checkpointed.
const BUFFER_KEY: &str = "cloud_sync_pending";
/// Most jobs to push per worker tick (bounds catch-up bursts after reconnect).
const MAX_PUSH_PER_TICK: usize = 16;
/// Refresh the cached identity/policy roughly every this many worker ticks.
const REFRESH_EVERY_TICKS: u64 = 15;

/// One queued observation awaiting push. `content_hash` is the dedupe key.
///
/// The `embedding` is computed on-device by Continuum's existing local model
/// (MiniLM, 384-d) and shipped with the descriptor so the cloud stores it
/// verbatim — no OpenAI call, no model key on the server. `embed_model` records
/// provenance so the backend can reject mixed embedding spaces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CloudSyncJob {
    pub descriptor: Descriptor,
    pub content_hash: String,
    pub enqueued_at_ms: i64,
    /// Local text embedding (e.g. 384-d MiniLM). Empty if the record had none.
    #[serde(default)]
    pub embedding: Vec<f32>,
    /// Identifier of the local embedding model that produced `embedding`.
    #[serde(default)]
    pub embed_model: String,
}

/// Cached cloud identity + policy, refreshed off the hot path by the worker and
/// read synchronously at enqueue time.
#[derive(Debug, Clone, Default)]
pub struct CloudRuntime {
    pub configured: bool,
    pub signed_in: bool,
    pub user_id: Option<String>,
    pub cluster_id: Option<String>,
    pub letta_agent_id: Option<String>,
    pub policy: ClusterSharePolicy,
    /// Local opt-in flag (only consulted when policy is `OptIn`).
    pub local_opt_in: bool,
}

impl CloudRuntime {
    /// Cloud is wired up enough to attempt sharing (configured + signed in +
    /// joined to a cluster).
    pub fn ready(&self) -> bool {
        self.configured && self.signed_in && self.cluster_id.is_some()
    }
}

/// FIFO + content-hash-deduped queue. Mirrors `MemoryReviewQueue`.
#[derive(Debug, Default)]
pub struct CloudSyncQueue {
    jobs: Mutex<VecDeque<CloudSyncJob>>,
}

impl CloudSyncQueue {
    pub fn new() -> Self {
        Self {
            jobs: Mutex::new(VecDeque::new()),
        }
    }

    /// Push `job`. Returns false if a pending job with the same `content_hash`
    /// already exists.
    pub fn enqueue(&self, job: CloudSyncJob) -> bool {
        let mut jobs = self.jobs.lock();
        if jobs.iter().any(|j| j.content_hash == job.content_hash) {
            return false;
        }
        jobs.push_back(job);
        true
    }

    pub fn dequeue(&self) -> Option<CloudSyncJob> {
        self.jobs.lock().pop_front()
    }

    pub fn len(&self) -> usize {
        self.jobs.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.jobs.lock().is_empty()
    }

    pub fn snapshot(&self) -> Vec<CloudSyncJob> {
        self.jobs.lock().iter().cloned().collect()
    }

    /// Replace the queue contents (used when restoring the offline buffer).
    pub fn restore(&self, jobs: Vec<CloudSyncJob>) {
        *self.jobs.lock() = VecDeque::from(jobs);
    }
}

/// Everything the sync pipeline keeps on [`AppState`]: the queue, the L1 dedup
/// window, the cached runtime, and counters for the status surface.
pub struct CloudSyncState {
    pub queue: CloudSyncQueue,
    pub dedup: cloud::dedup::RecentDedup,
    pub runtime: RwLock<CloudRuntime>,
    synced: AtomicU64,
    deduped: AtomicU64,
    blocked: AtomicU64,
    local_only: AtomicU64,
    withheld: AtomicU64,
    failed: AtomicU64,
    last_synced_at_ms: AtomicI64,
    last_error: Mutex<Option<String>>,
}

impl Default for CloudSyncState {
    fn default() -> Self {
        Self::new()
    }
}

impl CloudSyncState {
    pub fn new() -> Self {
        Self {
            queue: CloudSyncQueue::new(),
            dedup: cloud::dedup::RecentDedup::default(),
            runtime: RwLock::new(CloudRuntime::default()),
            synced: AtomicU64::new(0),
            deduped: AtomicU64::new(0),
            blocked: AtomicU64::new(0),
            local_only: AtomicU64::new(0),
            withheld: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            last_synced_at_ms: AtomicI64::new(0),
            last_error: Mutex::new(None),
        }
    }

    pub fn status(&self) -> CloudSyncStatus {
        let rt = self.runtime.read();
        CloudSyncStatus {
            configured: rt.configured,
            signed_in: rt.signed_in,
            cluster_id: rt.cluster_id.clone(),
            policy: rt.policy,
            queue_depth: self.queue.len(),
            synced: self.synced.load(Ordering::Relaxed),
            deduped: self.deduped.load(Ordering::Relaxed),
            blocked: self.blocked.load(Ordering::Relaxed),
            local_only: self.local_only.load(Ordering::Relaxed),
            withheld: self.withheld.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
            last_synced_at_ms: self.last_synced_at_ms.load(Ordering::Relaxed),
            last_error: self.last_error.lock().clone(),
        }
    }

    fn set_last_error(&self, err: Option<String>) {
        *self.last_error.lock() = err;
    }
}

/// Status surfaced to the UI / IPC.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudSyncStatus {
    pub configured: bool,
    pub signed_in: bool,
    pub cluster_id: Option<String>,
    pub policy: ClusterSharePolicy,
    pub queue_depth: usize,
    pub synced: u64,
    pub deduped: u64,
    pub blocked: u64,
    pub local_only: u64,
    pub withheld: u64,
    pub failed: u64,
    pub last_synced_at_ms: i64,
    pub last_error: Option<String>,
}

/// What [`decide`] did with one observation — for logging/telemetry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecideOutcome {
    Blocked,
    LocalOnly,
    /// Eligible but the cluster policy withheld it.
    Withheld,
    /// Already seen within the L1 window.
    Deduped,
    /// Newly enqueued for push.
    Enqueued,
}

/// Run the per-observation decision: classify → policy gate → scrub → L1 dedup
/// → enqueue, updating counters. Pure of any network/IO so it is unit-testable.
///
/// `embedding` is the record's locally-computed vector and `embed_model` its
/// provenance; both ride along to the cloud so it never re-embeds.
pub fn decide(
    st: &CloudSyncState,
    descriptor: &Descriptor,
    ctx: &ClassifyCtx,
    now_ms: i64,
    embedding: Vec<f32>,
    embed_model: &str,
) -> DecideOutcome {
    match classify(descriptor, ctx) {
        ShareDecision::Blocked => {
            st.blocked.fetch_add(1, Ordering::Relaxed);
            return DecideOutcome::Blocked;
        }
        ShareDecision::LocalOnly => {
            st.local_only.fetch_add(1, Ordering::Relaxed);
            return DecideOutcome::LocalOnly;
        }
        ShareDecision::SharedAnon => {}
    }

    let (policy, local_opt_in) = {
        let rt = st.runtime.read();
        (rt.policy, rt.local_opt_in)
    };
    if !allows_graph_push(policy, ShareDecision::SharedAnon, local_opt_in) {
        st.withheld.fetch_add(1, Ordering::Relaxed);
        return DecideOutcome::Withheld;
    }

    let scrubbed = scrub(descriptor);
    let content_hash = scrubbed.content_hash();
    if st.dedup.is_duplicate_at(&content_hash, now_ms) {
        st.deduped.fetch_add(1, Ordering::Relaxed);
        return DecideOutcome::Deduped;
    }

    st.queue.enqueue(CloudSyncJob {
        descriptor: scrubbed,
        content_hash,
        enqueued_at_ms: now_ms,
        embedding,
        embed_model: embed_model.to_string(),
    });
    DecideOutcome::Enqueued
}

/// Persist the queue snapshot to the local state store (offline durability).
pub fn persist(st: &CloudSyncState, store: &StateStore) {
    if let Err(e) = store.save_json(BUFFER_KEY, &st.queue.snapshot()) {
        tracing::debug!(target: "continuum::cloud_sync", "buffer persist failed: {e}");
    }
}

/// Restore the queue from the local state store on startup.
pub fn restore(st: &CloudSyncState, store: &StateStore) {
    match store.load_json::<Vec<CloudSyncJob>>(BUFFER_KEY) {
        Ok(Some(jobs)) if !jobs.is_empty() => {
            let n = jobs.len();
            st.queue.restore(jobs);
            tracing::info!(target: "continuum::cloud_sync", count = n, "restored offline sync buffer");
        }
        Ok(_) => {}
        Err(e) => tracing::debug!(target: "continuum::cloud_sync", "buffer restore failed: {e}"),
    }
}

/// Body posted to the `agent-sync` Edge Function on the authenticated (JWT)
/// path. The server derives `user_id` from the bearer token, so the desktop
/// ships no identifiers or shared secret. We DO ship a precomputed local
/// `embedding` (Continuum's on-device vector, projected to the cluster's
/// 1536-d space — see [`crate::cloud::embed`]) plus its provenance, so the
/// server stores it verbatim instead of calling OpenAI. The field is omitted
/// when empty so the server can fall back to its own embedding path.
pub fn build_agent_sync_body(cluster_id: &str, job: &CloudSyncJob) -> serde_json::Value {
    let descriptor = &job.descriptor;
    let mut body = serde_json::json!({
        "cluster_id": cluster_id,
        "descriptor": {
            "app": descriptor.app,
            "topic": descriptor.topic,
            "concept": descriptor.concept,
            "error_type": descriptor.error_type,
        },
    });
    if !job.embedding.is_empty() {
        body["embedding"] = serde_json::json!(job.embedding);
        body["embed_model"] = serde_json::json!(job.embed_model);
        body["embed_dim"] = serde_json::json!(job.embedding.len());
    }
    body
}

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("Continuum/1.0")
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("HTTP client init failed: {e}"))
}

/// POST one descriptor to `agent-sync`. Returns `Ok(())` on a 2xx (synchronized
/// or server-side deduplicated); `Err` on transient failure so the caller can
/// re-enqueue and retry.
async fn push_node(
    client: &reqwest::Client,
    cfg: &CloudConfig,
    access_token: &str,
    cluster_id: &str,
    job: &CloudSyncJob,
) -> Result<(), String> {
    let body = build_agent_sync_body(cluster_id, job);

    let res = client
        .post(cfg.function_url("agent-sync"))
        .header("apikey", &cfg.anon_key)
        .bearer_auth(access_token)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("agent-sync unreachable: {e}"))?;

    if res.status().is_success() {
        Ok(())
    } else {
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        Err(format!("agent-sync HTTP {status}: {text}"))
    }
}

/// Refresh the cached identity + cluster policy off the hot path.
async fn refresh_runtime(state: &AppState) {
    let Some(cfg) = CloudConfig::from_env() else {
        *state.cloud_sync.runtime.write() = CloudRuntime::default();
        return;
    };
    if cloud::session::current().is_none() {
        *state.cloud_sync.runtime.write() = CloudRuntime {
            configured: true,
            ..CloudRuntime::default()
        };
        return;
    }
    // Refresh the access token if needed, then resolve identity (best effort).
    let session = match cloud::ensure_fresh_session(&cfg).await {
        Ok(s) => s,
        Err(_) => {
            *state.cloud_sync.runtime.write() = CloudRuntime {
                configured: true,
                signed_in: false,
                ..CloudRuntime::default()
            };
            return;
        }
    };
    let identity = cloud::auth::resolve_identity(&cfg, &session).await.ok();
    let cluster_id = identity.as_ref().and_then(|i| i.cluster_id.clone());
    let letta_agent_id = identity.as_ref().and_then(|i| i.letta_agent_id.clone());

    // Policy precedence: env override (testing) → backend cluster column →
    // safe default (Disabled).
    let policy = match ClusterSharePolicy::from_env() {
        Some(p) => p,
        None => match &cluster_id {
            Some(cid) => resolve_cluster_policy(&cfg, &session, cid)
                .await
                .unwrap_or_default(),
            None => ClusterSharePolicy::default(),
        },
    };

    *state.cloud_sync.runtime.write() = CloudRuntime {
        configured: true,
        signed_in: true,
        user_id: Some(session.user_id.clone()),
        cluster_id,
        letta_agent_id,
        policy,
        // Running the signed-in desktop is treated as the local opt-in.
        local_opt_in: true,
    };
}

/// Best-effort read of a cluster-level `sharing_mode` from the backend. The
/// reference schema does not carry this column yet, so a missing/unreadable
/// value resolves to `None` (→ default Disabled). Forward-compatible: once the
/// DB owner adds `clusters.sharing_mode`, this picks it up with no code change.
async fn resolve_cluster_policy(
    cfg: &CloudConfig,
    session: &cloud::CloudSession,
    cluster_id: &str,
) -> Option<ClusterSharePolicy> {
    let client = http_client().ok()?;
    let res = client
        .get(cfg.rest_url(&format!("clusters?select=*&id=eq.{cluster_id}")))
        .header("apikey", &cfg.anon_key)
        .bearer_auth(&session.access_token)
        .send()
        .await
        .ok()?;
    if !res.status().is_success() {
        return None;
    }
    let rows: Vec<serde_json::Value> = res.json().await.ok()?;
    let mode = rows.first()?.get("sharing_mode").and_then(|v| v.as_str())?;
    ClusterSharePolicy::parse(mode)
}

/// Spawn the background sync worker: periodically refresh identity/policy and
/// drain the queue to `agent-sync`. No-ops cheaply when cloud is unconfigured.
pub fn spawn_worker(state: Arc<AppState>, interval: Duration) {
    // Use Tauri's async runtime: `spawn_worker` is invoked from `.setup()` on
    // the main thread, where a bare `tokio::spawn` has no reactor in scope.
    tauri::async_runtime::spawn(async move {
        restore(&state.cloud_sync, &state.state_store);
        let client = match http_client() {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(target: "continuum::cloud_sync", "worker disabled: {e}");
                return;
            }
        };
        let mut tick: u64 = 0;
        loop {
            if tick % REFRESH_EVERY_TICKS == 0 {
                refresh_runtime(&state).await;
            }
            tick = tick.wrapping_add(1);
            drain_once(&state, &client).await;
            tokio::time::sleep(interval).await;
        }
    });
}

/// Drain up to [`MAX_PUSH_PER_TICK`] queued jobs. Stops early and re-enqueues on
/// the first transient failure so an offline burst is retried next tick.
async fn drain_once(state: &AppState, client: &reqwest::Client) {
    let st = &state.cloud_sync;
    if st.queue.is_empty() {
        return;
    }
    let rt = st.runtime.read().clone();
    if !rt.ready() {
        return;
    }
    let Some(cfg) = CloudConfig::from_env() else {
        return;
    };
    let Some(cluster_id) = rt.cluster_id.clone() else {
        return;
    };
    // Authenticated push: derive identity server-side from a fresh JWT, so the
    // desktop ships no shared secret or client-supplied user id.
    let session = match cloud::ensure_fresh_session(&cfg).await {
        Ok(s) => s,
        Err(e) => {
            st.set_last_error(Some(format!("auth refresh failed: {e}")));
            return;
        }
    };

    let mut pushed = 0usize;
    let mut dirty = false;
    while pushed < MAX_PUSH_PER_TICK {
        let Some(job) = st.queue.dequeue() else {
            break;
        };
        match push_node(client, &cfg, &session.access_token, &cluster_id, &job).await {
            Ok(()) => {
                st.synced.fetch_add(1, Ordering::Relaxed);
                st.last_synced_at_ms
                    .store(chrono::Utc::now().timestamp_millis(), Ordering::Relaxed);
                st.set_last_error(None);
                pushed += 1;
                dirty = true;
            }
            Err(e) => {
                // Transient: put it back and stop draining this tick.
                st.failed.fetch_add(1, Ordering::Relaxed);
                st.set_last_error(Some(e.clone()));
                st.queue.enqueue(job);
                tracing::debug!(target: "continuum::cloud_sync", "push failed, will retry: {e}");
                dirty = true;
                break;
            }
        }
    }
    if dirty {
        persist(st, &state.state_store);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_MODEL: &str = "all-MiniLM-L6-v2";

    fn test_embedding() -> Vec<f32> {
        vec![0.1, 0.2, 0.3, 0.4]
    }

    fn job(hash: &str) -> CloudSyncJob {
        CloudSyncJob {
            descriptor: Descriptor {
                app: "VS Code".to_string(),
                topic: "rust".to_string(),
                concept: "editing".to_string(),
                error_type: None,
            },
            content_hash: hash.to_string(),
            enqueued_at_ms: 0,
            embedding: test_embedding(),
            embed_model: TEST_MODEL.to_string(),
        }
    }

    #[test]
    fn queue_dedupes_by_content_hash() {
        let q = CloudSyncQueue::new();
        assert!(q.enqueue(job("a")));
        assert!(q.enqueue(job("b")));
        assert!(!q.enqueue(job("a")));
        assert_eq!(q.len(), 2);
    }

    #[test]
    fn queue_is_fifo_and_requeue_works_after_dequeue() {
        let q = CloudSyncQueue::new();
        q.enqueue(job("a"));
        q.enqueue(job("b"));
        assert_eq!(q.dequeue().unwrap().content_hash, "a");
        // After dequeue the dedupe block lifts (worker re-enqueues on failure).
        assert!(q.enqueue(job("a")));
        assert_eq!(q.len(), 2);
    }

    #[test]
    fn snapshot_and_restore_roundtrip() {
        let q = CloudSyncQueue::new();
        q.enqueue(job("a"));
        q.enqueue(job("b"));
        let snap = q.snapshot();
        let q2 = CloudSyncQueue::new();
        q2.restore(snap);
        assert_eq!(q2.len(), 2);
        assert_eq!(q2.dequeue().unwrap().content_hash, "a");
    }

    fn ctx<'a>(app: &'a str, private_mode: bool) -> ClassifyCtx<'a> {
        ClassifyCtx {
            bundle_id: None,
            app_name: Some(app),
            url: None,
            window_title: None,
            private_mode,
            user_blocklist: &[],
        }
    }

    fn shareable() -> Descriptor {
        Descriptor {
            app: "VS Code".to_string(),
            topic: "rust pipeline".to_string(),
            concept: "editing the capture loop".to_string(),
            error_type: None,
        }
    }

    #[test]
    fn decide_enqueues_when_policy_members() {
        let st = CloudSyncState::new();
        st.runtime.write().policy = ClusterSharePolicy::Members;
        let d = shareable();
        assert_eq!(
            decide(&st, &d, &ctx("VS Code", false), 0, test_embedding(), TEST_MODEL),
            DecideOutcome::Enqueued
        );
        assert_eq!(st.queue.len(), 1);
        // The local embedding rides along on the queued job.
        let queued = st.queue.dequeue().unwrap();
        assert_eq!(queued.embedding, test_embedding());
        assert_eq!(queued.embed_model, TEST_MODEL);
    }

    #[test]
    fn decide_withholds_when_policy_disabled() {
        let st = CloudSyncState::new(); // default policy = Disabled
        let d = shareable();
        assert_eq!(
            decide(&st, &d, &ctx("VS Code", false), 0, test_embedding(), TEST_MODEL),
            DecideOutcome::Withheld
        );
        assert_eq!(st.queue.len(), 0);
        assert_eq!(st.status().withheld, 1);
    }

    #[test]
    fn decide_dedups_repeats_within_window() {
        let st = CloudSyncState::new();
        st.runtime.write().policy = ClusterSharePolicy::Members;
        let d = shareable();
        assert_eq!(
            decide(&st, &d, &ctx("VS Code", false), 0, test_embedding(), TEST_MODEL),
            DecideOutcome::Enqueued
        );
        assert_eq!(
            decide(&st, &d, &ctx("VS Code", false), 1000, test_embedding(), TEST_MODEL),
            DecideOutcome::Deduped
        );
        assert_eq!(st.queue.len(), 1);
        assert_eq!(st.status().deduped, 1);
    }

    #[test]
    fn decide_local_only_in_private_mode() {
        let st = CloudSyncState::new();
        st.runtime.write().policy = ClusterSharePolicy::Members;
        let d = shareable();
        assert_eq!(
            decide(&st, &d, &ctx("VS Code", true), 0, test_embedding(), TEST_MODEL),
            DecideOutcome::LocalOnly
        );
        assert_eq!(st.queue.len(), 0);
    }

    #[test]
    fn build_body_jwt_shape_with_local_embedding() {
        let body = build_agent_sync_body("c1", &job("h"));
        assert_eq!(body["cluster_id"], "c1");
        assert_eq!(body["descriptor"]["app"], "VS Code");
        assert_eq!(body["descriptor"]["error_type"], serde_json::Value::Null);
        // JWT path: identity comes from the bearer token, so no user id/secret.
        assert!(body.get("user_id").is_none());
        assert!(body.get("agent_id").is_none());
        // But the precomputed local embedding IS shipped (no server OpenAI call).
        assert_eq!(body["embed_model"], TEST_MODEL);
        assert_eq!(body["embed_dim"], 4);
        assert!((body["embedding"][0].as_f64().unwrap() - 0.1).abs() < 1e-6);
    }

    #[test]
    fn build_body_omits_empty_embedding() {
        let mut j = job("h");
        j.embedding = vec![];
        let body = build_agent_sync_body("c1", &j);
        assert!(body.get("embedding").is_none());
        assert!(body.get("embed_dim").is_none());
    }
}
