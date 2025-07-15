use std::net::SocketAddr;

use anyhow::anyhow;
use config::{HealthConfig, TlsServerConfig};

use axum::{Json, Router, routing::get};
use http::StatusCode;

#[derive(Debug, serde::Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub(crate) enum HealthState {
    /// Indicates that the server is healthy and operational.
    Healthy,

    /// Indicates that the server is unhealthy and not operational.
    #[expect(dead_code)] // I assume we'll use this sometime
    Unhealthy,
}

/// Handles health check requests and returns the current health status of the server.
pub(crate) async fn health() -> (StatusCode, Json<HealthState>) {
    (StatusCode::OK, Json(HealthState::Healthy))
}

/// Binds the health check endpoint to the specified address and configuration.
pub(super) async fn bind_health_endpoint(
    addr: SocketAddr,
    tls_config: Option<TlsServerConfig>,
    health_config: HealthConfig,
) -> anyhow::Result<()> {
    let scheme = if tls_config.is_some() { "https" } else { "http" };
    let path = &health_config.path;
    let app = Router::new().route(path, get(health)).into_make_service();

    log::info!("Health check endpoint exposed at {scheme}://{addr}{path}");

    match tls_config {
        Some(tls) => {
            let rustls_config = axum_server::tls_rustls::RustlsConfig::from_pem_file(&tls.certificate, &tls.key)
                .await
                .map_err(|e| anyhow!("Failed to load TLS certificate and key: {}", e))?;

            axum_server::bind_rustls(addr, rustls_config)
                .serve(app)
                .await
                .map_err(|e| anyhow!("Failed to start HTTP server in the health endpoint: {e}"))?;
        }
        None => axum_server::bind(addr)
            .serve(app)
            .await
            .map_err(|e| anyhow!("Failed to start HTTP server in the health endpoint: {e}"))?,
    }

    Ok(())
}
