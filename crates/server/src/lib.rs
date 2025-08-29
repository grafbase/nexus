//! Nexus server library.
//!
//! Provides a reusable server function to serve Nexus either for the binary, or for the integration tests.

#![deny(missing_docs)]

mod auth;
mod client_id;
mod cors;
mod csrf;
mod health;
mod metrics;
mod rate_limit;
mod well_known;

use std::net::SocketAddr;

use ::rate_limit::RateLimitManager;
use anyhow::anyhow;
use auth::AuthLayer;
use axum::{Router, routing::get};
use axum_server::tls_rustls::RustlsConfig;
use client_id::ClientIdentificationLayer;
use config::Config;
use rate_limit::RateLimitLayer;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tower_http::cors::CorsLayer;

/// Configuration for serving Nexus.
pub struct ServeConfig {
    /// The socket address (IP and port) the server will bind to
    pub listen_address: SocketAddr,
    /// The deserialized Nexus TOML configuration.
    pub config: Config,
    /// Cancellation token for graceful shutdown
    pub shutdown_signal: CancellationToken,
}

/// Initialize telemetry if configured
async fn init_telemetry(config: &Config) -> anyhow::Result<Option<telemetry::TelemetryGuard>> {
    if let Some(telemetry_config) = &config.telemetry {
        log::info!("Initializing telemetry subsystem");
        Ok(Some(telemetry::init(telemetry_config).await?))
    } else {
        log::debug!("Telemetry not configured, skipping initialization");
        Ok(None)
    }
}

/// Starts and runs the Nexus server with the provided configuration.
pub async fn serve(
    ServeConfig {
        listen_address,
        config,
        shutdown_signal,
    }: ServeConfig,
) -> anyhow::Result<()> {
    // Initialize telemetry if configured
    let _telemetry_guard = init_telemetry(&config).await?;

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

    // Track which endpoints actually get initialized
    let mut mcp_actually_exposed = false;
    let mut llm_actually_exposed = false;

    // Apply CORS to MCP router before merging
    // Expose MCP endpoint if enabled
    if config.mcp.enabled() {
        let mut mcp_config_builder = mcp::RouterConfig::builder(config.clone());

        if let Some(ref manager) = rate_limit_manager {
            mcp_config_builder = mcp_config_builder.rate_limit_manager(manager.clone());
        }

        match mcp::router(mcp_config_builder.build()).await {
            Ok(mcp_router) => {
                protected_router = protected_router.merge(mcp_router.layer(cors.clone()));
                mcp_actually_exposed = true;
            }
            Err(e) => {
                log::error!("Failed to initialize MCP router: {e}");
            }
        }
    }

    // Apply CORS to LLM router before merging
    // Only expose LLM endpoint if enabled AND has configured providers
    if config.llm.enabled() && config.llm.has_providers() {
        match llm::router(&config).await {
            Ok(llm_router) => {
                protected_router = protected_router.merge(llm_router.layer(cors.clone()));
                llm_actually_exposed = true;
            }
            Err(e) => {
                log::error!("Failed to initialize LLM router: {e}");
            }
        }
    } else {
        log::debug!("LLM is enabled but no providers are configured - LLM endpoint will not be exposed");
    }

    // Apply client identification middleware for access control
    // IMPORTANT: In Axum, layers applied later in code run FIRST in execution
    // So this must be applied BEFORE auth layer to run AFTER auth validates JWT
    match config.server.client_identification {
        Some(ref ident) if ident.enabled => {
            log::debug!("Applying client identification middleware for access control");
            let layer = ClientIdentificationLayer::new(ident.clone());
            protected_router = protected_router.layer(layer);
        }
        _ => {
            log::debug!("Client identification is disabled");
        }
    }

    // Apply OAuth authentication to protected routes
    // This runs BEFORE client identification (due to layer ordering) so JWT is available
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

    // Apply metrics middleware if telemetry is enabled
    if config.telemetry.is_some() {
        app = app.layer(metrics::MetricsLayer::new());
    }

    let listener = TcpListener::bind(listen_address)
        .await
        .map_err(|e| anyhow!("Failed to bind to {listen_address}: {e}"))?;

    // Check what endpoints are actually exposed
    if !mcp_actually_exposed && !llm_actually_exposed {
        log::warn!(
            "Server starting with no functional endpoints. \
            Configure MCP servers or LLM providers to enable functionality."
        );
    }

    match &config.server.tls {
        Some(tls_config) => {
            let rustls_config = RustlsConfig::from_pem_file(&tls_config.certificate, &tls_config.key)
                .await
                .map_err(|e| anyhow!("Failed to load TLS certificate and key: {e}"))?;

            if mcp_actually_exposed {
                log::info!("MCP endpoint available at: https://{listen_address}{}", config.mcp.path);
            }

            if llm_actually_exposed {
                log::info!("LLM endpoint available at: https://{listen_address}{}", config.llm.path);
            }

            let server =
                axum_server::from_tcp_rustls(listener.into_std()?, rustls_config).serve(app.into_make_service());

            // Run with graceful shutdown
            tokio::select! {
                result = server => {
                    result.map_err(|e| anyhow!("Failed to start HTTPS server: {e}"))?;
                }
                _ = shutdown_signal.cancelled() => {
                    log::info!("Received shutdown signal, shutting down gracefully...");
                    // The TelemetryGuard will be dropped when this function returns
                }
            }
        }
        None => {
            if mcp_actually_exposed {
                log::info!("MCP endpoint available at: http://{listen_address}{}", config.mcp.path);
            }

            if llm_actually_exposed {
                log::info!("LLM endpoint available at: http://{listen_address}{}", config.llm.path);
            }

            // Run with graceful shutdown
            tokio::select! {
                result = axum::serve(listener, app) => {
                    result.map_err(|e| anyhow!("Failed to start HTTP server: {}", e))?;
                }
                _ = shutdown_signal.cancelled() => {
                    log::info!("Received shutdown signal, shutting down gracefully...");
                    // The TelemetryGuard will be dropped when this function returns
                }
            }
        }
    }

    Ok(())
}
