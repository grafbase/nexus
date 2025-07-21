//! Nexus server library.
//!
//! Provides a reusable server function to serve Nexus either for the binary, or for the integration tests.

#![deny(missing_docs)]

mod cors;
mod csrf;
mod health;

use std::net::SocketAddr;

use anyhow::anyhow;
use axum::{Router, routing::get};
use axum_server::tls_rustls::RustlsConfig;
use config::Config;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;

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

    // Create CORS layer first, like Grafbase does
    let cors = if let Some(cors_config) = &config.server.cors {
        cors::generate(cors_config)
    } else {
        CorsLayer::permissive()
    };

    // Apply CORS to MCP router before merging
    if config.mcp.enabled {
        let mcp_router = mcp::router(&config.mcp).await?.layer(cors.clone());
        app = app.merge(mcp_router);
    }

    // Apply CORS to health endpoint
    if config.server.health.enabled {
        if let Some(listen) = config.server.health.listen {
            tokio::spawn(health::bind_health_endpoint(
                listen,
                config.server.tls.clone(),
                config.server.health,
            ));
        } else {
            let health_router = Router::new()
                .route(&config.server.health.path, get(health::health))
                .layer(cors.clone());
            app = app.merge(health_router);
        }
    }

    if config.server.csrf.enabled {
        app = csrf::inject_layer(app, &config.server.csrf);
    }

    let listener = TcpListener::bind(listen_address)
        .await
        .map_err(|e| anyhow!("Failed to bind to {listen_address}: {e}"))?;

    match &config.server.tls {
        Some(tls_config) => {
            let rustls_config = RustlsConfig::from_pem_file(&tls_config.certificate, &tls_config.key)
                .await
                .map_err(|e| anyhow!("Failed to load TLS certificate and key: {e}"))?;

            if config.mcp.enabled {
                log::info!("MCP endpoint available at: https://{listen_address}{}", config.mcp.path);
            }

            let std_listener = listener.into_std()?;

            axum_server::from_tcp_rustls(std_listener, rustls_config)
                .serve(app.into_make_service())
                .await
                .map_err(|e| anyhow!("Failed to start HTTPS server: {}", e))?;
        }
        None => {
            if config.mcp.enabled {
                log::info!("MCP endpoint available at: http://{listen_address}{}", config.mcp.path);
            }

            axum::serve(listener, app)
                .await
                .map_err(|e| anyhow!("Failed to start HTTP server: {}", e))?;
        }
    }

    Ok(())
}
