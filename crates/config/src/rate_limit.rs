//! Rate limiting configuration structures.

use duration_str::deserialize_duration;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Rate limiting configuration for the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RateLimitConfig {
    /// Whether rate limiting is enabled.
    #[serde(default)]
    pub enabled: bool,
    /// Global rate limit applied to all requests.
    #[serde(default)]
    pub global: Option<RateLimitQuota>,
    /// Rate limit per IP address.
    #[serde(default)]
    pub per_ip: Option<RateLimitQuota>,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            global: None,
            per_ip: None,
        }
    }
}

/// Configuration for a rate limit quota.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RateLimitQuota {
    /// Maximum number of requests allowed within the duration window.
    pub limit: u32,
    /// Time window for the rate limit.
    #[serde(deserialize_with = "deserialize_duration")]
    pub duration: Duration,
}

impl Default for RateLimitQuota {
    fn default() -> Self {
        Self {
            limit: 60,
            duration: Duration::from_secs(60),
        }
    }
}

