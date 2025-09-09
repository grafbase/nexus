//! Redis-based rate limit storage using the averaging fixed window algorithm.

pub(crate) mod tracing;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use redis::Script;
use telemetry::metrics::{
    REDIS_COMMAND_DURATION, REDIS_POOL_CONNECTIONS_AVAILABLE, REDIS_POOL_CONNECTIONS_IN_USE, Recorder,
};

use super::redis_pool::{Pool, create_pool};
use super::{RateLimitContext, RateLimitResult, RateLimitStorage, StorageError, TokenRateLimitContext};
use config::RedisConfig;

/// Lua script for atomic rate limit check and increment.
/// This script implements the averaging fixed window algorithm atomically.
const RATE_LIMIT_SCRIPT: &str = include_str!("redis/rate_limit.lua");

/// Lua script for atomic multi-token rate limit check and increment.
/// This script consumes multiple tokens at once in the averaging fixed window algorithm.
const RATE_LIMIT_TOKENS_SCRIPT: &str = include_str!("redis/rate_limit_tokens.lua");

/// Redis-based rate limit storage implementation.
pub struct RedisStorage {
    /// Redis connection pool.
    pool: Pool,
    /// Key prefix for all rate limit keys.
    key_prefix: String,
    /// Response timeout for Redis commands.
    #[allow(dead_code)] // Will be used for timeouts later
    response_timeout: Duration,
    rate_limit_script: Script,
    rate_limit_tokens_script: Script,
    /// Metrics gauges for connection pool monitoring
    connections_in_use_gauge: opentelemetry::metrics::Gauge<u64>,
    connections_available_gauge: opentelemetry::metrics::Gauge<u64>,
}

impl RedisStorage {
    /// Get the connection pool status.
    pub(crate) fn pool_status(&self) -> deadpool::Status {
        self.pool.status()
    }

    /// Create a new Redis storage instance.
    pub async fn new(config: &RedisConfig) -> Result<Self, StorageError> {
        // Create the connection pool
        let pool = create_pool(config)
            .map_err(|e| StorageError::Connection(format!("Failed to create Redis connection pool: {e}")))?;

        // Test the connection
        let mut conn = pool
            .get()
            .await
            .map_err(|e| StorageError::Connection(format!("Failed to get Redis connection from pool: {e}")))?;

        let _: String = redis::cmd("PING")
            .query_async(&mut *conn)
            .await
            .map_err(|e| StorageError::Connection(format!("Failed to ping Redis server: {e}")))?;

        // Use Lua script for atomic check-and-increment
        let rate_limit_script = Script::new(RATE_LIMIT_SCRIPT);
        let rate_limit_tokens_script = Script::new(RATE_LIMIT_TOKENS_SCRIPT);

        // Initialize metrics gauges
        let connections_in_use_gauge = telemetry::metrics::meter()
            .u64_gauge(REDIS_POOL_CONNECTIONS_IN_USE)
            .build();
        let connections_available_gauge = telemetry::metrics::meter()
            .u64_gauge(REDIS_POOL_CONNECTIONS_AVAILABLE)
            .build();

        Ok(Self {
            pool,
            key_prefix: config
                .key_prefix
                .clone()
                .unwrap_or_else(|| "nexus:rate_limit:".to_string()),
            response_timeout: config.response_timeout.unwrap_or_else(|| Duration::from_secs(1)),
            rate_limit_script,
            rate_limit_tokens_script,
            connections_in_use_gauge,
            connections_available_gauge,
        })
    }

    /// Generate Redis keys for the current and previous time windows.
    fn generate_keys(&self, key: &str, interval: Duration) -> (String, String, u64, f64) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let window_size = interval.as_secs();
        let current_bucket = now / window_size;
        let previous_bucket = current_bucket - 1;

        // Calculate how far we are into the current window (0.0 to 1.0)
        let bucket_percentage = (now % window_size) as f64 / window_size as f64;

        let current_key = format!("{}{}:{current_bucket}", self.key_prefix, key);
        let previous_key = format!("{}{}:{previous_bucket}", self.key_prefix, key);

        (current_key, previous_key, window_size, bucket_percentage)
    }

    /// Record pool metrics (connections in use and available)
    fn record_pool_metrics(&self) {
        let status = self.pool.status();

        // Record connections in use
        self.connections_in_use_gauge
            .record(status.size as u64 - status.available as u64, &[]);

        // Record connections available
        self.connections_available_gauge.record(status.available as u64, &[]);
    }
}

impl RateLimitStorage for RedisStorage {
    async fn check_and_consume(
        &self,
        context: &RateLimitContext<'_>,
        limit: u32,
        interval: Duration,
    ) -> Result<RateLimitResult, StorageError> {
        // Create the storage key for tracking rate limits
        let key = match context {
            RateLimitContext::Global => "global".to_string(),
            RateLimitContext::PerIp { ip } => format!("ip:{ip}"),
            RateLimitContext::PerServer { server } => format!("server:{server}"),
            RateLimitContext::PerTool { server, tool } => format!("server:{server}:tool:{tool}"),
        };

        let (current_key, previous_key, window_size, bucket_percentage) = self.generate_keys(&key, interval);

        // Get connection from pool
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| StorageError::Connection(e.to_string()))?;

        // Record pool metrics
        self.record_pool_metrics();

        let expire_time = window_size * 2; // Keep keys for 2 windows

        // Execute Lua script with metrics
        let mut cmd_recorder = Recorder::new(REDIS_COMMAND_DURATION);
        cmd_recorder.push_attribute("operation", "check_and_consume");

        let result: Vec<i64> = match self
            .rate_limit_script
            .key(&current_key)
            .key(&previous_key)
            .arg(limit)
            .arg(window_size)
            .arg(expire_time)
            .arg(bucket_percentage)
            .invoke_async(&mut *conn)
            .await
        {
            Ok(result) => {
                cmd_recorder.push_attribute("status", "success");
                cmd_recorder.record();
                result
            }
            Err(e) => {
                cmd_recorder.push_attribute("status", "error");
                cmd_recorder.push_attribute("error.type", "script_execution");
                cmd_recorder.record();
                return Err(StorageError::Query(format!("Rate limit script failed: {e}")));
            }
        };

        let allowed = result[0] == 1;

        if allowed {
            Ok(RateLimitResult {
                allowed: true,
                retry_after: None,
            })
        } else {
            // Calculate retry_after based on when the current window ends
            let now_secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            let window_end = ((now_secs / window_size) + 1) * window_size;
            let retry_after = Duration::from_secs(window_end - now_secs);

            Ok(RateLimitResult {
                allowed: false,
                retry_after: Some(retry_after),
            })
        }
    }

    async fn check_and_consume_tokens(
        &self,
        context: &TokenRateLimitContext<'_>,
        tokens: u32,
        limit: u32,
        interval: Duration,
    ) -> Result<RateLimitResult, StorageError> {
        // Create the storage key for tracking individual client rate limits
        let key = format!(
            "token:{}:{}:{}:{}",
            context.client_id,
            context.group.unwrap_or("default"),
            context.provider,
            context.model.unwrap_or("default")
        );

        let (current_key, previous_key, window_size, bucket_percentage) = self.generate_keys(&key, interval);

        // Get connection from pool
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| StorageError::Connection(e.to_string()))?;

        // Record pool metrics
        self.record_pool_metrics();

        let expire_time = window_size * 2; // Keep keys for 2 windows

        // Execute token Lua script with metrics
        let mut cmd_recorder = Recorder::new(REDIS_COMMAND_DURATION);
        cmd_recorder.push_attribute("operation", "check_and_consume_tokens");
        cmd_recorder.push_attribute("tokens", tokens as i64);

        let result: Vec<i64> = match self
            .rate_limit_tokens_script
            .key(&current_key)
            .key(&previous_key)
            .arg(tokens)
            .arg(limit)
            .arg(window_size)
            .arg(expire_time)
            .arg(bucket_percentage)
            .invoke_async(&mut *conn)
            .await
        {
            Ok(result) => {
                cmd_recorder.push_attribute("status", "success");
                cmd_recorder.record();
                result
            }
            Err(e) => {
                cmd_recorder.push_attribute("status", "error");
                cmd_recorder.push_attribute("error.type", "script_execution");
                cmd_recorder.record();
                return Err(StorageError::Query(format!("Token rate limit script failed: {e}")));
            }
        };

        let allowed = result[0] == 1;

        if allowed {
            log::debug!("Successfully consumed {} tokens within rate limit", tokens);

            Ok(RateLimitResult {
                allowed: true,
                retry_after: None,
            })
        } else {
            log::debug!("Token rate limit exceeded - cannot consume {} tokens", tokens);

            // Calculate retry_after based on when the current window ends
            let now_secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            let window_end = ((now_secs / window_size) + 1) * window_size;
            let retry_after = Duration::from_secs(window_end - now_secs);

            Ok(RateLimitResult {
                allowed: false,
                retry_after: Some(retry_after),
            })
        }
    }
}
