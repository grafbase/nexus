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
use context::Authentication;
use rate_limit::RateLimitLayer;
use std::sync::Arc;
use telemetry::TelemetryGuard;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tower_http::cors::CorsLayer;

use crate::{csrf::CsrfLayer, tracing::TracingLayer};

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
    /// The version string to log on startup
    pub version: String,
    /// Optional oneshot sender to send back the bound address (useful if port 0 was specified)
    pub bound_addr_sender: Option<tokio::sync::oneshot::Sender<SocketAddr>>,
}

/// Starts and runs the Nexus server with the provided configuration.
pub async fn serve(
    ServeConfig {
        listen_address,
        config,
        shutdown_signal,
        log_filter,
        version,
        bound_addr_sender,
    }: ServeConfig,
) -> anyhow::Result<()> {
    let _telemetry_guard = init_otel(&config, log_filter).await;

    // Log the version as the first message after logger initialization
    log::info!("Nexus {version}");
    let mut app = Router::new();

    let rate_limit_manager = if config.server.rate_limits.enabled {
        log::debug!("Initializing rate limit manager with configured limits");
        let manager =
            RateLimitManager::new(config.server.rate_limits.clone(), config.mcp.clone(), &config.telemetry).await?;

        Some(Arc::new(manager))
    } else {
        log::debug!("Rate limiting disabled - no manager created");
        None
    };

    let cors = if let Some(cors_config) = &config.server.cors {
        cors::new_layer(cors_config)
    } else {
        CorsLayer::permissive()
    };
    let csrf = CsrfLayer::new(&config.server.csrf);

    let layers_before_auth = {
        tower::ServiceBuilder::new()
            .layer(cors.clone())
            .layer(csrf.clone())
            .layer(metrics::MetricsLayer::new())
    };

    let nexus_only_auth_layer = AuthLayer::new(config.server.oauth.clone());

    let layers_after_auth = {
        let client_identification = ClientIdentificationLayer::new(config.server.client_identification.clone());
        let rate_limit = RateLimitLayer::new(config.server.client_ip.clone(), rate_limit_manager.clone());

        tower::ServiceBuilder::new()
            .layer(client_identification)
            .layer(TracingLayer::with_config(Arc::new(config.telemetry.clone())))
            .layer(rate_limit)
    };

    // Track which endpoints actually get initialized
    let mut mcp_actually_exposed = false;
    let mut llm_actually_exposed = false;
    let mut proxy_actually_exposed = false;

    // Expose MCP endpoint if enabled
    if config.mcp.enabled() {
        match mcp::router(mcp::RouterConfig {
            config: config.clone(),
            rate_limit_manager,
        })
        .await
        {
            Ok(mcp_router) => {
                app = app.merge(
                    mcp_router.layer(
                        tower::ServiceBuilder::new()
                            .layer(layers_before_auth.clone())
                            .layer(nexus_only_auth_layer.clone())
                            .layer(layers_after_auth.clone()),
                    ),
                );
                mcp_actually_exposed = true;
            }
            Err(e) => {
                log::error!("Failed to initialize MCP router: {e}");
            }
        }
    }

    if config.llm.enabled && config.llm.proxy.anthropic.enabled {
        proxy_actually_exposed = true;
        app = app.merge(
            llm::proxy::anthropic::router(&config.llm.proxy.anthropic.path).layer(
                tower::ServiceBuilder::new()
                    .layer(layers_before_auth.clone())
                    .layer(AuthLayer::new_with_native_provider(
                        config.server.oauth.clone(),
                        |parts: &http::request::Parts| Authentication {
                            has_anthropic_authorization: parts.headers.contains_key(http::header::AUTHORIZATION),
                            ..Default::default()
                        },
                    ))
                    .layer(layers_after_auth.clone()),
            ),
        );
        log::info!(
            "Anthropic proxy endpoint: http://{listen_address}{}",
            config.llm.proxy.anthropic.path
        );
    }

    // Only expose LLM endpoint if enabled AND has configured providers
    if config.llm.enabled && config.llm.has_providers() {
        let server = llm::build_server(&config).await.map_err(|err| {
            log::error!("Failed to initialize LLM router: {err:?}");
            anyhow!("Failed to initialize LLM router: {err}")
        })?;

        if config.llm.protocols.openai.enabled {
            app = app.nest(
                &config.llm.protocols.openai.path,
                llm::openai_endpoint_router().with_state(server.clone()).layer(
                    tower::ServiceBuilder::new()
                        .layer(layers_before_auth.clone())
                        .layer(nexus_only_auth_layer.clone())
                        .layer(layers_after_auth.clone()),
                ),
            );
            llm_actually_exposed = true;
        }

        if config.llm.protocols.anthropic.enabled {
            app = app.nest(
                &config.llm.protocols.anthropic.path,
                llm::anthropic_endpoint_router().with_state(server.clone()).layer(
                    tower::ServiceBuilder::new()
                        .layer(layers_before_auth.clone())
                        .layer(AuthLayer::new_with_native_provider(
                            config.server.oauth.clone(),
                            |parts: &http::request::Parts| Authentication {
                                has_anthropic_authorization: parts.headers.contains_key(http::header::AUTHORIZATION),
                                ..Default::default()
                            },
                        ))
                        .layer(layers_after_auth.clone()),
                ),
            );
            llm_actually_exposed = true;
        }
    } else {
        log::debug!("LLM is enabled but no providers are configured - LLM endpoint will not be exposed");
    }

    // Apply OAuth authentication to protected routes
    // This runs BEFORE client identification (due to layer ordering) so JWT is available
    if let Some(config) = &config.server.oauth {
        // Add OAuth metadata endpoint (this should be public, not protected)
        let response = well_known::oauth_metadata(config);
        app = app.route(
            "/.well-known/oauth-protected-resource",
            get(async move || response.clone()),
        );
    }

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
                // We shouldn't have one IMHO, but all the tests rely on this right now...
                .layer(csrf)
                .layer(cors);

            app = app.merge(health_router);
        }
    }

    let listener = TcpListener::bind(listen_address)
        .await
        .map_err(|e| anyhow!("Failed to bind to {listen_address}: {e}"))?;

    if let Some(sender) = bound_addr_sender {
        sender
            .send(listener.local_addr()?)
            .expect("Failed to send back bound address.");
    }

    // Check what endpoints are actually exposed
    if !mcp_actually_exposed && !llm_actually_exposed && !proxy_actually_exposed {
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
                log::info!("MCP endpoint: https://{listen_address}{}", config.mcp.path);
            }

            if llm_actually_exposed {
                if config.llm.protocols.openai.enabled {
                    log::info!(
                        "OpenAI LLM endpoint: https://{listen_address}{}",
                        config.llm.protocols.openai.path
                    );
                }
                if config.llm.protocols.anthropic.enabled {
                    log::info!(
                        "Anthropic LLM endpoint: https://{listen_address}{}",
                        config.llm.protocols.anthropic.path
                    );
                }
            }

            let server = axum_server::from_tcp_rustls(listener.into_std()?, rustls_config)
                .serve(app.into_make_service_with_connect_info::<SocketAddr>());

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
                log::info!("MCP endpoint: http://{listen_address}{}", config.mcp.path);
            }

            if llm_actually_exposed {
                if config.llm.protocols.openai.enabled {
                    log::info!(
                        "OpenAI LLM endpoint: http://{listen_address}{}",
                        config.llm.protocols.openai.path
                    );
                }
                if config.llm.protocols.anthropic.enabled {
                    log::info!(
                        "Anthropic LLM endpoint: http://{listen_address}{}",
                        config.llm.protocols.anthropic.path
                    );
                }
            }

            // Run with graceful shutdown
            tokio::select! {
                result = axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()) => {
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
    // Don't let telemetry code log during initialization to avoid recursion
    match telemetry::init(&config.telemetry).await {
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
}
