//! Rate limit manager implementation.

use std::sync::Arc;

use config::{McpConfig, RateLimitConfig};

use crate::error::RateLimitError;
use crate::request::RateLimitRequest;
use crate::storage::{InMemoryStorage, RateLimitStorage};

/// Manager for rate limiting with support for multiple limit types.
pub struct RateLimitManager {
    /// Rate limit configuration.
    config: Arc<RateLimitConfig>,
    /// MCP configuration for server lookups.
    mcp_config: Arc<McpConfig>,
    /// Storage backend.
    storage: InMemoryStorage,
}

impl RateLimitManager {
    /// Create a new rate limit manager with in-memory storage.
    pub fn new(config: RateLimitConfig, mcp_config: McpConfig) -> Self {
        Self {
            config: Arc::new(config),
            mcp_config: Arc::new(mcp_config),
            storage: InMemoryStorage::new(),
        }
    }
    
    /// Check if rate limiting is enabled (either server-level or MCP-level).
    pub fn is_enabled(&self) -> bool {
        // Server-level rate limiting is enabled
        if self.config.enabled {
            return true;
        }
        
        // Check if any MCP servers have rate limits configured
        self.mcp_config.servers.values().any(|server| server.rate_limit().is_some())
    }
    
    /// Check all applicable rate limits for a request.
    ///
    /// This checks in order: global, per-IP, per-server, per-tool.
    /// Returns an error with the first limit that is exceeded.
    pub async fn check_request(&self, request: &RateLimitRequest) -> Result<(), RateLimitError> {
        if !self.is_enabled() {
            return Ok(());
        }
        
        self.check_global_limit().await?;
        self.check_ip_limit(request).await?;
        self.check_server_tool_limit(request).await?;
        
        Ok(())
    }
    
    async fn check_global_limit(&self) -> Result<(), RateLimitError> {
        let Some(quota) = &self.config.global else {
            return Ok(());
        };
        
        let result = self.storage
            .check_and_consume("global", quota.limit, quota.duration)
            .await?;
            
        if !result.allowed {
            return Err(RateLimitError::GlobalLimitExceeded {
                retry_after: result.retry_after.unwrap_or_default(),
            });
        }
        
        Ok(())
    }
    
    async fn check_ip_limit(&self, request: &RateLimitRequest) -> Result<(), RateLimitError> {
        let Some(ip) = request.ip else {
            return Ok(());
        };
        
        let Some(quota) = &self.config.per_ip else {
            return Ok(());
        };
        
        let key = format!("ip:{ip}");
        let result = self.storage
            .check_and_consume(&key, quota.limit, quota.duration)
            .await?;
            
        if !result.allowed {
            return Err(RateLimitError::IpLimitExceeded {
                retry_after: result.retry_after.unwrap_or_default(),
            });
        }
        
        Ok(())
    }
    
    async fn check_server_tool_limit(&self, request: &RateLimitRequest) -> Result<(), RateLimitError> {
        let Some(server_name) = &request.server_name else {
            log::debug!("No server name in request - skipping server/tool rate limit");
            return Ok(());
        };
        
        let Some(server) = self.mcp_config.servers.get(server_name) else {
            log::debug!("Server {server_name} not found in config - skipping rate limit");
            return Ok(());
        };
        
        let Some(rate_limit) = server.rate_limit() else {
            log::debug!("No rate limit configured for server {server_name} - skipping");
            return Ok(());
        };
        
        log::debug!("Found rate limit config for server {server_name}: limit={}, duration={:?}", 
            rate_limit.limit, rate_limit.duration);
        
        // Determine which limit to use
        let (limit, duration, key) = match &request.tool_name {
            Some(tool_name) => {
                let quota = rate_limit.tools
                    .get(tool_name)
                    .map(|q| (q.limit, q.duration))
                    .unwrap_or((rate_limit.limit, rate_limit.duration));
                    
                (quota.0, quota.1, format!("server:{server_name}:tool:{tool_name}"))
            }
            None => (rate_limit.limit, rate_limit.duration, format!("server:{server_name}"))
        };
        
        log::debug!("Checking rate limit with key={key}, limit={limit}, duration={duration:?}");
            
        let result = self.storage
            .check_and_consume(&key, limit, duration)
            .await?;
            
        log::debug!("Rate limit result: allowed={}, retry_after={:?}", 
            result.allowed, result.retry_after);
            
        if !result.allowed {
            match &request.tool_name {
                Some(tool_name) => Err(RateLimitError::ToolLimitExceeded {
                    server: server_name.to_string(),
                    tool: tool_name.to_string(),
                    retry_after: result.retry_after.unwrap_or_default(),
                }),
                None => Err(RateLimitError::ServerLimitExceeded {
                    server: server_name.to_string(),
                    retry_after: result.retry_after.unwrap_or_default(),
                }),
            }
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::RateLimitRequest;
    use std::time::Duration;
    
    fn create_test_config(enabled: bool) -> (RateLimitConfig, McpConfig) {
        let rate_config = RateLimitConfig {
            enabled,
            global: Some(config::RateLimitQuota {
                limit: 100,
                duration: Duration::from_secs(60),
            }),
            per_ip: Some(config::RateLimitQuota {
                limit: 10,
                duration: Duration::from_secs(60),
            }),
        };
        
        let mcp_config = McpConfig::default();
        
        (rate_config, mcp_config)
    }
    
    #[tokio::test]
    async fn test_rate_limiting_disabled() {
        let (mut config, mcp_config) = create_test_config(false);
        config.enabled = false;
        
        let manager = RateLimitManager::new(config, mcp_config);
        
        // Should always allow when disabled
        for _ in 0..200 {
            let request = RateLimitRequest::builder()
                .ip_str("127.0.0.1").unwrap()
                .build();
            let result = manager.check_request(&request).await;
            assert!(result.is_ok());
        }
    }
    
    #[tokio::test]
    async fn test_global_rate_limit() {
        let (config, mcp_config) = create_test_config(true);
        let manager = RateLimitManager::new(config, mcp_config);
        
        // Global limit is 100 per minute, but with burst allowance
        // The exact number depends on the burst calculation
        let mut allowed_count = 0;
        let request = RateLimitRequest::builder().build();
        for _ in 0..150 {
            if manager.check_request(&request).await.is_ok() {
                allowed_count += 1;
            }
        }
        
        // Should allow some requests but not all
        assert!(allowed_count > 0);
        assert!(allowed_count < 150);
    }
    
    #[tokio::test]
    async fn test_per_ip_rate_limit() {
        let (config, mcp_config) = create_test_config(true);
        let manager = RateLimitManager::new(config, mcp_config);
        
        let request1 = RateLimitRequest::builder()
            .ip_str("127.0.0.1").unwrap()
            .build();
        let request2 = RateLimitRequest::builder()
            .ip_str("127.0.0.2").unwrap()
            .build();
        
        // Each IP should have its own limit
        let mut ip1_allowed = 0;
        let mut ip2_allowed = 0;
        
        for _ in 0..20 {
            if manager.check_request(&request1).await.is_ok() {
                ip1_allowed += 1;
            }
            if manager.check_request(&request2).await.is_ok() {
                ip2_allowed += 1;
            }
        }
        
        // Both IPs should hit their limits independently
        assert!(ip1_allowed > 0 && ip1_allowed < 20);
        assert!(ip2_allowed > 0 && ip2_allowed < 20);
    }
}