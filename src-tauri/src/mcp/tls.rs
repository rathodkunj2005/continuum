//! Self-signed TLS certificate generation and persistence for the MCP server.
//!
//! On first launch, generates an ECDSA P-256 self-signed certificate valid for
//! localhost, 127.0.0.1, and 0.0.0.0. The cert and key are cached to
//! `~/.continuum/mcp_cert.pem` and `~/.continuum/mcp_key.pem` so mobile clients only
//! need to trust the certificate once.

use rcgen::{CertificateParams, DnType, KeyPair, SanType};
use std::fs;
use std::path::PathBuf;

fn cert_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".continuum")
}

fn cert_path() -> PathBuf {
    cert_dir().join("mcp_cert.pem")
}

fn key_path() -> PathBuf {
    cert_dir().join("mcp_key.pem")
}

/// Load or generate a self-signed TLS configuration for the MCP server.
/// Returns an `axum_server::tls_rustls::RustlsConfig`.
pub async fn load_or_create_rustls_config() -> Result<axum_server::tls_rustls::RustlsConfig, String>
{
    let cert_file = cert_path();
    let key_file = key_path();

    // If both files exist, try to load them
    if cert_file.exists() && key_file.exists() {
        tracing::info!("Loading existing MCP TLS cert from {:?}", cert_file);
        match axum_server::tls_rustls::RustlsConfig::from_pem_file(&cert_file, &key_file).await {
            Ok(config) => return Ok(config),
            Err(e) => {
                tracing::warn!("Failed to load existing TLS cert, regenerating: {}", e);
            }
        }
    }

    // Generate a new self-signed certificate
    tracing::info!("Generating self-signed MCP TLS certificate...");

    let key_pair = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)
        .map_err(|e| format!("Failed to generate key pair: {e}"))?;

    let mut params = CertificateParams::default();
    params
        .distinguished_name
        .push(DnType::CommonName, "Continuum MCP Server");
    params
        .distinguished_name
        .push(DnType::OrganizationName, "Continuum");
    params.subject_alt_names = vec![
        SanType::DnsName(
            "localhost"
                .try_into()
                .map_err(|e| format!("Invalid DNS name: {e}"))?,
        ),
        SanType::IpAddress(std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1))),
        SanType::IpAddress(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED)),
    ];
    // Valid for 10 years
    params.not_before = rcgen::date_time_ymd(2024, 1, 1);
    params.not_after = rcgen::date_time_ymd(2034, 1, 1);

    let cert = params
        .self_signed(&key_pair)
        .map_err(|e| format!("Failed to generate self-signed cert: {e}"))?;

    let cert_pem = cert.pem();
    let key_pem = key_pair.serialize_pem();

    // Write to disk
    let dir = cert_dir();
    let _ = fs::create_dir_all(&dir);

    fs::write(&cert_file, &cert_pem).map_err(|e| format!("Failed to write cert PEM: {e}"))?;
    fs::write(&key_file, &key_pem).map_err(|e| format!("Failed to write key PEM: {e}"))?;

    // Restrict permissions on key file
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&key_file, fs::Permissions::from_mode(0o600));
    }

    tracing::info!("Self-signed TLS certificate saved to {:?}", cert_file);

    let config = axum_server::tls_rustls::RustlsConfig::from_pem(
        cert_pem.into_bytes(),
        key_pem.into_bytes(),
    )
    .await
    .map_err(|e| format!("Failed to create RustlsConfig: {e}"))?;

    Ok(config)
}

/// Returns the PEM-encoded certificate string (for clients that need to trust it).
pub fn get_cert_pem() -> Option<String> {
    fs::read_to_string(cert_path()).ok()
}
