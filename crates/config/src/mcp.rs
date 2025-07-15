use std::collections::BTreeMap;

use serde::Deserialize;
use url::Url;

/// Configuration for MCP (Model Context Protocol) settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpConfig {
    /// Whether MCP is enabled or disabled.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// The path for MCP endpoint.
    #[serde(default = "default_path")]
    pub path: String,
    /// Map of server names to their configurations.
    #[serde(default)]
    pub servers: BTreeMap<String, McpServer>,
}

/// Protocol type for HTTP-based MCP servers.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[derive(Default)]
pub enum HttpProtocol {
    /// Server-Sent Events protocol.
    Sse,
    /// Streaming HTTP protocol.
    #[default]
    StreamingHttp,
}

/// Configuration for an individual MCP server.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged, rename_all = "kebab-case", deny_unknown_fields)]
pub enum McpServer {
    /// A server that runs as a subprocess with command and arguments.
    Stdio {
        /// Command and arguments to run the server.
        cmd: Vec<String>,
    },
    /// A server accessible via HTTP URI.
    Http {
        /// URI of the HTTP server.
        uri: Url,
        /// Protocol to use for HTTP communication.
        #[serde(default)]
        protocol: HttpProtocol,
    },
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

fn default_enabled() -> bool {
    true
}

fn default_path() -> String {
    "/mcp".to_string()
}
