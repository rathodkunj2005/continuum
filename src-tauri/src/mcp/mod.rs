//! MCP server for FNDR — local-first with secure tunnel/public deployment modes.
//!
//! Features:
//!  - Deployment modes: local (default), tunnel, public
//!  - Binds to `127.0.0.1:0` by default; public mode can bind non-loopback
//!  - Writes `~/.fndr/mcp.json` for client discovery
//!  - Bearer-token authentication (required by default outside local mode)
//!  - CORS layer permissive for local editor / tool connections
//!  - Supports legacy SSE and streamable-HTTP style GET/POST on `/mcp`
//!  - `spawn_blocking` for SQLite + embedding calls
//!  - 30-second timeout on LLM inference

pub mod tls;
pub mod token;

use crate::context_runtime::{self, CodeContextRequest, ContextRequest, DecisionProposal};
use crate::embedding::Embedder;
use crate::meeting;
use crate::search::HybridSearcher;
use crate::AppState;
use axum::{
    extract::{ConnectInfo, OriginalUri, State},
    http::{header, HeaderMap, StatusCode, Uri},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use chrono::TimeZone;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::ReceiverStream;
use tower_http::cors::{Any, CorsLayer};

// ---------------------------------------------------------------------------
// Public status type (returned to Tauri frontend)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerStatus {
    pub running: bool,
    pub mode: String,
    pub host: String,
    pub port: u16,
    pub endpoint: String,
    pub public_endpoint: Option<String>,
    pub public_sse_endpoint: Option<String>,
    pub token: String,
    pub use_tls: bool,
    pub require_auth: bool,
    pub auth_mode: String,
    pub last_error: Option<String>,
}

// ---------------------------------------------------------------------------
// Internal runtime state
// ---------------------------------------------------------------------------

struct McpRuntime {
    running: bool,
    mode: McpDeploymentMode,
    host: String,
    port: u16,
    endpoint: String,
    public_endpoint: Option<String>,
    public_sse_endpoint: Option<String>,
    token: String,
    use_tls: bool,
    require_auth: bool,
    shutdown: Option<oneshot::Sender<()>>,
    server_handle: Option<axum_server::Handle>,
    task: Option<JoinHandle<()>>,
    last_error: Option<String>,
}

impl Default for McpRuntime {
    fn default() -> Self {
        Self {
            running: false,
            mode: McpDeploymentMode::Local,
            host: LOOPBACK_HOST.to_string(),
            port: 0,
            endpoint: String::new(),
            public_endpoint: None,
            public_sse_endpoint: None,
            token: String::new(),
            use_tls: false,
            require_auth: false,
            shutdown: None,
            server_handle: None,
            task: None,
            last_error: None,
        }
    }
}

#[derive(Clone)]
struct HttpState {
    app_state: Arc<AppState>,
    token: String,
    mode: McpDeploymentMode,
    require_auth: bool,
    allow_loopback_auth_bypass: bool,
    allowed_origins: Vec<String>,
    public_endpoint: Option<String>,
    public_sse_endpoint: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum McpDeploymentMode {
    Local,
    Tunnel,
    Public,
}

impl McpDeploymentMode {
    fn as_str(self) -> &'static str {
        match self {
            McpDeploymentMode::Local => "local",
            McpDeploymentMode::Tunnel => "tunnel",
            McpDeploymentMode::Public => "public",
        }
    }

    fn local_only(self) -> bool {
        matches!(self, McpDeploymentMode::Local)
    }

    fn allows_non_loopback_bind(self) -> bool {
        matches!(self, McpDeploymentMode::Public)
    }

    fn default_require_auth(self) -> bool {
        !matches!(self, McpDeploymentMode::Local)
    }

    fn default_loopback_auth_bypass(self) -> bool {
        matches!(self, McpDeploymentMode::Local)
    }
}

// ---------------------------------------------------------------------------
// JSON-RPC types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Option<Value>,
    #[serde(default)]
    jsonrpc: Option<String>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

#[derive(Debug, Deserialize)]
struct ToolCallParams {
    name: String,
    #[serde(default)]
    arguments: Value,
}

#[derive(Debug, Deserialize)]
struct SearchMemoriesArgs {
    query: String,
    #[serde(default)]
    time_filter: Option<String>,
    #[serde(default)]
    app_filter: Option<String>,
    #[serde(default = "default_search_limit")]
    limit: usize,
}

#[derive(Debug, Deserialize)]
struct AskFndrArgs {
    query: String,
}

#[derive(Debug, Deserialize)]
struct StartMeetingArgs {
    title: String,
    #[serde(default)]
    participants: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct GetMeetingTranscriptArgs {
    meeting_id: String,
}

#[derive(Debug, Deserialize)]
struct SearchMeetingTranscriptsArgs {
    query: String,
    #[serde(default = "default_search_limit")]
    limit: usize,
}

#[derive(Debug, Deserialize)]
struct GetAmbientContextArgs {
    #[serde(default = "default_ambient_limit")]
    limit: usize,
}

#[derive(Debug, Deserialize)]
struct SearchFullContextArgs {
    query: String,
    #[serde(default)]
    time_window: Option<Value>,
    #[serde(default = "default_full_context_limit")]
    limit: usize,
    #[serde(default)]
    include_raw: bool,
}

#[derive(Debug, Deserialize)]
struct GetContextPackArgs {
    topic: String,
    #[serde(default)]
    time_window: Option<Value>,
    #[serde(default = "default_context_pack_depth")]
    depth: String,
}

#[derive(Debug, Deserialize)]
struct AgentBriefArgs {
    topic: String,
    #[serde(default = "default_agent_brief_budget")]
    token_budget: u32,
    #[serde(default)]
    include_raw_evidence: bool,
}

#[derive(Debug, Deserialize)]
struct TimelineArgs {
    #[serde(default)]
    from: Option<Value>,
    #[serde(default)]
    to: Option<Value>,
    #[serde(default = "default_timeline_granularity")]
    granularity: String,
}

#[derive(Debug, Deserialize, Default)]
struct ActiveFocusArgs {
    #[serde(default)]
    lookback_minutes: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct SearchRawArgs {
    query: String,
    #[serde(default)]
    time_window: Option<Value>,
    #[serde(default = "default_full_context_limit")]
    limit: usize,
}

#[derive(Debug, Deserialize)]
struct ProjectsArgs {
    #[serde(default = "default_projects_limit")]
    limit: usize,
}

#[derive(Debug, Deserialize)]
struct ProjectContextArgs {
    project: String,
    #[serde(default)]
    time_window: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct DecisionsArgs {
    #[serde(default)]
    project: Option<String>,
    #[serde(default = "default_full_context_limit")]
    limit: usize,
}

#[derive(Debug, Deserialize)]
struct ErrorsArgs {
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    time_window: Option<Value>,
    #[serde(default = "default_full_context_limit")]
    limit: usize,
}

#[derive(Debug, Deserialize)]
struct BlockersArgs {
    #[serde(default)]
    project: Option<String>,
    #[serde(default = "default_full_context_limit")]
    limit: usize,
}

#[derive(Debug, Deserialize)]
struct TodosArgs {
    #[serde(default)]
    project: Option<String>,
    #[serde(default = "default_full_context_limit")]
    limit: usize,
}

#[derive(Debug, Deserialize)]
struct GraphQueryArgs {
    query: String,
    #[serde(default = "default_full_context_limit")]
    limit: usize,
}

fn default_graph_context_depth() -> u32 {
    2
}

#[derive(Debug, Deserialize)]
struct GraphContextArgs {
    /// Filter project nodes / wiki stub; optional.
    #[serde(default)]
    project: Option<String>,
    /// When set, include a BFS neighborhood around this insight-graph node UUID.
    #[serde(default)]
    start_node_id: Option<String>,
    #[serde(default = "default_graph_context_depth")]
    depth: u32,
}

#[derive(Debug, Deserialize)]
struct RecentChangesArgs {
    #[serde(default = "default_recent_changes_lookback_minutes")]
    lookback_minutes: u32,
    #[serde(default = "default_full_context_limit")]
    limit: usize,
}

#[derive(Debug, Deserialize, Default)]
struct FndrDiffArgs {
    session_id: String,
    #[serde(default)]
    since_timestamp: Option<i64>,
}

#[derive(Debug, Deserialize, Default)]
struct WarmStartArgs {
    #[serde(default)]
    client_name: Option<String>,
    #[serde(default)]
    current_task: Option<String>,
    #[serde(default = "default_agent_brief_budget")]
    token_budget: u32,
    #[serde(default = "default_true")]
    include_recent_activity: bool,
    #[serde(default = "default_true")]
    include_project_context: bool,
    #[serde(default = "default_true")]
    include_decisions: bool,
    #[serde(default = "default_true")]
    include_open_tasks: bool,
}

#[derive(Debug, Deserialize, Default)]
struct AgentOnboardingArgs {
    #[serde(default = "default_agent_brief_budget")]
    token_budget: u32,
}

#[derive(Debug, Deserialize, Default)]
struct ProjectWikiArgs {
    #[serde(default)]
    project: Option<String>,
    #[serde(default = "default_full_context_limit")]
    limit: usize,
}

#[derive(Debug, Deserialize, Default)]
struct ClaimsArgs {
    #[serde(default)]
    project: Option<String>,
    #[serde(default = "default_full_context_limit")]
    limit: usize,
}

#[derive(Debug, Deserialize, Default)]
struct BreakthroughArgs {
    #[serde(default)]
    project: Option<String>,
    #[serde(default = "default_full_context_limit")]
    limit: usize,
}

#[derive(Debug, Deserialize, Default)]
struct SourceEvidenceArgs {
    #[serde(default)]
    page_id: Option<String>,
    #[serde(default)]
    memory_id: Option<String>,
    #[serde(default = "default_full_context_limit")]
    limit: usize,
    #[serde(default)]
    include_raw: bool,
}

fn default_ambient_limit() -> usize {
    5
}

fn default_search_limit() -> usize {
    10
}

fn default_full_context_limit() -> usize {
    12
}

fn default_agent_brief_budget() -> u32 {
    1800
}

fn default_context_pack_depth() -> String {
    "standard".to_string()
}

fn default_timeline_granularity() -> String {
    "session".to_string()
}

fn default_projects_limit() -> usize {
    20
}

fn default_recent_changes_lookback_minutes() -> u32 {
    180
}

fn default_true() -> bool {
    true
}

// ---------------------------------------------------------------------------
// Global singleton runtime
// ---------------------------------------------------------------------------

static MCP_RUNTIME: OnceLock<Mutex<McpRuntime>> = OnceLock::new();
const LOOPBACK_HOST: &str = "127.0.0.1";

fn runtime() -> &'static Mutex<McpRuntime> {
    MCP_RUNTIME.get_or_init(|| Mutex::new(McpRuntime::default()))
}

fn to_status(rt: &McpRuntime) -> McpServerStatus {
    McpServerStatus {
        running: rt.running,
        mode: rt.mode.as_str().to_string(),
        host: rt.host.clone(),
        port: rt.port,
        endpoint: rt.endpoint.clone(),
        public_endpoint: rt.public_endpoint.clone(),
        public_sse_endpoint: rt.public_sse_endpoint.clone(),
        token: rt.token.clone(),
        use_tls: rt.use_tls,
        require_auth: rt.require_auth,
        auth_mode: auth_mode_label(rt.require_auth),
        last_error: rt.last_error.clone(),
    }
}

// ---------------------------------------------------------------------------
// Discovery file
// ---------------------------------------------------------------------------

fn discovery_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".fndr")
        .join("mcp.json")
}

fn write_discovery(
    mode: McpDeploymentMode,
    host: &str,
    port: u16,
    token: &str,
    use_tls: bool,
    require_auth: bool,
    public_endpoint: Option<&str>,
    public_sse_endpoint: Option<&str>,
) {
    let path = discovery_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let scheme = if use_tls { "https" } else { "http" };
    let endpoint = format!("{}://{}:{}/mcp", scheme, host, port);
    let cert_pem = if use_tls { tls::get_cert_pem() } else { None };
    let streamable_endpoint = endpoint.clone();
    let payload = json!({
        "host": host,
        "bind_host": host,
        "port": port,
        "token": token,
        "endpoint": endpoint,
        "sse_endpoint": format!("{}://{}:{}/mcp/sse", scheme, host, port),
        "tls": use_tls,
        "cert_pem": cert_pem,
        "auth_required": require_auth,
        "auth_mode": auth_mode_label(require_auth),
        "mode": mode.as_str(),
        "local_only": mode.local_only(),
        "streamable_http_endpoint": streamable_endpoint,
        "public_endpoint": public_endpoint,
        "public_sse_endpoint": public_sse_endpoint
    });
    match std::fs::write(
        &path,
        serde_json::to_string_pretty(&payload).unwrap_or_default(),
    ) {
        Ok(_) => tracing::info!("MCP discovery file written to {:?}", path),
        Err(e) => tracing::warn!("Failed to write MCP discovery file: {}", e),
    }
}

fn remove_discovery() {
    let _ = std::fs::remove_file(discovery_path());
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn status() -> McpServerStatus {
    let mut rt = runtime().lock();

    if rt.running {
        if let Some(task) = rt.task.as_ref() {
            if task.is_finished() {
                rt.running = false;
                rt.shutdown = None;
                rt.task = None;
                if rt.last_error.is_none() {
                    rt.last_error = Some("MCP server exited unexpectedly".to_string());
                }
            }
        }
    }

    to_status(&rt)
}

pub async fn start(
    app_state: Arc<AppState>,
    host: Option<String>,
    port: Option<u16>,
) -> Result<McpServerStatus, String> {
    let mode = mcp_mode();
    let requested_host = host.unwrap_or_else(|| LOOPBACK_HOST.to_string());
    if !mode.allows_non_loopback_bind() && !is_loopback_host(&requested_host) {
        return Err(format!(
            "FNDR MCP mode '{}' only supports localhost transport. Refusing to bind to {requested_host}.",
            mode.as_str()
        ));
    }
    let host = if mode.allows_non_loopback_bind() {
        requested_host
    } else {
        LOOPBACK_HOST.to_string()
    };
    let port = port.unwrap_or(0);
    let require_auth = mcp_require_auth(mode);
    let allow_loopback_auth_bypass = mcp_allow_loopback_auth_bypass(mode);
    let allowed_origins = mcp_allowed_origins();

    {
        let rt = runtime().lock();
        if rt.running {
            return Ok(to_status(&rt));
        }
    }

    let use_tls = mcp_use_tls();

    // Load (or generate) the bearer token
    let tok = token::load_or_create();

    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .map_err(|e| format!("Invalid MCP bind address: {e}"))?;

    // axum-server::bind doesn't expose local_addr() before serving,
    // so we probe first, drop the socket, and immediately re-bind.
    let actual_addr = if port == 0 {
        let probe = std::net::TcpListener::bind(&addr)
            .map_err(|e| format!("Failed to probe for free port: {e}"))?;
        let resolved = probe
            .local_addr()
            .map_err(|e| format!("Failed to get local address: {e}"))?;
        drop(probe);
        resolved
    } else {
        addr
    };
    let actual_port = actual_addr.port();
    let scheme = if use_tls { "https" } else { "http" };
    let endpoint = format!("{scheme}://{}:{}/mcp", host, actual_port);
    let public_endpoint = mcp_public_endpoint();
    let public_sse_endpoint = public_endpoint
        .as_deref()
        .map(|value| with_path(value, "/mcp/sse"));

    tracing::info!(
        mode = %mode.as_str(),
        bind_host = %host,
        port = actual_port,
        use_tls,
        require_auth,
        loopback_auth_bypass = allow_loopback_auth_bypass,
        "Starting FNDR MCP server"
    );

    write_discovery(
        mode,
        &host,
        actual_port,
        &tok,
        use_tls,
        require_auth,
        public_endpoint.as_deref(),
        public_sse_endpoint.as_deref(),
    );

    let server_state = Arc::new(HttpState {
        app_state,
        token: tok.clone(),
        mode,
        require_auth,
        allow_loopback_auth_bypass,
        allowed_origins,
        public_endpoint: public_endpoint.clone(),
        public_sse_endpoint: public_sse_endpoint.clone(),
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let router = Router::new()
        .route("/", get(root_handler))
        .route("/mcp", get(mcp_stream_handler).post(mcp_handler))
        .route("/mcp/sse", get(sse_handler))
        .route("/mcp/messages", post(mcp_handler))
        .with_state(server_state)
        .layer(cors);

    let (shutdown_tx, _shutdown_rx) = oneshot::channel();
    let handle = axum_server::Handle::new();
    let server_handle = handle.clone();

    let task = if use_tls {
        let tls_config = tls::load_or_create_rustls_config().await?;
        tokio::spawn(async move {
            if let Err(err) = axum_server::bind_rustls(actual_addr, tls_config)
                .handle(server_handle)
                .serve(router.into_make_service_with_connect_info::<SocketAddr>())
                .await
            {
                tracing::error!("MCP HTTPS server error: {}", err);
            }
        })
    } else {
        tokio::spawn(async move {
            if let Err(err) = axum_server::bind(actual_addr)
                .handle(server_handle)
                .serve(router.into_make_service_with_connect_info::<SocketAddr>())
                .await
            {
                tracing::error!("MCP HTTP server error: {}", err);
            }
        })
    };

    let mut rt = runtime().lock();
    rt.running = true;
    rt.mode = mode;
    rt.host = host;
    rt.port = actual_port;
    rt.endpoint = endpoint;
    rt.public_endpoint = public_endpoint;
    rt.public_sse_endpoint = public_sse_endpoint;
    rt.token = tok;
    rt.use_tls = use_tls;
    rt.require_auth = require_auth;
    rt.shutdown = Some(shutdown_tx);
    rt.server_handle = Some(handle);
    rt.task = Some(task);
    rt.last_error = None;
    Ok(to_status(&rt))
}

pub async fn stop() -> McpServerStatus {
    let (shutdown, server_handle, task) = {
        let mut rt = runtime().lock();
        rt.running = false;
        (rt.shutdown.take(), rt.server_handle.take(), rt.task.take())
    };

    if let Some(h) = server_handle {
        h.shutdown();
    }
    if let Some(tx) = shutdown {
        let _ = tx.send(());
    }
    if let Some(task) = task {
        let _ = task.await;
    }

    remove_discovery();
    status()
}

// ---------------------------------------------------------------------------
// Authentication helper
// ---------------------------------------------------------------------------

fn check_auth(headers: &HeaderMap, expected_token: &str) -> bool {
    let auth_header = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    auth_header
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|t| t == expected_token)
        .unwrap_or(false)
}

fn mcp_mode() -> McpDeploymentMode {
    let value = std::env::var("FNDR_MCP_MODE")
        .unwrap_or_else(|_| "local".to_string())
        .trim()
        .to_ascii_lowercase();
    match value.as_str() {
        "local" => McpDeploymentMode::Local,
        "tunnel" => McpDeploymentMode::Tunnel,
        "public" => McpDeploymentMode::Public,
        _ => {
            tracing::warn!("Invalid FNDR_MCP_MODE='{value}', falling back to 'local'");
            McpDeploymentMode::Local
        }
    }
}

fn mcp_require_auth(mode: McpDeploymentMode) -> bool {
    std::env::var("FNDR_MCP_REQUIRE_AUTH")
        .ok()
        .and_then(|value| parse_bool_env(&value))
        .unwrap_or_else(|| mode.default_require_auth())
}

fn mcp_allow_loopback_auth_bypass(mode: McpDeploymentMode) -> bool {
    std::env::var("FNDR_MCP_ALLOW_LOOPBACK_AUTH_BYPASS")
        .ok()
        .and_then(|value| parse_bool_env(&value))
        .unwrap_or_else(|| mode.default_loopback_auth_bypass())
}

fn mcp_use_tls() -> bool {
    std::env::var("FNDR_MCP_ENABLE_TLS")
        .ok()
        .and_then(|value| parse_bool_env(&value))
        .unwrap_or(false)
}

fn mcp_allowed_origins() -> Vec<String> {
    std::env::var("FNDR_MCP_ALLOWED_ORIGINS")
        .ok()
        .map(|value| {
            value
                .split(',')
                .filter_map(normalize_origin)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn mcp_public_endpoint() -> Option<String> {
    std::env::var("FNDR_MCP_PUBLIC_BASE_URL")
        .ok()
        .and_then(|value| normalize_base_url(&value))
        .map(|base| with_path(&base, "/mcp"))
}

fn parse_bool_env(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn auth_mode_label(require_auth: bool) -> String {
    if require_auth {
        "required".to_string()
    } else {
        "disabled for localhost".to_string()
    }
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false)
}

fn is_local_peer(peer_addr: SocketAddr) -> bool {
    peer_addr.ip().is_loopback()
}

fn is_local_handshake_method(rpc_method: Option<&str>) -> bool {
    matches!(rpc_method, Some("initialize" | "tools/list" | "tools.list"))
}

fn should_bypass_http_auth(
    peer_addr: SocketAddr,
    allow_loopback_auth_bypass: bool,
    require_auth: bool,
    rpc_method: Option<&str>,
) -> bool {
    if !require_auth {
        return true;
    }
    if !allow_loopback_auth_bypass {
        return false;
    }
    if !is_local_peer(peer_addr) {
        return false;
    }
    is_local_handshake_method(rpc_method)
}

fn log_auth_bypass(peer_addr: SocketAddr, uri: &Uri, rpc_method: Option<&str>, reason: &str) {
    tracing::info!(
        peer = %peer_addr,
        path = %uri.path(),
        rpc_method = rpc_method.unwrap_or("unknown"),
        reason,
        "MCP auth bypassed for localhost request"
    );
}

fn normalize_origin(value: &str) -> Option<String> {
    let normalized = value.trim().trim_end_matches('/').to_ascii_lowercase();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn normalize_base_url(value: &str) -> Option<String> {
    let normalized = value.trim().trim_end_matches('/').to_string();
    if normalized.starts_with("http://") || normalized.starts_with("https://") {
        Some(normalized)
    } else {
        None
    }
}

fn with_path(base: &str, path: &str) -> String {
    format!("{}{}", base.trim_end_matches('/'), path)
}

fn is_origin_allowed(
    mode: McpDeploymentMode,
    headers: &HeaderMap,
    allowed_origins: &[String],
) -> bool {
    if matches!(mode, McpDeploymentMode::Local) {
        return true;
    }
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok());
    let Some(origin) = origin else {
        return true;
    };
    let Some(normalized) = normalize_origin(origin) else {
        return false;
    };
    if normalized == "null" {
        return false;
    }
    if allowed_origins.is_empty() {
        return false;
    }
    allowed_origins.iter().any(|item| item == &normalized)
}

fn jsonrpc_method_hint(payload: &Value) -> Option<&str> {
    match payload {
        Value::Object(map) => map.get("method").and_then(Value::as_str),
        Value::Array(items) => items.iter().find_map(jsonrpc_method_hint),
        _ => None,
    }
}

fn unauthorized_jsonrpc_response(payload: &Value) -> Response {
    let response_payload = unauthorized_jsonrpc_payload(payload);
    (StatusCode::UNAUTHORIZED, Json(response_payload)).into_response()
}

fn unauthorized_jsonrpc_payload(payload: &Value) -> Value {
    match payload {
        Value::Array(items) => {
            let responses = items
                .iter()
                .filter_map(unauthorized_jsonrpc_item)
                .collect::<Vec<_>>();
            if responses.is_empty() {
                error_response(
                    Value::Null,
                    -32001,
                    "Unauthorized: valid Bearer token required".to_string(),
                )
            } else {
                Value::Array(responses)
            }
        }
        _ => unauthorized_jsonrpc_item(payload).unwrap_or_else(|| {
            error_response(
                Value::Null,
                -32001,
                "Unauthorized: valid Bearer token required".to_string(),
            )
        }),
    }
}

fn unauthorized_jsonrpc_item(payload: &Value) -> Option<Value> {
    payload.as_object().map(|object| {
        error_response(
            object.get("id").cloned().unwrap_or(Value::Null),
            -32001,
            "Unauthorized: valid Bearer token required".to_string(),
        )
    })
}

// ---------------------------------------------------------------------------
// Route handlers
// ---------------------------------------------------------------------------

/// Unauthenticated probe — lets clients discover the server without a token.
async fn root_handler(State(state): State<Arc<HttpState>>) -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({
            "name": "FNDR MCP Server",
            "mcp_endpoint": "/mcp",
            "sse_endpoint": "/mcp/sse",
            "transport": ["streamable_http", "sse"],
            "auth_required": state.require_auth,
            "auth_mode": auth_mode_label(state.require_auth),
            "mode": state.mode.as_str(),
            "local_only": state.mode.local_only(),
            "public_endpoint": state.public_endpoint.clone(),
            "public_sse_endpoint": state.public_sse_endpoint.clone()
        })),
    )
}

/// GET /mcp — streamable HTTP-style SSE entrypoint.
async fn mcp_stream_handler(
    State(state): State<Arc<HttpState>>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
) -> Response {
    sse_handler_inner(state, peer_addr, uri, headers, true).await
}

/// POST /mcp  and  POST /mcp/messages — localhost JSON-RPC handler.
async fn mcp_handler(
    State(state): State<Arc<HttpState>>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Response {
    if !is_origin_allowed(state.mode, &headers, &state.allowed_origins) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error": "Forbidden: invalid Origin header"})),
        )
            .into_response();
    }

    let rpc_method = jsonrpc_method_hint(&payload);
    if should_bypass_http_auth(
        peer_addr,
        state.allow_loopback_auth_bypass,
        state.require_auth,
        rpc_method,
    ) {
        let reason = if state.require_auth {
            "local initialize/tools/list exemption"
        } else {
            "localhost auth disabled"
        };
        log_auth_bypass(peer_addr, &uri, rpc_method, reason);
    } else if !check_auth(&headers, &state.token) {
        return unauthorized_jsonrpc_response(&payload);
    }

    let app_state = state.app_state.clone();
    let handled = tokio::task::spawn_blocking(move || {
        let handle = tokio::runtime::Handle::current();
        handle.block_on(handle_payload(payload, app_state))
    })
    .await;

    match handled {
        Ok(Some(response_payload)) => (StatusCode::OK, Json(response_payload)).into_response(),
        Ok(None) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("MCP handler task failed: {err}") })),
        )
            .into_response(),
    }
}

/// GET /mcp/sse — SSE streaming transport (MCP spec 2024-11-05).
///
/// Sends an initial `endpoint` event pointing the client at POST /mcp/messages,
/// then keeps the stream alive with periodic pings.
async fn sse_handler(
    State(state): State<Arc<HttpState>>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
) -> Response {
    sse_handler_inner(state, peer_addr, uri, headers, false).await
}

async fn sse_handler_inner(
    state: Arc<HttpState>,
    peer_addr: SocketAddr,
    uri: Uri,
    headers: HeaderMap,
    use_streamable_endpoint_event: bool,
) -> Response {
    if !is_origin_allowed(state.mode, &headers, &state.allowed_origins) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error": "Forbidden: invalid Origin header"})),
        )
            .into_response();
    }

    if should_bypass_http_auth(
        peer_addr,
        state.allow_loopback_auth_bypass,
        state.require_auth,
        None,
    ) {
        log_auth_bypass(peer_addr, &uri, Some("sse"), "localhost auth disabled");
    } else if !check_auth(&headers, &state.token) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Unauthorized: valid Bearer token required"})),
        )
            .into_response();
    }

    let session_id = uuid::Uuid::new_v4().to_string();
    let messages_url = if use_streamable_endpoint_event {
        format!("/mcp?session={session_id}")
    } else {
        format!("/mcp/messages?session={session_id}")
    };

    // Channel for the endpoint event + keepalives
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(16);

    // Send the initial endpoint event
    let endpoint_event = Event::default()
        .event("endpoint")
        .data(messages_url.clone());
    let _ = tx.send(Ok(endpoint_event)).await;

    // Spawn a task that keeps the stream pinging so clients don't time out
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(15)).await;
            if tx.send(Ok(Event::default().comment("ping"))).await.is_err() {
                break;
            }
        }
    });

    let stream = ReceiverStream::new(rx);
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

// ---------------------------------------------------------------------------
// JSON-RPC dispatch
// ---------------------------------------------------------------------------

async fn handle_payload(payload: Value, app_state: Arc<AppState>) -> Option<Value> {
    if let Value::Array(items) = payload {
        let mut responses = Vec::new();
        for item in items {
            if let Some(resp) = handle_single_request(item, app_state.clone()).await {
                responses.push(resp);
            }
        }
        if responses.is_empty() {
            None
        } else {
            Some(Value::Array(responses))
        }
    } else {
        handle_single_request(payload, app_state).await
    }
}

async fn handle_single_request(raw: Value, app_state: Arc<AppState>) -> Option<Value> {
    let req: JsonRpcRequest = match serde_json::from_value(raw) {
        Ok(req) => req,
        Err(err) => {
            return Some(error_response(
                Value::Null,
                -32600,
                format!("Invalid request: {err}"),
            ));
        }
    };

    let is_notification = req.id.is_none();
    let id = req.id.clone().unwrap_or(Value::Null);

    if req.jsonrpc.as_deref() != Some("2.0") {
        if is_notification {
            return None;
        }
        return Some(error_response(
            id,
            -32600,
            "Invalid JSON-RPC version; expected 2.0".to_string(),
        ));
    }

    let response = match req.method.as_str() {
        "initialize" => Ok(initialize_result(req.params)),
        "notifications/initialized" | "notifications.initialized" => {
            if is_notification {
                return None;
            }
            Ok(json!({}))
        }
        "ping" => Ok(json!({})),
        "tools/list" | "tools.list" => Ok(tools_list_result()),
        "tools/call" | "tools.call" => call_tool(req.params, app_state).await,
        _ => Err(JsonRpcError {
            code: -32601,
            message: format!("Method not found: {}", req.method),
        }),
    };

    if is_notification {
        return None;
    }

    Some(match response {
        Ok(result) => success_response(id, result),
        Err(err) => error_response(id, err.code, err.message),
    })
}

// ---------------------------------------------------------------------------
// MCP capability declarations
// ---------------------------------------------------------------------------

fn initialize_result(params: Option<Value>) -> Value {
    let protocol_version = params
        .as_ref()
        .and_then(|p| p.get("protocolVersion"))
        .and_then(Value::as_str)
        .unwrap_or("2024-11-05");

    json!({
        "protocolVersion": protocol_version,
        "capabilities": {
            "tools": { "listChanged": false }
        },
        "serverInfo": {
            "name": "FNDR",
            "version": env!("CARGO_PKG_VERSION")
        },
        "instructions": "FNDR exposes private local memory search and Q&A tools. All data lives on your machine."
    })
}

fn tools_list_result() -> Value {
    json!({
        "tools": [
            {
                "name": "memory.search_full_context",
                "description": "Agent-first full-context search with semantic + keyword matches, related memory links, timeline context, and synthesized next steps.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Semantic search query" },
                        "time_window": {
                            "description": "Optional range. Accepts '1h','24h','7d','today','yesterday', ISO timestamps, unix ms, or {from,to}.",
                            "oneOf": [
                                { "type": "string" },
                                { "type": "number" },
                                {
                                    "type": "object",
                                    "properties": {
                                        "from": { "oneOf": [{ "type": "string" }, { "type": "number" }] },
                                        "to": { "oneOf": [{ "type": "string" }, { "type": "number" }] }
                                    }
                                }
                            ]
                        },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 100 },
                        "include_raw": { "type": "boolean" }
                    },
                    "required": ["query"]
                }
            },
            {
                "name": "memory.get_context_pack",
                "description": "Build an instant continuation pack for agents: active project, goals, files, URLs, errors, blockers, decisions, todos, and likely next actions.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "topic": { "type": "string" },
                        "time_window": {
                            "oneOf": [
                                { "type": "string" },
                                { "type": "number" },
                                {
                                    "type": "object",
                                    "properties": {
                                        "from": { "oneOf": [{ "type": "string" }, { "type": "number" }] },
                                        "to": { "oneOf": [{ "type": "string" }, { "type": "number" }] }
                                    }
                                }
                            ]
                        },
                        "depth": { "type": "string", "enum": ["shallow", "standard", "deep"] }
                    },
                    "required": ["topic"]
                }
            },
            {
                "name": "memory.agent_brief",
                "description": "Return a compact LLM-ready brief with timeline, facts, decisions, errors, files, URLs, and optional raw evidence.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "topic": { "type": "string" },
                        "token_budget": { "type": "integer", "minimum": 256, "maximum": 12000 },
                        "include_raw_evidence": { "type": "boolean" }
                    },
                    "required": ["topic"]
                }
            },
            {
                "name": "memory.timeline",
                "description": "Chronological activity timeline grouped by session/hour/day/app/project, including timestamps, summaries, apps, windows, URLs, and memory IDs.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "from": { "oneOf": [{ "type": "string" }, { "type": "number" }] },
                        "to": { "oneOf": [{ "type": "string" }, { "type": "number" }] },
                        "granularity": { "type": "string", "enum": ["session", "hour", "day", "app", "project"] }
                    }
                }
            },
            {
                "name": "memory.active_focus",
                "description": "Infer active app/window/project/task intent from recent activity for live agent assistance.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "lookback_minutes": { "type": "integer", "minimum": 1, "maximum": 1440 }
                    }
                }
            },
            {
                "name": "memory.warm_start",
                "description": "Return an agent warm-start context with active focus, likely project, relevant wiki pages, decisions, blockers, todos, files, URLs, and graph-neighbored memories.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "client_name": { "type": "string" },
                        "current_task": { "type": "string" },
                        "token_budget": { "type": "integer", "minimum": 256, "maximum": 12000 },
                        "include_recent_activity": { "type": "boolean" },
                        "include_project_context": { "type": "boolean" },
                        "include_decisions": { "type": "boolean" },
                        "include_open_tasks": { "type": "boolean" }
                    }
                }
            },
            {
                "name": "memory.agent_onboarding",
                "description": "FNDR-native onboarding context for new agents: stable profile context, active projects, working preferences, constraints, recent decisions, blockers, and high-value context packs.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "token_budget": { "type": "integer", "minimum": 256, "maximum": 12000 }
                    }
                }
            },
            {
                "name": "memory.project_wiki",
                "description": "Return compiled project/topic wiki pages with source-backed evidence and related knowledge pages.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "project": { "type": "string" },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 100 }
                    }
                }
            },
            {
                "name": "memory.claims",
                "description": "Return synthesized claim pages derived from repeated MemoryEvents with supporting source memory IDs.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "project": { "type": "string" },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 100 }
                    }
                }
            },
            {
                "name": "memory.breakthroughs",
                "description": "Return breakthrough pages for solved problems, architecture direction, and high-value reusable insights.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "project": { "type": "string" },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 100 }
                    }
                }
            },
            {
                "name": "memory.source_evidence",
                "description": "Return source-backed evidence for a knowledge page or memory. Raw OCR only included when include_raw=true.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "page_id": { "type": "string" },
                        "memory_id": { "type": "string" },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 100 },
                        "include_raw": { "type": "boolean" }
                    }
                }
            },
            {
                "name": "memory.search_raw",
                "description": "Return raw semantic + keyword memory hits with minimal synthesis.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "time_window": {
                            "oneOf": [
                                { "type": "string" },
                                { "type": "number" },
                                {
                                    "type": "object",
                                    "properties": {
                                        "from": { "oneOf": [{ "type": "string" }, { "type": "number" }] },
                                        "to": { "oneOf": [{ "type": "string" }, { "type": "number" }] }
                                    }
                                }
                            ]
                        },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 100 }
                    },
                    "required": ["query"]
                }
            },
            {
                "name": "memory.projects",
                "description": "List recently active projects inferred from activity and memory records.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "limit": { "type": "integer", "minimum": 1, "maximum": 200 }
                    }
                }
            },
            {
                "name": "memory.project_context",
                "description": "Return focused context for one project: summary, goals, files, errors, blockers, decisions, todos, and evidence.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "project": { "type": "string" },
                        "time_window": {
                            "oneOf": [
                                { "type": "string" },
                                { "type": "number" },
                                {
                                    "type": "object",
                                    "properties": {
                                        "from": { "oneOf": [{ "type": "string" }, { "type": "number" }] },
                                        "to": { "oneOf": [{ "type": "string" }, { "type": "number" }] }
                                    }
                                }
                            ]
                        }
                    },
                    "required": ["project"]
                }
            },
            {
                "name": "memory.decisions",
                "description": "List recent decisions from FNDR's decision ledger.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "project": { "type": "string" },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 100 }
                    }
                }
            },
            {
                "name": "memory.errors",
                "description": "List recent captured errors with project and evidence context.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "project": { "type": "string" },
                        "time_window": {
                            "oneOf": [
                                { "type": "string" },
                                { "type": "number" },
                                {
                                    "type": "object",
                                    "properties": {
                                        "from": { "oneOf": [{ "type": "string" }, { "type": "number" }] },
                                        "to": { "oneOf": [{ "type": "string" }, { "type": "number" }] }
                                    }
                                }
                            ]
                        },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 100 }
                    }
                }
            },
            {
                "name": "memory.blockers",
                "description": "List active blockers/failures inferred from recent context.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "project": { "type": "string" },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 100 }
                    }
                }
            },
            {
                "name": "memory.todos",
                "description": "List active todos/reminders/followups from local FNDR tasks.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "project": { "type": "string" },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 200 }
                    }
                }
            },
            {
                "name": "memory.graph_query",
                "description": "Query the local FNDR graph entities and edges by keyword.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 200 }
                    },
                    "required": ["query"]
                }
            },
            {
                "name": "memory.graph_context",
                "description": "Insight Lance graph context: top project nodes, high-confidence edges, conflicts, optional wiki stub, and optional BFS neighborhood from `start_node_id` (UUID). For legacy string-id graph rows, use `memory.graph_query`.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "project": { "type": "string" },
                        "start_node_id": { "type": "string" },
                        "depth": { "type": "integer", "minimum": 1, "maximum": 3 }
                    }
                }
            },
            {
                "name": "memory.recent_changes",
                "description": "Summarize recent memory/task/error changes over a lookback period.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "lookback_minutes": { "type": "integer", "minimum": 1, "maximum": 10080 },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 200 }
                    }
                }
            },
            {
                "name": "search_memories",
                "description": "Search FNDR memory records by semantic + keyword relevance.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query":       { "type": "string", "description": "Search query text" },
                        "time_filter": { "type": "string", "enum": ["1h","24h","7d","today","yesterday"] },
                        "app_filter":  { "type": "string", "description": "Filter by app name" },
                        "limit":       { "type": "integer", "minimum": 1, "maximum": 50 }
                    },
                    "required": ["query"]
                }
            },
            {
                "name": "ask_fndr",
                "description": "Ask FNDR a question and get an answer grounded in captured memories. Times out after 30 seconds.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Question about captured activity" }
                    },
                    "required": ["query"]
                }
            },
            {
                "name": "get_fndr_stats",
                "description": "Return current capture/storage stats.",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            },
            {
                "name": "start_meeting",
                "description": "Start a meeting recording session (Whisper large-v3 turbo GGUF on demand).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
                        "participants": { "type": "array", "items": { "type": "string" } }
                    },
                    "required": ["title"]
                }
            },
            {
                "name": "stop_meeting",
                "description": "Stop the active meeting session.",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            },
            {
                "name": "get_meeting_transcript",
                "description": "Fetch transcript data for a meeting id.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "meeting_id": { "type": "string" }
                    },
                    "required": ["meeting_id"]
                }
            },
            {
                "name": "search_meeting_transcripts",
                "description": "Search across meeting transcripts stored locally.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 100 }
                    },
                    "required": ["query"]
                }
            },
            {
                "name": "get_ambient_context",
                "description": "Return what the user is actively working on right now: frontmost app, recent memory snippets, and window context. Use this to give code editors, AI assistants, or other clients real-time awareness of the user's current task — the 'Time Machine for IDEs' feature.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "limit": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": 20,
                            "description": "Number of recent memory snippets to include (default: 5)"
                        }
                    }
                }
            },
            {
                "name": "fndr_context",
                "description": "Build a source-backed FNDR context pack for an agent session.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "agent_type": { "type": "string" },
                        "budget_tokens": { "type": "integer", "minimum": 200, "maximum": 12000 },
                        "session_id": { "type": "string" },
                        "active_files": { "type": "array", "items": { "type": "string" } },
                        "project": { "type": "string" }
                    }
                }
            },
            {
                "name": "fndr_search_code_context",
                "description": "Return coding-oriented context for the active repo and files.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "repo": { "type": "string" },
                        "files": { "type": "array", "items": { "type": "string" } },
                        "budget_tokens": { "type": "integer", "minimum": 200, "maximum": 12000 }
                    }
                }
            },
            {
                "name": "fndr_diff",
                "description": "Return only new or changed FNDR context for a session since the last injection or explicit timestamp.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "session_id": { "type": "string" },
                        "since_timestamp": { "type": "integer" }
                    },
                    "required": ["session_id"]
                }
            },
            {
                "name": "fndr_get_recent_working_state",
                "description": "Return FNDR's best current understanding of what the user was just doing.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "project": { "type": "string" }
                    }
                }
            },
            {
                "name": "fndr_remember_decision",
                "description": "Append a proposed project decision to FNDR's decision ledger.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "project": { "type": "string" },
                        "title": { "type": "string" },
                        "summary": { "type": "string" },
                        "proposed_by": { "type": "string" },
                        "evidence_ids": { "type": "array", "items": { "type": "string" } }
                    },
                    "required": ["title"]
                }
            },
            {
                "name": "fndr_health_check",
                "description": "Return FNDR context runtime health, embedding contract status, and storage health.",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            }
        ]
    })
}

// ---------------------------------------------------------------------------
// Tool implementations
// ---------------------------------------------------------------------------

async fn call_tool(params: Option<Value>, app_state: Arc<AppState>) -> Result<Value, JsonRpcError> {
    let params: ToolCallParams = serde_json::from_value(params.unwrap_or_else(|| json!({})))
        .map_err(|err| JsonRpcError {
            code: -32602,
            message: format!("Invalid tools/call params: {err}"),
        })?;

    match params.name.as_str() {
        "memory.search_full_context" => {
            let args: SearchFullContextArgs =
                serde_json::from_value(params.arguments).map_err(|err| JsonRpcError {
                    code: -32602,
                    message: format!("Invalid memory.search_full_context args: {err}"),
                })?;
            run_memory_search_full_context(app_state, args).await
        }
        "memory.get_context_pack" => {
            let args: GetContextPackArgs =
                serde_json::from_value(params.arguments).map_err(|err| JsonRpcError {
                    code: -32602,
                    message: format!("Invalid memory.get_context_pack args: {err}"),
                })?;
            run_memory_get_context_pack(app_state, args).await
        }
        "memory.agent_brief" => {
            let args: AgentBriefArgs =
                serde_json::from_value(params.arguments).map_err(|err| JsonRpcError {
                    code: -32602,
                    message: format!("Invalid memory.agent_brief args: {err}"),
                })?;
            run_memory_agent_brief(app_state, args).await
        }
        "memory.timeline" => {
            let args: TimelineArgs =
                serde_json::from_value(params.arguments).unwrap_or_else(|_| TimelineArgs {
                    from: None,
                    to: None,
                    granularity: default_timeline_granularity(),
                });
            run_memory_timeline(app_state, args).await
        }
        "memory.active_focus" => {
            let args: ActiveFocusArgs =
                serde_json::from_value(params.arguments).unwrap_or_default();
            run_memory_active_focus(app_state, args).await
        }
        "memory.warm_start" => {
            let args: WarmStartArgs = serde_json::from_value(params.arguments).unwrap_or_default();
            run_memory_warm_start(app_state, args).await
        }
        "memory.agent_onboarding" => {
            let args: AgentOnboardingArgs =
                serde_json::from_value(params.arguments).unwrap_or_default();
            run_memory_agent_onboarding(app_state, args).await
        }
        "memory.project_wiki" => {
            let args: ProjectWikiArgs =
                serde_json::from_value(params.arguments).unwrap_or_default();
            run_memory_project_wiki(app_state, args).await
        }
        "memory.claims" => {
            let args: ClaimsArgs = serde_json::from_value(params.arguments).unwrap_or_default();
            run_memory_claims(app_state, args).await
        }
        "memory.breakthroughs" => {
            let args: BreakthroughArgs =
                serde_json::from_value(params.arguments).unwrap_or_default();
            run_memory_breakthroughs(app_state, args).await
        }
        "memory.source_evidence" => {
            let args: SourceEvidenceArgs =
                serde_json::from_value(params.arguments).unwrap_or_default();
            run_memory_source_evidence(app_state, args).await
        }
        "memory.search_raw" => {
            let args: SearchRawArgs =
                serde_json::from_value(params.arguments).map_err(|err| JsonRpcError {
                    code: -32602,
                    message: format!("Invalid memory.search_raw args: {err}"),
                })?;
            run_memory_search_raw(app_state, args).await
        }
        "memory.projects" => {
            let args: ProjectsArgs =
                serde_json::from_value(params.arguments).unwrap_or_else(|_| ProjectsArgs {
                    limit: default_projects_limit(),
                });
            run_memory_projects(app_state, args).await
        }
        "memory.project_context" => {
            let args: ProjectContextArgs =
                serde_json::from_value(params.arguments).map_err(|err| JsonRpcError {
                    code: -32602,
                    message: format!("Invalid memory.project_context args: {err}"),
                })?;
            run_memory_project_context(app_state, args).await
        }
        "memory.decisions" => {
            let args: DecisionsArgs =
                serde_json::from_value(params.arguments).unwrap_or_else(|_| DecisionsArgs {
                    project: None,
                    limit: default_full_context_limit(),
                });
            run_memory_decisions(app_state, args).await
        }
        "memory.errors" => {
            let args: ErrorsArgs =
                serde_json::from_value(params.arguments).unwrap_or_else(|_| ErrorsArgs {
                    project: None,
                    time_window: None,
                    limit: default_full_context_limit(),
                });
            run_memory_errors(app_state, args).await
        }
        "memory.blockers" => {
            let args: BlockersArgs =
                serde_json::from_value(params.arguments).unwrap_or_else(|_| BlockersArgs {
                    project: None,
                    limit: default_full_context_limit(),
                });
            run_memory_blockers(app_state, args).await
        }
        "memory.todos" => {
            let args: TodosArgs =
                serde_json::from_value(params.arguments).unwrap_or_else(|_| TodosArgs {
                    project: None,
                    limit: default_full_context_limit(),
                });
            run_memory_todos(app_state, args).await
        }
        "memory.graph_query" => {
            let args: GraphQueryArgs =
                serde_json::from_value(params.arguments).map_err(|err| JsonRpcError {
                    code: -32602,
                    message: format!("Invalid memory.graph_query args: {err}"),
                })?;
            run_memory_graph_query(app_state, args).await
        }
        "memory.graph_context" => {
            let args: GraphContextArgs =
                serde_json::from_value(params.arguments).unwrap_or(GraphContextArgs {
                    project: None,
                    start_node_id: None,
                    depth: default_graph_context_depth(),
                });
            run_memory_graph_context(app_state, args).await
        }
        "memory.recent_changes" => {
            let args: RecentChangesArgs =
                serde_json::from_value(params.arguments).unwrap_or_else(|_| RecentChangesArgs {
                    lookback_minutes: default_recent_changes_lookback_minutes(),
                    limit: default_full_context_limit(),
                });
            run_memory_recent_changes(app_state, args).await
        }
        "search_memories" => {
            let args: SearchMemoriesArgs =
                serde_json::from_value(params.arguments).map_err(|err| JsonRpcError {
                    code: -32602,
                    message: format!("Invalid search_memories args: {err}"),
                })?;
            run_search_memories(app_state, args).await
        }
        "ask_fndr" => {
            let args: AskFndrArgs =
                serde_json::from_value(params.arguments).map_err(|err| JsonRpcError {
                    code: -32602,
                    message: format!("Invalid ask_fndr args: {err}"),
                })?;
            run_ask_fndr(app_state, args).await
        }
        "get_fndr_stats" => run_get_stats(app_state).await,
        "start_meeting" => {
            let args: StartMeetingArgs =
                serde_json::from_value(params.arguments).map_err(|err| JsonRpcError {
                    code: -32602,
                    message: format!("Invalid start_meeting args: {err}"),
                })?;
            run_start_meeting(args).await
        }
        "stop_meeting" => run_stop_meeting().await,
        "get_meeting_transcript" => {
            let args: GetMeetingTranscriptArgs =
                serde_json::from_value(params.arguments).map_err(|err| JsonRpcError {
                    code: -32602,
                    message: format!("Invalid get_meeting_transcript args: {err}"),
                })?;
            run_get_meeting_transcript(args).await
        }
        "search_meeting_transcripts" => {
            let args: SearchMeetingTranscriptsArgs = serde_json::from_value(params.arguments)
                .map_err(|err| JsonRpcError {
                    code: -32602,
                    message: format!("Invalid search_meeting_transcripts args: {err}"),
                })?;
            run_search_meeting_transcripts(args).await
        }
        "get_ambient_context" => {
            let args: GetAmbientContextArgs = serde_json::from_value(params.arguments)
                .unwrap_or_else(|_| GetAmbientContextArgs {
                    limit: default_ambient_limit(),
                });
            run_get_ambient_context(app_state, args).await
        }
        "fndr_context" => {
            let args: ContextRequest =
                serde_json::from_value(params.arguments).map_err(|err| JsonRpcError {
                    code: -32602,
                    message: format!("Invalid fndr_context args: {err}"),
                })?;
            run_fndr_context(app_state, args).await
        }
        "fndr_search_code_context" => {
            let args: CodeContextRequest =
                serde_json::from_value(params.arguments).map_err(|err| JsonRpcError {
                    code: -32602,
                    message: format!("Invalid fndr_search_code_context args: {err}"),
                })?;
            run_fndr_search_code_context(app_state, args).await
        }
        "fndr_diff" => {
            let args: FndrDiffArgs =
                serde_json::from_value(params.arguments).map_err(|err| JsonRpcError {
                    code: -32602,
                    message: format!("Invalid fndr_diff args: {err}"),
                })?;
            run_fndr_diff(app_state, args).await
        }
        "fndr_get_recent_working_state" => {
            let args: ContextRequest = serde_json::from_value(params.arguments)
                .unwrap_or_else(|_| ContextRequest::default());
            run_fndr_get_recent_working_state(app_state, args).await
        }
        "fndr_remember_decision" => {
            let args: DecisionProposal =
                serde_json::from_value(params.arguments).map_err(|err| JsonRpcError {
                    code: -32602,
                    message: format!("Invalid fndr_remember_decision args: {err}"),
                })?;
            run_fndr_remember_decision(app_state, args).await
        }
        "fndr_health_check" => run_fndr_health_check(app_state).await,
        unknown => Ok(tool_error(format!("Unknown tool: {unknown}"))),
    }
}

async fn run_search_memories(
    app_state: Arc<AppState>,
    args: SearchMemoriesArgs,
) -> Result<Value, JsonRpcError> {
    let limit = args.limit.clamp(1, 50);
    let context_pack = context_runtime::build_context_pack(
        &app_state,
        ContextRequest {
            query: args.query.clone(),
            agent_type: "chat_agent".to_string(),
            budget_tokens: 1200,
            session_id: None,
            active_files: Vec::new(),
            project: None,
        },
    )
    .await
    .map_err(internal_tool_error)?;
    let embedder = Embedder::new().map_err(internal_tool_error)?;
    let results = HybridSearcher::search(
        &app_state.store,
        &embedder,
        &args.query,
        limit,
        args.time_filter.as_deref(),
        args.app_filter.as_deref(),
    )
    .await
    .map_err(internal_tool_error)?;

    Ok(tool_success(json!({
        "query": args.query,
        "count": results.len(),
        "results": results,
        "context_pack": context_pack
    })))
}

async fn run_ask_fndr(app_state: Arc<AppState>, args: AskFndrArgs) -> Result<Value, JsonRpcError> {
    let pack = context_runtime::build_context_pack(
        &app_state,
        ContextRequest {
            query: args.query.clone(),
            agent_type: "chat_agent".to_string(),
            budget_tokens: 1600,
            session_id: None,
            active_files: Vec::new(),
            project: None,
        },
    )
    .await
    .map_err(internal_tool_error)?;

    let embedder = Embedder::new().map_err(internal_tool_error)?;
    let results = HybridSearcher::search(&app_state.store, &embedder, &args.query, 8, None, None)
        .await
        .map_err(internal_tool_error)?;

    if results.is_empty() && pack.evidence.is_empty() && pack.relevant_files.is_empty() {
        return Ok(tool_success(json!({
            "answer": "I couldn't find relevant memories for that question yet.",
            "sources": [],
            "context_pack": pack
        })));
    }

    let mut context_sections = Vec::new();
    context_sections.push(context_runtime::render_pack_markdown(&pack));
    if !results.is_empty() {
        context_sections.push(
            results
                .iter()
                .take(8)
                .map(|r| {
                    format!(
                        "[{}] App: {} | Window: {} | Snippet: {} | URL: {}",
                        r.timestamp,
                        r.app_name,
                        r.window_title,
                        r.snippet,
                        r.url.clone().unwrap_or_else(|| "n/a".to_string())
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    let context = context_sections.join("\n\n");

    let answer_future = async {
        match app_state.ensure_inference_engine().await {
            Ok(Some(engine)) => engine.answer(&args.query, &context).await,
            Ok(None) => pack.summary.clone(),
            Err(err) => format!("AI intelligence is temporarily unavailable: {}", err),
        }
    };

    // 30-second timeout on LLM inference so slow models don't block forever
    let answer = tokio::time::timeout(Duration::from_secs(30), answer_future)
        .await
        .unwrap_or_else(|_| "Inference timed out after 30 seconds.".to_string());

    let sources: Vec<Value> = results
        .iter()
        .take(5)
        .map(|r| {
            json!({
                "id": r.id,
                "timestamp": r.timestamp,
                "app_name": r.app_name,
                "window_title": r.window_title,
                "snippet": r.snippet,
                "url": r.url
            })
        })
        .collect();

    Ok(tool_success(json!({
        "answer": answer,
        "sources": sources,
        "context_pack": pack
    })))
}

async fn run_get_stats(app_state: Arc<AppState>) -> Result<Value, JsonRpcError> {
    let stats = app_state
        .store
        .get_stats()
        .await
        .map_err(internal_tool_error)?;

    Ok(tool_success(json!({
        "stats": stats,
        "capture": {
            "is_capturing": app_state.is_capturing(),
            "is_paused": app_state.is_paused.load(std::sync::atomic::Ordering::SeqCst),
            "frames_captured": app_state.frames_captured.load(std::sync::atomic::Ordering::Relaxed),
            "frames_dropped": app_state.frames_dropped.load(std::sync::atomic::Ordering::Relaxed)
        }
    })))
}

async fn run_start_meeting(args: StartMeetingArgs) -> Result<Value, JsonRpcError> {
    let status = meeting::start_recording(
        None,
        args.title,
        args.participants.unwrap_or_default(),
        None,
    )
    .await
    .map_err(internal_tool_error)?;

    Ok(tool_success(json!({ "status": status })))
}

async fn run_stop_meeting() -> Result<Value, JsonRpcError> {
    let status = meeting::stop_recording()
        .await
        .map_err(internal_tool_error)?;
    Ok(tool_success(json!({ "status": status })))
}

async fn run_get_meeting_transcript(args: GetMeetingTranscriptArgs) -> Result<Value, JsonRpcError> {
    let transcript = meeting::get_meeting_transcript(&args.meeting_id)
        .await
        .map_err(internal_tool_error)?;
    Ok(tool_success(json!({ "transcript": transcript })))
}

async fn run_search_meeting_transcripts(
    args: SearchMeetingTranscriptsArgs,
) -> Result<Value, JsonRpcError> {
    let results = meeting::search_meeting_transcripts(&args.query, args.limit)
        .await
        .map_err(internal_tool_error)?;
    Ok(tool_success(json!({
        "query": args.query,
        "count": results.len(),
        "results": results
    })))
}

async fn run_get_ambient_context(
    app_state: Arc<AppState>,
    args: GetAmbientContextArgs,
) -> Result<Value, JsonRpcError> {
    let _limit = args.limit.clamp(1, 20);
    let frontmost_app =
        crate::capture::macos_frontmost_app_name().unwrap_or_else(|| "Unknown".to_string());
    let focus_task = app_state.focus_task.read().clone();
    let focus_drift_count = app_state
        .focus_drift_count
        .load(std::sync::atomic::Ordering::Relaxed);
    let working_state = context_runtime::get_recent_working_state(&app_state, None)
        .await
        .map_err(internal_tool_error)?;

    Ok(tool_success(json!({
        "frontmost_app": frontmost_app,
        "focus_task": focus_task,
        "focus_drift_count": focus_drift_count,
        "summary": working_state.summary,
        "working_state": working_state
    })))
}

async fn run_fndr_context(
    app_state: Arc<AppState>,
    args: ContextRequest,
) -> Result<Value, JsonRpcError> {
    let pack = context_runtime::build_context_pack(&app_state, args)
        .await
        .map_err(internal_tool_error)?;
    Ok(tool_success(json!({ "context_pack": pack })))
}

async fn run_fndr_search_code_context(
    app_state: Arc<AppState>,
    args: CodeContextRequest,
) -> Result<Value, JsonRpcError> {
    let code_context = context_runtime::build_code_context(&app_state, args)
        .await
        .map_err(internal_tool_error)?;
    Ok(tool_success(json!({ "code_context": code_context })))
}

async fn run_fndr_diff(
    app_state: Arc<AppState>,
    args: FndrDiffArgs,
) -> Result<Value, JsonRpcError> {
    let delta =
        context_runtime::build_context_delta(&app_state, &args.session_id, args.since_timestamp)
            .await
            .map_err(internal_tool_error)?;
    Ok(tool_success(json!({ "context_delta": delta })))
}

async fn run_fndr_get_recent_working_state(
    app_state: Arc<AppState>,
    args: ContextRequest,
) -> Result<Value, JsonRpcError> {
    let working_state = context_runtime::get_recent_working_state(&app_state, args.project)
        .await
        .map_err(internal_tool_error)?;
    Ok(tool_success(json!({ "working_state": working_state })))
}

async fn run_fndr_remember_decision(
    app_state: Arc<AppState>,
    args: DecisionProposal,
) -> Result<Value, JsonRpcError> {
    let decision = context_runtime::remember_decision(&app_state, args)
        .await
        .map_err(internal_tool_error)?;
    Ok(tool_success(json!({ "decision": decision })))
}

async fn run_fndr_health_check(app_state: Arc<AppState>) -> Result<Value, JsonRpcError> {
    let health = context_runtime::health_check(&app_state)
        .await
        .map_err(internal_tool_error)?;
    Ok(tool_success(json!({ "health": health })))
}

#[derive(Debug, Clone, Serialize)]
struct MemoryIndexStatusInfo {
    status: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    errors: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    latest_memory_timestamp: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
struct ParsedTimeWindow {
    label: String,
    time_filter: Option<String>,
    start_ms: Option<i64>,
    end_ms: Option<i64>,
}

async fn run_memory_search_full_context(
    app_state: Arc<AppState>,
    args: SearchFullContextArgs,
) -> Result<Value, JsonRpcError> {
    let limit = args.limit.clamp(1, 100);
    let time_window = parse_time_window_value(args.time_window.as_ref())?;
    let index_status = inspect_memory_index_status(&app_state).await?;
    let embedder = Embedder::new().map_err(internal_tool_error)?;

    let semantic_matches = filter_results_by_window(
        HybridSearcher::search(
            &app_state.store,
            &embedder,
            args.query.trim(),
            limit,
            time_window.time_filter.as_deref(),
            None,
        )
        .await
        .map_err(internal_tool_error)?,
        &time_window,
    );
    let keyword_matches = filter_results_by_window(
        app_state
            .store
            .keyword_search(
                args.query.trim(),
                limit,
                time_window.time_filter.as_deref(),
                None,
            )
            .await
            .map_err(internal_tool_error)?,
        &time_window,
    );

    let merged = dedupe_results_by_id(
        semantic_matches
            .iter()
            .cloned()
            .chain(keyword_matches.iter().cloned())
            .collect(),
    );

    let memory_map = load_memories_for_results(&app_state, &merged).await?;
    let related_memories = fetch_related_memories(
        &app_state,
        &memory_map.values().cloned().collect::<Vec<_>>(),
        limit.saturating_mul(2),
    )
    .await?;

    let mut timeline_range = derive_window_from_results(&merged);
    if let Some(start) = timeline_range.0 {
        timeline_range.0 = Some(start - chrono::Duration::minutes(45).num_milliseconds());
    }
    if let Some(end) = timeline_range.1 {
        timeline_range.1 = Some(end + chrono::Duration::minutes(45).num_milliseconds());
    }
    let timeline_rows = fetch_results_in_range(
        &app_state,
        timeline_range.0.or(time_window.start_ms),
        timeline_range.1.or(time_window.end_ms),
        120,
    )
    .await?;
    let timeline = build_timeline_buckets(timeline_rows, "session");

    let context_pack = context_runtime::build_context_pack(
        &app_state,
        ContextRequest {
            query: args.query.clone(),
            agent_type: "memory_agent".to_string(),
            budget_tokens: 2600,
            session_id: None,
            active_files: Vec::new(),
            project: None,
        },
    )
    .await
    .map_err(internal_tool_error)?;

    let related_urls = aggregate_urls(&merged, &memory_map);
    let related_files = aggregate_files(&merged, &memory_map);
    let suggested_next_steps = suggested_next_steps_from_pack(&context_pack);

    let semantic_json = build_result_rows(&semantic_matches, &memory_map, args.include_raw);
    let keyword_json = build_result_rows(&keyword_matches, &memory_map, args.include_raw);
    let related_json = build_memory_rows(&related_memories, args.include_raw);

    let raw_evidence = if args.include_raw {
        Some(
            memory_map
                .values()
                .map(|memory| {
                    json!({
                        "memory_id": memory.id,
                        "timestamp": memory.timestamp,
                        "app_name": memory.app_name,
                        "window_title": memory.window_title,
                        "url": memory.url,
                        "text": trim_chars(&memory.text, 1600),
                        "clean_text": trim_chars(&memory.clean_text, 1200),
                        "internal_context": trim_chars(&memory.internal_context, 1200)
                    })
                })
                .collect::<Vec<_>>(),
        )
    } else {
        None
    };

    Ok(tool_success(json!({
        "query": args.query,
        "time_window": time_window,
        "index_status": index_status,
        "semantic_matches": semantic_json,
        "keyword_matches": keyword_json,
        "related_memories": related_json,
        "related_urls": related_urls,
        "related_files": related_files,
        "timeline_around_matches": timeline,
        "synthesized_summary": context_pack.summary,
        "suggested_next_steps": suggested_next_steps,
        "context_pack_id": context_pack.id,
        "include_raw": args.include_raw,
        "raw_evidence": raw_evidence
    })))
}

async fn run_memory_get_context_pack(
    app_state: Arc<AppState>,
    args: GetContextPackArgs,
) -> Result<Value, JsonRpcError> {
    let time_window = parse_time_window_value(args.time_window.as_ref())?;
    let index_status = inspect_memory_index_status(&app_state).await?;
    let depth = args.depth.trim().to_ascii_lowercase();
    let budget_tokens = match depth.as_str() {
        "shallow" => 1200,
        "deep" => 4200,
        _ => 2400,
    };

    let pack = context_runtime::build_context_pack(
        &app_state,
        ContextRequest {
            query: args.topic.clone(),
            agent_type: "agent_context_pack".to_string(),
            budget_tokens,
            session_id: None,
            active_files: Vec::new(),
            project: None,
        },
    )
    .await
    .map_err(internal_tool_error)?;
    let working_state = context_runtime::get_recent_working_state(&app_state, pack.project.clone())
        .await
        .map_err(internal_tool_error)?;

    let relevant_results = filter_results_by_window(
        app_state
            .store
            .list_recent_results(80, None)
            .await
            .map_err(internal_tool_error)?,
        &time_window,
    );
    let memory_map = load_memories_for_results(&app_state, &relevant_results).await?;

    let files_touched = aggregate_files(&relevant_results, &memory_map);
    let urls_seen = aggregate_urls(&relevant_results, &memory_map);
    let blockers = pack
        .known_failures
        .iter()
        .map(|failure| {
            json!({
                "id": failure.id,
                "title": failure.title,
                "summary": failure.summary,
                "error": failure.error,
                "related_files": failure.related_files,
                "last_seen_at": failure.last_seen_at
            })
        })
        .collect::<Vec<_>>();

    let recent_work = build_timeline_buckets(relevant_results.clone(), "session");
    let recent_memory_rows = relevant_results
        .iter()
        .take(20)
        .cloned()
        .collect::<Vec<_>>();
    let recent_memories = build_result_rows(&recent_memory_rows, &memory_map, false);
    let next_actions = suggested_next_steps_from_pack(&pack);

    Ok(tool_success(json!({
        "topic": args.topic,
        "depth": depth,
        "time_window": time_window,
        "index_status": index_status,
        "active_project": pack.project,
        "current_goal": pack.active_goal,
        "recent_work": recent_work,
        "files_touched": files_touched,
        "urls_seen": urls_seen,
        "errors": working_state.recent_errors,
        "blockers": blockers,
        "decisions": pack.recent_decisions,
        "todos": pack.open_tasks,
        "relevant_memories": recent_memories,
        "graph_neighbors": pack.included,
        "next_actions": next_actions,
        "summary": pack.summary,
        "context_pack_id": pack.id
    })))
}

async fn run_memory_agent_brief(
    app_state: Arc<AppState>,
    args: AgentBriefArgs,
) -> Result<Value, JsonRpcError> {
    let budget = args.token_budget.clamp(256, 12000);
    let index_status = inspect_memory_index_status(&app_state).await?;
    let factor = (budget as f32 / 1800.0).clamp(0.4, 4.0);
    let max_items = (8.0 * factor) as usize;
    let timeline_items = (5.0 * factor) as usize;

    let pack = context_runtime::build_context_pack(
        &app_state,
        ContextRequest {
            query: args.topic.clone(),
            agent_type: "llm_brief".to_string(),
            budget_tokens: budget,
            session_id: None,
            active_files: Vec::new(),
            project: None,
        },
    )
    .await
    .map_err(internal_tool_error)?;

    let results = app_state
        .store
        .list_recent_results(max_items.saturating_mul(3).max(12), None)
        .await
        .map_err(internal_tool_error)?;
    let timeline = build_timeline_buckets(results.clone(), "session");
    let memory_map = load_memories_for_results(&app_state, &results).await?;
    let files = aggregate_files(&results, &memory_map)
        .into_iter()
        .take(max_items)
        .collect::<Vec<_>>();
    let urls = aggregate_urls(&results, &memory_map)
        .into_iter()
        .take(max_items)
        .collect::<Vec<_>>();

    let facts = build_result_rows(
        &results.into_iter().take(max_items).collect::<Vec<_>>(),
        &memory_map,
        false,
    );
    let decisions = pack
        .recent_decisions
        .iter()
        .take(max_items)
        .cloned()
        .collect::<Vec<_>>();
    let errors = pack
        .known_failures
        .iter()
        .take(max_items)
        .map(|failure| {
            json!({
                "title": failure.title,
                "summary": failure.summary,
                "error": failure.error,
                "last_seen_at": failure.last_seen_at
            })
        })
        .collect::<Vec<_>>();

    let support = if args.include_raw_evidence {
        memory_map
            .values()
            .take(max_items)
            .map(|memory| {
                json!({
                    "memory_id": memory.id,
                    "timestamp": memory.timestamp,
                    "app_name": memory.app_name,
                    "window_title": memory.window_title,
                    "url": memory.url,
                    "snippet": trim_chars(&memory.snippet, 200),
                    "text": trim_chars(&memory.clean_text, 1000)
                })
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    let likely_next_actions = suggested_next_steps_from_pack(&pack);
    let llm_payload = format!(
        "Topic: {}\nProject: {}\nSummary: {}\nCurrent goal: {}\nNext actions: {}\n",
        args.topic,
        pack.project
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        pack.summary,
        pack.active_goal
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        likely_next_actions.join(" | ")
    );
    let max_payload_chars = budget as usize * 4;
    let llm_payload = trim_chars(&llm_payload, max_payload_chars);
    let estimated_tokens = (serde_json::to_string(&facts).unwrap_or_default().len() as u32 / 4)
        + (llm_payload.len() as u32 / 4);

    Ok(tool_success(json!({
        "topic": args.topic,
        "token_budget": budget,
        "estimated_tokens": estimated_tokens,
        "index_status": index_status,
        "summary": pack.summary,
        "timeline": timeline.into_iter().take(timeline_items).collect::<Vec<_>>(),
        "facts": facts,
        "decisions": decisions,
        "errors": errors,
        "files": files,
        "urls": urls,
        "graph_neighbors": pack.included,
        "supporting_snippets": support,
        "next_actions": likely_next_actions,
        "llm_payload": llm_payload
    })))
}

async fn run_memory_timeline(
    app_state: Arc<AppState>,
    args: TimelineArgs,
) -> Result<Value, JsonRpcError> {
    let index_status = inspect_memory_index_status(&app_state).await?;
    let now = chrono::Utc::now().timestamp_millis();
    let to_ms = match args.to.as_ref() {
        Some(value) => parse_timestamp_value(value, "to")?,
        None => now,
    };
    let from_ms = match args.from.as_ref() {
        Some(value) => parse_timestamp_value(value, "from")?,
        None => {
            let local_now = chrono::Local::now();
            local_now
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .map(|naive| {
                    let local = chrono::Local
                        .from_local_datetime(&naive)
                        .earliest()
                        .unwrap_or(local_now);
                    local.timestamp_millis()
                })
                .unwrap_or(now - chrono::Duration::hours(24).num_milliseconds())
        }
    };
    if from_ms > to_ms {
        return Err(JsonRpcError {
            code: -32602,
            message: "Invalid timeline range: `from` must be <= `to`.".to_string(),
        });
    }

    let rows = fetch_results_in_range(&app_state, Some(from_ms), Some(to_ms), 5000).await?;
    let buckets = build_timeline_buckets(rows.clone(), &args.granularity);

    Ok(tool_success(json!({
        "from": from_ms,
        "to": to_ms,
        "granularity": args.granularity,
        "index_status": index_status,
        "total_events": rows.len(),
        "timeline": buckets
    })))
}

async fn run_memory_active_focus(
    app_state: Arc<AppState>,
    args: ActiveFocusArgs,
) -> Result<Value, JsonRpcError> {
    let index_status = inspect_memory_index_status(&app_state).await?;
    let lookback_minutes = args.lookback_minutes.unwrap_or(30).clamp(1, 1440);
    let now = chrono::Utc::now().timestamp_millis();
    let start = now - chrono::Duration::minutes(lookback_minutes as i64).num_milliseconds();
    let frontmost_app =
        crate::capture::macos_frontmost_app_name().unwrap_or_else(|| "Unknown".to_string());
    let recent = app_state
        .store
        .get_search_results_in_range(start, now)
        .await
        .map_err(internal_tool_error)?;
    let recent = recent
        .into_iter()
        .rev()
        .take(30)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>();
    let working_state = context_runtime::get_recent_working_state(&app_state, None)
        .await
        .map_err(internal_tool_error)?;
    let latest = recent.last().cloned();
    let memory_map = load_memories_for_results(&app_state, &recent).await?;
    let relevant_memories = build_result_rows(&recent, &memory_map, false);

    Ok(tool_success(json!({
        "lookback_minutes": lookback_minutes,
        "index_status": index_status,
        "current_app_guess": frontmost_app,
        "current_window_guess": latest.as_ref().map(|row| row.window_title.clone()),
        "current_project_guess": working_state.project,
        "current_task_guess": working_state.active_goal,
        "likely_intent": working_state.summary,
        "recent_context": build_timeline_buckets(recent.clone(), "session"),
        "relevant_memories": relevant_memories,
        "confidence": working_state.confidence
    })))
}

fn knowledge_page_to_json(page: &crate::storage::KnowledgePage) -> Value {
    json!({
        "payload_schema_version": 1,
        "page_id": page.page_id,
        "page_type": page.page_type,
        "title": page.title,
        "page_context": page.page_context,
        "canonical_entities": page.canonical_entities,
        "supporting_memory_ids": page.supporting_memory_ids,
        "supporting_evidence_ids": page.supporting_evidence_ids,
        "related_page_ids": page.related_page_ids,
        "confidence_score": page.confidence_score,
        "stability": page.stability,
        "first_seen": page.first_seen,
        "last_updated": page.last_updated,
        "project": page.project,
        "topic": page.topic,
        "workflow": page.workflow,
    })
}

fn filter_pages_by_type(
    pages: &[crate::storage::KnowledgePage],
    page_type: crate::storage::KnowledgePageType,
    limit: usize,
) -> Vec<Value> {
    pages
        .iter()
        .filter(|page| page.page_type == page_type)
        .take(limit.max(1))
        .map(knowledge_page_to_json)
        .collect()
}

async fn ensure_knowledge_pages(
    app_state: &AppState,
    project: Option<&str>,
) -> Result<Vec<crate::storage::KnowledgePage>, JsonRpcError> {
    let pages = context_runtime::compile_knowledge_pages(app_state, project)
        .await
        .map_err(internal_tool_error)?;
    if !pages.is_empty() {
        let _ = app_state.store.upsert_knowledge_pages(&pages).await;
    }
    Ok(pages)
}

async fn run_memory_warm_start(
    app_state: Arc<AppState>,
    args: WarmStartArgs,
) -> Result<Value, JsonRpcError> {
    let budget = args.token_budget.clamp(256, 12000);
    let focus = run_memory_active_focus(app_state.clone(), ActiveFocusArgs::default()).await?;
    let focus_payload = focus
        .get("structuredContent")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let project_guess = focus_payload
        .get("current_project_guess")
        .and_then(Value::as_str)
        .map(|value| value.to_string());
    let pages = ensure_knowledge_pages(app_state.as_ref(), project_guess.as_deref()).await?;
    let project_pages =
        filter_pages_by_type(&pages, crate::storage::KnowledgePageType::ProjectPage, 6);
    let claim_pages = filter_pages_by_type(&pages, crate::storage::KnowledgePageType::ClaimPage, 8);
    let decision_pages =
        filter_pages_by_type(&pages, crate::storage::KnowledgePageType::DecisionPage, 8);
    let breakthrough_pages = filter_pages_by_type(
        &pages,
        crate::storage::KnowledgePageType::BreakthroughPage,
        8,
    );

    let pack = context_runtime::build_context_pack(
        app_state.as_ref(),
        ContextRequest {
            query: args.current_task.clone().unwrap_or_default(),
            agent_type: args
                .client_name
                .clone()
                .unwrap_or_else(|| "mcp_warm_start".to_string()),
            budget_tokens: budget,
            session_id: None,
            active_files: Vec::new(),
            project: project_guess.clone(),
        },
    )
    .await
    .map_err(internal_tool_error)?;

    let recent_results = app_state
        .store
        .list_recent_results(24, None)
        .await
        .map_err(internal_tool_error)?;
    let memory_map = load_memories_for_results(&app_state, &recent_results).await?;
    let recent_contexts = if args.include_recent_activity {
        build_result_rows(
            &recent_results.iter().take(12).cloned().collect::<Vec<_>>(),
            &memory_map,
            false,
        )
    } else {
        Vec::new()
    };
    let next_steps = suggested_next_steps_from_pack(&pack);
    let open_tasks = if args.include_open_tasks {
        pack.open_tasks.clone()
    } else {
        Vec::new()
    };

    Ok(tool_success(json!({
        "orientation": pack.summary,
        "current_focus": focus_payload.get("likely_intent").cloned().unwrap_or(Value::Null),
        "likely_project": project_guess,
        "relevant_project_pages": if args.include_project_context { project_pages } else { Vec::new() },
        "relevant_claims": claim_pages,
        "recent_memory_contexts": recent_contexts,
        "decisions": if args.include_decisions { decision_pages } else { Vec::new() },
        "breakthroughs": breakthrough_pages,
        "blockers": pack.known_failures,
        "todos": open_tasks,
        "files_and_urls": {
            "files": pack.relevant_files,
            "urls": aggregate_urls(&recent_results, &memory_map)
        },
        "graph_neighbors": pack.included,
        "what_the_agent_should_remember": next_steps,
        "token_budget": budget
    })))
}

async fn run_memory_agent_onboarding(
    app_state: Arc<AppState>,
    args: AgentOnboardingArgs,
) -> Result<Value, JsonRpcError> {
    let budget = args.token_budget.clamp(256, 12000);
    let pages = ensure_knowledge_pages(app_state.as_ref(), None).await?;
    let project_pages =
        filter_pages_by_type(&pages, crate::storage::KnowledgePageType::ProjectPage, 8);
    let decision_pages =
        filter_pages_by_type(&pages, crate::storage::KnowledgePageType::DecisionPage, 8);
    let breakthrough_pages = filter_pages_by_type(
        &pages,
        crate::storage::KnowledgePageType::BreakthroughPage,
        8,
    );
    let contradiction_pages = filter_pages_by_type(
        &pages,
        crate::storage::KnowledgePageType::ContradictionPage,
        8,
    );

    let pack = context_runtime::build_context_pack(
        app_state.as_ref(),
        ContextRequest {
            query: "agent onboarding".to_string(),
            agent_type: "agent_onboarding".to_string(),
            budget_tokens: budget,
            session_id: None,
            active_files: Vec::new(),
            project: None,
        },
    )
    .await
    .map_err(internal_tool_error)?;

    Ok(tool_success(json!({
        "user_profile_context": pack.summary,
        "active_projects": project_pages,
        "working_preferences": pack.do_not_do,
        "current_focus": pack.active_goal,
        "recurring_constraints": pack.do_not_do,
        "recent_decisions": decision_pages,
        "open_blockers": pack.known_failures,
        "important_tools": pack.relevant_files,
        "high_value_context_packs": {
            "context_pack_id": pack.id,
            "confidence": pack.confidence
        },
        "breakthroughs": breakthrough_pages,
        "contradictions": contradiction_pages
    })))
}

async fn run_memory_project_wiki(
    app_state: Arc<AppState>,
    args: ProjectWikiArgs,
) -> Result<Value, JsonRpcError> {
    let project = args.project.as_deref();
    let pages = ensure_knowledge_pages(app_state.as_ref(), project).await?;
    let limit = args.limit.clamp(1, 100);
    let filtered = pages
        .iter()
        .filter(|page| {
            if let Some(project) = project {
                page.project
                    .as_deref()
                    .map(|value| value.eq_ignore_ascii_case(project))
                    .unwrap_or(false)
            } else {
                page.page_type == crate::storage::KnowledgePageType::ProjectPage
                    || page.page_type == crate::storage::KnowledgePageType::TopicPage
            }
        })
        .take(limit)
        .map(knowledge_page_to_json)
        .collect::<Vec<_>>();
    Ok(tool_success(json!({
        "project": args.project,
        "pages": filtered
    })))
}

async fn run_memory_claims(
    app_state: Arc<AppState>,
    args: ClaimsArgs,
) -> Result<Value, JsonRpcError> {
    let pages = ensure_knowledge_pages(app_state.as_ref(), args.project.as_deref()).await?;
    let claims = filter_pages_by_type(
        &pages,
        crate::storage::KnowledgePageType::ClaimPage,
        args.limit.clamp(1, 100),
    );
    Ok(tool_success(json!({
        "project": args.project,
        "claims": claims
    })))
}

async fn run_memory_breakthroughs(
    app_state: Arc<AppState>,
    args: BreakthroughArgs,
) -> Result<Value, JsonRpcError> {
    let pages = ensure_knowledge_pages(app_state.as_ref(), args.project.as_deref()).await?;
    let breakthroughs = filter_pages_by_type(
        &pages,
        crate::storage::KnowledgePageType::BreakthroughPage,
        args.limit.clamp(1, 100),
    );
    Ok(tool_success(json!({
        "project": args.project,
        "breakthroughs": breakthroughs
    })))
}

async fn run_memory_source_evidence(
    app_state: Arc<AppState>,
    args: SourceEvidenceArgs,
) -> Result<Value, JsonRpcError> {
    let mut memory_ids = Vec::new();
    if let Some(memory_id) = args.memory_id.as_deref() {
        memory_ids.push(memory_id.to_string());
    }
    if let Some(page_id) = args.page_id.as_deref() {
        if let Some(page) = app_state
            .store
            .get_knowledge_page(page_id)
            .await
            .map_err(internal_tool_error)?
        {
            memory_ids.extend(page.supporting_memory_ids);
        }
    }
    memory_ids = dedupe_strings_preserve_order(memory_ids);
    let limit = args.limit.clamp(1, 100);
    memory_ids.truncate(limit);
    let mut memories = Vec::new();
    for memory_id in &memory_ids {
        if let Some(memory) = app_state
            .store
            .get_memory_by_id(memory_id)
            .await
            .map_err(internal_tool_error)?
        {
            let mut row = json!({
                "memory_id": memory.id,
                "timestamp": memory.timestamp,
                "memory_context": memory.memory_context,
                "project": memory.project,
                "topic": memory.topic,
                "workflow": memory.workflow,
                "intent": memory.user_intent,
                "decisions": memory.decisions,
                "errors": memory.errors,
                "blockers": memory.blockers,
                "todos": memory.todos,
                "results": memory.results,
                "entities": memory.entities,
                "files": memory.files_touched,
                "url": memory.url,
                "graph_neighbors": memory.related_memory_ids,
            });
            if args.include_raw {
                row["raw"] = json!({
                    "text": trim_chars(&memory.text, 1200),
                    "clean_text": trim_chars(&memory.clean_text, 1200),
                    "raw_evidence": trim_chars(&memory.raw_evidence, 1500),
                });
            }
            memories.push(row);
        }
    }

    Ok(tool_success(json!({
        "page_id": args.page_id,
        "memory_id": args.memory_id,
        "include_raw": args.include_raw,
        "evidence": memories
    })))
}

async fn run_memory_search_raw(
    app_state: Arc<AppState>,
    args: SearchRawArgs,
) -> Result<Value, JsonRpcError> {
    let index_status = inspect_memory_index_status(&app_state).await?;
    let time_window = parse_time_window_value(args.time_window.as_ref())?;
    let limit = args.limit.clamp(1, 100);
    let embedder = Embedder::new().map_err(internal_tool_error)?;
    let semantic = filter_results_by_window(
        HybridSearcher::search(
            &app_state.store,
            &embedder,
            args.query.trim(),
            limit,
            time_window.time_filter.as_deref(),
            None,
        )
        .await
        .map_err(internal_tool_error)?,
        &time_window,
    );
    let keyword = filter_results_by_window(
        app_state
            .store
            .keyword_search(
                args.query.trim(),
                limit,
                time_window.time_filter.as_deref(),
                None,
            )
            .await
            .map_err(internal_tool_error)?,
        &time_window,
    );
    let memory_map = load_memories_for_results(
        &app_state,
        &semantic
            .iter()
            .cloned()
            .chain(keyword.iter().cloned())
            .collect::<Vec<_>>(),
    )
    .await?;

    Ok(tool_success(json!({
        "query": args.query,
        "time_window": time_window,
        "index_status": index_status,
        "semantic": build_result_rows(&semantic, &memory_map, false),
        "keyword": build_result_rows(&keyword, &memory_map, false)
    })))
}

async fn run_memory_projects(
    app_state: Arc<AppState>,
    args: ProjectsArgs,
) -> Result<Value, JsonRpcError> {
    let index_status = inspect_memory_index_status(&app_state).await?;
    let events = app_state
        .store
        .list_activity_events(args.limit.clamp(1, 200).saturating_mul(8), None)
        .await
        .map_err(internal_tool_error)?;
    let mut by_project: HashMap<String, Vec<_>> = HashMap::new();
    for event in events {
        let project = event
            .project
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        by_project.entry(project).or_default().push(event);
    }

    let mut projects = by_project
        .into_iter()
        .map(|(project, mut items)| {
            items.sort_by_key(|event| std::cmp::Reverse(event.end_time));
            let recent = items.first().cloned();
            json!({
                "project": project,
                "activity_count": items.len(),
                "last_active_at": recent.as_ref().map(|e| e.end_time),
                "summary": recent.map(|e| e.summary).unwrap_or_default(),
            })
        })
        .collect::<Vec<_>>();
    projects.sort_by(|left, right| {
        right["last_active_at"]
            .as_i64()
            .unwrap_or_default()
            .cmp(&left["last_active_at"].as_i64().unwrap_or_default())
    });
    projects.truncate(args.limit.clamp(1, 200));

    Ok(tool_success(json!({
        "index_status": index_status,
        "projects": projects
    })))
}

async fn run_memory_project_context(
    app_state: Arc<AppState>,
    args: ProjectContextArgs,
) -> Result<Value, JsonRpcError> {
    let index_status = inspect_memory_index_status(&app_state).await?;
    let time_window = parse_time_window_value(args.time_window.as_ref())?;
    let project = args.project.trim().to_string();
    if project.is_empty() {
        return Err(JsonRpcError {
            code: -32602,
            message: "project is required".to_string(),
        });
    }

    let pack = context_runtime::build_context_pack(
        &app_state,
        ContextRequest {
            query: project.clone(),
            agent_type: "project_context".to_string(),
            budget_tokens: 3200,
            session_id: None,
            active_files: Vec::new(),
            project: Some(project.clone()),
        },
    )
    .await
    .map_err(internal_tool_error)?;

    let events = app_state
        .store
        .list_activity_events(80, Some(&project))
        .await
        .map_err(internal_tool_error)?
        .into_iter()
        .filter(|event| timestamp_in_window(event.end_time, &time_window))
        .collect::<Vec<_>>();
    let relevant_ids = events
        .iter()
        .map(|event| event.memory_id.clone())
        .collect::<HashSet<_>>();
    let mut relevant_results = Vec::new();
    for memory_id in &relevant_ids {
        if let Some(memory) = app_state
            .store
            .get_memory_by_id(memory_id)
            .await
            .map_err(internal_tool_error)?
        {
            relevant_results.push(memory_to_search_result(&memory));
        }
    }
    relevant_results.sort_by_key(|row| std::cmp::Reverse(row.timestamp));
    let memory_map = load_memories_for_results(&app_state, &relevant_results).await?;

    let relevant_memory_rows = relevant_results
        .iter()
        .take(20)
        .cloned()
        .collect::<Vec<_>>();

    Ok(tool_success(json!({
        "project": project,
        "time_window": time_window,
        "index_status": index_status,
        "summary": pack.summary,
        "active_goal": pack.active_goal,
        "files": pack.relevant_files,
        "urls": aggregate_urls(&relevant_results, &memory_map),
        "errors": events.iter().flat_map(|e| e.errors.clone()).collect::<Vec<_>>(),
        "blockers": pack.known_failures,
        "decisions": pack.recent_decisions,
        "todos": pack.open_tasks,
        "next_actions": suggested_next_steps_from_pack(&pack),
        "relevant_memories": build_result_rows(&relevant_memory_rows, &memory_map, false),
    })))
}

async fn run_memory_decisions(
    app_state: Arc<AppState>,
    args: DecisionsArgs,
) -> Result<Value, JsonRpcError> {
    let index_status = inspect_memory_index_status(&app_state).await?;
    let limit = args.limit.clamp(1, 100);
    let decisions = app_state
        .store
        .list_decision_ledger_entries(limit, args.project.as_deref())
        .await
        .map_err(internal_tool_error)?;
    Ok(tool_success(json!({
        "project": args.project,
        "limit": limit,
        "index_status": index_status,
        "decisions": decisions
    })))
}

async fn run_memory_errors(
    app_state: Arc<AppState>,
    args: ErrorsArgs,
) -> Result<Value, JsonRpcError> {
    let index_status = inspect_memory_index_status(&app_state).await?;
    let time_window = parse_time_window_value(args.time_window.as_ref())?;
    let limit = args.limit.clamp(1, 100);
    let events = app_state
        .store
        .list_activity_events(limit.saturating_mul(10), args.project.as_deref())
        .await
        .map_err(internal_tool_error)?
        .into_iter()
        .filter(|event| timestamp_in_window(event.end_time, &time_window))
        .collect::<Vec<_>>();
    let mut rows = Vec::new();
    for event in events {
        for error in event.errors {
            rows.push(json!({
                "memory_id": event.memory_id,
                "timestamp": event.end_time,
                "project": event.project,
                "title": event.title,
                "error": error,
                "summary": event.summary,
                "confidence": event.confidence
            }));
        }
    }
    rows.sort_by(|left, right| {
        right["timestamp"]
            .as_i64()
            .unwrap_or_default()
            .cmp(&left["timestamp"].as_i64().unwrap_or_default())
    });
    rows.truncate(limit);
    Ok(tool_success(json!({
        "project": args.project,
        "time_window": time_window,
        "index_status": index_status,
        "errors": rows
    })))
}

async fn run_memory_blockers(
    app_state: Arc<AppState>,
    args: BlockersArgs,
) -> Result<Value, JsonRpcError> {
    let index_status = inspect_memory_index_status(&app_state).await?;
    let working = context_runtime::get_recent_working_state(&app_state, args.project.clone())
        .await
        .map_err(internal_tool_error)?;
    let blockers = working
        .known_failures
        .into_iter()
        .take(args.limit.clamp(1, 100))
        .collect::<Vec<_>>();
    Ok(tool_success(json!({
        "project": args.project,
        "index_status": index_status,
        "blockers": blockers
    })))
}

async fn run_memory_todos(
    app_state: Arc<AppState>,
    args: TodosArgs,
) -> Result<Value, JsonRpcError> {
    let index_status = inspect_memory_index_status(&app_state).await?;
    let limit = args.limit.clamp(1, 200);
    let tasks = app_state
        .store
        .list_tasks()
        .await
        .map_err(internal_tool_error)?;
    let mut rows = Vec::new();
    for task in tasks
        .into_iter()
        .filter(|task| !task.is_completed && !task.is_dismissed)
    {
        if let Some(project_filter) = args.project.as_deref() {
            let mut matched = false;
            if let Some(source_id) = task.source_memory_id.as_deref() {
                if let Some(event) = app_state
                    .store
                    .get_activity_event_by_memory_id(source_id)
                    .await
                    .map_err(internal_tool_error)?
                {
                    matched = event.project.as_deref() == Some(project_filter);
                }
            }
            if !matched {
                for memory_id in &task.linked_memory_ids {
                    if let Some(event) = app_state
                        .store
                        .get_activity_event_by_memory_id(memory_id)
                        .await
                        .map_err(internal_tool_error)?
                    {
                        if event.project.as_deref() == Some(project_filter) {
                            matched = true;
                            break;
                        }
                    }
                }
            }
            if !matched {
                continue;
            }
        }

        rows.push(json!({
            "id": task.id,
            "title": task.title,
            "description": task.description,
            "source_app": task.source_app,
            "source_memory_id": task.source_memory_id,
            "created_at": task.created_at,
            "due_at": task.due_date,
            "task_type": format!("{:?}", task.task_type).to_ascii_lowercase(),
            "linked_urls": task.linked_urls,
            "linked_memory_ids": task.linked_memory_ids
        }));
    }
    rows.sort_by(|left, right| {
        right["created_at"]
            .as_i64()
            .unwrap_or_default()
            .cmp(&left["created_at"].as_i64().unwrap_or_default())
    });
    rows.truncate(limit);

    Ok(tool_success(json!({
        "project": args.project,
        "index_status": index_status,
        "todos": rows
    })))
}

async fn run_memory_graph_query(
    app_state: Arc<AppState>,
    args: GraphQueryArgs,
) -> Result<Value, JsonRpcError> {
    let index_status = inspect_memory_index_status(&app_state).await?;
    let query = args.query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return Err(JsonRpcError {
            code: -32602,
            message: "query is required".to_string(),
        });
    }
    let limit = args.limit.clamp(1, 200);
    let mut nodes = app_state
        .store
        .get_all_nodes()
        .await
        .map_err(internal_tool_error)?
        .into_iter()
        .filter(|node| {
            node.id.to_ascii_lowercase().contains(&query)
                || node.label.to_ascii_lowercase().contains(&query)
                || node
                    .metadata
                    .to_string()
                    .to_ascii_lowercase()
                    .contains(&query)
        })
        .collect::<Vec<_>>();
    nodes.sort_by_key(|node| std::cmp::Reverse(node.created_at));
    nodes.truncate(limit);
    let node_ids = nodes
        .iter()
        .map(|node| node.id.clone())
        .collect::<HashSet<_>>();
    let edges = app_state
        .store
        .get_all_edges()
        .await
        .map_err(internal_tool_error)?
        .into_iter()
        .filter(|edge| node_ids.contains(&edge.source) || node_ids.contains(&edge.target))
        .take(limit.saturating_mul(2))
        .collect::<Vec<_>>();

    Ok(tool_success(json!({
        "query": args.query,
        "index_status": index_status,
        "nodes": nodes,
        "edges": edges
    })))
}

async fn run_memory_graph_context(
    app_state: Arc<AppState>,
    args: GraphContextArgs,
) -> Result<Value, JsonRpcError> {
    let index_status = inspect_memory_index_status(&app_state).await?;
    let proj = args
        .project
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty());

    if let Some(start) = args
        .start_node_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let id = uuid::Uuid::parse_str(start).map_err(|_| JsonRpcError {
            code: -32602,
            message: "start_node_id must be a UUID for the insight graph".to_string(),
        })?;
        let depth = args.depth.clamp(1, 3);
        let gs = crate::storage::graph_store::GraphStore::new(app_state.store.clone());
        let neighborhood = gs
            .get_subgraph(id, depth)
            .await
            .map_err(internal_tool_error)?;
        let insight = context_runtime::insight_graph_context_mcp(app_state.as_ref(), proj).await;
        return Ok(tool_success(json!({
            "index_status": index_status,
            "neighborhood": neighborhood,
            "insight": insight,
        })));
    }

    let insight = context_runtime::insight_graph_context_mcp(app_state.as_ref(), proj).await;
    Ok(tool_success(json!({
        "index_status": index_status,
        "insight": insight,
    })))
}

async fn run_memory_recent_changes(
    app_state: Arc<AppState>,
    args: RecentChangesArgs,
) -> Result<Value, JsonRpcError> {
    let index_status = inspect_memory_index_status(&app_state).await?;
    let lookback = args.lookback_minutes.clamp(1, 10080);
    let limit = args.limit.clamp(1, 200);
    let end = chrono::Utc::now().timestamp_millis();
    let start = end - chrono::Duration::minutes(lookback as i64).num_milliseconds();
    let recent_results = app_state
        .store
        .get_search_results_in_range(start, end)
        .await
        .map_err(internal_tool_error)?;
    let memory_map = load_memories_for_results(&app_state, &recent_results).await?;
    let recent_memory_rows = recent_results
        .iter()
        .rev()
        .take(limit)
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>();
    let rows = build_result_rows(&recent_memory_rows, &memory_map, false);
    let working = context_runtime::get_recent_working_state(&app_state, None)
        .await
        .map_err(internal_tool_error)?;
    Ok(tool_success(json!({
        "lookback_minutes": lookback,
        "index_status": index_status,
        "recent_memories": rows,
        "recent_errors": working.recent_errors,
        "recent_commands": working.recent_commands,
        "open_tasks": working.open_tasks,
        "summary": working.summary
    })))
}

async fn inspect_memory_index_status(
    app_state: &Arc<AppState>,
) -> Result<MemoryIndexStatusInfo, JsonRpcError> {
    let has_memories = app_state
        .store
        .has_memories()
        .await
        .map_err(internal_tool_error)?;
    if !has_memories {
        return Err(JsonRpcError {
            code: -32010,
            message: "Memory index unavailable: no indexed memories found on this laptop."
                .to_string(),
        });
    }

    let health = context_runtime::health_check(app_state)
        .await
        .map_err(internal_tool_error)?;
    let latest = app_state
        .store
        .list_recent_results(1, None)
        .await
        .map_err(internal_tool_error)?
        .into_iter()
        .next();
    let latest_ts = latest.as_ref().map(|value| value.timestamp);
    let mut status = "ready".to_string();
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    if health.status.eq_ignore_ascii_case("degraded") {
        status = "degraded".to_string();
        if health.degraded_reasons.is_empty() {
            errors.push("Memory index degraded: context runtime health is degraded.".to_string());
        } else {
            for reason in health.degraded_reasons {
                errors.push(format!("Memory index degraded: {reason}"));
            }
        }
    }
    if let Some(ts) = latest_ts {
        let stale_cutoff =
            chrono::Utc::now().timestamp_millis() - chrono::Duration::hours(6).num_milliseconds();
        if ts < stale_cutoff {
            status = if status == "degraded" {
                "degraded_stale".to_string()
            } else {
                "stale".to_string()
            };
            warnings.push(
                "Memory index appears stale: latest memory is older than 6 hours.".to_string(),
            );
        }
    }

    Ok(MemoryIndexStatusInfo {
        status,
        errors,
        warnings,
        latest_memory_timestamp: latest_ts,
    })
}

fn parse_time_window_value(value: Option<&Value>) -> Result<ParsedTimeWindow, JsonRpcError> {
    let now = chrono::Utc::now().timestamp_millis();
    let default_window = ParsedTimeWindow {
        label: "all_time".to_string(),
        time_filter: None,
        start_ms: None,
        end_ms: Some(now),
    };
    let Some(raw) = value else {
        return Ok(default_window);
    };

    match raw {
        Value::String(s) => parse_time_window_string(s),
        Value::Number(number) => {
            let millis = number.as_i64().ok_or_else(|| JsonRpcError {
                code: -32602,
                message: "Invalid time_window number, expected unix milliseconds.".to_string(),
            })?;
            Ok(ParsedTimeWindow {
                label: "from_timestamp".to_string(),
                time_filter: None,
                start_ms: Some(millis),
                end_ms: Some(now),
            })
        }
        Value::Object(map) => {
            let from = map
                .get("from")
                .map(|v| parse_timestamp_value(v, "time_window.from"))
                .transpose()?;
            let to = map
                .get("to")
                .map(|v| parse_timestamp_value(v, "time_window.to"))
                .transpose()?
                .or(Some(now));
            if let (Some(start), Some(end)) = (from, to) {
                if start > end {
                    return Err(JsonRpcError {
                        code: -32602,
                        message: "Invalid time_window: `from` must be <= `to`.".to_string(),
                    });
                }
            }
            Ok(ParsedTimeWindow {
                label: "custom_range".to_string(),
                time_filter: None,
                start_ms: from,
                end_ms: to,
            })
        }
        _ => Err(JsonRpcError {
            code: -32602,
            message: "Invalid time_window. Use string, number, or {from,to}.".to_string(),
        }),
    }
}

fn parse_time_window_string(raw: &str) -> Result<ParsedTimeWindow, JsonRpcError> {
    let value = raw.trim().to_ascii_lowercase();
    let now = chrono::Utc::now().timestamp_millis();
    let window = match value.as_str() {
        "1h" => ParsedTimeWindow {
            label: value,
            time_filter: Some("1h".to_string()),
            start_ms: Some(now - chrono::Duration::hours(1).num_milliseconds()),
            end_ms: Some(now),
        },
        "24h" => ParsedTimeWindow {
            label: value,
            time_filter: Some("24h".to_string()),
            start_ms: Some(now - chrono::Duration::hours(24).num_milliseconds()),
            end_ms: Some(now),
        },
        "7d" => ParsedTimeWindow {
            label: value,
            time_filter: Some("7d".to_string()),
            start_ms: Some(now - chrono::Duration::days(7).num_milliseconds()),
            end_ms: Some(now),
        },
        "today" => {
            let local_now = chrono::Local::now();
            let start = local_now
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .and_then(|naive| chrono::Local.from_local_datetime(&naive).earliest())
                .map(|dt| dt.timestamp_millis())
                .unwrap_or(now - chrono::Duration::hours(24).num_milliseconds());
            ParsedTimeWindow {
                label: value,
                time_filter: Some("today".to_string()),
                start_ms: Some(start),
                end_ms: Some(now),
            }
        }
        "yesterday" => {
            let local_now = chrono::Local::now();
            let today_start = local_now
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .and_then(|naive| chrono::Local.from_local_datetime(&naive).earliest())
                .map(|dt| dt.timestamp_millis())
                .unwrap_or(now - chrono::Duration::hours(24).num_milliseconds());
            ParsedTimeWindow {
                label: value,
                time_filter: Some("yesterday".to_string()),
                start_ms: Some(today_start - chrono::Duration::hours(24).num_milliseconds()),
                end_ms: Some(today_start - 1),
            }
        }
        _ => {
            let parsed = parse_timestamp_str(raw).ok_or_else(|| JsonRpcError {
                code: -32602,
                message: format!(
                    "Unsupported time_window '{raw}'. Use 1h, 24h, 7d, today, yesterday, unix ms, RFC3339, or {{from,to}}."
                ),
            })?;
            ParsedTimeWindow {
                label: "from_timestamp".to_string(),
                time_filter: None,
                start_ms: Some(parsed),
                end_ms: Some(now),
            }
        }
    };
    Ok(window)
}

fn parse_timestamp_value(value: &Value, field_name: &str) -> Result<i64, JsonRpcError> {
    match value {
        Value::Number(number) => number.as_i64().ok_or_else(|| JsonRpcError {
            code: -32602,
            message: format!("Invalid {field_name}: expected unix milliseconds."),
        }),
        Value::String(text) => parse_timestamp_str(text).ok_or_else(|| JsonRpcError {
            code: -32602,
            message: format!("Invalid {field_name}: expected unix ms or RFC3339 timestamp."),
        }),
        _ => Err(JsonRpcError {
            code: -32602,
            message: format!("Invalid {field_name}: expected string or number."),
        }),
    }
}

fn parse_timestamp_str(value: &str) -> Option<i64> {
    if let Ok(ms) = value.trim().parse::<i64>() {
        return Some(ms);
    }
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

fn trim_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let truncated = value.chars().take(max_chars).collect::<String>();
    format!("{truncated}...")
}

fn timestamp_in_window(timestamp: i64, window: &ParsedTimeWindow) -> bool {
    if let Some(start) = window.start_ms {
        if timestamp < start {
            return false;
        }
    }
    if let Some(end) = window.end_ms {
        if timestamp > end {
            return false;
        }
    }
    true
}

fn filter_results_by_window(
    rows: Vec<crate::storage::SearchResult>,
    window: &ParsedTimeWindow,
) -> Vec<crate::storage::SearchResult> {
    rows.into_iter()
        .filter(|row| timestamp_in_window(row.timestamp, window))
        .collect()
}

async fn fetch_results_in_range(
    app_state: &Arc<AppState>,
    start_ms: Option<i64>,
    end_ms: Option<i64>,
    max_rows: usize,
) -> Result<Vec<crate::storage::SearchResult>, JsonRpcError> {
    if let (Some(start), Some(end)) = (start_ms, end_ms) {
        let mut rows = app_state
            .store
            .get_search_results_in_range(start, end)
            .await
            .map_err(internal_tool_error)?;
        if rows.len() > max_rows {
            rows = rows[rows.len() - max_rows..].to_vec();
        }
        return Ok(rows);
    }
    let mut rows = app_state
        .store
        .list_recent_results(max_rows.max(1), None)
        .await
        .map_err(internal_tool_error)?;
    rows.sort_by_key(|row| row.timestamp);
    Ok(rows)
}

fn dedupe_results_by_id(
    rows: Vec<crate::storage::SearchResult>,
) -> Vec<crate::storage::SearchResult> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for row in rows {
        if seen.insert(row.id.clone()) {
            deduped.push(row);
        }
    }
    deduped
}

async fn load_memories_for_results(
    app_state: &Arc<AppState>,
    rows: &[crate::storage::SearchResult],
) -> Result<HashMap<String, crate::storage::MemoryRecord>, JsonRpcError> {
    let mut map = HashMap::new();
    for row in rows {
        if map.contains_key(&row.id) {
            continue;
        }
        if let Some(memory) = app_state
            .store
            .get_memory_by_id(&row.id)
            .await
            .map_err(internal_tool_error)?
        {
            map.insert(row.id.clone(), memory);
        }
    }
    Ok(map)
}

async fn fetch_related_memories(
    app_state: &Arc<AppState>,
    memories: &[crate::storage::MemoryRecord],
    limit: usize,
) -> Result<Vec<crate::storage::MemoryRecord>, JsonRpcError> {
    let mut ids = HashSet::new();
    for memory in memories {
        if let Some(parent) = memory.parent_id.as_deref() {
            ids.insert(parent.to_string());
        }
        for related in &memory.related_ids {
            ids.insert(related.clone());
        }
        for related in &memory.consolidated_from {
            ids.insert(related.clone());
        }
    }
    let mut results = Vec::new();
    for memory_id in ids.into_iter().take(limit.max(1)) {
        if let Some(memory) = app_state
            .store
            .get_memory_by_id(&memory_id)
            .await
            .map_err(internal_tool_error)?
        {
            results.push(memory);
        }
    }
    results.sort_by_key(|memory| std::cmp::Reverse(memory.timestamp));
    results.truncate(limit.max(1));
    Ok(results)
}

fn derive_window_from_results(rows: &[crate::storage::SearchResult]) -> (Option<i64>, Option<i64>) {
    if rows.is_empty() {
        return (None, None);
    }
    let mut min_ts = i64::MAX;
    let mut max_ts = i64::MIN;
    for row in rows {
        min_ts = min_ts.min(row.timestamp);
        max_ts = max_ts.max(row.timestamp);
    }
    (Some(min_ts), Some(max_ts))
}

fn build_result_rows(
    rows: &[crate::storage::SearchResult],
    memory_map: &HashMap<String, crate::storage::MemoryRecord>,
    include_raw: bool,
) -> Vec<Value> {
    rows.iter()
        .map(|row| result_row_to_json(row, memory_map.get(&row.id), include_raw))
        .collect()
}

fn build_memory_rows(memories: &[crate::storage::MemoryRecord], include_raw: bool) -> Vec<Value> {
    memories
        .iter()
        .map(|memory| {
            let mut base = json!({
                "memory_id": memory.id,
                "timestamp": memory.timestamp,
                "app_name": memory.app_name,
                "window_title": memory.window_title,
                "url": memory.url,
                "project": (!memory.project.is_empty()).then(|| memory.project.clone()),
                "snippet": memory.snippet,
                "display_summary": memory.display_summary,
                "files_touched": memory.files_touched,
                "errors": memory.errors,
                "decisions": memory.decisions,
                "next_steps": memory.next_steps,
                "source_type": infer_source_type(memory.url.as_deref(), &memory.app_name),
                "confidence": memory.extraction_confidence
            });
            if include_raw {
                base["raw"] = json!({
                    "text": trim_chars(&memory.text, 1200),
                    "clean_text": trim_chars(&memory.clean_text, 1200),
                    "internal_context": trim_chars(&memory.internal_context, 900)
                });
            }
            base
        })
        .collect()
}

fn result_row_to_json(
    row: &crate::storage::SearchResult,
    memory: Option<&crate::storage::MemoryRecord>,
    include_raw: bool,
) -> Value {
    let mut base = json!({
        "memory_id": row.id,
        "timestamp": row.timestamp,
        "app_name": row.app_name,
        "window_title": row.window_title,
        "url": row.url,
        "project": (!row.project.is_empty()).then(|| row.project.clone()),
        "snippet": row.snippet,
        "display_summary": row.display_summary,
        "memory_context": if !row.memory_context.trim().is_empty() { row.memory_context.clone() } else { row.display_summary.clone() },
        "user_intent": row.user_intent,
        "topic": row.topic,
        "workflow": row.workflow,
        "score": row.score,
        "confidence": if row.extraction_confidence > 0.0 { row.extraction_confidence } else { row.ocr_confidence },
        "files_touched": row.files_touched,
        "errors": memory.map(|m| m.errors.clone()).unwrap_or_default(),
        "decisions": memory.map(|m| m.decisions.clone()).unwrap_or_default(),
        "next_steps": memory.map(|m| m.next_steps.clone()).unwrap_or_default(),
        "source_type": infer_source_type(row.url.as_deref(), &row.app_name),
    });
    if include_raw {
        base["raw"] = json!({
            "text": memory.map(|m| trim_chars(&m.text, 1200)).unwrap_or_else(|| trim_chars(&row.text, 1200)),
            "clean_text": memory.map(|m| trim_chars(&m.clean_text, 1200)).unwrap_or_else(|| trim_chars(&row.clean_text, 1200)),
            "internal_context": memory.map(|m| trim_chars(&m.internal_context, 900)).unwrap_or_else(|| trim_chars(&row.internal_context, 900)),
        });
    }
    base
}

fn memory_to_search_result(memory: &crate::storage::MemoryRecord) -> crate::storage::SearchResult {
    crate::storage::SearchResult {
        id: memory.id.clone(),
        timestamp: memory.timestamp,
        app_name: memory.app_name.clone(),
        bundle_id: memory.bundle_id.clone(),
        window_title: memory.window_title.clone(),
        session_id: memory.session_id.clone(),
        text: memory.text.clone(),
        clean_text: memory.clean_text.clone(),
        ocr_confidence: memory.ocr_confidence,
        ocr_block_count: memory.ocr_block_count,
        snippet: memory.snippet.clone(),
        display_summary: memory.display_summary.clone(),
        internal_context: memory.internal_context.clone(),
        summary_source: memory.summary_source.clone(),
        noise_score: memory.noise_score,
        session_key: memory.session_key.clone(),
        lexical_shadow: memory.lexical_shadow.clone(),
        memory_context: memory.memory_context.clone(),
        user_intent: memory.user_intent.clone(),
        topic: memory.topic.clone(),
        workflow: memory.workflow.clone(),
        search_aliases: memory.search_aliases.clone(),
        related_memory_ids: memory.related_memory_ids.clone(),
        evidence_confidence: memory.evidence_confidence,
        confidence_score: memory.confidence_score,
        importance_score: memory.importance_score,
        specificity_score: memory.specificity_score,
        intent_score: memory.intent_score,
        entity_score: memory.entity_score,
        agent_usefulness_score: memory.agent_usefulness_score,
        ocr_noise_score: memory.ocr_noise_score,
        score: 1.0,
        screenshot_path: memory.screenshot_path.clone(),
        url: memory.url.clone(),
        decay_score: memory.decay_score,
        schema_version: memory.schema_version,
        activity_type: memory.activity_type.clone(),
        files_touched: memory.files_touched.clone(),
        session_duration_mins: memory.session_duration_mins,
        project: memory.project.clone(),
        tags: memory.tags.clone(),
        outcome: memory.outcome.clone(),
        extraction_confidence: memory.extraction_confidence,
        anchor_coverage_score: memory.anchor_coverage_score,
        extracted_entities: memory.entities.clone(),
        content_hash: memory.content_hash.clone(),
        dedup_fingerprint: memory.dedup_fingerprint.clone(),
        is_consolidated: memory.is_consolidated,
        is_soft_deleted: memory.is_soft_deleted,
        insight_what_happened: memory.insight_what_happened.clone(),
        insight_why_mattered: memory.insight_why_mattered.clone(),
        insight_what_changed: memory.insight_what_changed.clone(),
        insight_context_thread: memory.insight_context_thread.clone(),
        insight_spans_json: memory.insight_spans_json.clone(),
        insight_card_confidence: memory.insight_card_confidence,
    }
}

fn aggregate_urls(
    rows: &[crate::storage::SearchResult],
    memory_map: &HashMap<String, crate::storage::MemoryRecord>,
) -> Vec<Value> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for row in rows {
        if let Some(url) = row.url.as_deref() {
            if !url.trim().is_empty() {
                *counts.entry(url.to_string()).or_default() += 1;
            }
        }
        if let Some(memory) = memory_map.get(&row.id) {
            if let Some(url) = memory.url.as_deref() {
                if !url.trim().is_empty() {
                    *counts.entry(url.to_string()).or_default() += 1;
                }
            }
        }
    }
    let mut urls = counts
        .into_iter()
        .map(|(url, count)| json!({ "url": url, "count": count }))
        .collect::<Vec<_>>();
    urls.sort_by(|left, right| {
        right["count"]
            .as_u64()
            .unwrap_or_default()
            .cmp(&left["count"].as_u64().unwrap_or_default())
    });
    urls.truncate(50);
    urls
}

fn aggregate_files(
    rows: &[crate::storage::SearchResult],
    memory_map: &HashMap<String, crate::storage::MemoryRecord>,
) -> Vec<Value> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for row in rows {
        for file in &row.files_touched {
            if !file.trim().is_empty() {
                *counts.entry(file.clone()).or_default() += 1;
            }
        }
        if let Some(memory) = memory_map.get(&row.id) {
            for file in &memory.files_touched {
                if !file.trim().is_empty() {
                    *counts.entry(file.clone()).or_default() += 1;
                }
            }
        }
    }
    let mut files = counts
        .into_iter()
        .map(|(path, count)| json!({ "path": path, "count": count }))
        .collect::<Vec<_>>();
    files.sort_by(|left, right| {
        right["count"]
            .as_u64()
            .unwrap_or_default()
            .cmp(&left["count"].as_u64().unwrap_or_default())
    });
    files.truncate(80);
    files
}

fn suggested_next_steps_from_pack(pack: &crate::storage::ContextPack) -> Vec<String> {
    let mut steps = Vec::new();
    if let Some(step) = pack.recommended_next_action.as_deref() {
        if !step.trim().is_empty() {
            steps.push(step.to_string());
        }
    }
    for task in &pack.open_tasks {
        if !task.title.trim().is_empty() {
            steps.push(task.title.clone());
        }
    }
    for issue in &pack.open_issues {
        if !issue.title.trim().is_empty() {
            steps.push(issue.title.clone());
        }
    }
    for failure in &pack.known_failures {
        if !failure.summary.trim().is_empty() {
            steps.push(format!("Resolve: {}", failure.summary));
        }
    }
    dedupe_strings_preserve_order(steps)
        .into_iter()
        .take(8)
        .collect()
}

fn dedupe_strings_preserve_order(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for value in values {
        let key = value.trim().to_ascii_lowercase();
        if key.is_empty() || !seen.insert(key) {
            continue;
        }
        deduped.push(value);
    }
    deduped
}

fn infer_source_type(url: Option<&str>, app_name: &str) -> String {
    if url.is_some() {
        return "browser".to_string();
    }
    let lower = app_name.to_ascii_lowercase();
    if lower.contains("code")
        || lower.contains("cursor")
        || lower.contains("xcode")
        || lower.contains("terminal")
    {
        "coding".to_string()
    } else if lower.contains("slack")
        || lower.contains("mail")
        || lower.contains("message")
        || lower.contains("teams")
    {
        "communication".to_string()
    } else {
        "application".to_string()
    }
}

fn build_timeline_buckets(
    mut rows: Vec<crate::storage::SearchResult>,
    granularity: &str,
) -> Vec<Value> {
    if rows.is_empty() {
        return Vec::new();
    }
    rows.sort_by_key(|row| row.timestamp);
    let mode = granularity.trim().to_ascii_lowercase();
    if mode == "session" {
        return build_session_buckets(rows);
    }

    let mut grouped: HashMap<String, Vec<crate::storage::SearchResult>> = HashMap::new();
    for row in rows {
        let key = match mode.as_str() {
            "hour" => {
                let secs = row.timestamp.div_euclid(1000);
                format!("hour:{}", secs.div_euclid(3600))
            }
            "day" => {
                let secs = row.timestamp.div_euclid(1000);
                format!("day:{}", secs.div_euclid(86400))
            }
            "app" => format!("app:{}", row.app_name.to_ascii_lowercase()),
            "project" => format!("project:{}", row.project.to_ascii_lowercase()),
            _ => format!("session:{}", row.session_id),
        };
        grouped.entry(key).or_default().push(row);
    }
    let mut buckets = grouped
        .into_values()
        .map(build_bucket_row)
        .collect::<Vec<_>>();
    buckets.sort_by(|left, right| {
        left["start_time"]
            .as_i64()
            .unwrap_or_default()
            .cmp(&right["start_time"].as_i64().unwrap_or_default())
    });
    buckets
}

fn build_session_buckets(rows: Vec<crate::storage::SearchResult>) -> Vec<Value> {
    let mut sessions: Vec<Vec<crate::storage::SearchResult>> = Vec::new();
    let session_gap_ms = chrono::Duration::minutes(20).num_milliseconds();
    for row in rows {
        if let Some(last_group) = sessions.last_mut() {
            if let Some(last) = last_group.last() {
                let same_app = last.app_name == row.app_name;
                let same_project = last.project == row.project;
                let close = row.timestamp - last.timestamp <= session_gap_ms;
                if same_app && same_project && close {
                    last_group.push(row);
                    continue;
                }
            }
        }
        sessions.push(vec![row]);
    }
    sessions.into_iter().map(build_bucket_row).collect()
}

fn build_bucket_row(group: Vec<crate::storage::SearchResult>) -> Value {
    let start = group.first().map(|row| row.timestamp).unwrap_or_default();
    let end = group.last().map(|row| row.timestamp).unwrap_or(start);
    let summary = group
        .last()
        .map(|row| {
            if !row.memory_context.trim().is_empty() {
                row.memory_context.clone()
            } else if !row.display_summary.trim().is_empty() {
                row.display_summary.clone()
            } else {
                row.snippet.clone()
            }
        })
        .unwrap_or_default();
    let apps = dedupe_strings_preserve_order(
        group
            .iter()
            .map(|row| row.app_name.clone())
            .collect::<Vec<_>>(),
    );
    let windows = dedupe_strings_preserve_order(
        group
            .iter()
            .map(|row| row.window_title.clone())
            .collect::<Vec<_>>(),
    );
    let urls = dedupe_strings_preserve_order(
        group
            .iter()
            .filter_map(|row| row.url.clone())
            .collect::<Vec<_>>(),
    );
    let memory_ids = group.iter().map(|row| row.id.clone()).collect::<Vec<_>>();
    let project = group
        .iter()
        .map(|row| row.project.trim())
        .find(|value| !value.is_empty())
        .map(|value| value.to_string());
    let activity_types = dedupe_strings_preserve_order(
        group
            .iter()
            .map(|row| row.activity_type.clone())
            .filter(|value| !value.trim().is_empty())
            .collect::<Vec<_>>(),
    );
    let topics = dedupe_strings_preserve_order(
        group
            .iter()
            .map(|row| row.topic.clone())
            .filter(|value| !value.trim().is_empty() && value != "unknown")
            .collect::<Vec<_>>(),
    );
    let workflows = dedupe_strings_preserve_order(
        group
            .iter()
            .map(|row| row.workflow.clone())
            .filter(|value| !value.trim().is_empty() && value != "unknown")
            .collect::<Vec<_>>(),
    );
    let intents = dedupe_strings_preserve_order(
        group
            .iter()
            .map(|row| row.user_intent.clone())
            .filter(|value| !value.trim().is_empty())
            .collect::<Vec<_>>(),
    );
    json!({
        "start_time": start,
        "end_time": end,
        "summary": summary,
        "project": project,
        "topics": topics,
        "workflows": workflows,
        "intents": intents,
        "apps": apps,
        "windows": windows,
        "urls": urls,
        "activity_types": activity_types,
        "memory_ids": memory_ids,
        "count": group.len(),
    })
}

// ---------------------------------------------------------------------------
// Response helpers
// ---------------------------------------------------------------------------

fn tool_success(payload: Value) -> Value {
    json!({
        "content": [
            {
                "type": "text",
                "text": serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string())
            }
        ],
        "structuredContent": payload
    })
}

fn tool_error(message: String) -> Value {
    json!({
        "isError": true,
        "content": [{ "type": "text", "text": message }]
    })
}

fn success_response(id: Value, result: Value) -> Value {
    serde_json::to_value(JsonRpcResponse {
        jsonrpc: "2.0",
        id,
        result: Some(result),
        error: None,
    })
    .unwrap_or_else(|_| {
        json!({"jsonrpc":"2.0","id":Value::Null,"error":{"code":-32603,"message":"Internal serialization error"}})
    })
}

fn error_response(id: Value, code: i64, message: String) -> Value {
    serde_json::to_value(JsonRpcResponse {
        jsonrpc: "2.0",
        id,
        result: None,
        error: Some(JsonRpcError { code, message }),
    })
    .unwrap_or_else(|_| {
        json!({"jsonrpc":"2.0","id":Value::Null,"error":{"code":-32603,"message":"Internal serialization error"}})
    })
}

fn internal_tool_error<E: std::fmt::Display>(err: E) -> JsonRpcError {
    JsonRpcError {
        code: -32000,
        message: format!("Tool execution failed: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::memory::graph::GraphStore;
    use crate::storage::{StateStore, Store};
    use tempfile::tempdir;

    fn build_test_app_state() -> Arc<AppState> {
        let temp_dir = tempdir().expect("tempdir");
        let data_dir = temp_dir.path().to_path_buf();
        std::mem::forget(temp_dir);
        let store = Arc::new(Store::new(&data_dir).expect("store"));
        let state_store = Arc::new(StateStore::new(&data_dir).expect("state store"));
        let graph = GraphStore::new(store.clone());
        Arc::new(AppState::new(
            data_dir,
            Config::default(),
            store,
            state_store,
            graph,
            None,
            None,
        ))
    }

    async fn wait_for_server(base_url: &str) {
        let client = reqwest::Client::new();
        for _ in 0..40 {
            if client.get(base_url).send().await.is_ok() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("MCP server did not become ready at {base_url}");
    }

    #[test]
    fn local_handshake_methods_bypass_auth_when_loopback_bypass_is_enabled() {
        let peer = SocketAddr::from(([127, 0, 0, 1], 8080));
        assert!(should_bypass_http_auth(
            peer,
            true,
            true,
            Some("initialize")
        ));
        assert!(should_bypass_http_auth(
            peer,
            true,
            true,
            Some("tools/list")
        ));
        assert!(!should_bypass_http_auth(
            peer,
            true,
            true,
            Some("tools/call")
        ));
        assert!(!should_bypass_http_auth(
            peer,
            false,
            true,
            Some("initialize")
        ));
        assert!(should_bypass_http_auth(
            peer,
            false,
            false,
            Some("tools/call")
        ));
    }

    #[test]
    fn localhost_initialize_tools_list_and_call_work_without_auth() {
        std::env::remove_var("FNDR_MCP_REQUIRE_AUTH");
        let app_state = build_test_app_state();
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        runtime.block_on(async move {
            let _ = stop().await;
            let status = start(app_state, None, Some(0)).await.expect("start mcp");
            let base_url = format!("http://{}:{}/", status.host, status.port);
            wait_for_server(&base_url).await;

            let client = reqwest::Client::new();

            let initialize = client
                .post(&status.endpoint)
                .header("Content-Type", "application/json")
                .json(&json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2024-11-05",
                        "capabilities": {},
                        "clientInfo": { "name": "reqwest-test", "version": "0.1.0" }
                    }
                }))
                .send()
                .await
                .expect("initialize request");
            assert_eq!(initialize.status(), reqwest::StatusCode::OK);
            let initialize_body: Value = initialize.json().await.expect("initialize json");
            assert_eq!(initialize_body["jsonrpc"], "2.0");
            assert_eq!(initialize_body["result"]["serverInfo"]["name"], "FNDR");

            let tools_list = client
                .post(&status.endpoint)
                .header("Content-Type", "application/json")
                .json(&json!({
                    "jsonrpc": "2.0",
                    "id": 2,
                    "method": "tools/list"
                }))
                .send()
                .await
                .expect("tools/list request");
            assert_eq!(tools_list.status(), reqwest::StatusCode::OK);
            let tools_list_body: Value = tools_list.json().await.expect("tools/list json");
            assert_eq!(tools_list_body["jsonrpc"], "2.0");
            assert!(tools_list_body["result"]["tools"].is_array());

            let tool_call = client
                .post(&status.endpoint)
                .header("Content-Type", "application/json")
                .json(&json!({
                    "jsonrpc": "2.0",
                    "id": 3,
                    "method": "tools/call",
                    "params": {
                        "name": "fndr_health_check",
                        "arguments": {}
                    }
                }))
                .send()
                .await
                .expect("tools/call request");
            assert_eq!(tool_call.status(), reqwest::StatusCode::OK);
            let tool_call_body: Value = tool_call.json().await.expect("tools/call json");
            assert_eq!(tool_call_body["jsonrpc"], "2.0");
            assert!(tool_call_body["result"]["structuredContent"]["health"].is_object());

            let _ = stop().await;
        });
    }

    #[test]
    fn knowledge_page_json_contract_has_schema_version() {
        use crate::storage::{KnowledgePage, KnowledgePageType};
        let page = KnowledgePage {
            page_id: "kp:test".to_string(),
            page_type: KnowledgePageType::TopicPage,
            title: "Example".to_string(),
            ..Default::default()
        };
        let v = super::knowledge_page_to_json(&page);
        assert_eq!(
            v.get("payload_schema_version").and_then(|x| x.as_u64()),
            Some(1)
        );
        assert!(v.get("page_id").is_some());
        assert!(v.get("stability").is_some());
    }
}
