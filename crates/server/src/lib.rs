//! Nexus server library.
//!
//! Provides a reusable server function to serve Nexus either for the binary, or for the integration tests.

#![deny(missing_docs)]

mod health;

use std::net::SocketAddr;

use anyhow::anyhow;
use axum::{Router, routing::get};
use axum_server::tls_rustls::RustlsConfig;
use config::Config;
use tokio::net::TcpListener;

/// Configuration for serving Nexus.
pub struct ServeConfig {
    /// The socket address (IP and port) the server will bind to
    pub listen_address: SocketAddr,
    /// The deserialized Nexus TOML configuration.
    pub config: Config,
}

/// Starts and runs the Nexus server with the provided configuration.
pub async fn serve(ServeConfig { listen_address, config }: ServeConfig) -> anyhow::Result<()> {
    let mut app = Router::new();

    if config.mcp.enabled {
        app = app.merge(mcp::router(&config.mcp)?);
    }

    if config.server.health.enabled {
        if let Some(listen) = config.server.health.listen {
            tokio::spawn(health::bind_health_endpoint(
                listen,
                config.server.tls.clone(),
                config.server.health,
            ));
        } else {
            app = app.route(&config.server.health.path, get(health::health));
        }
    }

    let listener = TcpListener::bind(listen_address)
        .await
        .map_err(|e| anyhow!("Failed to bind to {listen_address}: {e}"))?;

    match &config.server.tls {
        Some(tls_config) => {
            // Setup TLS
            let rustls_config = RustlsConfig::from_pem_file(&tls_config.certificate, &tls_config.key)
                .await
                .map_err(|e| anyhow!("Failed to load TLS certificate and key: {e}"))?;

            if config.mcp.enabled {
                log::info!("MCP endpoint available at: https://{listen_address}{}", config.mcp.path);
            }

            // Convert tokio listener to std listener for axum-server
            let std_listener = listener.into_std()?;

            // Start the HTTPS server
            axum_server::from_tcp_rustls(std_listener, rustls_config)
                .serve(app.into_make_service())
                .await
                .map_err(|e| anyhow!("Failed to start HTTPS server: {}", e))?;
        }
        None => {
            if config.mcp.enabled {
                log::info!("MCP endpoint available at: http://{listen_address}{}", config.mcp.path);
            }

            // Start the HTTP server
            axum::serve(listener, app)
                .await
                .map_err(|e| anyhow!("Failed to start HTTP server: {}", e))?;
        }
    }

    Ok(())
}
