//! Nexus server library.
//!
//! Provides a reusable server function to serve Nexus either for the binary, or for the integration tests.

#![deny(missing_docs)]

mod auth;
mod cors;
mod csrf;
mod health;
mod rate_limit;
mod well_known;

use std::net::SocketAddr;

use anyhow::anyhow;
use auth::AuthLayer;
use axum::{Router, routing::get};
use axum_server::tls_rustls::RustlsConfig;
use config::Config;
use rate_limit::RateLimitLayer;
use std::sync::Arc;
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
    
    // Create rate limit manager if either server-level or MCP-level rate limiting is configured
    let has_mcp_rate_limits = config.mcp.servers.values()
        .any(|server| server.rate_limit().is_some());
    
    log::debug!("Rate limiting status: server_enabled={}, has_mcp_limits={}", 
        config.server.rate_limit.enabled, has_mcp_rate_limits);
    
    let rate_limit_manager = if config.server.rate_limit.enabled || has_mcp_rate_limits {
        log::debug!("Creating rate limit manager");
        Some(Arc::new(::rate_limit::RateLimitManager::new(
            config.server.rate_limit.clone(),
            config.mcp.clone(),
        )))
    } else {
        log::debug!("No rate limit manager created");
        None
    };

    // Create a router for protected routes (that require OAuth)
    let mut protected_router = Router::new();

    // Apply CORS to MCP router before merging
    if config.mcp.enabled {
        let mut mcp_config_builder = mcp::RouterConfig::builder(config.clone());
        
        if let Some(ref manager) = rate_limit_manager {
            mcp_config_builder = mcp_config_builder.rate_limit_manager(manager.clone());
        }
        
        let mcp_router = mcp::router(mcp_config_builder.build()).await?.layer(cors.clone());
        protected_router = protected_router.merge(mcp_router);
    }

    // Apply OAuth authentication to protected routes
    if let Some(ref oauth_config) = config.server.oauth {
        protected_router = protected_router.layer(AuthLayer::new(oauth_config.clone()));

        // Add OAuth metadata endpoint (this should be public, not protected)
        let oauth_config_clone = oauth_config.clone();
        app = app.route(
            "/.well-known/oauth-protected-resource",
            get(move || well_known::oauth_metadata(oauth_config_clone.clone())),
        );
    }

    // Merge protected routes into main app
    app = app.merge(protected_router);

    // Add health endpoint (unprotected)
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

    // Apply rate limiting HTTP middleware only if server-level rate limiting is enabled
    // (global and IP-based limits only - MCP limits are handled in the MCP layer)
    if config.server.rate_limit.enabled {
        if let Some(manager) = &rate_limit_manager {
            log::debug!("Applying HTTP rate limiting middleware");
            app = app.layer(RateLimitLayer::new(manager.clone()));
        }
    }
    
    // Apply CSRF protection to the entire app if enabled
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
                .map_err(|e| anyhow!("Failed to start HTTPS server: {e}"))?;
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
