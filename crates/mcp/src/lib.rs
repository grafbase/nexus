//! MCP (Multi-Client Protocol) library for routing HTTP requests to multiple MCP backends.

#![deny(missing_docs)]

mod cache;
mod config;
mod downstream;
mod index;
mod server;

use std::{sync::Arc, time::Duration};

use axum::{Router, routing};
use rmcp::{
    model::ProtocolVersion,
    transport::{
        StreamableHttpServerConfig, StreamableHttpService, streamable_http_server::session::never::NeverSessionManager,
    },
};
use server::McpServer;

pub use config::{RouterConfig, RouterConfigBuilder};

pub(crate) const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion::V_2025_06_18;

/// Creates an axum router for MCP.
pub async fn router(
    RouterConfig {
        config,
        rate_limit_manager,
    }: RouterConfig,
) -> anyhow::Result<Router> {
    let path = config.mcp.path.clone();
    let mut builder = McpServer::builder(config);

    if let Some(manager) = rate_limit_manager {
        builder = builder.rate_limit_manager(manager);
    }

    // Builder now handles metrics middleware decision
    let handler = builder.build().await?;

    let service = StreamableHttpService::new(
        move || Ok(handler.clone()),
        Arc::new(NeverSessionManager::default()),
        StreamableHttpServerConfig {
            sse_keep_alive: Some(Duration::from_secs(5)),
            stateful_mode: false,
        },
    );

    Ok(Router::new().route(&path, routing::any_service(service)))
}
