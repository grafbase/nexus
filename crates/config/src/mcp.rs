use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub protocol: McpProtocol,
    #[serde(default = "default_path")]
    pub path: String,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            enabled: true,
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
