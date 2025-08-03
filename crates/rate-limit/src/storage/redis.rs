//! Redis-based rate limit storage using the averaging fixed window algorithm.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use redis::Script;

use super::{RateLimitResult, RateLimitStorage, StorageError};
use super::redis_pool::{Pool, create_pool};
use config::RedisConfig;

/// Lua script for atomic rate limit check and increment.
/// This script implements the averaging fixed window algorithm atomically.
const RATE_LIMIT_SCRIPT: &str = r#"
    local current_key = KEYS[1]
    local previous_key = KEYS[2]
    local limit = tonumber(ARGV[1])
    local current_window = tonumber(ARGV[2])
    local expire_time = tonumber(ARGV[3])
    local bucket_percentage = tonumber(ARGV[4])
    
    -- Get counts from both windows
    local current_count = tonumber(redis.call('GET', current_key) or '0')
    local previous_count = tonumber(redis.call('GET', previous_key) or '0')
    
    -- Calculate weighted count
    local weighted_count = previous_count * (1.0 - bucket_percentage) + current_count
    
    -- Check if limit would be exceeded
    if weighted_count >= limit then
        return {0, current_count, previous_count}  -- Not allowed
    end
    
    -- Increment current window
    current_count = redis.call('INCR', current_key)
    redis.call('EXPIRE', current_key, expire_time)
    
    return {1, current_count, previous_count}  -- Allowed
"#;

/// Redis-based rate limit storage implementation.
pub struct RedisStorage {
    /// Redis connection pool.
    pool: Pool,
    /// Key prefix for all rate limit keys.
    key_prefix: String,
    /// Response timeout for Redis commands.
    #[allow(dead_code)] // Will be used for timeouts later
    response_timeout: Duration,
}

impl RedisStorage {
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

        Ok(Self {
            pool,
            key_prefix: config
                .key_prefix
                .clone()
                .unwrap_or_else(|| "nexus:rate_limit:".to_string()),
            response_timeout: config.response_timeout.unwrap_or_else(|| Duration::from_secs(1)),
        })
    }

    /// Generate Redis keys for the current and previous time windows.
    fn generate_keys(&self, key: &str, duration: Duration) -> (String, String, u64, f64) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let window_size = duration.as_secs();
        let current_bucket = now / window_size;
        let previous_bucket = current_bucket - 1;

        // Calculate how far we are into the current window (0.0 to 1.0)
        let bucket_percentage = (now % window_size) as f64 / window_size as f64;

        let current_key = format!("{}{}:{current_bucket}", self.key_prefix, key);
        let previous_key = format!("{}{}:{previous_bucket}", self.key_prefix, key);

        (current_key, previous_key, window_size, bucket_percentage)
    }
}

impl RateLimitStorage for RedisStorage {
    async fn check_and_consume(
        &self,
        key: &str,
        limit: u32,
        duration: Duration,
    ) -> Result<RateLimitResult, StorageError> {
        let (current_key, previous_key, window_size, bucket_percentage) = self.generate_keys(key, duration);

        // Get connection from pool
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| StorageError::Connection(e.to_string()))?;

        // Use Lua script for atomic check-and-increment
        let script = Script::new(RATE_LIMIT_SCRIPT);
        let expire_time = window_size * 2; // Keep keys for 2 windows
        
        let result: Vec<i64> = script
            .key(&current_key)
            .key(&previous_key)
            .arg(limit)
            .arg(window_size)
            .arg(expire_time)
            .arg(bucket_percentage)
            .invoke_async(&mut *conn)
            .await
            .map_err(|e| StorageError::Query(format!("Rate limit script failed: {e}")))?;

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
}

