use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use duration_str::deserialize_duration;
use secrecy::SecretString;
use serde::{Deserialize, Deserializer, de::Error};
use url::Url;

/// Configuration for MCP (Model Context Protocol) settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct McpConfig {
    /// Whether MCP is enabled or disabled.
    pub enabled: bool,
    /// The path for MCP endpoint.
    pub path: String,
    /// Configuration for downstream connection caching.
    pub downstream_cache: McpDownstreamCacheConfig,
    /// Map of server names to their configurations.
    pub servers: BTreeMap<String, McpServer>,
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
        }
    }
}

/// Configuration for STDIO-based MCP servers.
///
/// STDIO servers are spawned as child processes and communicate via standard input/output
/// using JSON-RPC messages over the MCP protocol.
///
/// # Stdout/Stderr Configuration
///
/// - `stdout` must always be "pipe" for MCP JSON-RPC communication
/// - `stderr` can be configured to control subprocess logging behavior
///
/// **Note**: Due to limitations in the rmcp library's `TokioChildProcess` implementation,
/// stderr file redirection may not work as expected. The configuration is processed but
/// the rmcp transport layer may override these settings. This is a known limitation.
///
/// # Example Configuration
/// ```toml
/// [mcp.servers.python_server]
/// cmd = ["python", "-m", "mcp_server", "--port", "3000"]
/// env = { PYTHONPATH = "/opt/mcp", DEBUG = "1" }
/// cwd = "/tmp/mcp"
/// # stdout defaults to "pipe" (required for MCP)
/// stderr = "null"  # Discard logs (default)
///
/// [mcp.servers.debug_server]
/// cmd = ["node", "debug-server.js"]
/// stderr = "inherit"  # Show logs in console for debugging
///
/// [mcp.servers.logged_server]
/// cmd = ["./production-server"]
/// stderr = { file = "/var/log/mcp/server.log" }  # Attempt file logging
/// # Note: File logging may not work due to rmcp library limitations
/// ```
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

    /// Configuration for stdout handling.
    /// If not specified, defaults to "pipe" for MCP communication.
    #[serde(default)]
    pub stdout: StdioTarget,

    /// Configuration for stderr handling.
    /// If not specified, defaults to "null" to discard subprocess logs.
    ///
    /// Note: Due to rmcp library limitations, file redirection may not work as expected.
    #[serde(default = "default_stderr_target")]
    pub stderr: StdioTarget,
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
