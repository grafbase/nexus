//! In-memory rate limit storage using the governor crate.

use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use governor::clock::{Clock, DefaultClock};
use governor::state::keyed::DefaultKeyedStateStore;
use governor::{Quota, RateLimiter};
use mini_moka::sync::Cache;
use tokio::sync::Mutex;

use super::{RateLimitResult, RateLimitStorage, StorageError};

type KeyedRateLimiter = RateLimiter<String, DefaultKeyedStateStore<String>, DefaultClock>;

/// In-memory rate limit storage implementation.
pub struct InMemoryStorage {
    /// Cache of rate limiters by quota configuration.
    limiters: Cache<String, Arc<KeyedRateLimiter>>,
    /// Lock to prevent thundering herd when creating rate limiters.
    /// Maps cache key to a lock for that specific configuration.
    creation_locks: DashMap<String, Arc<Mutex<()>>>,
}

impl InMemoryStorage {
    /// Create a new in-memory storage instance.
    pub fn new() -> Self {
        let limiters = Cache::builder()
            .max_capacity(10000)
            .time_to_idle(Duration::from_secs(3600))
            .build();

        Self {
            limiters,
            creation_locks: DashMap::new(),
        }
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
        // Create a cache key based on limit and duration configuration.
        // We cache rate limiters by their configuration (limit + duration) rather than by the
        // actual key for efficiency. This allows multiple keys with the same rate limit
        // configuration to share a single rate limiter instance, reducing memory usage.
        //
        // The governor crate's keyed rate limiter internally tracks separate rate limit states
        // for each key, so sharing the rate limiter instance doesn't affect the per-key
        // rate limiting behavior. Each key still gets its own independent rate limit tracking.
        let duration_millis = duration.as_millis();
        let cache_key = format!("{limit}-{duration_millis}ms");

        log::debug!("Checking rate limit for key '{key}': {limit} requests allowed per {duration:?}");

        // Get or create the rate limiter for this configuration
        // First, try to get an existing limiter
        if let Some(limiter) = self.limiters.get(&cache_key) {
            log::debug!("Reusing cached rate limiter for configuration: {cache_key}");
            return self.check_rate_limit(limiter, key);
        }

        // If not found, we need to create one. Get or create a lock for this specific cache key
        // to prevent multiple threads from creating the same rate limiter
        let creation_lock = self
            .creation_locks
            .entry(cache_key.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();

        // Hold the lock while creating the rate limiter
        let _guard = creation_lock.lock().await;

        // Double-check if another thread created it while we were waiting for the lock
        if let Some(limiter) = self.limiters.get(&cache_key) {
            log::debug!("Another thread created the rate limiter while waiting for lock: {cache_key}");

            // Clean up the creation lock
            drop(_guard);

            self.creation_locks.remove(&cache_key);
            return self.check_rate_limit(limiter, key);
        }

        // Create the rate limiter
        let quota = quota_from_config(limit, duration)?;
        let limiter = Arc::new(RateLimiter::keyed(quota));

        // Insert into cache
        self.limiters.insert(cache_key.clone(), limiter.clone());
        log::debug!("Created new rate limiter instance for configuration: {cache_key}");

        // Clean up the creation lock
        drop(_guard);

        self.creation_locks.remove(&cache_key);
        self.check_rate_limit(limiter, key)
    }
}

impl InMemoryStorage {
    fn check_rate_limit(&self, limiter: Arc<KeyedRateLimiter>, key: &str) -> Result<RateLimitResult, StorageError> {
        // Check the rate limit
        match limiter.check_key(&key.to_string()) {
            Ok(_) => {
                log::debug!("Request allowed for key '{key}' - within rate limit");
                Ok(RateLimitResult {
                    allowed: true,
                    retry_after: None,
                })
            }
            Err(not_until) => {
                let retry_after = not_until.wait_time_from(DefaultClock::default().now());
                log::debug!("Request blocked for key '{key}' - rate limit exceeded, retry after {retry_after:?}");
                Ok(RateLimitResult {
                    allowed: false,
                    retry_after: Some(retry_after),
                })
            }
        }
    }
}

fn quota_from_config(limit: u32, duration: Duration) -> Result<Quota, StorageError> {
    // Convert to per-second rate
    let per_second_f64 = (limit as f64 / duration.as_secs_f64()).max(1.0);
    let per_second = per_second_f64 as u32;

    // Calculate burst capacity (10% of limit or minimum 5)
    let burst = (limit / 10).max(5).min(limit);

    log::debug!(
        "Calculating rate limit quota: {limit} requests per {duration:?}, converted to {per_second}/second with burst capacity of {burst}"
    );

    let per_second = per_second
        .try_into()
        .map_err(|_| StorageError::Internal(format!("Invalid per-second rate: {per_second}")))?;

    let burst = burst
        .try_into()
        .map_err(|_| StorageError::Internal(format!("Invalid burst size: {burst}")))?;

    let quota = Quota::per_second(per_second).allow_burst(burst);
    log::debug!("Successfully created rate limit quota configuration: {quota:?}");

    Ok(quota)
}
