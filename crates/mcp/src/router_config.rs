//! Configuration for the MCP router.

use std::sync::Arc;

/// Configuration for creating an MCP router.
pub struct RouterConfig {
    /// The main configuration.
    pub config: config::Config,
    /// Optional rate limit manager.
    pub rate_limit_manager: Option<Arc<rate_limit::RateLimitManager>>,
}

impl RouterConfig {
    /// Create a new router configuration builder.
    pub fn builder(config: config::Config) -> RouterConfigBuilder {
        RouterConfigBuilder {
            config,
            rate_limit_manager: None,
        }
    }
}

/// Builder for creating router configuration.
pub struct RouterConfigBuilder {
    config: config::Config,
    rate_limit_manager: Option<Arc<rate_limit::RateLimitManager>>,
}

impl RouterConfigBuilder {
    /// Set the rate limit manager.
    pub fn rate_limit_manager(mut self, manager: Arc<rate_limit::RateLimitManager>) -> Self {
        self.rate_limit_manager = Some(manager);
        self
    }
    
    /// Build the router configuration.
    pub fn build(self) -> RouterConfig {
        RouterConfig {
            config: self.config,
            rate_limit_manager: self.rate_limit_manager,
        }
    }
}