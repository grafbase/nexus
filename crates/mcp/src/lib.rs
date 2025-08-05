//! MCP (Multi-Client Protocol) library for routing HTTP requests to multiple MCP backends.

#![deny(missing_docs)]

mod cache;
mod config;
mod downstream;
mod index;
mod server;
mod server_builder;

use std::{sync::Arc, time::Duration};

use axum::{Router, routing};
use rmcp::{
    model::ProtocolVersion,
    transport::{
        StreamableHttpServerConfig, StreamableHttpService, streamable_http_server::session::never::NeverSessionManager,
    },
};

pub use config::{RouterConfig, RouterConfigBuilder};

pub(crate) const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion::V_2025_03_26;

/// Creates an axum router for MCP.
pub async fn router(
    RouterConfig {
        config,
        rate_limit_manager,
    }: RouterConfig,
) -> anyhow::Result<Router> {
    let mut builder = server::McpServer::builder(config.clone());

    if let Some(manager) = rate_limit_manager {
        builder = builder.rate_limit_manager(manager);
    }

    let mcp_server = builder.build().await?;

    let service = StreamableHttpService::new(
        move || Ok(mcp_server.clone()),
        Arc::new(NeverSessionManager::default()),
        StreamableHttpServerConfig {
            sse_keep_alive: Some(Duration::from_secs(5)),
            stateful_mode: false,
        },
    );

    Ok(Router::new().route(&config.mcp.path, routing::any_service(service)))
}
