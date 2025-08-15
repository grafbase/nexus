//! Type definitions for MCP (Model Context Protocol)
//!
//! This module provides type aliases and wrapper types for the MCP protocol.

use serde::{Deserialize, Serialize};
use serde_json::Value;

// Re-export pmcp types with simpler names
pub use pmcp::types::{
    CallToolRequest, CallToolResult, Content, GetPromptRequest, GetPromptResult, ListPromptsResult,
    ListResourcesResult, ListToolsResult, PromptInfo as Prompt, ReadResourceRequest, ReadResourceResult,
    ResourceInfo as Resource, ToolInfo as Tool,
};

// Create our own ServerInfo since pmcp doesn't have it
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
    pub protocol_version: String,
    pub capabilities: ServerCapabilities,
    pub instructions: Option<String>,
}

// Create our own ServerCapabilities
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServerCapabilities {
    pub tools: Option<Value>,
    pub prompts: Option<Value>,
    pub resources: Option<Value>,
}

impl ServerCapabilities {
    pub fn new() -> Self {
        Self {
            tools: Some(serde_json::json!({})),
            prompts: Some(serde_json::json!({})),
            resources: Some(serde_json::json!({})),
        }
    }
}

// Protocol version constant
pub const PROTOCOL_VERSION: &str = "2025-03-26";

// Error type compatibility
pub type McpError = pmcp::Error;

// Helper functions for creating errors
pub fn error_internal(msg: impl Into<String>) -> McpError {
    pmcp::Error::internal(msg.into())
}

pub fn error_not_found(resource: impl Into<String>) -> McpError {
    pmcp::Error::not_found(resource.into())
}

pub fn error_validation(msg: impl Into<String>) -> McpError {
    pmcp::Error::validation(msg.into())
}

pub fn error_protocol(code: i32, msg: impl Into<String>) -> McpError {
    pmcp::Error::protocol(pmcp::ErrorCode::other(code), msg.into())
}
