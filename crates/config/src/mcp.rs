use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Deserialize;
use url::Url;

/// Configuration for MCP (Model Context Protocol) settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct McpConfig {
    /// Whether MCP is enabled or disabled.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// The path for MCP endpoint.
    #[serde(default = "default_path")]
    pub path: String,
    /// Map of server names to their configurations.
    pub servers: BTreeMap<String, McpServer>,
}

/// Configuration for an individual MCP server.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case", untagged, deny_unknown_fields)]
pub enum McpServer {
    /// A server that runs as a subprocess with command and arguments.
    Stdio {
        /// Command and arguments to run the server.
        cmd: Vec<String>,
    },
    /// A server accessible via HTTP.
    Http(Box<HttpConfig>),
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            path: "/mcp".to_string(),
            servers: BTreeMap::new(),
        }
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
}

/// TLS configuration for HTTP-based MCP servers.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TlsClientConfig {
    /// Whether to verify TLS certificates.
    #[serde(default = "default_verify_tls")]
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

fn default_enabled() -> bool {
    true
}

fn default_path() -> String {
    "/mcp".to_string()
}

fn default_verify_tls() -> bool {
    true
}
