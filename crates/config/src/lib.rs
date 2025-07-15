//! Nexus configuration structures to map the nexus.toml configuration.

#![deny(missing_docs)]

mod loader;
mod mcp;

use std::{
    borrow::Cow,
    net::SocketAddr,
    path::{Path, PathBuf},
};

pub use mcp::McpConfig;
use serde::Deserialize;

/// Main configuration structure for the Nexus application.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// HTTP server configuration settings.
    #[serde(default)]
    pub server: ServerConfig,
    /// Model Context Protocol configuration settings.
    #[serde(default)]
    pub mcp: McpConfig,
}

impl Config {
    /// Load configuration from a file path.
    pub fn load<P: AsRef<Path>>(path: P) -> anyhow::Result<Config> {
        loader::load(path)
    }
}

/// HTTP server configuration settings.
#[derive(Default, Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    /// The socket address the server should listen on.
    pub listen_address: Option<SocketAddr>,
    /// TLS configuration for secure connections.
    pub tls: Option<TlsConfig>,
    /// Health endpoint configuration.
    #[serde(default)]
    pub health: HealthConfig,
}

/// TLS configuration for secure connections.
#[derive(Default, Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TlsConfig {
    /// Path to the TLS certificate PEM file.
    pub certificate: PathBuf,
    /// Path to the TLS private key PEM file.
    pub key: PathBuf,
}

/// Health endpoint configuration.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HealthConfig {
    /// Whether the health endpoint is enabled.
    pub enabled: bool,
    /// The socket address the health endpoint should listen on.
    pub listen: Option<SocketAddr>,
    /// The path for the health endpoint.
    pub path: Cow<'static, str>,
}

impl Default for HealthConfig {
    fn default() -> Self {
        HealthConfig {
            enabled: true,
            listen: None,
            path: Cow::Borrowed("/health"),
        }
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use crate::Config;

    #[test]
    fn all_values() {
        let config = indoc! {r#"
            [server]
            listen_address = "127.0.0.1:8080"

            [mcp]
            enabled = false
            path = "/mcp-path"
        "#};

        let config: Config = toml::from_str(config).unwrap();

        insta::assert_debug_snapshot!(&config, @r#"
        Config {
            server: ServerConfig {
                listen_address: Some(
                    127.0.0.1:8080,
                ),
                tls: None,
                health: HealthConfig {
                    enabled: true,
                    listen: None,
                    path: "/health",
                },
            },
            mcp: McpConfig {
                enabled: false,
                path: "/mcp-path",
            },
        }
        "#);
    }

    #[test]
    fn defaults() {
        let config: Config = toml::from_str("").unwrap();

        insta::assert_debug_snapshot!(&config, @r#"
        Config {
            server: ServerConfig {
                listen_address: None,
                tls: None,
                health: HealthConfig {
                    enabled: true,
                    listen: None,
                    path: "/health",
                },
            },
            mcp: McpConfig {
                enabled: true,
                path: "/mcp",
            },
        }
        "#);
    }
}
