//! Builder for the MCP server.

use super::{McpServer, handler::McpHandler, metrics::MetricsMiddleware, tracing::TracingMiddleware};
use rate_limit::RateLimitManager;
use std::sync::Arc;

/// Builder for creating an MCP server handler.
pub(crate) struct McpServerBuilder {
    pub(crate) config: config::Config,
    pub(crate) rate_limit_manager: Option<Arc<RateLimitManager>>,
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
    pub fn rate_limit_manager(mut self, manager: Arc<RateLimitManager>) -> Self {
        self.rate_limit_manager = Some(manager);
        self
    }

    /// Build the MCP handler with optional telemetry middleware.
    pub(crate) async fn build(self) -> anyhow::Result<McpHandler> {
        // Check if metrics are enabled
        let metrics_enabled = self.config.telemetry.metrics_otlp_config().is_some();

        // Check if tracing is enabled
        let tracing_enabled = self.config.telemetry.traces_otlp_config().is_some();

        let mcp_config = self.config.mcp.clone();
        let mcp_server = McpServer::new(self).await?;

        // Build the middleware pipeline using the enum
        log::debug!(
            "Building MCP handler - tracing: {}, metrics: {}",
            tracing_enabled,
            metrics_enabled
        );
        let handler = match (tracing_enabled, metrics_enabled) {
            (true, true) => {
                // Both tracing and metrics: tracing -> metrics -> server
                log::debug!("Creating MCP handler with full telemetry (tracing + metrics)");
                McpHandler::WithFullTelemetry(TracingMiddleware::new(MetricsMiddleware::new(mcp_server), mcp_config))
            }
            (true, false) => {
                // Only tracing
                log::debug!("Creating MCP handler with tracing only");
                McpHandler::WithTracingOnly(TracingMiddleware::new(mcp_server, mcp_config))
            }
            (false, true) => {
                // Only metrics
                log::debug!("Creating MCP handler with metrics only");
                McpHandler::WithMetricsOnly(MetricsMiddleware::new(mcp_server))
            }
            (false, false) => {
                // No telemetry
                log::debug!("Creating MCP handler without telemetry");
                McpHandler::WithoutTelemetry(mcp_server)
            }
        };

        Ok(handler)
    }
}
