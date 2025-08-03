//! Redis-based rate limit storage using the averaging fixed window algorithm.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use deadpool_redis::{Pool, Runtime};
use redis::RedisError;

use super::{RateLimitResult, RateLimitStorage, StorageError};
use config::{RedisConfig, RedisPoolConfig};

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
        // Parse Redis URL
        let mut redis_config = deadpool_redis::Config {
            url: Some(config.url.clone()),
            ..Default::default()
        };
        
        // Apply pool configuration if provided
        if let Some(pool_config) = build_pool_config(&config.pool) {
            redis_config.pool = Some(pool_config);
        }

        // Create the connection pool
        let pool = redis_config
            .create_pool(Some(Runtime::Tokio1))
            .map_err(|e| StorageError::Connection(format!("Failed to create Redis connection pool: {e}")))?;

        // Test the connection
        let mut conn = pool
            .get()
            .await
            .map_err(|e| StorageError::Connection(format!("Failed to get Redis connection from pool: {e}")))?;
        
        let _: String = redis::cmd("PING")
            .query_async(&mut conn)
            .await
            .map_err(|e| StorageError::Connection(format!("Failed to ping Redis server: {e}")))?;

        Ok(Self {
            pool,
            key_prefix: config.key_prefix.clone().unwrap_or_else(|| "nexus:rate_limit:".to_string()),
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
        
        let current_key = format!("{}{}__{}", self.key_prefix, key, current_bucket);
        let previous_key = format!("{}{}__{}", self.key_prefix, key, previous_bucket);
        
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
        let mut conn = self.pool
            .get()
            .await
            .map_err(|e| StorageError::Connection(e.to_string()))?;
        
        // Pipeline the queries for efficiency
        let (previous_count, current_count): (Option<u32>, Option<u32>) = redis::pipe()
            .get(&previous_key)
            .get(&current_key)
            .query_async(&mut conn)
            .await
            .map_err(|e: RedisError| StorageError::Query(e.to_string()))?;
        
        let previous_count = previous_count.unwrap_or(0) as f64;
        let current_count = current_count.unwrap_or(0) as f64;
        
        // Calculate the weighted count using the averaging fixed window algorithm
        let weighted_count = previous_count * (1.0 - bucket_percentage) + current_count;
        
        if weighted_count >= limit as f64 {
            // Calculate retry_after based on when the current window ends
            let now_secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let window_end = ((now_secs / window_size) + 1) * window_size;
            let retry_after = Duration::from_secs(window_end - now_secs);
            
            return Ok(RateLimitResult {
                allowed: false,
                retry_after: Some(retry_after),
            });
        }
        
        // Increment the counter in a separate task to avoid blocking
        let pool = self.pool.clone();
        let current_key = current_key.clone();
        let expire_time = window_size * 2; // Keep keys for 2 windows
        
        tokio::spawn(async move {
            if let Ok(mut conn) = pool.get().await {
                let _: Result<(), RedisError> = redis::pipe()
                    .incr(&current_key, 1)
                    .expire(&current_key, expire_time as i64)
                    .query_async(&mut conn)
                    .await;
            }
        });
        
        Ok(RateLimitResult {
            allowed: true,
            retry_after: None,
        })
    }
}

/// Build deadpool configuration from our config.
fn build_pool_config(config: &RedisPoolConfig) -> Option<deadpool_redis::PoolConfig> {
    use deadpool_redis::{PoolConfig, Timeouts};
    
    let mut pool_config = PoolConfig::default();
    
    if let Some(max_size) = config.max_size {
        pool_config.max_size = max_size;
    }
    
    let mut has_timeouts = false;
    let mut timeouts = Timeouts::default();
    
    if let Some(timeout_create) = config.timeout_create {
        timeouts.create = Some(timeout_create);
        has_timeouts = true;
    }
    
    if let Some(timeout_wait) = config.timeout_wait {
        timeouts.wait = Some(timeout_wait);
        has_timeouts = true;
    }
    
    if let Some(timeout_recycle) = config.timeout_recycle {
        timeouts.recycle = Some(timeout_recycle);
        has_timeouts = true;
    }
    
    if has_timeouts {
        pool_config.timeouts = timeouts;
    }
    
    Some(pool_config)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_generate_keys() {
        // This test doesn't need a real pool since it only tests key generation
        let redis_config = deadpool_redis::Config {
            url: Some("redis://localhost:6379".to_string()),
            ..Default::default()
        };
        let pool = redis_config.create_pool(Some(Runtime::Tokio1)).unwrap();
        
        let storage = RedisStorage {
            pool,
            key_prefix: "test:".to_string(),
            response_timeout: Duration::from_secs(1),
        };
        
        let (current, previous, window, percentage) = storage.generate_keys("user123", Duration::from_secs(60));
        
        assert!(current.starts_with("test:user123__"));
        assert!(previous.starts_with("test:user123__"));
        assert_eq!(window, 60);
        assert!(percentage >= 0.0 && percentage <= 1.0);
        
        // Check that keys are different
        assert_ne!(current, previous);
    }
}