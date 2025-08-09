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

use ::rate_limit::RateLimitManager;
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

    let rate_limit_manager = if config.server.rate_limits.enabled {
        log::debug!("Initializing rate limit manager with configured limits");
        let manager = RateLimitManager::new(config.server.rate_limits.clone(), config.mcp.clone()).await?;

        Some(Arc::new(manager))
    } else {
        log::debug!("Rate limiting disabled - no manager created");
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

    // Apply CORS to LLM router before merging
    if config.llm.enabled {
        let llm_router = llm::router(config.llm.clone()).await?;
        protected_router = protected_router.merge(llm_router.layer(cors.clone()));
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

    // Apply rate limiting HTTP middleware only if server-level rate limiting is enabled
    // (global and IP-based limits only - MCP limits are handled in the MCP layer)
    if config.server.rate_limits.enabled
        && let Some(manager) = rate_limit_manager
    {
        log::debug!("Applying HTTP rate limiting middleware to protected routes");
        protected_router = protected_router.layer(RateLimitLayer::new(manager));
    }

    // Merge protected routes (with rate limiting) into main app
    app = app.merge(protected_router);

    // Add health endpoint (unprotected - added AFTER rate limiting)
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

            if config.llm.enabled {
                log::info!("AI endpoint available at: https://{listen_address}{}", config.llm.path);
            }

            axum_server::from_tcp_rustls(listener.into_std()?, rustls_config)
                .serve(app.into_make_service())
                .await
                .map_err(|e| anyhow!("Failed to start HTTPS server: {e}"))?;
        }
        None => {
            if config.mcp.enabled {
                log::info!("MCP endpoint available at: http://{listen_address}{}", config.mcp.path);
            }

            if config.llm.enabled {
                log::info!("AI endpoint available at: http://{listen_address}{}", config.llm.path);
            }

            axum::serve(listener, app)
                .await
                .map_err(|e| anyhow!("Failed to start HTTP server: {}", e))?;
        }
    }

    Ok(())
}
