//! Nexus configuration structures to map the nexus.toml configuration.

#![deny(missing_docs)]

mod cors;
mod loader;
mod mcp;

use std::{
    borrow::Cow,
    net::SocketAddr,
    path::{Path, PathBuf},
};

pub use cors::*;
pub use mcp::{
    ClientAuthConfig, ClientOauthConfig, ClientTokenConfig, HttpConfig, HttpProtocol, McpConfig, McpServer,
    TlsClientConfig,
};
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
    /// CORS configuration
    pub cors: Option<CorsConfig>,
    /// CSRF configuration
    #[serde(default)]
    pub csrf: CsrfConfig,
}

/// CSRF (Cross-Site Request Forgery) protection configuration.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CsrfConfig {
    /// Whether CSRF protection is enabled.
    pub enabled: bool,
    /// The name of the header to use for CSRF tokens.
    pub header_name: String,
}

impl Default for CsrfConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            header_name: "X-Nexus-CSRF-Protection".into(),
        }
    }
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
    use std::time::Duration;

    use ascii::AsciiString;
    use indoc::indoc;

    use crate::{
        Config,
        cors::{AnyOrAsciiStringArray, AnyOrHttpMethodArray, AnyOrUrlArray, HttpMethod},
    };

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
                cors: None,
                csrf: CsrfConfig {
                    enabled: false,
                    header_name: "X-Nexus-CSRF-Protection",
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
                cors: None,
                csrf: CsrfConfig {
                    enabled: false,
                    header_name: "X-Nexus-CSRF-Protection",
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

    #[test]
    fn cors_allow_credentials() {
        let input = indoc! {r#"
            [server.cors]
            allow_credentials = true
        "#};

        let config: Config = toml::from_str(input).unwrap();
        let cors = config.server.cors.unwrap();

        assert!(cors.allow_credentials);
    }

    #[test]
    fn cors_allow_credentials_default() {
        let input = indoc! {r#"
            [server.cors]
        "#};

        let config: Config = toml::from_str(input).unwrap();
        let cors = config.server.cors.unwrap();

        assert!(!cors.allow_credentials);
    }

    #[test]
    fn cors_max_age() {
        let input = indoc! {r#"
           [server.cors]
           max_age = "60s"
        "#};

        let config: Config = toml::from_str(input).unwrap();
        let cors = config.server.cors.unwrap();

        assert_eq!(Some(Duration::from_secs(60)), cors.max_age);
    }

    #[test]
    fn cors_allow_origins_default() {
        let input = indoc! {r#"
            [server.cors]
        "#};

        let config: Config = toml::from_str(input).unwrap();
        let cors = config.server.cors.unwrap();

        assert_eq!(None, cors.allow_origins)
    }

    #[test]
    fn cors_allow_origins_any() {
        let input = indoc! {r#"
            [server.cors]
            allow_origins = "*"
        "#};

        let config: Config = toml::from_str(input).unwrap();
        let cors = config.server.cors.unwrap();

        assert_eq!(Some(AnyOrUrlArray::Any), cors.allow_origins)
    }

    #[test]
    fn cors_allow_origins_explicit() {
        let input = indoc! {r#"
            [server.cors]
            allow_origins = ["https://app.grafbase.com"]
        "#};

        let config: Config = toml::from_str(input).unwrap();
        let cors = config.server.cors.unwrap();
        let expected = AnyOrUrlArray::Explicit(vec!["https://app.grafbase.com".parse().unwrap()]);

        assert_eq!(Some(expected), cors.allow_origins)
    }

    #[test]
    fn cors_allow_origins_invalid_url() {
        let input = indoc! {r#"
            [server.cors]
            allow_origins = ["foo"]
        "#};

        let error = toml::from_str::<Config>(input).unwrap_err();

        insta::assert_snapshot!(&error.to_string(), @r#"
        TOML parse error at line 2, column 18
          |
        2 | allow_origins = ["foo"]
          |                  ^^^^^
        relative URL without a base: "foo"
        "#);
    }

    #[test]
    fn cors_allow_methods_default() {
        let input = indoc! {r#"
            [server.cors]
        "#};

        let config: Config = toml::from_str(input).unwrap();
        let cors = config.server.cors.unwrap();

        assert_eq!(None, cors.allow_methods)
    }

    #[test]
    fn cors_allow_methods_any() {
        let input = indoc! {r#"
            [server.cors]
            allow_methods = "*"
        "#};

        let config: Config = toml::from_str(input).unwrap();
        let cors = config.server.cors.unwrap();

        assert_eq!(Some(AnyOrHttpMethodArray::Any), cors.allow_methods)
    }

    #[test]
    fn cors_allow_methods_explicit() {
        let input = indoc! {r#"
            [server.cors]
            allow_methods = ["POST"]
        "#};

        let config: Config = toml::from_str(input).unwrap();
        let cors = config.server.cors.unwrap();
        let expected = AnyOrHttpMethodArray::Explicit(vec![HttpMethod::Post]);

        assert_eq!(Some(expected), cors.allow_methods)
    }

    #[test]
    fn cors_allow_methods_invalid_method() {
        let input = indoc! {r#"
            [server.cors]
            allow_methods = ["MEOW"]
        "#};

        let error = toml::from_str::<Config>(input).unwrap_err();

        insta::assert_snapshot!(&error.to_string(), @r#"
        TOML parse error at line 2, column 18
          |
        2 | allow_methods = ["MEOW"]
          |                  ^^^^^^
        unknown variant `MEOW`, expected one of `GET`, `POST`, `PUT`, `DELETE`, `HEAD`, `OPTIONS`, `CONNECT`, `PATCH`, `TRACE`
        "#);
    }

    #[test]
    fn cors_allow_headers_default() {
        let input = indoc! {r#"
            [server.cors]
        "#};

        let config: Config = toml::from_str(input).unwrap();
        let cors = config.server.cors.unwrap();

        assert_eq!(None, cors.allow_headers)
    }

    #[test]
    fn cors_allow_headers_any() {
        let input = indoc! {r#"
            [server.cors]
            allow_headers = "*"
        "#};

        let config: Config = toml::from_str(input).unwrap();
        let cors = config.server.cors.unwrap();

        assert_eq!(Some(AnyOrAsciiStringArray::Any), cors.allow_headers)
    }

    #[test]
    fn cors_allow_headers_explicit() {
        let input = indoc! {r#"
            [server.cors]
            allow_headers = ["Content-Type"]
        "#};

        let config: Config = toml::from_str(input).unwrap();
        let cors = config.server.cors.unwrap();

        let expected = AnyOrAsciiStringArray::Explicit(vec![AsciiString::from_ascii(b"Content-Type").unwrap()]);

        assert_eq!(Some(expected), cors.allow_headers)
    }

    #[test]
    fn cors_allow_headers_invalid() {
        let input = indoc! {r#"
            [server.cors]
            allow_headers = ["😂😂😂"]
        "#};

        let error = toml::from_str::<Config>(input).unwrap_err();

        insta::assert_snapshot!(&error.to_string(), @r#"
        TOML parse error at line 2, column 18
          |
        2 | allow_headers = ["😂😂😂"]
          |                  ^^^^^^^^^^^^^^
        invalid value: string "😂😂😂", expected an ascii string
        "#);
    }

    #[test]
    fn cors_expose_headers_default() {
        let input = indoc! {r#"
            [server.cors]
        "#};

        let config: Config = toml::from_str(input).unwrap();
        let cors = config.server.cors.unwrap();

        assert_eq!(None, cors.expose_headers);
    }

    #[test]
    fn cors_expose_headers_any() {
        let input = indoc! {r#"
            [server.cors]
            expose_headers = "*"
        "#};

        let config: Config = toml::from_str(input).unwrap();
        let cors = config.server.cors.unwrap();

        assert_eq!(Some(AnyOrAsciiStringArray::Any), cors.expose_headers);
    }

    #[test]
    fn cors_expose_headers_explicit() {
        let input = indoc! {r#"
            [server.cors]
            expose_headers = ["Content-Type"]
        "#};

        let config: Config = toml::from_str(input).unwrap();
        let cors = config.server.cors.unwrap();

        let expected = AnyOrAsciiStringArray::Explicit(vec![AsciiString::from_ascii(b"Content-Type").unwrap()]);

        assert_eq!(Some(expected), cors.expose_headers);
    }

    #[test]
    fn cors_expose_headers_invalid() {
        let input = indoc! {r#"
            [server.cors]
            expose_headers = ["😂😂😂"]
        "#};

        let error = toml::from_str::<Config>(input).unwrap_err();

        insta::assert_snapshot!(&error.to_string(), @r#"
        TOML parse error at line 2, column 19
          |
        2 | expose_headers = ["😂😂😂"]
          |                   ^^^^^^^^^^^^^^
        invalid value: string "😂😂😂", expected an ascii string
        "#);
    }

    #[test]
    fn cors_allow_private_network_default() {
        let input = indoc! {r#"
            [server.cors]
        "#};

        let config: Config = toml::from_str(input).unwrap();
        let cors = config.server.cors.unwrap();

        assert!(!cors.allow_private_network);
    }

    #[test]
    fn cors_allow_private_network_explicit() {
        let input = indoc! {r#"
            [server.cors]
            allow_private_network = true
        "#};

        let config: Config = toml::from_str(input).unwrap();
        let cors = config.server.cors.unwrap();

        assert!(cors.allow_private_network);
    }

    #[test]
    fn mcp_server_with_token_auth() {
        let config = indoc! {r#"
             [mcp.servers.github_api]
             protocol = "streamable-http"
             url = "https://api.githubcopilot.com/mcp/"

             [mcp.servers.github_api.auth]
             token = "Something"
         "#};

        let config: Config = toml::from_str(config).unwrap();

        insta::assert_debug_snapshot!(&config.mcp.servers, @r#"
         {
             "github_api": Http(
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
                                 "api.githubcopilot.com",
                             ),
                         ),
                         port: None,
                         path: "/mcp/",
                         query: None,
                         fragment: None,
                     },
                     tls: None,
                     message_url: None,
                     auth: Some(
                        Token(
                            ClientTokenConfig {
                                token: SecretBox<str>([REDACTED]),
                            },
                        ),
                     ),
                 },
             ),
         }
         "#);
    }
}
