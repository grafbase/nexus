use serde::{Deserialize, Serialize};

use crate::protocol::unknown_fields::UnknownFields;

/// MCP server definition when calling Anthropic.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpServerURLDefinition {
    /// Logical name for the MCP server.
    pub name: String,

    /// Server kind identifier (currently only "url").
    #[serde(rename = "type")]
    pub kind: McpServerType,

    /// Endpoint URL that Anthropic should invoke.
    pub url: String,

    /// Optional authorization token passed to the MCP server.
    #[serde(default)]
    pub authorization_token: Option<String>,

    /// Optional restrictions on which tools are enabled.
    #[serde(default)]
    pub tool_configuration: Option<McpServerToolConfiguration>,

    /// Additional MCP server data forwarded verbatim.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Supported MCP server kinds.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum McpServerType {
    Url,
    #[serde(untagged)]
    Unknown(String),
}

/// Optional per-server tool configuration overrides.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpServerToolConfiguration {
    /// Explicit allow-list of tool names callable via this server.
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,

    /// Whether the server is currently enabled.
    #[serde(default)]
    pub enabled: Option<bool>,

    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}
