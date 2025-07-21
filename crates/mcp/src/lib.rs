//! MCP (Multi-Client Protocol) library for routing HTTP requests to multiple MCP backends.

#![deny(missing_docs)]

mod downstream;
mod index;
mod server;
mod tool;

use std::{sync::Arc, time::Duration};

use axum::{Router, http::StatusCode, routing};
use config::McpConfig;
use rmcp::transport::{
    StreamableHttpServerConfig, StreamableHttpService, streamable_http_server::session::never::NeverSessionManager,
};

/// Creates an axum router for MCP.
pub async fn router(config: &McpConfig) -> anyhow::Result<Router> {
    log::info!("Creating MCP router for path: {}", config.path);
    let mcp_server = server::McpServer::new(config).await?;

    let service = StreamableHttpService::new(
        move || Ok(mcp_server.clone()),
        Arc::new(NeverSessionManager::default()),
        StreamableHttpServerConfig {
            sse_keep_alive: Some(Duration::from_secs(5)),
            stateful_mode: false,
        },
    );

    // Handler for OPTIONS requests
    async fn handle_options() -> StatusCode {
        log::info!("Handling OPTIONS request for MCP");
        StatusCode::OK
    }

    // Use method routing to explicitly handle OPTIONS
    Ok(Router::new().route(
        &config.path,
        routing::get_service(service.clone())
            .post_service(service.clone())
            .delete_service(service)
            .options(handle_options),
    ))
}
