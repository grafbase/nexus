//! Rate limiting configuration structures.

use duration_str::{deserialize_duration, deserialize_option_duration};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Rate limiting configuration for the server.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct RateLimitConfig {
    /// Whether rate limiting is enabled.
    pub enabled: bool,
    /// Storage backend configuration.
    #[serde(default)]
    pub storage: StorageConfig,
    /// Global rate limit applied to all requests.
    pub global: Option<RateLimitQuota>,
    /// Rate limit per IP address.
    pub per_ip: Option<RateLimitQuota>,
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

/// Storage backend configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum StorageConfig {
    /// In-memory storage (default).
    Memory,
    /// Redis storage with configuration.
    Redis(Box<RedisConfig>),
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self::Memory
    }
}

/// Redis storage configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedisConfig {
    /// Redis connection URL (redis:// or rediss:// for TLS).
    pub url: String,
    /// Connection pool configuration.
    #[serde(default)]
    pub pool: RedisPoolConfig,
    /// TLS configuration.
    pub tls: Option<RedisTlsConfig>,
    /// Key prefix for all rate limit keys.
    #[serde(default = "default_key_prefix")]
    pub key_prefix: Option<String>,
    /// Response timeout for Redis commands.
    #[serde(
        default = "default_response_timeout",
        deserialize_with = "deserialize_option_duration"
    )]
    pub response_timeout: Option<Duration>,
    /// Connection timeout.
    #[serde(
        default = "default_connection_timeout",
        deserialize_with = "deserialize_option_duration"
    )]
    pub connection_timeout: Option<Duration>,
}

fn default_key_prefix() -> Option<String> {
    Some("nexus:rate_limit:".to_string())
}

fn default_response_timeout() -> Option<Duration> {
    Some(Duration::from_secs(1))
}

fn default_connection_timeout() -> Option<Duration> {
    Some(Duration::from_secs(5))
}

/// Redis connection pool configuration (deadpool-redis).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedisPoolConfig {
    /// Maximum number of connections.
    pub max_size: Option<usize>,
    /// Minimum number of idle connections.
    pub min_idle: Option<usize>,
    /// Timeout for creating connections.
    #[serde(default, deserialize_with = "deserialize_option_duration")]
    pub timeout_create: Option<Duration>,
    /// Timeout for waiting for a connection.
    #[serde(default, deserialize_with = "deserialize_option_duration")]
    pub timeout_wait: Option<Duration>,
    /// Timeout before recycling idle connections.
    #[serde(default, deserialize_with = "deserialize_option_duration")]
    pub timeout_recycle: Option<Duration>,
}

impl Default for RedisPoolConfig {
    fn default() -> Self {
        Self {
            max_size: Some(16),
            min_idle: Some(0),
            timeout_create: Some(Duration::from_secs(5)),
            timeout_wait: Some(Duration::from_secs(5)),
            timeout_recycle: Some(Duration::from_secs(300)),
        }
    }
}

/// Redis TLS configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedisTlsConfig {
    /// Enable TLS (automatically enabled for rediss:// URLs).
    pub enabled: bool,
    /// Allow insecure connections (skip certificate validation).
    pub insecure: Option<bool>,
    /// Path to CA certificate file.
    pub ca_cert_path: Option<String>,
    /// Path to client certificate file (for mutual TLS).
    pub client_cert_path: Option<String>,
    /// Path to client key file (for mutual TLS).
    pub client_key_path: Option<String>,
}

impl Default for RedisConfig {
    fn default() -> Self {
        Self {
            url: "redis://localhost:6379/0".to_string(),
            pool: RedisPoolConfig::default(),
            tls: None,
            key_prefix: Some("nexus:rate_limit:".to_string()),
            response_timeout: Some(Duration::from_secs(1)),
            connection_timeout: Some(Duration::from_secs(5)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_storage_config() {
        let config = StorageConfig::default();
        insta::assert_debug_snapshot!(config, @r###"
        Memory
        "###);
    }

    #[test]
    fn deserialize_memory_storage() {
        let toml = r#"
            type = "memory"
        "#;
        let config: StorageConfig = toml::from_str(toml).unwrap();
        insta::assert_debug_snapshot!(config, @r###"
        Memory
        "###);
    }

    #[test]
    fn deserialize_redis_storage_minimal() {
        let toml = r#"
            type = "redis"
            url = "redis://localhost:6379/0"
        "#;
        let config: StorageConfig = toml::from_str(toml).unwrap();
        insta::assert_debug_snapshot!(config, @r#"
        Redis(
            RedisConfig {
                url: "redis://localhost:6379/0",
                pool: RedisPoolConfig {
                    max_size: Some(
                        16,
                    ),
                    min_idle: Some(
                        0,
                    ),
                    timeout_create: Some(
                        5s,
                    ),
                    timeout_wait: Some(
                        5s,
                    ),
                    timeout_recycle: Some(
                        300s,
                    ),
                },
                tls: None,
                key_prefix: Some(
                    "nexus:rate_limit:",
                ),
                response_timeout: Some(
                    1s,
                ),
                connection_timeout: Some(
                    5s,
                ),
            },
        )
        "#);
    }

    #[test]
    fn deserialize_redis_storage_full() {
        let toml = r#"
            type = "redis"
            url = "rediss://localhost:6380/0"
            key_prefix = "my_app:"
            response_timeout = "2s"
            connection_timeout = "10s"

            [pool]
            max_size = 32
            min_idle = 4
            timeout_create = "10s"
            timeout_wait = "2s"
            timeout_recycle = "600s"

            [tls]
            enabled = true
            insecure = false
            ca_cert_path = "/path/to/ca.crt"
            client_cert_path = "/path/to/client.crt"
            client_key_path = "/path/to/client.key"
        "#;
        let config: StorageConfig = toml::from_str(toml).unwrap();
        insta::assert_debug_snapshot!(config, @r###"
        Redis(
            RedisConfig {
                url: "rediss://localhost:6380/0",
                pool: RedisPoolConfig {
                    max_size: Some(
                        32,
                    ),
                    min_idle: Some(
                        4,
                    ),
                    timeout_create: Some(
                        10s,
                    ),
                    timeout_wait: Some(
                        2s,
                    ),
                    timeout_recycle: Some(
                        600s,
                    ),
                },
                tls: Some(
                    RedisTlsConfig {
                        enabled: true,
                        insecure: Some(
                            false,
                        ),
                        ca_cert_path: Some(
                            "/path/to/ca.crt",
                        ),
                        client_cert_path: Some(
                            "/path/to/client.crt",
                        ),
                        client_key_path: Some(
                            "/path/to/client.key",
                        ),
                    },
                ),
                key_prefix: Some(
                    "my_app:",
                ),
                response_timeout: Some(
                    2s,
                ),
                connection_timeout: Some(
                    10s,
                ),
            },
        )
        "###);
    }

    #[test]
    fn rate_limit_config_with_storage() {
        let toml = r#"
            enabled = true

            [storage]
            type = "redis"
            url = "redis://localhost:6379"

            [global]
            limit = 1000
            duration = "60s"

            [per_ip]
            limit = 100
            duration = "60s"
        "#;
        let config: RateLimitConfig = toml::from_str(toml).unwrap();
        insta::assert_debug_snapshot!(config, @r#"
        RateLimitConfig {
            enabled: true,
            storage: Redis(
                RedisConfig {
                    url: "redis://localhost:6379",
                    pool: RedisPoolConfig {
                        max_size: Some(
                            16,
                        ),
                        min_idle: Some(
                            0,
                        ),
                        timeout_create: Some(
                            5s,
                        ),
                        timeout_wait: Some(
                            5s,
                        ),
                        timeout_recycle: Some(
                            300s,
                        ),
                    },
                    tls: None,
                    key_prefix: Some(
                        "nexus:rate_limit:",
                    ),
                    response_timeout: Some(
                        1s,
                    ),
                    connection_timeout: Some(
                        5s,
                    ),
                },
            ),
            global: Some(
                RateLimitQuota {
                    limit: 1000,
                    duration: 60s,
                },
            ),
            per_ip: Some(
                RateLimitQuota {
                    limit: 100,
                    duration: 60s,
                },
            ),
        }
        "#);
    }
}
