//! Rate limit manager implementation.

use std::sync::Arc;

use config::{McpConfig, RateLimitConfig, StorageConfig};

use crate::error::RateLimitError;
use crate::request::RateLimitRequest;
use crate::storage::{InMemoryStorage, RateLimitResult, RateLimitStorage, StorageError};

/// Storage backend for rate limiting.
enum Storage {
    Memory(InMemoryStorage),
    Redis(crate::storage::redis::RedisStorage),
}

impl Storage {
    async fn check_and_consume(
        &self,
        key: &str,
        limit: u32,
        duration: std::time::Duration,
    ) -> Result<RateLimitResult, StorageError> {
        match self {
            Storage::Memory(storage) => storage.check_and_consume(key, limit, duration).await,
            Storage::Redis(storage) => storage.check_and_consume(key, limit, duration).await,
        }
    }
}

/// Manager for rate limiting with support for multiple limit types.
pub struct RateLimitManager {
    inner: Arc<RateLimitInner>,
}

pub struct RateLimitInner {
    config: RateLimitConfig,
    mcp_config: McpConfig,
    storage: Storage,
}

impl RateLimitManager {
    /// Create a new rate limit manager with configured storage backend.
    pub async fn new(config: RateLimitConfig, mcp_config: McpConfig) -> Result<Self, RateLimitError> {
        let storage = match &config.storage {
            StorageConfig::Memory => Storage::Memory(InMemoryStorage::new()),
            StorageConfig::Redis(redis_config) => {
                use crate::storage::redis::RedisStorage;
                let redis_storage = RedisStorage::new(redis_config).await.map_err(RateLimitError::Storage)?;
                Storage::Redis(redis_storage)
            }
        };

        let inner = Arc::new(RateLimitInner {
            config,
            mcp_config,
            storage,
        });

        Ok(Self { inner })
    }

    /// Check all applicable rate limits for a request.
    ///
    /// This checks in order: global, per-IP, per-server, per-tool.
    /// Returns an error with the first limit that is exceeded.
    pub async fn check_request(&self, request: &RateLimitRequest) -> Result<(), RateLimitError> {
        if !self.inner.config.enabled {
            return Ok(());
        }

        self.check_global_limit().await?;
        self.check_ip_limit(request).await?;
        self.check_server_tool_limit(request).await?;

        Ok(())
    }

    async fn check_global_limit(&self) -> Result<(), RateLimitError> {
        let Some(quota) = &self.inner.config.global else {
            return Ok(());
        };

        let result = self
            .inner
            .storage
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

        let Some(quota) = &self.inner.config.per_ip else {
            return Ok(());
        };

        let key = format!("ip:{ip}");

        let result = self
            .inner
            .storage
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
            log::debug!("No server name provided in request - skipping server-specific rate limits");
            return Ok(());
        };

        let Some(server) = self.inner.mcp_config.servers.get(server_name) else {
            log::debug!("Server '{server_name}' not found in configuration - skipping rate limit check");
            return Ok(());
        };

        let Some(rate_limit) = server.rate_limit() else {
            log::debug!("Rate limiting not configured for server '{server_name}' - allowing request");
            return Ok(());
        };

        log::debug!(
            "Found rate limit configuration for server '{server_name}': {}/{:?}",
            rate_limit.limit,
            rate_limit.duration
        );

        // Determine which limit to use
        let (limit, duration, key) = match &request.tool_name {
            Some(tool_name) => {
                let quota = rate_limit
                    .tools
                    .get(tool_name)
                    .map(|q| (q.limit, q.duration))
                    .unwrap_or((rate_limit.limit, rate_limit.duration));

                (quota.0, quota.1, format!("server:{server_name}:tool:{tool_name}"))
            }
            None => (rate_limit.limit, rate_limit.duration, format!("server:{server_name}")),
        };

        log::debug!("Evaluating rate limit: key='{key}', quota={limit} requests per {duration:?}");

        let result = self.inner.storage.check_and_consume(&key, limit, duration).await?;

        log::debug!(
            "Rate limit decision: {} (retry after: {:?})",
            if result.allowed { "ALLOWED" } else { "BLOCKED" },
            result.retry_after
        );

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
