//! Companion API — the local-network HTTP surface that the FNDR iPhone and
//! Apple Watch apps talk to.
//!
//! Design summary:
//!   - One Axum router on a dedicated port, sibling to the MCP server.
//!   - TLS (rustls) with the same self-signed cert the MCP server uses, so
//!     iOS only has to trust one cert per Mac.
//!   - Pairing protocol issues opaque 48-char random access tokens, stored in
//!     `StateStore` under key `companion_devices`. Tokens are revocable.
//!   - `/v1/pair/complete`, `/v1/pair/start`, `/v1/status`,
//!     `/v1/capture/control`, `/v1/memories/manual` are mounted in slice 1.
//!   - `/v1/ask`, `/v1/memories/search`, `/v1/memories/:id`, `/v1/feedback`
//!     cover slices 3-7.
//!
//! Endpoint discovery file: `~/.fndr/companion.json` (host, port, tls, cert).

pub mod auth;
pub mod device_registry;
pub mod dto;
pub mod errors;
pub mod handlers;
pub mod pairing;

use crate::companion::auth::{require_device, CompanionAuthState};
use crate::companion::device_registry::DeviceRegistry;
use crate::companion::dto::CompanionEndpoint;
use crate::companion::handlers::pairing::PairingHttpState;
use crate::companion::pairing::{PairingEndpointHint, PairingService};
use crate::mcp::tls;
use crate::AppState;
use axum::routing::{get, post};
use axum::Router;
use parking_lot::Mutex;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use tokio::task::JoinHandle;
use tower_http::cors::{Any, CorsLayer};

const LAN_BIND_HOST: &str = "0.0.0.0";
const LOOPBACK_HOST: &str = "127.0.0.1";

/// Read once on startup; stays constant for the process lifetime.
fn discovery_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".fndr")
        .join("companion.json")
}

#[derive(Debug, Clone)]
pub struct CompanionStatus {
    pub running: bool,
    pub host: String,
    pub port: u16,
    pub tls: bool,
    pub base_url: String,
    pub mac_name: String,
    pub last_error: Option<String>,
}

struct CompanionRuntime {
    running: bool,
    host: String,
    port: u16,
    tls: bool,
    base_url: String,
    mac_name: String,
    last_error: Option<String>,
    task: Option<JoinHandle<()>>,
    server_handle: Option<axum_server::Handle>,
    pairing: Option<Arc<PairingService>>,
    registry: Option<Arc<DeviceRegistry>>,
}

impl Default for CompanionRuntime {
    fn default() -> Self {
        Self {
            running: false,
            host: String::new(),
            port: 0,
            tls: true,
            base_url: String::new(),
            mac_name: String::new(),
            last_error: None,
            task: None,
            server_handle: None,
            pairing: None,
            registry: None,
        }
    }
}

static COMPANION_RUNTIME: OnceLock<Mutex<CompanionRuntime>> = OnceLock::new();

fn runtime() -> &'static Mutex<CompanionRuntime> {
    COMPANION_RUNTIME.get_or_init(|| Mutex::new(CompanionRuntime::default()))
}

fn to_status(rt: &CompanionRuntime) -> CompanionStatus {
    CompanionStatus {
        running: rt.running,
        host: rt.host.clone(),
        port: rt.port,
        tls: rt.tls,
        base_url: rt.base_url.clone(),
        mac_name: rt.mac_name.clone(),
        last_error: rt.last_error.clone(),
    }
}

pub fn status() -> CompanionStatus {
    let rt = runtime().lock();
    to_status(&rt)
}

/// Returns the live pairing service if the server is running.
pub fn pairing_service() -> Option<Arc<PairingService>> {
    runtime().lock().pairing.clone()
}

/// Returns the live device registry if the server is running.
pub fn device_registry() -> Option<Arc<DeviceRegistry>> {
    runtime().lock().registry.clone()
}

pub fn endpoint() -> Option<CompanionEndpoint> {
    let rt = runtime().lock();
    if !rt.running {
        return None;
    }
    Some(CompanionEndpoint {
        host: rt.host.clone(),
        port: rt.port,
        base_url: rt.base_url.clone(),
        tls: rt.tls,
        cert_fingerprint_sha256: tls_cert_fingerprint(),
        mac_name: rt.mac_name.clone(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

fn tls_cert_fingerprint() -> Option<String> {
    let pem = tls::get_cert_pem()?;
    // Best-effort: fingerprint of the entire PEM (cert + key file mode is
    // separate). iOS just needs a stable identifier for the cert it's pinned to.
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(pem.as_bytes());
    let digest = hasher.finalize();
    Some(
        digest
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
    )
}

/// Start the Companion API on the given (host, port). Pass `None` for port to
/// let the OS pick a free one.
pub async fn start(
    app_state: Arc<AppState>,
    host: Option<String>,
    port: Option<u16>,
) -> Result<CompanionStatus, String> {
    {
        let rt = runtime().lock();
        if rt.running {
            return Ok(to_status(&rt));
        }
    }

    let bind_host = host.unwrap_or_else(|| LAN_BIND_HOST.to_string());
    let advertised_host = resolve_advertised_host(&bind_host);
    let port = port.unwrap_or(0);

    let addr: SocketAddr = format!("{bind_host}:{port}")
        .parse()
        .map_err(|e| format!("invalid bind address: {e}"))?;

    // axum-server does not expose the bound port before serve(); probe + drop.
    let actual_addr = if port == 0 {
        let probe =
            std::net::TcpListener::bind(addr).map_err(|e| format!("port probe failed: {e}"))?;
        let resolved = probe
            .local_addr()
            .map_err(|e| format!("port resolve failed: {e}"))?;
        drop(probe);
        resolved
    } else {
        addr
    };
    let actual_port = actual_addr.port();

    let registry = Arc::new(
        DeviceRegistry::new(app_state.state_store.clone())
            .map_err(|e| format!("device registry init failed: {e:?}"))?,
    );
    let pairing_service = Arc::new(PairingService::new(registry.clone()));

    let mac_name = crate::companion::handlers::status::mac_display_name();
    let endpoint_hint = PairingEndpointHint {
        host: advertised_host.clone(),
        port: actual_port,
        tls: true,
        mac_name: mac_name.clone(),
        cert_fingerprint_sha256: tls_cert_fingerprint(),
    };

    let base_url = format!("https://{advertised_host}:{actual_port}");

    let pairing_state = Arc::new(PairingHttpState {
        service: pairing_service.clone(),
        endpoint_hint: endpoint_hint.clone(),
    });
    let auth_state = Arc::new(CompanionAuthState {
        registry: registry.clone(),
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Authenticated routes share both the app state and the auth middleware.
    let authenticated = Router::new()
        .route("/v1/ask", post(crate::companion::handlers::ask::ask))
        .route(
            "/v1/memories/search",
            post(crate::companion::handlers::search::search_memories),
        )
        .route(
            "/v1/memories/{memory_id}",
            get(crate::companion::handlers::memories::get_memory),
        )
        .route(
            "/v1/status",
            get(crate::companion::handlers::status::get_status),
        )
        .route(
            "/v1/capture/control",
            post(crate::companion::handlers::capture::control),
        )
        .route(
            "/v1/memories/manual",
            post(crate::companion::handlers::memories::create_manual),
        )
        .route(
            "/v1/feedback",
            post(crate::companion::handlers::feedback::submit_feedback),
        )
        .layer(axum::middleware::from_fn_with_state(
            auth_state.clone(),
            require_device,
        ))
        .with_state(app_state.clone());

    // Pairing routes are NOT behind the auth middleware. They are still safe
    // because the only "secret" they hand out is the access token, and the
    // caller must already know the short pairing code (out-of-band — typed on
    // the iPhone after scanning the QR shown on the Mac).
    let pairing_router = Router::new()
        .route(
            "/v1/pair/start",
            post(crate::companion::handlers::pairing::start),
        )
        .route(
            "/v1/pair/complete",
            post(crate::companion::handlers::pairing::complete),
        )
        .with_state(pairing_state.clone());

    let root_router: Router = Router::new()
        .route("/", get(root_handler))
        .route("/v1/health", get(health_handler));

    let app = root_router
        .merge(pairing_router)
        .merge(authenticated)
        .layer(cors);

    write_discovery(&advertised_host, actual_port, true, &mac_name);

    let handle = axum_server::Handle::new();
    let server_handle = handle.clone();
    let task = {
        let tls_config = tls::load_or_create_rustls_config()
            .await
            .map_err(|e| format!("companion tls init failed: {e}"))?;
        tokio::spawn(async move {
            if let Err(err) = axum_server::bind_rustls(actual_addr, tls_config)
                .handle(server_handle)
                .serve(app.into_make_service_with_connect_info::<SocketAddr>())
                .await
            {
                tracing::error!("Companion API server error: {}", err);
            }
        })
    };

    let mut rt = runtime().lock();
    rt.running = true;
    rt.host = advertised_host;
    rt.port = actual_port;
    rt.tls = true;
    rt.base_url = base_url;
    rt.mac_name = mac_name;
    rt.last_error = None;
    rt.task = Some(task);
    rt.server_handle = Some(handle);
    rt.pairing = Some(pairing_service);
    rt.registry = Some(registry);

    tracing::info!(
        host = %rt.host,
        bind_host = %bind_host,
        port = rt.port,
        "Companion API started"
    );

    Ok(to_status(&rt))
}

pub async fn stop() -> CompanionStatus {
    let (server_handle, task) = {
        let mut rt = runtime().lock();
        rt.running = false;
        (rt.server_handle.take(), rt.task.take())
    };
    if let Some(h) = server_handle {
        h.shutdown();
    }
    if let Some(t) = task {
        let _ = t.await;
    }

    let _ = std::fs::remove_file(discovery_path());

    let rt = runtime().lock();
    to_status(&rt)
}

fn write_discovery(host: &str, port: u16, tls_enabled: bool, mac_name: &str) {
    let path = discovery_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let scheme = if tls_enabled { "https" } else { "http" };
    let payload = serde_json::json!({
        "host": host,
        "port": port,
        "tls": tls_enabled,
        "base_url": format!("{}://{}:{}", scheme, host, port),
        "cert_fingerprint_sha256": tls_cert_fingerprint(),
        "mac_name": mac_name,
        "app_version": env!("CARGO_PKG_VERSION"),
    });
    if let Err(e) = std::fs::write(
        &path,
        serde_json::to_string_pretty(&payload).unwrap_or_default(),
    ) {
        tracing::warn!("Failed to write Companion discovery file: {}", e);
    }
}

async fn root_handler() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "service": "fndr_companion",
        "version": env!("CARGO_PKG_VERSION"),
        "endpoints": [
            "/v1/pair/start",
            "/v1/pair/complete",
            "/v1/status",
            "/v1/capture/control",
            "/v1/memories/manual",
            "/v1/memories/search",
            "/v1/memories/:memory_id",
            "/v1/ask",
            "/v1/feedback"
        ],
    }))
}

fn resolve_advertised_host(bind_host: &str) -> String {
    if !is_unspecified_host(bind_host) {
        return bind_host.to_string();
    }

    detect_primary_lan_ipv4()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| LOOPBACK_HOST.to_string())
}

fn is_unspecified_host(host: &str) -> bool {
    host == LAN_BIND_HOST
        || host == "::"
        || host
            .parse::<IpAddr>()
            .map(|ip| ip.is_unspecified())
            .unwrap_or(false)
}

fn detect_primary_lan_ipv4() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(8, 8, 8, 8), 80)).ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_unspecified() => Some(ip),
        _ => None,
    }
}

async fn health_handler() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({"ok": true}))
}
