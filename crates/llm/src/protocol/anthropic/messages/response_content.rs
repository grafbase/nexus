use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::protocol::unknown_fields::UnknownFields;

/// Content blocks returned by Anthropic message responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseContent {
    Text(ResponseTextBlock),
    Thinking(ResponseThinkingBlock),
    RedactedThinking(ResponseRedactedThinkingBlock),
    ToolUse(ResponseToolUseBlock),
    ServerToolUse(ResponseServerToolUseBlock),
    WebSearchToolResult(ResponseWebSearchToolResultBlock),
    WebFetchToolResult(ResponseWebFetchToolResultBlock),
    CodeExecutionToolResult(ResponseCodeExecutionToolResultBlock),
    BashCodeExecutionToolResult(ResponseBashCodeExecutionToolResultBlock),
    TextEditorCodeExecutionToolResult(ResponseTextEditorCodeExecutionToolResultBlock),
    McpToolUse(ResponseMcpToolUseBlock),
    McpToolResult(ResponseMcpToolResultBlock),
    ContainerUpload(ResponseContainerUploadBlock),

    #[serde(untagged)]
    Unknown(Value),
}

/// Text returned by Claude, optionally accompanied by citations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseTextBlock {
    /// Raw assistant text generated for this block.
    pub text: String,
    /// Citations supporting the text block, retained verbatim.
    #[serde(default)]
    pub citations: Option<Vec<Value>>,
    /// Forward-compatible storage for unsupported properties.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Model thinking content surfaced when the thinking capability is enabled.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseThinkingBlock {
    /// Signature used by Anthropic to verify the thinking payload.
    pub signature: String,
    /// Raw thinking text emitted by the model.
    pub thinking: String,
    /// Forward-compatible storage for unsupported properties.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Redacted thinking output revealed when thinking content is withheld.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseRedactedThinkingBlock {
    /// Opaque data blob describing the redacted thinking segment.
    pub data: String,
    /// Forward-compatible storage for unsupported properties.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Tool invocation requested by Claude within the conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseToolUseBlock {
    /// Unique identifier assigned to the tool call.
    pub id: String,
    /// Tool input payload provided by the model.
    pub input: Value,
    /// Name of the tool being invoked.
    pub name: String,
    /// Forward-compatible storage for unsupported properties.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Anthropic-managed tool invocation such as web search or code execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseServerToolUseBlock {
    /// Unique identifier assigned to the server-managed tool call.
    pub id: String,
    /// JSON payload supplied to the server tool.
    pub input: Value,
    /// Name of the managed tool that was invoked.
    pub name: String,
    /// Forward-compatible storage for unsupported properties.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Result payload returned from Anthropic's web search tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseWebSearchToolResultBlock {
    /// Either the tool result error or an array of search hits.
    pub content: Value,
    /// Identifier linking the result back to the originating tool use.
    pub tool_use_id: String,
    /// Forward-compatible storage for unsupported properties.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Result payload returned from Anthropic's web fetch tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseWebFetchToolResultBlock {
    /// Either a fetch error or the retrieved document content.
    pub content: Value,
    /// Identifier linking the result back to the originating tool use.
    pub tool_use_id: String,
    /// Forward-compatible storage for unsupported properties.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Result payload returned from Anthropic's code execution tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseCodeExecutionToolResultBlock {
    /// Either an execution error or the structured execution result.
    pub content: Value,
    /// Identifier linking the result back to the originating tool use.
    pub tool_use_id: String,
    /// Forward-compatible storage for unsupported properties.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Result payload returned from Anthropic's bash execution tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseBashCodeExecutionToolResultBlock {
    /// Either a bash execution error or the structured execution result.
    pub content: Value,
    /// Identifier linking the result back to the originating tool use.
    pub tool_use_id: String,
    /// Forward-compatible storage for unsupported properties.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Result payload returned from Anthropic's text editor execution tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseTextEditorCodeExecutionToolResultBlock {
    /// Either a text editor execution error or one of the editor result blocks.
    pub content: Value,
    /// Identifier linking the result back to the originating tool use.
    pub tool_use_id: String,
    /// Forward-compatible storage for unsupported properties.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// MCP tool invocation emitted by Claude during a response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseMcpToolUseBlock {
    /// Unique identifier assigned to the MCP tool invocation.
    pub id: String,
    /// JSON payload delivered to the MCP tool.
    pub input: Value,
    /// Name of the MCP tool being invoked.
    pub name: String,
    /// Name of the MCP server that owns the tool.
    pub server_name: String,
    /// Forward-compatible storage for unsupported properties.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Result payload returned from an MCP tool invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseMcpToolResultBlock {
    /// Either a raw string or structured blocks describing the tool output.
    pub content: Value,
    /// Indicates whether the tool invocation failed.
    pub is_error: bool,
    /// Identifier linking the result back to the originating tool use.
    pub tool_use_id: String,
    /// Forward-compatible storage for unsupported properties.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Container upload details surfaced when files are attached to a container run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseContainerUploadBlock {
    /// Identifier of the uploaded file available to the container.
    pub file_id: String,
    /// Forward-compatible storage for unsupported properties.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}
