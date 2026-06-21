//! Persistent MCP API token.
//!
//! On first launch, generates a UUID v4 bearer token and writes it to
//! `~/.continuum/mcp_token` (mode 0o600). Subsequent calls return the cached token.
//! The token is used to authenticate all MCP HTTP requests.

use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

static TOKEN: OnceLock<String> = OnceLock::new();

fn token_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".continuum")
        .join("mcp_token")
}

/// Load the existing token from disk, or generate and persist a new one.
pub fn load_or_create() -> String {
    TOKEN
        .get_or_init(|| {
            let path = token_path();

            // Try to load existing token
            if let Ok(existing) = fs::read_to_string(&path) {
                let token = existing.trim().to_string();
                if !token.is_empty() {
                    tracing::debug!("Loaded existing MCP token from {:?}", path);
                    return token;
                }
            }

            // Generate a fresh token
            let token = uuid::Uuid::new_v4().to_string();

            // Ensure directory exists
            if let Some(parent) = path.parent() {
                if let Err(e) = fs::create_dir_all(parent) {
                    tracing::warn!("Failed to create ~/.continuum dir: {}", e);
                }
            }

            // Write token (best-effort chmod 600)
            match fs::write(&path, &token) {
                Ok(_) => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
                    }
                    tracing::info!("Generated new MCP token, saved to {:?}", path);
                }
                Err(e) => {
                    tracing::warn!("Failed to persist MCP token: {}", e);
                }
            }

            token
        })
        .clone()
}
