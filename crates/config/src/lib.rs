//! Nexus configuration structures to map the nexus.toml configuration.

#![deny(missing_docs)]

mod client_identity;
mod cors;
mod headers;
mod http_types;
mod llm;
mod loader;
mod mcp;
mod rate_limit;

use std::{
    borrow::Cow,
    collections::BTreeSet,
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};

pub use client_identity::ClientIdentity;
pub use cors::*;
use duration_str::deserialize_option_duration;
pub use headers::{
    HeaderForward, HeaderInsert, HeaderRemove, HeaderRenameDuplicate, HeaderRule, McpHeaderRule, NameOrPattern,
    NamePattern,
};
pub use http_types::{HeaderName, HeaderValue};
pub use llm::{
    ApiModelConfig, ApiProviderConfig, BedrockModelConfig, BedrockProviderConfig, LlmConfig, LlmProviderConfig,
    ModelConfig, ProviderType,
};
pub use mcp::{
    ClientAuthConfig, HttpConfig, HttpProtocol, McpConfig, McpServer, McpServerRateLimit, StdioConfig, StdioTarget,
    StdioTargetType, TlsClientConfig,
};
pub use rate_limit::*;
use serde::Deserialize;
use url::Url;

/// Main configuration structure for the Nexus application.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// HTTP server configuration settings.
    #[serde(default)]
    pub server: ServerConfig,
    /// Model Context Protocol configuration settings.
    #[serde(default)]
    pub mcp: McpConfig,
    /// LLM configuration settings.
    #[serde(default)]
    pub llm: LlmConfig,
}

impl Config {
    /// Load configuration from a file path.
    pub fn load<P: AsRef<Path>>(path: P) -> anyhow::Result<Config> {
        loader::load(path)
    }

    /// Validates that the configuration has at least one functional downstream.
    pub fn validate(&self) -> anyhow::Result<()> {
        loader::validate_has_downstreams(self)
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
    /// OAuth2 configuration
    pub oauth: Option<OauthConfig>,
    /// Rate limiting configuration
    #[serde(default)]
    pub rate_limits: RateLimitConfig,
    /// Client identification configuration for token-based rate limiting
    #[serde(default)]
    pub client_identification: Option<ClientIdentificationConfig>,
}

impl ServerConfig {
    /// Returns whether OAuth2 authentication is configured for this server.
    pub fn uses_oauth(&self) -> bool {
        self.oauth.is_some()
    }
}

/// Identification source - either JWT claim or HTTP header.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum IdentificationSource {
    /// Extract from JWT claim.
    JwtClaim {
        /// JWT claim path (e.g., "sub", "plan").
        jwt_claim: String,
    },
    /// Extract from HTTP header.
    HttpHeader {
        /// HTTP header name (e.g., "X-Client-Id", "X-Group-Id").
        http_header: String,
    },
}

impl Default for IdentificationSource {
    fn default() -> Self {
        Self::JwtClaim {
            jwt_claim: "sub".to_string(),
        }
    }
}

/// Client identification extraction configuration.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ClientIdentificationConfig {
    /// Whether client identification is enabled.
    #[serde(default)]
    pub enabled: bool,

    /// Validation settings for client identification.
    #[serde(default)]
    pub validation: ClientIdentificationValidation,

    /// Client ID extraction source.
    pub client_id: IdentificationSource,

    /// Group ID extraction source.
    #[serde(default)]
    pub group_id: Option<IdentificationSource>,
}

/// Validation settings for client identification.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ClientIdentificationValidation {
    /// List of valid group values. All group names in rate limits must be from this list.
    #[serde(default)]
    pub group_values: BTreeSet<String>,
}

/// OAuth2 configuration for authentication.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OauthConfig {
    /// The JWKs URL of the OAuth2 provider.
    pub url: Url,
    /// Polling interval for JWKs updates.
    #[serde(default, deserialize_with = "deserialize_option_duration")]
    pub poll_interval: Option<Duration>,
    /// Expected issuer (iss claim) for token validation.
    pub expected_issuer: Option<String>,
    /// Expected audience (aud claim) for token validation.
    pub expected_audience: Option<String>,
    /// Protected resource configuration.
    pub protected_resource: ProtectedResourceConfig,
}

/// Configuration for OAuth2 protected resources.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtectedResourceConfig {
    /// The URL of the protected resource.
    pub resource: Url,
    /// List of authorization server URLs.
    pub authorization_servers: Vec<Url>,
}

impl ProtectedResourceConfig {
    /// Returns the resource documentation URL.
    pub fn resource_documentation(&self) -> Url {
        self.resource.join("/.well-known/oauth-protected-resource").unwrap()
    }
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
    use insta::assert_debug_snapshot;

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
                oauth: None,
                rate_limits: RateLimitConfig {
                    enabled: false,
                    storage: Memory,
                    global: None,
                    per_ip: None,
                },
                client_identification: None,
            },
            mcp: McpConfig {
                enabled: false,
                path: "/mcp-path",
                downstream_cache: McpDownstreamCacheConfig {
                    max_size: 1000,
                    idle_timeout: 600s,
                },
                servers: {},
                enable_structured_content: true,
                headers: [],
            },
            llm: LlmConfig {
                enabled: true,
                path: "/llm",
                providers: {},
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
                oauth: None,
                rate_limits: RateLimitConfig {
                    enabled: false,
                    storage: Memory,
                    global: None,
                    per_ip: None,
                },
                client_identification: None,
            },
            mcp: McpConfig {
                enabled: true,
                path: "/mcp",
                downstream_cache: McpDownstreamCacheConfig {
                    max_size: 1000,
                    idle_timeout: 600s,
                },
                servers: {},
                enable_structured_content: true,
                headers: [],
            },
            llm: LlmConfig {
                enabled: true,
                path: "/llm",
                providers: {},
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
            "local_code_interpreter": Stdio(
                StdioConfig {
                    cmd: [
                        "/usr/bin/mcp/code_interpreter_server",
                        "--json-output",
                    ],
                    env: {},
                    cwd: None,
                    stderr: Simple(
                        Null,
                    ),
                    rate_limits: None,
                },
            ),
        }
        "#);
    }

    #[test]
    fn mcp_stdio_server_with_env_and_cwd() {
        let config = indoc! {r#"
            [mcp.servers.local_interpreter]
            cmd = ["python", "-m", "mcp_server", "--port", "3000"]
            env = { PYTHONPATH = "/opt/mcp", DEBUG = "1" }
            cwd = "/tmp/mcp"
        "#};

        let config: Config = toml::from_str(config).expect("Failed to parse config");

        insta::assert_debug_snapshot!(&config.mcp.servers, @r#"
        {
            "local_interpreter": Stdio(
                StdioConfig {
                    cmd: [
                        "python",
                        "-m",
                        "mcp_server",
                        "--port",
                        "3000",
                    ],
                    env: {
                        "DEBUG": "1",
                        "PYTHONPATH": "/opt/mcp",
                    },
                    cwd: Some(
                        "/tmp/mcp",
                    ),
                    stderr: Simple(
                        Null,
                    ),
                    rate_limits: None,
                },
            ),
        }
        "#);
    }

    #[test]
    fn mcp_stdio_server_empty_command_fails() {
        let config = indoc! {r#"
            [mcp.servers.invalid]
            cmd = []
        "#};

        let result: Result<Config, _> = toml::from_str(config);
        assert!(result.is_err());

        let error_msg = result.unwrap_err().to_string();
        // The error occurs at the enum level because untagged enum can't match the variant
        // This still validates that empty commands are rejected at parse time
        assert!(error_msg.contains("data did not match any variant") || error_msg.contains("Command cannot be empty"));
    }

    #[test]
    fn mcp_stdio_server_minimal_config() {
        let config = indoc! {r#"
            [mcp.servers.simple]
            cmd = ["echo", "hello"]
        "#};

        let config: Config = toml::from_str(config).expect("Failed to parse config");

        insta::assert_debug_snapshot!(&config.mcp.servers, @r#"
        {
            "simple": Stdio(
                StdioConfig {
                    cmd: [
                        "echo",
                        "hello",
                    ],
                    env: {},
                    cwd: None,
                    stderr: Simple(
                        Null,
                    ),
                    rate_limits: None,
                },
            ),
        }
        "#);
    }

    #[test]
    fn mcp_stdio_server_single_command() {
        let config = indoc! {r#"
            [mcp.servers.single]
            cmd = ["./server"]
        "#};

        let config: Config = toml::from_str(config).expect("Failed to parse config");

        insta::assert_debug_snapshot!(&config.mcp.servers, @r#"
        {
            "single": Stdio(
                StdioConfig {
                    cmd: [
                        "./server",
                    ],
                    env: {},
                    cwd: None,
                    stderr: Simple(
                        Null,
                    ),
                    rate_limits: None,
                },
            ),
        }
        "#);
    }

    #[test]
    fn mcp_mixed_stdio_and_http_servers() {
        let config = indoc! {r#"
            [mcp.servers.stdio_server]
            cmd = ["python", "server.py"]

            [mcp.servers.http_server]
            url = "http://localhost:8080"
        "#};

        let config: Config = toml::from_str(config).expect("Failed to parse config");

        insta::assert_debug_snapshot!(&config.mcp.servers, @r#"
        {
            "http_server": Http(
                HttpConfig {
                    protocol: None,
                    url: Url {
                        scheme: "http",
                        cannot_be_a_base: false,
                        username: "",
                        password: None,
                        host: Some(
                            Domain(
                                "localhost",
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
                    auth: None,
                    rate_limits: None,
                    headers: [],
                },
            ),
            "stdio_server": Stdio(
                StdioConfig {
                    cmd: [
                        "python",
                        "server.py",
                    ],
                    env: {},
                    cwd: None,
                    stderr: Simple(
                        Null,
                    ),
                    rate_limits: None,
                },
            ),
        }
        "#);
    }

    #[test]
    fn mcp_stdio_server_with_stderr_config() {
        let config = indoc! {r#"
            [mcp.servers.configured_stdio]
            cmd = ["python", "server.py"]
            stderr = "inherit"

            [mcp.servers.file_logging_stdio]
            cmd = ["node", "server.js"]
            stderr = { file = "/tmp/server.log" }
        "#};

        let config: Config = toml::from_str(config).expect("Failed to parse config");

        insta::assert_debug_snapshot!(&config.mcp.servers, @r#"
        {
            "configured_stdio": Stdio(
                StdioConfig {
                    cmd: [
                        "python",
                        "server.py",
                    ],
                    env: {},
                    cwd: None,
                    stderr: Simple(
                        Inherit,
                    ),
                    rate_limits: None,
                },
            ),
            "file_logging_stdio": Stdio(
                StdioConfig {
                    cmd: [
                        "node",
                        "server.js",
                    ],
                    env: {},
                    cwd: None,
                    stderr: File {
                        file: "/tmp/server.log",
                    },
                    rate_limits: None,
                },
            ),
        }
        "#);
    }

    #[test]
    fn mcp_stdio_server_with_null_stderr() {
        let config = indoc! {r#"
            [mcp.servers.quiet_stdio]
            cmd = ["./quiet-server"]
            stderr = "null"
        "#};

        let config: Config = toml::from_str(config).expect("Failed to parse config");

        insta::assert_debug_snapshot!(&config.mcp.servers, @r#"
        {
            "quiet_stdio": Stdio(
                StdioConfig {
                    cmd: [
                        "./quiet-server",
                    ],
                    env: {},
                    cwd: None,
                    stderr: Simple(
                        Null,
                    ),
                    rate_limits: None,
                },
            ),
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
                    auth: None,
                    rate_limits: None,
                    headers: [],
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
                    auth: None,
                    rate_limits: None,
                    headers: [],
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
            downstream_cache: McpDownstreamCacheConfig {
                max_size: 1000,
                idle_timeout: 600s,
            },
            servers: {
                "another_stdio": Stdio(
                    StdioConfig {
                        cmd: [
                            "python",
                            "-m",
                            "mcp_server",
                            "--port",
                            "3000",
                        ],
                        env: {},
                        cwd: None,
                        stderr: Simple(
                            Null,
                        ),
                        rate_limits: None,
                    },
                ),
                "local_code_interpreter": Stdio(
                    StdioConfig {
                        cmd: [
                            "/usr/bin/mcp/code_interpreter_server",
                            "--json-output",
                        ],
                        env: {},
                        cwd: None,
                        stderr: Simple(
                            Null,
                        ),
                        rate_limits: None,
                    },
                ),
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
                        auth: None,
                        rate_limits: None,
                        headers: [],
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
                        auth: None,
                        rate_limits: None,
                        headers: [],
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
                        auth: None,
                        rate_limits: None,
                        headers: [],
                    },
                ),
            },
            enable_structured_content: true,
            headers: [],
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
                        Token {
                            token: SecretBox<str>([REDACTED]),
                        },
                    ),
                    rate_limits: None,
                    headers: [],
                },
            ),
        }
        "#);
    }

    #[test]
    fn mcp_server_forward_auth() {
        let config = indoc! {r#"
            [mcp.servers.github_api]
            protocol = "streamable-http"
            url = "https://api.githubcopilot.com/mcp/"

            [mcp.servers.github_api.auth]
            type = "forward"
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
                        Forward {
                            type: Forward,
                        },
                    ),
                    rate_limits: None,
                    headers: [],
                },
            ),
        }
        "#);
    }

    #[test]
    fn oauth_basic_config() {
        let config = indoc! {r#"
            [server.oauth]
            url = "https://auth.example.com/.well-known/jwks.json"
            poll_interval = "5m"

            [server.oauth.protected_resource]
            resource = "https://api.example.com"
            authorization_servers = ["https://auth.example.com"]
        "#};

        let config: Config = toml::from_str(config).unwrap();

        insta::assert_debug_snapshot!(&config.server.oauth, @r#"
        Some(
            OauthConfig {
                url: Url {
                    scheme: "https",
                    cannot_be_a_base: false,
                    username: "",
                    password: None,
                    host: Some(
                        Domain(
                            "auth.example.com",
                        ),
                    ),
                    port: None,
                    path: "/.well-known/jwks.json",
                    query: None,
                    fragment: None,
                },
                poll_interval: Some(
                    300s,
                ),
                expected_issuer: None,
                expected_audience: None,
                protected_resource: ProtectedResourceConfig {
                    resource: Url {
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
                    authorization_servers: [
                        Url {
                            scheme: "https",
                            cannot_be_a_base: false,
                            username: "",
                            password: None,
                            host: Some(
                                Domain(
                                    "auth.example.com",
                                ),
                            ),
                            port: None,
                            path: "/",
                            query: None,
                            fragment: None,
                        },
                    ],
                },
            },
        )
        "#);
    }

    #[test]
    fn oauth_minimal_config() {
        let config = indoc! {r#"
            [server.oauth]
            url = "https://auth.example.com/.well-known/jwks.json"

            [server.oauth.protected_resource]
            resource = "https://api.example.com"
            authorization_servers = ["https://auth.example.com"]
        "#};

        let config: Config = toml::from_str(config).unwrap();

        insta::assert_debug_snapshot!(&config.server.oauth, @r#"
        Some(
            OauthConfig {
                url: Url {
                    scheme: "https",
                    cannot_be_a_base: false,
                    username: "",
                    password: None,
                    host: Some(
                        Domain(
                            "auth.example.com",
                        ),
                    ),
                    port: None,
                    path: "/.well-known/jwks.json",
                    query: None,
                    fragment: None,
                },
                poll_interval: None,
                expected_issuer: None,
                expected_audience: None,
                protected_resource: ProtectedResourceConfig {
                    resource: Url {
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
                    authorization_servers: [
                        Url {
                            scheme: "https",
                            cannot_be_a_base: false,
                            username: "",
                            password: None,
                            host: Some(
                                Domain(
                                    "auth.example.com",
                                ),
                            ),
                            port: None,
                            path: "/",
                            query: None,
                            fragment: None,
                        },
                    ],
                },
            },
        )
        "#);
    }

    #[test]
    fn oauth_config_with_issuer_audience() {
        let config = indoc! {r#"
            [server.oauth]
            url = "https://auth.example.com/.well-known/jwks.json"
            poll_interval = "5m"
            expected_issuer = "https://auth.example.com"
            expected_audience = "my-app-client-id"

            [server.oauth.protected_resource]
            resource = "https://api.example.com"
            authorization_servers = ["https://auth.example.com"]
        "#};

        let config: Config = toml::from_str(config).unwrap();

        insta::assert_debug_snapshot!(&config.server.oauth, @r#"
        Some(
            OauthConfig {
                url: Url {
                    scheme: "https",
                    cannot_be_a_base: false,
                    username: "",
                    password: None,
                    host: Some(
                        Domain(
                            "auth.example.com",
                        ),
                    ),
                    port: None,
                    path: "/.well-known/jwks.json",
                    query: None,
                    fragment: None,
                },
                poll_interval: Some(
                    300s,
                ),
                expected_issuer: Some(
                    "https://auth.example.com",
                ),
                expected_audience: Some(
                    "my-app-client-id",
                ),
                protected_resource: ProtectedResourceConfig {
                    resource: Url {
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
                    authorization_servers: [
                        Url {
                            scheme: "https",
                            cannot_be_a_base: false,
                            username: "",
                            password: None,
                            host: Some(
                                Domain(
                                    "auth.example.com",
                                ),
                            ),
                            port: None,
                            path: "/",
                            query: None,
                            fragment: None,
                        },
                    ],
                },
            },
        )
        "#);
    }

    #[test]
    fn oauth_multiple_authorization_servers() {
        let config = indoc! {r#"
            [server.oauth]
            url = "https://auth.example.com/.well-known/jwks.json"

            [server.oauth.protected_resource]
            resource = "https://api.example.com"
            authorization_servers = [
                "https://auth1.example.com",
                "https://auth2.example.com",
                "https://auth3.example.com"
            ]
        "#};

        let config: Config = toml::from_str(config).unwrap();

        insta::assert_debug_snapshot!(&config.server.oauth, @r#"
        Some(
            OauthConfig {
                url: Url {
                    scheme: "https",
                    cannot_be_a_base: false,
                    username: "",
                    password: None,
                    host: Some(
                        Domain(
                            "auth.example.com",
                        ),
                    ),
                    port: None,
                    path: "/.well-known/jwks.json",
                    query: None,
                    fragment: None,
                },
                poll_interval: None,
                expected_issuer: None,
                expected_audience: None,
                protected_resource: ProtectedResourceConfig {
                    resource: Url {
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
                    authorization_servers: [
                        Url {
                            scheme: "https",
                            cannot_be_a_base: false,
                            username: "",
                            password: None,
                            host: Some(
                                Domain(
                                    "auth1.example.com",
                                ),
                            ),
                            port: None,
                            path: "/",
                            query: None,
                            fragment: None,
                        },
                        Url {
                            scheme: "https",
                            cannot_be_a_base: false,
                            username: "",
                            password: None,
                            host: Some(
                                Domain(
                                    "auth2.example.com",
                                ),
                            ),
                            port: None,
                            path: "/",
                            query: None,
                            fragment: None,
                        },
                        Url {
                            scheme: "https",
                            cannot_be_a_base: false,
                            username: "",
                            password: None,
                            host: Some(
                                Domain(
                                    "auth3.example.com",
                                ),
                            ),
                            port: None,
                            path: "/",
                            query: None,
                            fragment: None,
                        },
                    ],
                },
            },
        )
        "#);
    }

    #[test]
    fn oauth_resource_documentation() {
        let config = indoc! {r#"
            [server.oauth]
            url = "https://auth.example.com/.well-known/jwks.json"

            [server.oauth.protected_resource]
            resource = "https://api.example.com"
            authorization_servers = ["https://auth.example.com"]
        "#};

        let config: Config = toml::from_str(config).unwrap();
        let oauth_config = config.server.oauth.as_ref().unwrap();

        let resource_doc_url = oauth_config.protected_resource.resource_documentation();
        insta::assert_debug_snapshot!(&resource_doc_url.as_str(), @r#""https://api.example.com/.well-known/oauth-protected-resource""#);
    }

    #[test]
    fn oauth_with_various_poll_intervals() {
        let config = indoc! {r#"
            [server.oauth]
            url = "https://auth.example.com/.well-known/jwks.json"
            poll_interval = "30s"

            [server.oauth.protected_resource]
            resource = "https://api.example.com"
            authorization_servers = ["https://auth.example.com"]
        "#};

        let config: Config = toml::from_str(config).unwrap();

        insta::assert_debug_snapshot!(&config.server.oauth, @r#"
        Some(
            OauthConfig {
                url: Url {
                    scheme: "https",
                    cannot_be_a_base: false,
                    username: "",
                    password: None,
                    host: Some(
                        Domain(
                            "auth.example.com",
                        ),
                    ),
                    port: None,
                    path: "/.well-known/jwks.json",
                    query: None,
                    fragment: None,
                },
                poll_interval: Some(
                    30s,
                ),
                expected_issuer: None,
                expected_audience: None,
                protected_resource: ProtectedResourceConfig {
                    resource: Url {
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
                    authorization_servers: [
                        Url {
                            scheme: "https",
                            cannot_be_a_base: false,
                            username: "",
                            password: None,
                            host: Some(
                                Domain(
                                    "auth.example.com",
                                ),
                            ),
                            port: None,
                            path: "/",
                            query: None,
                            fragment: None,
                        },
                    ],
                },
            },
        )
        "#);
    }

    #[test]
    fn oauth_invalid_url_should_fail() {
        let config = indoc! {r#"
            [server.oauth]
            url = "not-a-valid-url"

            [server.oauth.protected_resource]
            resource = "https://api.example.com"
            authorization_servers = ["https://auth.example.com"]
        "#};

        let result: Result<Config, _> = toml::from_str(config);
        assert!(result.is_err());
    }

    #[test]
    fn oauth_invalid_authorization_server_url_should_fail() {
        let config = indoc! {r#"
            [server.oauth]
            url = "https://auth.example.com/.well-known/jwks.json"

            [server.oauth.protected_resource]
            resource = "https://api.example.com"
            authorization_servers = ["not-a-valid-url"]
        "#};

        let result: Result<Config, _> = toml::from_str(config);
        assert!(result.is_err());
    }

    #[test]
    fn rate_limit_default_config() {
        let config: Config = toml::from_str("").unwrap();

        insta::assert_debug_snapshot!(&config.server.rate_limits, @r"
        RateLimitConfig {
            enabled: false,
            storage: Memory,
            global: None,
            per_ip: None,
        }
        ");
    }

    #[test]
    fn rate_limit_full_config() {
        let config = indoc! {r#"
            [server.rate_limits]
            enabled = true

            [server.rate_limits.global]
            limit = 10000
            interval = "60s"

            [server.rate_limits.per_ip]
            limit = 60
            interval = "60s"
        "#};

        let config: Config = toml::from_str(config).unwrap();

        insta::assert_debug_snapshot!(&config.server.rate_limits, @r#"
        RateLimitConfig {
            enabled: true,
            storage: Memory,
            global: Some(
                RateLimitQuota {
                    limit: 10000,
                    interval: 60s,
                },
            ),
            per_ip: Some(
                RateLimitQuota {
                    limit: 60,
                    interval: 60s,
                },
            ),
        }
        "#);
    }

    #[test]
    fn llm_config_with_providers() {
        let config = indoc! {r#"
            [llm]
            enabled = true
            path = "/ai"

            [llm.providers.openai]
            type = "openai"
            api_key = "${OPENAI_API_KEY}"

            [llm.providers.openai.models.gpt-4]

            [llm.providers.anthropic]
            type = "anthropic"
            api_key = "{{ env.ANTHROPIC_API_KEY }}"

            [llm.providers.anthropic.models.claude-3-opus]
        "#};

        let config: Config = toml::from_str(config).unwrap();

        insta::assert_debug_snapshot!(&config.llm, @r#"
        LlmConfig {
            enabled: true,
            path: "/ai",
            providers: {
                "anthropic": Anthropic(
                    ApiProviderConfig {
                        api_key: Some(
                            SecretBox<str>([REDACTED]),
                        ),
                        base_url: None,
                        forward_token: false,
                        models: {
                            "claude-3-opus": ApiModelConfig {
                                rename: None,
                                rate_limits: None,
                                headers: [],
                            },
                        },
                        rate_limits: None,
                        headers: [],
                    },
                ),
                "openai": Openai(
                    ApiProviderConfig {
                        api_key: Some(
                            SecretBox<str>([REDACTED]),
                        ),
                        base_url: None,
                        forward_token: false,
                        models: {
                            "gpt-4": ApiModelConfig {
                                rename: None,
                                rate_limits: None,
                                headers: [],
                            },
                        },
                        rate_limits: None,
                        headers: [],
                    },
                ),
            },
        }
        "#);
    }

    #[test]
    fn client_identification_config() {
        let config = indoc! {r#"
            [server.client_identification]
            enabled = true
            client_id.jwt_claim = "sub"
            group_id.jwt_claim = "plan"

            [server.client_identification.validation]
            group_values = ["free", "pro", "enterprise"]
        "#};

        let config: Config = toml::from_str(config).unwrap();

        assert_debug_snapshot!(&config.server.client_identification, @r#"
        Some(
            ClientIdentificationConfig {
                enabled: true,
                validation: ClientIdentificationValidation {
                    group_values: {
                        "enterprise",
                        "free",
                        "pro",
                    },
                },
                client_id: JwtClaim {
                    jwt_claim: "sub",
                },
                group_id: Some(
                    JwtClaim {
                        jwt_claim: "plan",
                    },
                ),
            },
        )
        "#);
    }

    #[test]
    fn client_identification_http_headers() {
        let config = indoc! {r#"
            [server.client_identification]
            enabled = true
            client_id.http_header = "X-Client-Id"
            group_id.http_header = "X-Plan"

            [server.client_identification.validation]
            group_values = ["basic", "premium"]
        "#};

        let config: Config = toml::from_str(config).unwrap();

        assert_debug_snapshot!(&config.server.client_identification, @r#"
        Some(
            ClientIdentificationConfig {
                enabled: true,
                validation: ClientIdentificationValidation {
                    group_values: {
                        "basic",
                        "premium",
                    },
                },
                client_id: HttpHeader {
                    http_header: "X-Client-Id",
                },
                group_id: Some(
                    HttpHeader {
                        http_header: "X-Plan",
                    },
                ),
            },
        )
        "#);
    }

    #[test]
    fn rate_limit_validation_with_groups() {
        let config = indoc! {r#"
            [server.client_identification]
            enabled = true
            client_id.jwt_claim = "sub"
            group_id.jwt_claim = "plan"

            [server.client_identification.validation]
            group_values = ["free", "pro"]

            [llm.providers.openai]
            type = "openai"
            api_key = "test-key"

            [llm.providers.openai.rate_limits.per_user]
            input_token_limit = 50000
            interval = "60s"

            [llm.providers.openai.rate_limits.per_user.groups]
            free = { input_token_limit = 10000, interval = "60s" }
            pro = { input_token_limit = 100000, interval = "60s" }

            [llm.providers.openai.models.gpt-4]
        "#};

        let config: Config = toml::from_str(config).unwrap();
        let warnings = crate::loader::validate_rate_limits(&config).unwrap();

        // Should have warnings about model fallbacks
        insta::assert_debug_snapshot!(warnings, @r#"
        [
            "Group 'free' for model 'openai/gpt-4' will use provider group rate limit",
            "Group 'pro' for model 'openai/gpt-4' will use provider group rate limit",
        ]
        "#);
    }

    #[test]
    fn rate_limits_without_client_identification_fails() {
        let config = indoc! {r#"
            [server.client_identification]
            enabled = false

            [llm.providers.openai]
            type = "openai"
            api_key = "test-key"

            [llm.providers.openai.rate_limits.per_user]
            input_token_limit = 10000
            interval = "60s"

            [llm.providers.openai.models.gpt-4]
        "#};

        let config: Config = toml::from_str(config).unwrap();
        let result = crate::loader::validate_rate_limits(&config);

        assert!(result.is_err());
        let error = result.unwrap_err().to_string();

        insta::assert_snapshot!(error, @"LLM rate limits are configured but client identification is not enabled. Enable client identification in [server.client_identification]");
    }

    #[test]
    fn model_rate_limits_without_client_identification_fails() {
        let config = indoc! {r#"
            [server.client_identification]
            enabled = false

            [llm.providers.openai]
            type = "openai"
            api_key = "test-key"

            [llm.providers.openai.models.gpt-4.rate_limits.per_user]
            input_token_limit = 5000
            interval = "60s"
        "#};

        let config: Config = toml::from_str(config).unwrap();
        let result = crate::loader::validate_rate_limits(&config);

        assert!(result.is_err());
        let error = result.unwrap_err().to_string();
        insta::assert_snapshot!(error, @"LLM rate limits are configured but client identification is not enabled. Enable client identification in [server.client_identification]");
    }

    #[test]
    fn group_id_without_allowed_groups_fails() {
        let config = indoc! {r#"
            [server.client_identification]
            enabled = true
            client_id.jwt_claim = "sub"
            group_id.jwt_claim = "plan"

            [llm.providers.openai]
            type = "openai"
            api_key = "test-key"

            [llm.providers.openai.rate_limits.per_user]
            input_token_limit = 5000
            interval = "60s"

            [llm.providers.openai.models.gpt-4]
        "#};

        let config: Config = toml::from_str(config).unwrap();
        let result = crate::loader::validate_rate_limits(&config);

        assert!(result.is_err());
        let error = result.unwrap_err().to_string();

        insta::assert_snapshot!(error, @"group_id is configured for client identification but validation.group_values is empty. Define group_values in [server.client_identification.validation]");
    }

    #[test]
    fn group_rate_limits_without_group_id_fails() {
        let config = indoc! {r#"
            [server.client_identification]
            enabled = true
            client_id.jwt_claim = "sub"

            [llm.providers.openai]
            type = "openai"
            api_key = "test-key"

            [llm.providers.openai.rate_limits.per_user]
            input_token_limit = 5000
            interval = "60s"

            [llm.providers.openai.rate_limits.per_user.groups]
            free = { input_token_limit = 10000, interval = "60s" }

            [llm.providers.openai.models.gpt-4]
        "#};

        let config: Config = toml::from_str(config).unwrap();
        let result = crate::loader::validate_rate_limits(&config);

        assert!(result.is_err());
        let error = result.unwrap_err().to_string();

        insta::assert_snapshot!(error, @r#"
        Group-based rate limits are configured but group_id is not set in client identification.
        To fix this, add a group_id configuration to your [server.client_identification] section, for example:

        [server.client_identification]
        enabled = true
        client_id.http_header = "X-Client-ID"      # or client_id.jwt_claim = "sub"
        group_id.http_header = "X-Group-ID"        # or group_id.jwt_claim = "groups"

        [server.client_identification.validation]
        group_values = ["basic", "premium", "enterprise"]
        "#);
    }

    #[test]
    fn rate_limit_validation_invalid_group() {
        let config = indoc! {r#"
            [server.client_identification]
            enabled = true
            client_id.jwt_claim = "sub"
            group_id.jwt_claim = "plan"

            [server.client_identification.validation]
            group_values = ["free", "pro"]

            [llm.providers.openai]
            type = "openai"
            api_key = "test-key"

            [llm.providers.openai.rate_limits.per_user]
            input_token_limit = 50000
            interval = "60s"

            [llm.providers.openai.rate_limits.per_user.groups]
            enterprise = { input_token_limit = 1000000, interval = "60s" }

            [llm.providers.openai.models.gpt-4]
        "#};

        let config: Config = toml::from_str(config).unwrap();
        let result = crate::loader::validate_rate_limits(&config);

        assert!(result.is_err());
        let error = result.unwrap_err().to_string();

        insta::assert_snapshot!(error, @"Group 'enterprise' in provider 'openai' rate limits not found in group_values");
    }

    #[test]
    fn mcp_server_rate_limits() {
        let config = indoc! {r#"
            [mcp.servers.github_api]
            url = "https://api.github.com/mcp"
            [mcp.servers.github_api.rate_limits]
            limit = 30
            interval = "60s"
            [mcp.servers.github_api.rate_limits.tools]
            search = { limit = 60, interval = "60s" }
            create_issue = { limit = 10, interval = "60s" }

            [mcp.servers.local_tool]
            cmd = ["python", "server.py"]
            [mcp.servers.local_tool.rate_limits]
            limit = 100
            interval = "60s"
        "#};

        let config: Config = toml::from_str(config).unwrap();

        insta::assert_debug_snapshot!(&config.mcp.servers, @r#"
        {
            "github_api": Http(
                HttpConfig {
                    protocol: None,
                    url: Url {
                        scheme: "https",
                        cannot_be_a_base: false,
                        username: "",
                        password: None,
                        host: Some(
                            Domain(
                                "api.github.com",
                            ),
                        ),
                        port: None,
                        path: "/mcp",
                        query: None,
                        fragment: None,
                    },
                    tls: None,
                    message_url: None,
                    auth: None,
                    rate_limits: Some(
                        McpServerRateLimit {
                            limit: 30,
                            interval: 60s,
                            tools: {
                                "create_issue": RateLimitQuota {
                                    limit: 10,
                                    interval: 60s,
                                },
                                "search": RateLimitQuota {
                                    limit: 60,
                                    interval: 60s,
                                },
                            },
                        },
                    ),
                    headers: [],
                },
            ),
            "local_tool": Stdio(
                StdioConfig {
                    cmd: [
                        "python",
                        "server.py",
                    ],
                    env: {},
                    cwd: None,
                    stderr: Simple(
                        Null,
                    ),
                    rate_limits: Some(
                        McpServerRateLimit {
                            limit: 100,
                            interval: 60s,
                            tools: {},
                        },
                    ),
                },
            ),
        }
        "#);
    }

    #[test]
    fn mcp_enable_structured_content() {
        // Test that enable_structured_content defaults to true
        let config = indoc! {r#"
            [mcp]
            enabled = true
        "#};

        let config: Config = toml::from_str(config).unwrap();
        assert!(config.mcp.enable_structured_content);

        // Test explicit true
        let config = indoc! {r#"
            [mcp]
            enabled = true
            enable_structured_content = true
        "#};

        let config: Config = toml::from_str(config).unwrap();
        assert!(config.mcp.enable_structured_content);

        // Test explicit false
        let config = indoc! {r#"
            [mcp]
            enabled = true
            enable_structured_content = false
        "#};

        let config: Config = toml::from_str(config).unwrap();
        assert!(!config.mcp.enable_structured_content);

        // Test that Default trait gives us true
        let default_config = crate::McpConfig::default();
        assert!(default_config.enable_structured_content);
    }

    #[test]
    fn validation_logic_identifies_no_downstreams() {
        // Test that validation logic correctly identifies when no downstreams are configured
        // Note: We no longer prevent startup, but the validation logic is still available if needed
        let config = Config::default();
        let result = crate::loader::validate_has_downstreams(&config);
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();

        insta::assert_snapshot!(error_msg, @r#"
        No downstream servers configured. Nexus requires at least one MCP server or LLM provider to function.

        Example configuration:

        For MCP servers:

          [mcp.servers.example]
          cmd = ["path/to/mcp-server"]

        For LLM providers:

          [llm.providers.openai]
          type = "openai"
          api_key = "{{ env.OPENAI_API_KEY }}"

        See https://nexusrouter.com/docs for more configuration examples.
        "#);
    }

    #[test]
    fn validation_fails_when_both_disabled() {
        let config_str = indoc! {r#"
            [mcp]
            enabled = false

            [llm]
            enabled = false
        "#};

        let config: Config = toml::from_str(config_str).unwrap();
        let result = crate::loader::validate_has_downstreams(&config);
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();

        insta::assert_snapshot!(error_msg, @r#"
        No downstream servers configured. Nexus requires at least one MCP server or LLM provider to function.

        Example configuration:

        For MCP servers:

          [mcp.servers.example]
          cmd = ["path/to/mcp-server"]

        For LLM providers:

          [llm.providers.openai]
          type = "openai"
          api_key = "{{ env.OPENAI_API_KEY }}"

        See https://nexusrouter.com/docs for more configuration examples.
        "#);
    }

    #[test]
    fn validation_fails_when_mcp_enabled_but_no_servers() {
        let config_str = indoc! {r#"
            [mcp]
            enabled = true

            [llm]
            enabled = false
        "#};

        let config: Config = toml::from_str(config_str).unwrap();
        let result = crate::loader::validate_has_downstreams(&config);
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();

        insta::assert_snapshot!(error_msg, @r#"
        No downstream servers configured. Nexus requires at least one MCP server or LLM provider to function.

        Example configuration:

        For MCP servers:

          [mcp.servers.example]
          cmd = ["path/to/mcp-server"]

        For LLM providers:

          [llm.providers.openai]
          type = "openai"
          api_key = "{{ env.OPENAI_API_KEY }}"

        See https://nexusrouter.com/docs for more configuration examples.
        "#);
    }

    #[test]
    fn validation_fails_when_llm_enabled_but_no_providers() {
        let config_str = indoc! {r#"
            [mcp]
            enabled = false

            [llm]
            enabled = true
        "#};

        let config: Config = toml::from_str(config_str).unwrap();
        let result = crate::loader::validate_has_downstreams(&config);
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();

        insta::assert_snapshot!(error_msg, @r#"
        No downstream servers configured. Nexus requires at least one MCP server or LLM provider to function.

        Example configuration:

        For MCP servers:

          [mcp.servers.example]
          cmd = ["path/to/mcp-server"]

        For LLM providers:

          [llm.providers.openai]
          type = "openai"
          api_key = "{{ env.OPENAI_API_KEY }}"

        See https://nexusrouter.com/docs for more configuration examples.
        "#);
    }

    #[test]
    fn validation_passes_with_mcp_server() {
        let config_str = indoc! {r#"
            [mcp.servers.test]
            cmd = ["echo", "test"]
        "#};

        let config: Config = toml::from_str(config_str).unwrap();
        let result = crate::loader::validate_has_downstreams(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn validation_passes_with_llm_provider() {
        let config_str = indoc! {r#"
            [llm.providers.openai]
            type = "openai"
            api_key = "test-key"

            [llm.providers.openai.models.gpt-4]
        "#};

        let config: Config = toml::from_str(config_str).unwrap();
        let result = crate::loader::validate_has_downstreams(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn validation_passes_with_both_mcp_and_llm() {
        let config_str = indoc! {r#"
            [mcp.servers.test]
            cmd = ["echo", "test"]

            [llm.providers.openai]
            type = "openai"
            api_key = "test-key"

            [llm.providers.openai.models.gpt-4]
        "#};

        let config: Config = toml::from_str(config_str).unwrap();
        let result = crate::loader::validate_has_downstreams(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn validation_passes_when_mcp_disabled_but_llm_has_providers() {
        let config_str = indoc! {r#"
            [mcp]
            enabled = false

            [llm.providers.openai]
            type = "openai"
            api_key = "test-key"

            [llm.providers.openai.models.gpt-4]
        "#};

        let config: Config = toml::from_str(config_str).unwrap();
        let result = crate::loader::validate_has_downstreams(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn validation_passes_when_llm_disabled_but_mcp_has_servers() {
        let config_str = indoc! {r#"
            [mcp.servers.test]
            cmd = ["echo", "test"]

            [llm]
            enabled = false
        "#};

        let config: Config = toml::from_str(config_str).unwrap();
        let result = crate::loader::validate_has_downstreams(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn llm_provider_header_rules() {
        let config = indoc! {r#"
            [llm.providers.openai]
            type = "openai"
            api_key = "test-key"

            [[llm.providers.openai.headers]]
            rule = "insert"
            name = "x-custom-header"
            value = "provider-value"

            [[llm.providers.openai.headers]]
            rule = "forward"
            name = "x-request-id"

            [[llm.providers.openai.headers]]
            rule = "remove"
            name = "x-internal-header"

            [llm.providers.openai.models.gpt-4]
        "#};

        let config: Config = toml::from_str(config).unwrap();

        insta::assert_debug_snapshot!(&config.llm.providers["openai"], @r#"
        Openai(
            ApiProviderConfig {
                api_key: Some(
                    SecretBox<str>([REDACTED]),
                ),
                base_url: None,
                forward_token: false,
                models: {
                    "gpt-4": ApiModelConfig {
                        rename: None,
                        rate_limits: None,
                        headers: [],
                    },
                },
                rate_limits: None,
                headers: [
                    Insert(
                        HeaderInsert {
                            name: HeaderName(
                                "x-custom-header",
                            ),
                            value: HeaderValue(
                                "provider-value",
                            ),
                        },
                    ),
                    Forward(
                        HeaderForward {
                            name: Name(
                                HeaderName(
                                    "x-request-id",
                                ),
                            ),
                            default: None,
                            rename: None,
                        },
                    ),
                    Remove(
                        HeaderRemove {
                            name: Name(
                                HeaderName(
                                    "x-internal-header",
                                ),
                            ),
                        },
                    ),
                ],
            },
        )
        "#);
    }

    #[test]
    fn llm_model_header_rules_override() {
        let config = indoc! {r#"
            [llm.providers.openai]
            type = "openai"
            api_key = "test-key"

            [[llm.providers.openai.headers]]
            rule = "insert"
            name = "x-tier"
            value = "basic"

            [llm.providers.openai.models.gpt-3]

            [llm.providers.openai.models.gpt-4]
            [[llm.providers.openai.models.gpt-4.headers]]
            rule = "insert"
            name = "x-tier"
            value = "premium"

            [[llm.providers.openai.models.gpt-4.headers]]
            rule = "insert"
            name = "x-model-specific"
            value = "gpt4-only"
        "#};

        let config: Config = toml::from_str(config).unwrap();

        insta::assert_debug_snapshot!(&config.llm.providers["openai"], @r#"
        Openai(
            ApiProviderConfig {
                api_key: Some(
                    SecretBox<str>([REDACTED]),
                ),
                base_url: None,
                forward_token: false,
                models: {
                    "gpt-3": ApiModelConfig {
                        rename: None,
                        rate_limits: None,
                        headers: [],
                    },
                    "gpt-4": ApiModelConfig {
                        rename: None,
                        rate_limits: None,
                        headers: [
                            Insert(
                                HeaderInsert {
                                    name: HeaderName(
                                        "x-tier",
                                    ),
                                    value: HeaderValue(
                                        "premium",
                                    ),
                                },
                            ),
                            Insert(
                                HeaderInsert {
                                    name: HeaderName(
                                        "x-model-specific",
                                    ),
                                    value: HeaderValue(
                                        "gpt4-only",
                                    ),
                                },
                            ),
                        ],
                    },
                },
                rate_limits: None,
                headers: [
                    Insert(
                        HeaderInsert {
                            name: HeaderName(
                                "x-tier",
                            ),
                            value: HeaderValue(
                                "basic",
                            ),
                        },
                    ),
                ],
            },
        )
        "#);
    }

    #[test]
    fn mcp_header_rules_hierarchy() {
        let config = indoc! {r#"
            # Service-level headers (MCP only supports insert)
            [[mcp.headers]]
            rule = "insert"
            name = "x-service"
            value = "nexus"

            [[mcp.headers]]
            rule = "insert"
            name = "x-trace-id"
            value = "{{ env.TRACE_ID }}"

            # Server-level headers (HTTP server for tool support)
            [mcp.servers.database]
            url = "http://localhost:8080/mcp"

            [[mcp.servers.database.headers]]
            rule = "insert"
            name = "x-server"
            value = "database"

            [[mcp.servers.database.headers]]
            rule = "insert"
            name = "x-env"
            value = "prod"
        "#};

        let config: Config = toml::from_str(config).unwrap();

        // Snapshot the entire MCP config including all header rules at different levels
        insta::assert_debug_snapshot!(&config.mcp, @r#"
        McpConfig {
            enabled: true,
            path: "/mcp",
            downstream_cache: McpDownstreamCacheConfig {
                max_size: 1000,
                idle_timeout: 600s,
            },
            servers: {
                "database": Http(
                    HttpConfig {
                        protocol: None,
                        url: Url {
                            scheme: "http",
                            cannot_be_a_base: false,
                            username: "",
                            password: None,
                            host: Some(
                                Domain(
                                    "localhost",
                                ),
                            ),
                            port: Some(
                                8080,
                            ),
                            path: "/mcp",
                            query: None,
                            fragment: None,
                        },
                        tls: None,
                        message_url: None,
                        auth: None,
                        rate_limits: None,
                        headers: [
                            Insert(
                                HeaderInsert {
                                    name: HeaderName(
                                        "x-server",
                                    ),
                                    value: HeaderValue(
                                        "database",
                                    ),
                                },
                            ),
                            Insert(
                                HeaderInsert {
                                    name: HeaderName(
                                        "x-env",
                                    ),
                                    value: HeaderValue(
                                        "prod",
                                    ),
                                },
                            ),
                        ],
                    },
                ),
            },
            enable_structured_content: true,
            headers: [
                Insert(
                    HeaderInsert {
                        name: HeaderName(
                            "x-service",
                        ),
                        value: HeaderValue(
                            "nexus",
                        ),
                    },
                ),
                Insert(
                    HeaderInsert {
                        name: HeaderName(
                            "x-trace-id",
                        ),
                        value: HeaderValue(
                            "{{ env.TRACE_ID }}",
                        ),
                    },
                ),
            ],
        }
        "#);
    }

    #[test]
    fn llm_header_forward_with_rename() {
        // Test forward with rename for LLM providers (not MCP)
        let config = indoc! {r#"
            [llm.providers.openai]
            type = "openai"
            api_key = "test"

            [[llm.providers.openai.headers]]
            rule = "forward"
            name = "x-old-header"
            rename = "x-new-header"

            [[llm.providers.openai.headers]]
            rule = "forward"
            pattern = "^x-custom-"
            rename = "x-forwarded-custom-"

            [llm.providers.openai.models.gpt-4]
        "#};

        let config: Config = toml::from_str(config).unwrap();

        insta::assert_debug_snapshot!(&config.llm.providers["openai"], @r#"
        Openai(
            ApiProviderConfig {
                api_key: Some(
                    SecretBox<str>([REDACTED]),
                ),
                base_url: None,
                forward_token: false,
                models: {
                    "gpt-4": ApiModelConfig {
                        rename: None,
                        rate_limits: None,
                        headers: [],
                    },
                },
                rate_limits: None,
                headers: [
                    Forward(
                        HeaderForward {
                            name: Name(
                                HeaderName(
                                    "x-old-header",
                                ),
                            ),
                            default: None,
                            rename: Some(
                                HeaderName(
                                    "x-new-header",
                                ),
                            ),
                        },
                    ),
                    Forward(
                        HeaderForward {
                            name: Pattern(
                                NamePattern(
                                    Regex(
                                        "^x-custom-",
                                    ),
                                ),
                            ),
                            default: None,
                            rename: Some(
                                HeaderName(
                                    "x-forwarded-custom-",
                                ),
                            ),
                        },
                    ),
                ],
            },
        )
        "#);
    }

    #[test]
    fn llm_header_remove_patterns() {
        // Test remove patterns for LLM providers (not MCP)
        let config = indoc! {r#"
            [llm.providers.anthropic]
            type = "anthropic"
            api_key = "test"

            [[llm.providers.anthropic.headers]]
            rule = "remove"
            name = "x-specific-header"

            [[llm.providers.anthropic.headers]]
            rule = "remove"
            pattern = "^x-internal-"

            [[llm.providers.anthropic.headers]]
            rule = "remove"
            pattern = "^x-debug-.*-temp$"

            [llm.providers.anthropic.models.claude-3]
        "#};

        let config: Config = toml::from_str(config).unwrap();

        insta::assert_debug_snapshot!(&config.llm.providers["anthropic"], @r#"
        Anthropic(
            ApiProviderConfig {
                api_key: Some(
                    SecretBox<str>([REDACTED]),
                ),
                base_url: None,
                forward_token: false,
                models: {
                    "claude-3": ApiModelConfig {
                        rename: None,
                        rate_limits: None,
                        headers: [],
                    },
                },
                rate_limits: None,
                headers: [
                    Remove(
                        HeaderRemove {
                            name: Name(
                                HeaderName(
                                    "x-specific-header",
                                ),
                            ),
                        },
                    ),
                    Remove(
                        HeaderRemove {
                            name: Pattern(
                                NamePattern(
                                    Regex(
                                        "^x-internal-",
                                    ),
                                ),
                            ),
                        },
                    ),
                    Remove(
                        HeaderRemove {
                            name: Pattern(
                                NamePattern(
                                    Regex(
                                        "^x-debug-.*-temp$",
                                    ),
                                ),
                            ),
                        },
                    ),
                ],
            },
        )
        "#);
    }

    #[test]
    fn llm_header_rename_duplicate_with_default() {
        // Test rename_duplicate for LLM providers (not MCP)
        let config = indoc! {r#"
            [llm.providers.google]
            type = "google"
            api_key = "test"

            [[llm.providers.google.headers]]
            rule = "rename_duplicate"
            name = "authorization"
            rename = "x-original-auth"
            default = "Bearer default-token"

            [llm.providers.google.models."gemini-1.5-flash"]
        "#};

        let config: Config = toml::from_str(config).unwrap();

        insta::assert_debug_snapshot!(&config.llm.providers["google"], @r#"
        Google(
            ApiProviderConfig {
                api_key: Some(
                    SecretBox<str>([REDACTED]),
                ),
                base_url: None,
                forward_token: false,
                models: {
                    "gemini-1.5-flash": ApiModelConfig {
                        rename: None,
                        rate_limits: None,
                        headers: [],
                    },
                },
                rate_limits: None,
                headers: [
                    RenameDuplicate(
                        HeaderRenameDuplicate {
                            name: HeaderName(
                                "authorization",
                            ),
                            default: Some(
                                HeaderValue(
                                    "Bearer default-token",
                                ),
                            ),
                            rename: HeaderName(
                                "x-original-auth",
                            ),
                        },
                    ),
                ],
            },
        )
        "#);
    }

    #[test]
    fn complex_header_rules_combination() {
        let config = indoc! {r#"
            # Global MCP headers (only insert supported)
            [[mcp.headers]]
            rule = "insert"
            name = "x-global"
            value = "mcp-global"

            [[mcp.headers]]
            rule = "insert"
            name = "x-trace"
            value = "enabled"

            # HTTP server with headers and tools
            [mcp.servers.http_api]
            url = "https://api.example.com/mcp"

            [[mcp.servers.http_api.headers]]
            rule = "insert"
            name = "x-api-key"
            value = "{{ env.API_KEY }}"

            [[mcp.servers.http_api.headers]]
            rule = "insert"
            name = "x-source"
            value = "nexus"

            # STDIO server (headers not supported for STDIO transport)
            [mcp.servers.local_tool]
            cmd = ["python", "tool.py"]
        "#};

        let config: Config = toml::from_str(config).unwrap();

        // Snapshot the entire MCP config to verify all header rules
        insta::assert_debug_snapshot!(&config.mcp, @r#"
        McpConfig {
            enabled: true,
            path: "/mcp",
            downstream_cache: McpDownstreamCacheConfig {
                max_size: 1000,
                idle_timeout: 600s,
            },
            servers: {
                "http_api": Http(
                    HttpConfig {
                        protocol: None,
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
                            path: "/mcp",
                            query: None,
                            fragment: None,
                        },
                        tls: None,
                        message_url: None,
                        auth: None,
                        rate_limits: None,
                        headers: [
                            Insert(
                                HeaderInsert {
                                    name: HeaderName(
                                        "x-api-key",
                                    ),
                                    value: HeaderValue(
                                        "{{ env.API_KEY }}",
                                    ),
                                },
                            ),
                            Insert(
                                HeaderInsert {
                                    name: HeaderName(
                                        "x-source",
                                    ),
                                    value: HeaderValue(
                                        "nexus",
                                    ),
                                },
                            ),
                        ],
                    },
                ),
                "local_tool": Stdio(
                    StdioConfig {
                        cmd: [
                            "python",
                            "tool.py",
                        ],
                        env: {},
                        cwd: None,
                        stderr: Simple(
                            Null,
                        ),
                        rate_limits: None,
                    },
                ),
            },
            enable_structured_content: true,
            headers: [
                Insert(
                    HeaderInsert {
                        name: HeaderName(
                            "x-global",
                        ),
                        value: HeaderValue(
                            "mcp-global",
                        ),
                    },
                ),
                Insert(
                    HeaderInsert {
                        name: HeaderName(
                            "x-trace",
                        ),
                        value: HeaderValue(
                            "enabled",
                        ),
                    },
                ),
            ],
        }
        "#);
    }

    #[test]
    fn stdio_server_rejects_headers() {
        // Test that STDIO servers cannot have headers configured
        let config = indoc! {r#"
            [mcp.servers.local_tool]
            cmd = ["python", "tool.py"]

            [[mcp.servers.local_tool.headers]]
            rule = "insert"
            name = "x-not-allowed"
            value = "should-fail"
        "#};

        // This should fail to parse because STDIO servers don't support headers
        let result: Result<Config, _> = toml::from_str(config);
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.to_string().contains("did not match any variant"));
    }

    #[test]
    fn parse_full_header_rules_config() {
        let config = indoc! {r#"
            [mcp]
            enabled = true

            # Global MCP header rules (only insert supported)
            [[mcp.headers]]
            rule = "insert"
            name = "x-system"
            value = "nexus"

            [[mcp.headers]]
            rule = "insert"
            name = "x-service"
            value = "nexus"

            [mcp.servers.test_server]
            url = "http://localhost:8080"

            # Server-specific headers (only insert supported)
            [[mcp.servers.test_server.headers]]
            rule = "insert"
            name = "x-server"
            value = "test_server"

            [[mcp.servers.test_server.headers]]
            rule = "insert"
            name = "x-tier"
            value = "premium"

            [llm]
            enabled = true

            [llm.providers.openai]
            type = "openai"
            api_key = "test-key"

            # Provider-level headers
            [[llm.providers.openai.headers]]
            rule = "insert"
            name = "x-provider"
            value = "openai"

            [llm.providers.openai.models.gpt-4]

            # Model-specific headers
            [[llm.providers.openai.models.gpt-4.headers]]
            rule = "insert"
            name = "x-model-tier"
            value = "advanced"

            [llm.providers.anthropic]
            type = "anthropic"
            api_key = "test-key"

            # Using environment variable template
            [[llm.providers.anthropic.headers]]
            rule = "insert"
            name = "x-api-key"
            value = "{{ env.ANTHROPIC_KEY }}"

            [llm.providers.anthropic.models.claude-3]
        "#};

        let parsed: Config = toml::from_str(config).unwrap();

        // Snapshot entire config to verify all header rules
        insta::assert_debug_snapshot!(parsed, @r#"
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
                oauth: None,
                rate_limits: RateLimitConfig {
                    enabled: false,
                    storage: Memory,
                    global: None,
                    per_ip: None,
                },
                client_identification: None,
            },
            mcp: McpConfig {
                enabled: true,
                path: "/mcp",
                downstream_cache: McpDownstreamCacheConfig {
                    max_size: 1000,
                    idle_timeout: 600s,
                },
                servers: {
                    "test_server": Http(
                        HttpConfig {
                            protocol: None,
                            url: Url {
                                scheme: "http",
                                cannot_be_a_base: false,
                                username: "",
                                password: None,
                                host: Some(
                                    Domain(
                                        "localhost",
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
                            auth: None,
                            rate_limits: None,
                            headers: [
                                Insert(
                                    HeaderInsert {
                                        name: HeaderName(
                                            "x-server",
                                        ),
                                        value: HeaderValue(
                                            "test_server",
                                        ),
                                    },
                                ),
                                Insert(
                                    HeaderInsert {
                                        name: HeaderName(
                                            "x-tier",
                                        ),
                                        value: HeaderValue(
                                            "premium",
                                        ),
                                    },
                                ),
                            ],
                        },
                    ),
                },
                enable_structured_content: true,
                headers: [
                    Insert(
                        HeaderInsert {
                            name: HeaderName(
                                "x-system",
                            ),
                            value: HeaderValue(
                                "nexus",
                            ),
                        },
                    ),
                    Insert(
                        HeaderInsert {
                            name: HeaderName(
                                "x-service",
                            ),
                            value: HeaderValue(
                                "nexus",
                            ),
                        },
                    ),
                ],
            },
            llm: LlmConfig {
                enabled: true,
                path: "/llm",
                providers: {
                    "anthropic": Anthropic(
                        ApiProviderConfig {
                            api_key: Some(
                                SecretBox<str>([REDACTED]),
                            ),
                            base_url: None,
                            forward_token: false,
                            models: {
                                "claude-3": ApiModelConfig {
                                    rename: None,
                                    rate_limits: None,
                                    headers: [],
                                },
                            },
                            rate_limits: None,
                            headers: [
                                Insert(
                                    HeaderInsert {
                                        name: HeaderName(
                                            "x-api-key",
                                        ),
                                        value: HeaderValue(
                                            "{{ env.ANTHROPIC_KEY }}",
                                        ),
                                    },
                                ),
                            ],
                        },
                    ),
                    "openai": Openai(
                        ApiProviderConfig {
                            api_key: Some(
                                SecretBox<str>([REDACTED]),
                            ),
                            base_url: None,
                            forward_token: false,
                            models: {
                                "gpt-4": ApiModelConfig {
                                    rename: None,
                                    rate_limits: None,
                                    headers: [
                                        Insert(
                                            HeaderInsert {
                                                name: HeaderName(
                                                    "x-model-tier",
                                                ),
                                                value: HeaderValue(
                                                    "advanced",
                                                ),
                                            },
                                        ),
                                    ],
                                },
                            },
                            rate_limits: None,
                            headers: [
                                Insert(
                                    HeaderInsert {
                                        name: HeaderName(
                                            "x-provider",
                                        ),
                                        value: HeaderValue(
                                            "openai",
                                        ),
                                    },
                                ),
                            ],
                        },
                    ),
                },
            },
        }
        "#);
    }
}
