//! In-memory rate limit storage using the governor crate.

use std::sync::Arc;
use std::time::Duration;

use governor::clock::{Clock, DefaultClock};
use governor::state::keyed::DefaultKeyedStateStore;
use governor::{Quota, RateLimiter};
use mini_moka::sync::Cache;

use super::{RateLimitResult, RateLimitStorage, StorageError};

type KeyedRateLimiter = RateLimiter<String, DefaultKeyedStateStore<String>, DefaultClock>;

/// In-memory rate limit storage implementation.
pub struct InMemoryStorage {
    /// Cache of rate limiters by quota configuration.
    limiters: Cache<String, Arc<KeyedRateLimiter>>,
}

impl InMemoryStorage {
    /// Create a new in-memory storage instance.
    pub fn new() -> Self {
        Self {
            limiters: Cache::builder()
                .max_capacity(10000)
                .time_to_idle(Duration::from_secs(3600))
                .build(),
        }
    }
    
    fn quota_from_config(limit: u32, duration: Duration) -> Result<Quota, StorageError> {
        // Convert to per-second rate
        let per_second_f64 = (limit as f64 / duration.as_secs_f64()).max(1.0);
        let per_second = per_second_f64 as u32;
        
        // Calculate burst capacity (10% of limit or minimum 5)
        let burst = (limit / 10).max(5).min(limit);
        
        log::debug!("quota_from_config: limit={limit}, duration={duration:?}, per_second_f64={per_second_f64}, per_second={per_second}, burst={burst}");
        
        let per_second = per_second.try_into()
            .map_err(|_| StorageError::Internal(format!("Invalid per-second rate: {per_second}")))?;
        let burst = burst.try_into()
            .map_err(|_| StorageError::Internal(format!("Invalid burst size: {burst}")))?;
        
        let quota = Quota::per_second(per_second).allow_burst(burst);
        log::debug!("Created quota: {quota:?}");
        
        Ok(quota)
    }
}

impl Default for InMemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl RateLimitStorage for InMemoryStorage {
    async fn check_and_consume(
        &self,
        key: &str,
        limit: u32,
        duration: Duration,
    ) -> Result<RateLimitResult, StorageError> {
        // Create a cache key based on limit and duration
        let duration_secs = duration.as_secs();
        let cache_key = format!("{limit}-{duration_secs}");
        
        log::debug!("check_and_consume: key={key}, limit={limit}, duration={duration:?}, cache_key={cache_key}");
        
        // Get or create the rate limiter for this configuration
        let limiter = if let Some(limiter) = self.limiters.get(&cache_key) {
            log::debug!("Using existing rate limiter for cache_key={cache_key}");
            limiter
        } else {
            let quota = Self::quota_from_config(limit, duration)?;
            log::debug!("Created new rate limiter for cache_key={cache_key}, quota={quota:?}");
            let limiter = Arc::new(RateLimiter::keyed(quota));
            self.limiters.insert(cache_key.clone(), limiter.clone());
            limiter
        };
        
        // Check the rate limit
        match limiter.check_key(&key.to_string()) {
            Ok(_) => {
                log::debug!("Rate limit check ALLOWED for key={key}");
                Ok(RateLimitResult {
                    allowed: true,
                    retry_after: None,
                })
            },
            Err(not_until) => {
                let retry_after = not_until.wait_time_from(DefaultClock::default().now());
                log::debug!("Rate limit check BLOCKED for key={key}, retry_after={retry_after:?}");
                Ok(RateLimitResult {
                    allowed: false,
                    retry_after: Some(retry_after),
                })
            }
        }
    }
}

