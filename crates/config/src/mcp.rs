use serde::Deserialize;

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
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            enabled: true,
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
