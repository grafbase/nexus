//! Error types for rate limiting.

use crate::storage::StorageError;
use std::time::Duration;

/// Errors that can occur during rate limiting.
#[derive(Debug, thiserror::Error)]
pub enum RateLimitError {
    /// Global rate limit exceeded.
    #[error("Global rate limit exceeded")]
    GlobalLimitExceeded {
        /// Time to wait before retrying.
        retry_after: Duration,
    },

    /// IP-specific rate limit exceeded.
    #[error("IP rate limit exceeded")]
    IpLimitExceeded {
        /// Time to wait before retrying.
        retry_after: Duration,
    },

    /// Server-specific rate limit exceeded.
    #[error("Rate limit exceeded for server {server}")]
    ServerLimitExceeded {
        /// Name of the server that exceeded the limit.
        server: String,
        /// Time to wait before retrying.
        retry_after: Duration,
    },

    /// Tool-specific rate limit exceeded.
    #[error("Rate limit exceeded for tool {server}::{tool}")]
    ToolLimitExceeded {
        /// Name of the server.
        server: String,
        /// Name of the tool.
        tool: String,
        /// Time to wait before retrying.
        retry_after: Duration,
    },

    /// Storage backend error.
    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),
}

impl RateLimitError {
    /// Get the retry-after duration if available.
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::GlobalLimitExceeded { retry_after } => Some(*retry_after),
            Self::IpLimitExceeded { retry_after } => Some(*retry_after),
            Self::ServerLimitExceeded { retry_after, .. } => Some(*retry_after),
            Self::ToolLimitExceeded { retry_after, .. } => Some(*retry_after),
            Self::Storage(_) => None,
        }
    }
}
