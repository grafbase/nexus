//! Nexus configuration structures to map the nexus.toml configuration.

#![deny(missing_docs)]

mod loader;
mod mcp;

use std::{
    borrow::Cow,
    net::SocketAddr,
    path::{Path, PathBuf},
};

pub use mcp::{HttpProtocol, McpConfig, McpServer};
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
                servers: {},
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
                servers: {},
            },
        }
        "#);
    }

    #[test]
    fn mcp_stdio_server() {
        let config = indoc! {r#"
            [mcp.servers.local_code_interpreter]
            cmd = ["/usr/bin/mcp/code_interpreter_server", "--json-output"]
        "#};

        let config: Config = toml::from_str(config).unwrap();

        insta::assert_debug_snapshot!(&config.mcp.servers, @r#"
        {
            "local_code_interpreter": Stdio {
                cmd: [
                    "/usr/bin/mcp/code_interpreter_server",
                    "--json-output",
                ],
            },
        }
        "#);
    }

    #[test]
    fn mcp_http_server_default_protocol() {
        let config = indoc! {r#"
            [mcp.servers.public_knowledge_base]
            uri = "http://mcp-kb.internal:9000"
        "#};

        let config: Config = toml::from_str(config).unwrap();

        insta::assert_debug_snapshot!(&config.mcp.servers, @r#"
        {
            "public_knowledge_base": Http {
                uri: Url {
                    scheme: "http",
                    cannot_be_a_base: false,
                    username: "",
                    password: None,
                    host: Some(
                        Domain(
                            "mcp-kb.internal",
                        ),
                    ),
                    port: Some(
                        9000,
                    ),
                    path: "/",
                    query: None,
                    fragment: None,
                },
                protocol: StreamingHttp,
            },
        }
        "#);
    }

    #[test]
    fn mcp_http_server_streaming_protocol() {
        let config = indoc! {r#"
            [mcp.servers.streaming_kb]
            uri = "http://streaming-kb.internal:9000"
            protocol = "streaming-http"
        "#};

        let config: Config = toml::from_str(config).unwrap();

        insta::assert_debug_snapshot!(&config.mcp.servers, @r#"
        {
            "streaming_kb": Http {
                uri: Url {
                    scheme: "http",
                    cannot_be_a_base: false,
                    username: "",
                    password: None,
                    host: Some(
                        Domain(
                            "streaming-kb.internal",
                        ),
                    ),
                    port: Some(
                        9000,
                    ),
                    path: "/",
                    query: None,
                    fragment: None,
                },
                protocol: StreamingHttp,
            },
        }
        "#);
    }

    #[test]
    fn mcp_http_server_sse_protocol() {
        let config = indoc! {r#"
            [mcp.servers.sse_kb]
            uri = "http://sse-kb.internal:9000"
            protocol = "sse"
        "#};

        let config: Config = toml::from_str(config).unwrap();

        insta::assert_debug_snapshot!(&config.mcp.servers, @r#"
        {
            "sse_kb": Http {
                uri: Url {
                    scheme: "http",
                    cannot_be_a_base: false,
                    username: "",
                    password: None,
                    host: Some(
                        Domain(
                            "sse-kb.internal",
                        ),
                    ),
                    port: Some(
                        9000,
                    ),
                    path: "/",
                    query: None,
                    fragment: None,
                },
                protocol: Sse,
            },
        }
        "#);
    }

    #[test]
    fn mcp_mixed_servers() {
        let config = indoc! {r#"
            [mcp]
            enabled = true
            path = "/custom-mcp"

            [mcp.servers.local_code_interpreter]
            cmd = ["/usr/bin/mcp/code_interpreter_server", "--json-output"]

            [mcp.servers.public_knowledge_base]
            uri = "http://mcp-kb.internal:9000"

            [mcp.servers.streaming_api]
            uri = "http://streaming-api.internal:8080"
            protocol = "streaming-http"

            [mcp.servers.another_stdio]
            cmd = ["python", "-m", "mcp_server", "--port", "3000"]
        "#};

        let config: Config = toml::from_str(config).unwrap();

        insta::assert_debug_snapshot!(&config.mcp, @r#"
        McpConfig {
            enabled: true,
            path: "/custom-mcp",
            servers: {
                "another_stdio": Stdio {
                    cmd: [
                        "python",
                        "-m",
                        "mcp_server",
                        "--port",
                        "3000",
                    ],
                },
                "local_code_interpreter": Stdio {
                    cmd: [
                        "/usr/bin/mcp/code_interpreter_server",
                        "--json-output",
                    ],
                },
                "public_knowledge_base": Http {
                    uri: Url {
                        scheme: "http",
                        cannot_be_a_base: false,
                        username: "",
                        password: None,
                        host: Some(
                            Domain(
                                "mcp-kb.internal",
                            ),
                        ),
                        port: Some(
                            9000,
                        ),
                        path: "/",
                        query: None,
                        fragment: None,
                    },
                    protocol: StreamingHttp,
                },
                "streaming_api": Http {
                    uri: Url {
                        scheme: "http",
                        cannot_be_a_base: false,
                        username: "",
                        password: None,
                        host: Some(
                            Domain(
                                "streaming-api.internal",
                            ),
                        ),
                        port: Some(
                            8080,
                        ),
                        path: "/",
                        query: None,
                        fragment: None,
                    },
                    protocol: StreamingHttp,
                },
            },
        }
        "#);
    }
}
