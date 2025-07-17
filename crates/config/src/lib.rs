//! Nexus configuration structures to map the nexus.toml configuration.

#![deny(missing_docs)]

mod loader;
mod mcp;

use std::{
    borrow::Cow,
    net::SocketAddr,
    path::{Path, PathBuf},
};

pub use mcp::{HttpConfig, HttpProtocol, McpConfig, McpServer, TlsClientConfig};
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
    pub tls: Option<TlsServerConfig>,
    /// Health endpoint configuration.
    #[serde(default)]
    pub health: HealthConfig,
}

/// TLS configuration for secure connections.
#[derive(Default, Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TlsServerConfig {
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
    fn mcp_sse_server() {
        let config = indoc! {r#"
            [mcp.servers.sse_server]
            protocol = "sse"
            url = "http://example.com/sse"
            message_url = "http://example.com/message"

            [mcp.servers.sse_server.tls]
            verify_certs = false
            accept_invalid_hostnames = true
        "#};

        let config: Config = toml::from_str(config).unwrap();

        insta::assert_debug_snapshot!(&config.mcp.servers, @r#"
        {
            "sse_server": Http(
                HttpConfig {
                    protocol: Some(
                        Sse,
                    ),
                    url: Url {
                        scheme: "http",
                        cannot_be_a_base: false,
                        username: "",
                        password: None,
                        host: Some(
                            Domain(
                                "example.com",
                            ),
                        ),
                        port: None,
                        path: "/sse",
                        query: None,
                        fragment: None,
                    },
                    tls: Some(
                        TlsClientConfig {
                            verify_certs: false,
                            accept_invalid_hostnames: true,
                            root_ca_cert_path: None,
                            client_cert_path: None,
                            client_key_path: None,
                        },
                    ),
                    message_url: Some(
                        Url {
                            scheme: "http",
                            cannot_be_a_base: false,
                            username: "",
                            password: None,
                            host: Some(
                                Domain(
                                    "example.com",
                                ),
                            ),
                            port: None,
                            path: "/message",
                            query: None,
                            fragment: None,
                        },
                    ),
                },
            ),
        }
        "#);
    }

    #[test]
    fn mcp_streamable_http_server() {
        let config = indoc! {r#"
            [mcp.servers.http_server]
            protocol = "streamable-http"
            url = "https://api.example.com"

            [mcp.servers.http_server.tls]
            verify_certs = true
            root_ca_cert_path = "/path/to/ca.pem"
        "#};

        let config: Config = toml::from_str(config).unwrap();

        insta::assert_debug_snapshot!(&config.mcp.servers, @r#"
        {
            "http_server": Http(
                HttpConfig {
                    protocol: Some(
                        StreamableHttp,
                    ),
                    url: Url {
                        scheme: "https",
                        cannot_be_a_base: false,
                        username: "",
                        password: None,
                        host: Some(
                            Domain(
                                "api.example.com",
                            ),
                        ),
                        port: None,
                        path: "/",
                        query: None,
                        fragment: None,
                    },
                    tls: Some(
                        TlsClientConfig {
                            verify_certs: true,
                            accept_invalid_hostnames: false,
                            root_ca_cert_path: Some(
                                "/path/to/ca.pem",
                            ),
                            client_cert_path: None,
                            client_key_path: None,
                        },
                    ),
                    message_url: None,
                },
            ),
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

            [mcp.servers.sse_api]
            protocol = "sse"
            url = "http://sse-api.internal:8080/events"

            [mcp.servers.sse_api2]
            url = "http://sse-api.internal:8081/events"
            message_url = "http://sse-api.internal:8081/messages"

            [mcp.servers.streaming_api]
            protocol = "streamable-http"
            url = "http://streaming-api.internal:8080"

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
                "sse_api": Http(
                    HttpConfig {
                        protocol: Some(
                            Sse,
                        ),
                        url: Url {
                            scheme: "http",
                            cannot_be_a_base: false,
                            username: "",
                            password: None,
                            host: Some(
                                Domain(
                                    "sse-api.internal",
                                ),
                            ),
                            port: Some(
                                8080,
                            ),
                            path: "/events",
                            query: None,
                            fragment: None,
                        },
                        tls: None,
                        message_url: None,
                    },
                ),
                "sse_api2": Http(
                    HttpConfig {
                        protocol: None,
                        url: Url {
                            scheme: "http",
                            cannot_be_a_base: false,
                            username: "",
                            password: None,
                            host: Some(
                                Domain(
                                    "sse-api.internal",
                                ),
                            ),
                            port: Some(
                                8081,
                            ),
                            path: "/events",
                            query: None,
                            fragment: None,
                        },
                        tls: None,
                        message_url: Some(
                            Url {
                                scheme: "http",
                                cannot_be_a_base: false,
                                username: "",
                                password: None,
                                host: Some(
                                    Domain(
                                        "sse-api.internal",
                                    ),
                                ),
                                port: Some(
                                    8081,
                                ),
                                path: "/messages",
                                query: None,
                                fragment: None,
                            },
                        ),
                    },
                ),
                "streaming_api": Http(
                    HttpConfig {
                        protocol: Some(
                            StreamableHttp,
                        ),
                        url: Url {
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
                        tls: None,
                        message_url: None,
                    },
                ),
            },
        }
        "#);
    }
}
