//! MCP (Multi-Client Protocol) library for routing HTTP requests to multiple MCP backends.

#![deny(missing_docs)]

mod downstream;
mod index;
mod server;
mod tool;

use std::{sync::Arc, time::Duration};

use axum::{Router, routing};
use config::McpConfig;
use rmcp::{
    model::ProtocolVersion,
    transport::{
        StreamableHttpServerConfig, StreamableHttpService, streamable_http_server::session::never::NeverSessionManager,
    },
};

pub(crate) const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion::V_2025_03_26;

/// Creates an axum router for MCP.
pub async fn router(config: &McpConfig) -> anyhow::Result<Router> {
    let mcp_server = server::McpServer::new(config).await?;

    let service = StreamableHttpService::new(
        move || Ok(mcp_server.clone()),
        Arc::new(NeverSessionManager::default()),
        StreamableHttpServerConfig {
            sse_keep_alive: Some(Duration::from_secs(5)),
            stateful_mode: false,
        },
    );

    Ok(Router::new().route(&config.path, routing::any_service(service)))
}
