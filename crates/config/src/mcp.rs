use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Duration;

use duration_str::deserialize_duration;
use secrecy::SecretString;
use serde::{Deserialize, Deserializer, de::Error};
use url::Url;

use crate::RateLimitQuota;
use crate::headers::McpHeaderRule;

/// Configuration for MCP (Model Context Protocol) settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct McpConfig {
    /// Whether MCP is enabled or disabled.
    enabled: bool,
    /// The path for MCP endpoint.
    pub path: String,
    /// Configuration for downstream connection caching.
    pub downstream_cache: McpDownstreamCacheConfig,
    /// Map of server names to their configurations.
    pub servers: BTreeMap<String, McpServer>,
    /// Enable structured content responses for better performance and type safety.
    /// When true (default), the search tool uses the `structuredContent` field.
    /// When false, uses legacy `content` field with Content::json objects.
    pub enable_structured_content: bool,
    /// Global header insertion rules for all MCP requests.
    /// Only supports insert operations - applied at client initialization time.
    #[serde(default)]
    pub headers: Vec<McpHeaderRule>,
}

impl McpConfig {
    /// Whether MCP is enabled or not.
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Whether there are any MCP servers configured.
    pub fn has_servers(&self) -> bool {
        !self.servers.is_empty()
    }
}

/// Configuration for an individual MCP server.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case", untagged, deny_unknown_fields)]
pub enum McpServer {
    /// A server that runs as a subprocess with command and arguments.
    Stdio(Box<StdioConfig>),
    /// A server accessible via HTTP.
    Http(Box<HttpConfig>),
}

impl McpServer {
    /// Returns `true` if this MCP server configuration forwards authentication
    /// from the incoming request to the MCP server.
    pub fn forwards_authentication(&self) -> bool {
        match self {
            McpServer::Stdio(..) => false,
            McpServer::Http(config) => config.forwards_authentication(),
        }
    }

    /// Finalizes the MCP server configuration by applying authentication settings.
    ///
    /// For HTTP servers configured to forward authentication, this method will
    /// set up token-based authentication using the provided token. For all other
    /// server types, the configuration is returned unchanged.
    pub fn finalize(&self, token: Option<&SecretString>) -> Self {
        match self {
            McpServer::Http(config) if config.forwards_authentication() => {
                let mut config = config.clone();

                if let Some(token) = token {
                    config.auth = Some(ClientAuthConfig::Token { token: token.clone() });
                }

                Self::Http(config)
            }
            other => other.clone(),
        }
    }

    /// Returns the rate limit configuration for this server, if any.
    pub fn rate_limits(&self) -> Option<&McpServerRateLimit> {
        match self {
            McpServer::Stdio(config) => config.rate_limits.as_ref(),
            McpServer::Http(config) => config.rate_limits.as_ref(),
        }
    }

    /// Get the effective header rules for this server.
    /// STDIO servers don't support headers, so returns empty vec.
    pub fn get_effective_header_rules(&self) -> Vec<&McpHeaderRule> {
        match self {
            McpServer::Stdio(_) => Vec::new(),
            McpServer::Http(config) => config.get_effective_header_rules().collect(),
        }
    }

    /// Returns the allow configuration for this server, if any.
    pub fn allow(&self) -> Option<&BTreeSet<String>> {
        match self {
            McpServer::Stdio(config) => config.allow.as_ref(),
            McpServer::Http(config) => config.allow.as_ref(),
        }
    }

    /// Returns the deny configuration for this server, if any.
    pub fn deny(&self) -> Option<&BTreeSet<String>> {
        match self {
            McpServer::Stdio(config) => config.deny.as_ref(),
            McpServer::Http(config) => config.deny.as_ref(),
        }
    }

    /// Returns the tool-level access configuration for this server.
    pub fn tool_access_configs(&self) -> &BTreeMap<String, ToolAccessConfig> {
        match self {
            McpServer::Stdio(config) => &config.tools,
            McpServer::Http(config) => &config.tools,
        }
    }
}

/// Configuration for downstream connection caching.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct McpDownstreamCacheConfig {
    /// Maximum number of cached downstream connections.
    pub max_size: u64,
    /// How long a cached connection can be idle before being evicted.
    /// Accepts duration strings like "10m", "30s", "1h" or plain seconds as integer.
    #[serde(deserialize_with = "deserialize_duration")]
    pub idle_timeout: Duration,
}

impl Default for McpDownstreamCacheConfig {
    fn default() -> Self {
        Self {
            max_size: 1000,
            idle_timeout: Duration::from_secs(600),
        }
    }
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            path: "/mcp".to_string(),
            downstream_cache: McpDownstreamCacheConfig::default(),
            servers: BTreeMap::new(),
            enable_structured_content: true, // Default to true for best performance
            headers: Vec::new(),
        }
    }
}

/// Configuration for STDIO-based MCP servers.
///
/// STDIO servers are spawned as child processes and communicate via standard input/output
/// using JSON-RPC messages over the MCP protocol.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StdioConfig {
    /// Command and arguments to run the server.
    /// Must contain at least one element (the executable).
    ///
    /// The first element is treated as the executable, and subsequent elements as arguments.
    #[serde(deserialize_with = "deserialize_non_empty_command")]
    pub cmd: Vec<String>,

    /// Environment variables to set for the subprocess.
    /// These will be added to the child process environment.
    #[serde(default)]
    pub env: BTreeMap<String, String>,

    /// Working directory for the subprocess.
    /// If not specified, the child process will inherit the parent's working directory.
    #[serde(default)]
    pub cwd: Option<PathBuf>,

    /// Configuration for stderr handling.
    /// If not specified, defaults to "null" to discard subprocess logs.
    ///
    /// Note: Due to rmcp library limitations, file redirection may not work as expected.
    #[serde(default = "default_stderr_target")]
    pub stderr: StdioTarget,

    /// Rate limit configuration for this MCP server.
    #[serde(default)]
    pub rate_limits: Option<McpServerRateLimit>,

    /// Groups allowed to access this server.
    /// If defined and non-empty, only users with groups in this list can access.
    /// If undefined or empty when deny is also empty/undefined, all users can access.
    #[serde(default)]
    pub allow: Option<BTreeSet<String>>,

    /// Groups explicitly denied access to this server.
    /// Users with groups in this list are denied even if in allow.
    #[serde(default)]
    pub deny: Option<BTreeSet<String>>,

    /// Tool-level access control overrides.
    /// Maps tool names to their specific access configuration.
    #[serde(default)]
    pub tools: BTreeMap<String, ToolAccessConfig>,
}

impl StdioConfig {
    /// Returns the executable (first element of cmd).
    ///
    /// This is guaranteed to be non-empty due to validation during deserialization.
    pub fn executable(&self) -> &str {
        &self.cmd[0] // Safe because validation ensures non-empty
    }

    /// Returns the arguments (all elements after the first).
    pub fn args(&self) -> &[String] {
        &self.cmd[1..]
    }
}

/// Configuration for how to handle stdout/stderr streams of a child process.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case", untagged)]
pub enum StdioTarget {
    /// Simple string configuration.
    Simple(StdioTargetType),
    /// File configuration with path.
    File {
        /// Path to the file where output should be written.
        file: PathBuf,
    },
}

impl Default for StdioTarget {
    fn default() -> Self {
        Self::Simple(StdioTargetType::Pipe)
    }
}

/// Simple stdio target types.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StdioTargetType {
    /// Pipe the stream to the parent process (default for stdout).
    Pipe,
    /// Inherit the stream from the parent process.
    Inherit,
    /// Discard the stream output.
    Null,
}

/// Default stderr target - null to discard subprocess logs.
fn default_stderr_target() -> StdioTarget {
    StdioTarget::Simple(StdioTargetType::Null)
}

/// Custom deserializer for non-empty command vector.
/// Ensures validation happens at parse time, not runtime.
fn deserialize_non_empty_command<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let vec = Vec::<String>::deserialize(deserializer)?;

    match vec.split_first() {
        Some((_, _)) => Ok(vec),
        None => Err(D::Error::custom(
            "Command cannot be empty - must contain at least the executable",
        )),
    }
}

/// Protocol type for HTTP-based MCP servers.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum HttpProtocol {
    /// Server-Sent Events protocol.
    Sse,
    /// Streamable HTTP protocol.
    StreamableHttp,
}

/// A server accessible via HTTP.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpConfig {
    /// Protocol of the HTTP server.
    #[serde(default)]
    pub protocol: Option<HttpProtocol>,
    /// URL of the HTTP server.
    pub url: Url,
    /// TLS configuration options.
    #[serde(default)]
    pub tls: Option<TlsClientConfig>,
    /// Optional message endpoint for sending messages back to the server.
    /// If not provided, the client will try to derive it from the SSE endpoint
    /// or wait for the server to send a message endpoint event.
    #[serde(default)]
    pub message_url: Option<Url>,
    /// Optional authentication configuration.
    #[serde(default)]
    pub auth: Option<ClientAuthConfig>,
    /// Rate limit configuration for this MCP server.
    #[serde(default)]
    pub rate_limits: Option<McpServerRateLimit>,
    /// Header insertion rules for this server.
    /// Only supports insert operations - applied at client initialization time.
    #[serde(default)]
    pub headers: Vec<McpHeaderRule>,

    /// Groups allowed to access this server.
    /// If defined and non-empty, only users with groups in this list can access.
    /// If undefined or empty when deny is also empty/undefined, all users can access.
    #[serde(default)]
    pub allow: Option<BTreeSet<String>>,

    /// Groups explicitly denied access to this server.
    /// Users with groups in this list are denied even if in allow.
    #[serde(default)]
    pub deny: Option<BTreeSet<String>>,

    /// Tool-level access control overrides.
    /// Maps tool names to their specific access configuration.
    #[serde(default)]
    pub tools: BTreeMap<String, ToolAccessConfig>,
}

impl HttpConfig {
    /// Returns `true` if the configuration explicitly defines Server-Sent
    /// Events protocol.
    ///
    /// This method returns `true` in two cases:
    /// - The protocol is explicitly set to [`HttpProtocol::Sse`]
    /// - The protocol is not specified (`None`) but a `message_url` is provided,
    ///   which indicates SSE usage
    pub fn uses_sse(&self) -> bool {
        self.protocol == Some(HttpProtocol::Sse) || (self.protocol.is_none() && self.message_url.is_some())
    }

    /// Returns true, if the configuration explicitly defines Streamable
    /// HTTP protocol.
    pub fn uses_streamable_http(&self) -> bool {
        self.protocol == Some(HttpProtocol::StreamableHttp)
    }

    /// Returns true, if the configuration does not define a protocol
    /// and we need to detect it automatically.
    pub fn uses_protocol_detection(&self) -> bool {
        self.protocol.is_none()
    }

    /// Returns `true` if this HTTP configuration forwards authentication
    /// from the incoming request to the MCP server.
    pub fn forwards_authentication(&self) -> bool {
        match self.auth {
            Some(ref auth) => matches!(auth, ClientAuthConfig::Forward { .. }),
            None => false,
        }
    }

    /// Get the effective header rules for this server
    pub fn get_effective_header_rules(&self) -> impl ExactSizeIterator<Item = &McpHeaderRule> {
        self.headers.iter()
    }
}

/// TLS configuration for HTTP-based MCP servers.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TlsClientConfig {
    /// Whether to verify TLS certificates.
    pub verify_certs: bool,
    /// Whether to accept invalid hostnames in TLS certificates.
    pub accept_invalid_hostnames: bool,
    /// Path to a custom root CA certificate file.
    pub root_ca_cert_path: Option<PathBuf>,
    /// Path to client certificate file for mutual TLS.
    pub client_cert_path: Option<PathBuf>,
    /// Path to client private key file for mutual TLS.
    pub client_key_path: Option<PathBuf>,
}

impl Default for TlsClientConfig {
    fn default() -> Self {
        Self {
            verify_certs: true,
            accept_invalid_hostnames: false,
            root_ca_cert_path: None,
            client_cert_path: None,
            client_key_path: None,
        }
    }
}

/// Authentication configuration for HTTP-based MCP servers.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case", untagged, deny_unknown_fields)]
pub enum ClientAuthConfig {
    /// Token-based authentication.
    Token {
        /// Authentication token to send with requests.
        token: SecretString,
    },
    /// Forward the request authentication token to the MCP server.
    Forward {
        /// A tag to enable forwarding.
        r#type: ForwardType,
    },
}

/// Type indicating that authentication should be forwarded.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ForwardType {
    /// Forward authentication from the incoming request.
    Forward,
}

/// Rate limit configuration for an MCP server.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpServerRateLimit {
    /// The maximum number of requests allowed in the interval window.
    pub limit: u32,
    /// The interval window for the rate limit.
    #[serde(deserialize_with = "deserialize_duration")]
    pub interval: Duration,
    /// Optional per-tool rate limit overrides.
    #[serde(default)]
    pub tools: BTreeMap<String, RateLimitQuota>,
}

/// Access control configuration for individual tools.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolAccessConfig {
    /// Groups allowed to access this tool.
    /// Overrides server-level allow.
    #[serde(default)]
    pub allow: Option<BTreeSet<String>>,

    /// Groups explicitly denied access to this tool.
    /// Overrides server-level deny.
    #[serde(default)]
    pub deny: Option<BTreeSet<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use insta::assert_debug_snapshot;

    #[test]
    fn stdio_server_with_access_control() {
        let config_str = indoc! {r#"
            [servers.premium_tools]
            cmd = ["node", "premium-server.js"]
            allow = ["pro", "enterprise"]
            deny = ["suspended"]

            [servers.premium_tools.tools.advanced_analysis]
            allow = ["enterprise"]

            [servers.premium_tools.tools.basic_stats]
            deny = ["trial"]
        "#};

        let config: McpConfig = toml::from_str(config_str).unwrap();
        let server = config.servers.get("premium_tools").unwrap();

        assert_debug_snapshot!(server, @r###"
        Stdio(
            StdioConfig {
                cmd: [
                    "node",
                    "premium-server.js",
                ],
                env: {},
                cwd: None,
                stderr: Simple(
                    Null,
                ),
                rate_limits: None,
                allow: Some(
                    {
                        "enterprise",
                        "pro",
                    },
                ),
                deny: Some(
                    {
                        "suspended",
                    },
                ),
                tools: {
                    "advanced_analysis": ToolAccessConfig {
                        allow: Some(
                            {
                                "enterprise",
                            },
                        ),
                        deny: None,
                    },
                    "basic_stats": ToolAccessConfig {
                        allow: None,
                        deny: Some(
                            {
                                "trial",
                            },
                        ),
                    },
                },
            },
        )
        "###);
    }

    #[test]
    fn http_server_with_access_control() {
        let config_str = indoc! {r#"
            [servers.analytics]
            url = "http://analytics.example.com"
            allow = ["basic", "pro", "enterprise"]

            [servers.analytics.tools.ml_predict]
            allow = ["enterprise"]
            deny = ["beta_testers"]
        "#};

        let config: McpConfig = toml::from_str(config_str).unwrap();
        let server = config.servers.get("analytics").unwrap();

        assert_debug_snapshot!(server, @r###"
        Http(
            HttpConfig {
                protocol: None,
                url: Url {
                    scheme: "http",
                    cannot_be_a_base: false,
                    username: "",
                    password: None,
                    host: Some(
                        Domain(
                            "analytics.example.com",
                        ),
                    ),
                    port: None,
                    path: "/",
                    query: None,
                    fragment: None,
                },
                tls: None,
                message_url: None,
                auth: None,
                rate_limits: None,
                headers: [],
                allow: Some(
                    {
                        "basic",
                        "enterprise",
                        "pro",
                    },
                ),
                deny: None,
                tools: {
                    "ml_predict": ToolAccessConfig {
                        allow: Some(
                            {
                                "enterprise",
                            },
                        ),
                        deny: Some(
                            {
                                "beta_testers",
                            },
                        ),
                    },
                },
            },
        )
        "###);
    }

    #[test]
    fn server_without_access_control_defaults() {
        let config_str = indoc! {r#"
            [servers.public]
            cmd = ["python", "public_server.py"]
        "#};

        let config: McpConfig = toml::from_str(config_str).unwrap();
        let server = config.servers.get("public").unwrap();

        assert_debug_snapshot!(server, @r###"
        Stdio(
            StdioConfig {
                cmd: [
                    "python",
                    "public_server.py",
                ],
                env: {},
                cwd: None,
                stderr: Simple(
                    Null,
                ),
                rate_limits: None,
                allow: None,
                deny: None,
                tools: {},
            },
        )
        "###);
    }

    #[test]
    fn empty_allow_denies_all() {
        let config_str = indoc! {r#"
            [servers.restricted]
            cmd = ["node", "server.js"]
            allow = []
        "#};

        let config: McpConfig = toml::from_str(config_str).unwrap();
        let server = config.servers.get("restricted").unwrap();

        assert_debug_snapshot!(server.allow(), @r###"
        Some(
            {},
        )
        "###);
    }
}
