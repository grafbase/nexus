//! Storage backends for rate limiting.

use std::time::Duration;

pub mod memory;

pub use memory::InMemoryStorage;

/// Result type for rate limit checks.
pub struct RateLimitResult {
    /// Whether the request is allowed.
    pub allowed: bool,
    /// Time to wait before retrying if not allowed.
    pub retry_after: Option<Duration>,
}

/// Trait for rate limit storage backends.
#[allow(async_fn_in_trait)]
pub trait RateLimitStorage: Send + Sync {
    /// Check and potentially consume a token for the given key.
    async fn check_and_consume(
        &self,
        key: &str,
        limit: u32,
        duration: Duration,
    ) -> Result<RateLimitResult, StorageError>;
}

/// Errors that can occur in storage backends.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// Internal storage error.
    #[error("Storage error: {0}")]
    Internal(String),
}
