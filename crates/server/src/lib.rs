//! Nexus server library.
//!
//! Provides a reusable server function to serve Nexus either for the binary, or for the integration tests.

#![deny(missing_docs)]

mod auth;
mod client_id;
mod cors;
mod csrf;
mod health;
mod logger;
mod metrics;
mod rate_limit;
mod tracing;
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
use telemetry::TelemetryGuard;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tower_http::cors::CorsLayer;

use crate::tracing::TracingLayer;

/// Configuration for serving Nexus.
pub struct ServeConfig {
    /// The socket address (IP and port) the server will bind to
    pub listen_address: SocketAddr,
    /// The deserialized Nexus TOML configuration.
    pub config: Config,
    /// Cancellation token for graceful shutdown
    pub shutdown_signal: CancellationToken,
    /// Log filter string (e.g., "info" or "server=debug,mcp=debug")
    pub log_filter: String,
}

/// Starts and runs the Nexus server with the provided configuration.
pub async fn serve(
    ServeConfig {
        listen_address,
        config,
        shutdown_signal,
        log_filter,
    }: ServeConfig,
) -> anyhow::Result<()> {
    let _telemetry_guard = init_otel(&config, log_filter).await;
    let mut app = Router::new();

    // Create CORS layer first, like Grafbase does
    let cors = if let Some(cors_config) = &config.server.cors {
        cors::generate(cors_config)
    } else {
        CorsLayer::permissive()
    };

    let rate_limit_manager = if config.server.rate_limits.enabled {
        log::debug!("Initializing rate limit manager with configured limits");
        let manager = RateLimitManager::new(
            config.server.rate_limits.clone(),
            config.mcp.clone(),
            config.telemetry.as_ref(),
        )
        .await?;

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

    // Apply rate limiting HTTP middleware FIRST (so it runs LAST, after tracing creates parent span)
    // (global and IP-based limits only - MCP limits are handled in the MCP layer)
    if config.server.rate_limits.enabled
        && let Some(manager) = rate_limit_manager
    {
        log::debug!("Applying HTTP rate limiting middleware to protected routes");
        protected_router = protected_router.layer(RateLimitLayer::new(manager));
    }

    // Apply tracing middleware (runs after client identification, before rate limiting)
    // This ensures client.id and client.group are available and creates parent span for rate limiting
    if let Some(ref telemetry) = config.telemetry
        && telemetry.tracing_enabled()
    {
        log::debug!("Applying tracing middleware to protected routes");
        protected_router = protected_router.layer(TracingLayer::with_config(Arc::new(telemetry.clone())));
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

    // Apply metrics middleware to protected routes if telemetry is enabled
    if config.telemetry.is_some() {
        log::debug!("Applying metrics middleware to protected routes");
        protected_router = protected_router.layer(metrics::MetricsLayer::new());
    }

    // Merge protected routes (with all middleware) into main app
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
                if config.llm.protocols.openai.enabled {
                    log::info!(
                        "LLM endpoint (OpenAI protocol) available at: https://{listen_address}{}",
                        config.llm.protocols.openai.path
                    );
                }
                if config.llm.protocols.anthropic.enabled {
                    log::info!(
                        "LLM endpoint (Anthropic protocol) available at: https://{listen_address}{}",
                        config.llm.protocols.anthropic.path
                    );
                }
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
                if config.llm.protocols.openai.enabled {
                    log::info!(
                        "LLM endpoint (OpenAI protocol) available at: http://{listen_address}{}",
                        config.llm.protocols.openai.path
                    );
                }
                if config.llm.protocols.anthropic.enabled {
                    log::info!(
                        "LLM endpoint (Anthropic protocol) available at: http://{listen_address}{}",
                        config.llm.protocols.anthropic.path
                    );
                }
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

async fn init_otel(config: &Config, log_filter: String) -> Option<TelemetryGuard> {
    if let Some(telemetry_config) = &config.telemetry {
        // Don't let telemetry code log during initialization to avoid recursion
        match telemetry::init(telemetry_config).await {
            Ok(guard) => {
                // Initialize logger with OTEL appender if logs are enabled
                let otel_appender = guard.logs_appender().cloned();
                logger::init(&log_filter, otel_appender);

                Some(guard)
            }
            Err(e) => {
                eprintln!("Failed to initialize telemetry: {e}");
                // Initialize logger without OTEL
                logger::init(&log_filter, None);

                None
            }
        }
    } else {
        // No telemetry configured, initialize basic logger
        logger::init(&log_filter, None);

        None
    }
}
