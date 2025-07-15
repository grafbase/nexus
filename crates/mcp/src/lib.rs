//! MCP (Multi-Client Protocol) library for routing HTTP requests to multiple MCP backends.

#![deny(missing_docs)]

mod server;
mod tool;

use std::{sync::Arc, time::Duration};

use axum::Router;
use config::McpConfig;
use rmcp::transport::{
    StreamableHttpServerConfig, StreamableHttpService, streamable_http_server::session::never::NeverSessionManager,
};

/// Creates an axum router for MCP.
pub fn router(config: &McpConfig) -> anyhow::Result<Router> {
    let mcp_server = server::McpServer::new()?;

    let service = StreamableHttpService::new(
        move || Ok(mcp_server.clone()),
        Arc::new(NeverSessionManager::default()),
        StreamableHttpServerConfig {
            sse_keep_alive: Some(Duration::from_secs(5)),
            stateful_mode: false,
        },
    );

    Ok(Router::new().route_service(&config.path, service))
}
