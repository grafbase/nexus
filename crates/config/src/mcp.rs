use std::net::SocketAddr;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct McpConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    pub listen_address: Option<SocketAddr>,
    #[serde(default)]
    pub protocol: McpProtocol,
    #[serde(default = "default_path")]
    pub path: String,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            listen_address: None,
            protocol: McpProtocol::default(),
            path: "/mcp".to_string(),
        }
    }
}

fn default_enabled() -> bool {
    true
}

fn default_path() -> String {
    "/mcp".to_string()
}

#[derive(Default, Debug, Clone, Copy, Deserialize)]
pub enum McpProtocol {
    #[serde(rename = "sse")]
    Sse,
    #[serde(rename = "streamable-http")]
    #[default]
    StreamableHttp,
}
