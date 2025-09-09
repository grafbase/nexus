use std::time::Duration;

use fastrace::{Span, future::FutureExt, prelude::LocalSpan};

use super::super::{RateLimitContext, RateLimitResult, RateLimitStorage, StorageError, TokenRateLimitContext};
use super::RedisStorage;

/// Wrapper that adds tracing to Redis storage operations
pub(crate) struct TracedRedisStorage {
    inner: RedisStorage,
}

impl TracedRedisStorage {
    pub fn new(inner: RedisStorage) -> Self {
        Self { inner }
    }
}

impl RateLimitStorage for TracedRedisStorage {
    async fn check_and_consume(
        &self,
        context: &RateLimitContext<'_>,
        limit: u32,
        interval: Duration,
    ) -> Result<RateLimitResult, StorageError> {
        // Create span based on context type
        let (span_name, scope) = match context {
            RateLimitContext::Global => ("redis:check_and_consume:global", "global"),
            RateLimitContext::PerIp { .. } => ("redis:check_and_consume:ip", "ip"),
            RateLimitContext::PerServer { .. } => ("redis:check_and_consume:server", "server"),
            RateLimitContext::PerTool { .. } => ("redis:check_and_consume:tool", "tool"),
        };

        let span = Span::enter_with_local_parent(span_name);

        // Add context attributes
        span.add_property(|| ("redis.operation", "check_and_consume"));
        span.add_property(|| ("rate_limit.scope", scope));
        span.add_property(|| ("rate_limit.limit", limit.to_string()));
        span.add_property(|| ("rate_limit.interval_ms", interval.as_millis().to_string()));

        // Add pool status attributes
        let pool_status = self.inner.pool_status();
        span.add_property(|| ("redis.pool.size", pool_status.size.to_string()));
        span.add_property(|| ("redis.pool.available", pool_status.available.to_string()));
        span.add_property(|| {
            (
                "redis.pool.in_use",
                (pool_status.size - pool_status.available).to_string(),
            )
        });

        // Add specific context details
        match context {
            RateLimitContext::PerIp { ip } => {
                // Hash IP for privacy
                let hashed_ip = format!("{:x}", md5::compute(ip.to_string().as_bytes()));
                span.add_property(|| ("client.address_hash", hashed_ip));
            }
            RateLimitContext::PerServer { server } => {
                span.add_property(|| ("mcp.server", server.to_string()));
            }
            RateLimitContext::PerTool { server, tool } => {
                span.add_property(|| ("mcp.server", server.to_string()));
                span.add_property(|| ("mcp.tool", tool.to_string()));
            }
            _ => {}
        }

        let fut = async move {
            let result = self.inner.check_and_consume(context, limit, interval).await;

            match &result {
                Ok(rate_limit_result) => {
                    LocalSpan::add_property(|| ("rate_limit.allowed", rate_limit_result.allowed.to_string()));
                    if let Some(retry_after) = rate_limit_result.retry_after {
                        LocalSpan::add_property(|| ("rate_limit.retry_after_ms", retry_after.as_millis().to_string()));
                    }
                }
                Err(e) => {
                    LocalSpan::add_property(|| ("error", "true"));
                    LocalSpan::add_property(|| {
                        (
                            "error.type",
                            match e {
                                StorageError::Connection(_) => "connection_error",
                                StorageError::Query(_) => "query_error",
                                StorageError::Internal(_) => "internal_error",
                            },
                        )
                    });
                }
            }

            result
        };

        fut.in_span(span).await
    }

    async fn check_and_consume_tokens(
        &self,
        context: &TokenRateLimitContext<'_>,
        tokens: u32,
        limit: u32,
        interval: Duration,
    ) -> Result<RateLimitResult, StorageError> {
        let span = Span::enter_with_local_parent("redis:check_and_consume_tokens");

        // Add context attributes
        span.add_property(|| ("redis.operation", "check_and_consume_tokens"));
        span.add_property(|| ("rate_limit.scope", "token"));
        span.add_property(|| ("rate_limit.tokens", tokens.to_string()));
        span.add_property(|| ("rate_limit.limit", limit.to_string()));
        span.add_property(|| ("rate_limit.interval_ms", interval.as_millis().to_string()));

        // Add pool status attributes
        let pool_status = self.inner.pool_status();
        span.add_property(|| ("redis.pool.size", pool_status.size.to_string()));
        span.add_property(|| ("redis.pool.available", pool_status.available.to_string()));
        span.add_property(|| {
            (
                "redis.pool.in_use",
                (pool_status.size - pool_status.available).to_string(),
            )
        });

        // Add token context details
        span.add_property(|| ("llm.provider", context.provider.to_string()));

        if let Some(model) = context.model {
            span.add_property(|| ("llm.model", model.to_string()));
        }

        let fut = async move {
            let result = self
                .inner
                .check_and_consume_tokens(context, tokens, limit, interval)
                .await;

            match &result {
                Ok(rate_limit_result) => {
                    LocalSpan::add_property(|| ("rate_limit.allowed", rate_limit_result.allowed.to_string()));
                    if let Some(retry_after) = rate_limit_result.retry_after {
                        LocalSpan::add_property(|| ("rate_limit.retry_after_ms", retry_after.as_millis().to_string()));
                    }
                }
                Err(e) => {
                    LocalSpan::add_property(|| ("error", "true"));
                    LocalSpan::add_property(|| {
                        (
                            "error.type",
                            match e {
                                StorageError::Connection(_) => "connection_error",
                                StorageError::Query(_) => "query_error",
                                StorageError::Internal(_) => "internal_error",
                            },
                        )
                    });
                }
            }

            result
        };

        fut.in_span(span).await
    }
}
