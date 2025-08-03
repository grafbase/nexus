//! Builder for the MCP server.

use crate::server::McpServer;
use std::sync::Arc;

/// Builder for creating an MCP server.
pub(crate) struct McpServerBuilder {
    pub(crate) config: config::Config,
    pub(crate) rate_limit_manager: Option<Arc<rate_limit::RateLimitManager>>,
}

impl McpServerBuilder {
    /// Create a new MCP server builder.
    pub fn new(config: config::Config) -> Self {
        Self {
            config,
            rate_limit_manager: None,
        }
    }

    /// Set the rate limit manager.
    pub fn rate_limit_manager(mut self, manager: Arc<rate_limit::RateLimitManager>) -> Self {
        self.rate_limit_manager = Some(manager);
        self
    }

    /// Build the MCP server.
    pub(crate) async fn build(self) -> anyhow::Result<McpServer> {
        McpServer::new(self).await
    }
}
