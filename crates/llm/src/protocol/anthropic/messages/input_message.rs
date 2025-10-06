use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::protocol::unknown_fields::UnknownFields;

use super::cache_control::CacheControl;

/// A single input message provided to the Anthropic API.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InputMessage {
    /// Originating role for the message turn.
    pub role: Role,
    /// Message body provided as text or structured blocks.
    pub content: InputMessageContent,

    /// Extra message fields passed through untouched.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Supported Anthropic message roles.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    #[serde(untagged)]
    Unknown(String),
}

/// Message content may be provided as a raw string or as structured content blocks.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum InputMessageContent {
    Text(String),
    Items(Vec<InputMessageStructuredContent>),
}

/// Structured content blocks accepted by the Anthropic Messages API.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InputMessageStructuredContent {
    Text(RequestTextBlock),
    Image(RequestImageBlock),
    Document(RequestDocumentBlock),
    SearchResult(RequestSearchResultBlock),
    Thinking(RequestThinkingBlock),
    RedactedThinking(RequestRedactedThinkingBlock),
    ToolUse(RequestToolUseBlock),
    ToolResult(RequestToolResultBlock),
    ServerToolUse(RequestServerToolUseBlock),
    WebSearchToolResult(RequestWebSearchToolResultBlock),
    WebFetchToolResult(RequestWebFetchToolResultBlock),
    CodeExecutionToolResult(RequestCodeExecutionToolResultBlock),
    BashCodeExecutionToolResult(RequestBashCodeExecutionToolResultBlock),
    TextEditorCodeExecutionToolResult(RequestTextEditorCodeExecutionToolResultBlock),
    McpToolUse(RequestMcpToolUseBlock),
    McpToolResult(RequestMcpToolResultBlock),
    ContainerUpload(RequestContainerUploadBlock),
    #[serde(untagged)]
    Unknown(Value),
}

/// Text content supplied to the Anthropic API.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RequestTextBlock {
    /// Raw text body for the content block.
    pub text: String,

    /// Optional cache-control hints attached to the block.
    #[serde(default)]
    pub cache_control: Option<CacheControl>,

    /// Optional citations array preserved as provided.
    #[serde(default)]
    pub citations: Option<Vec<Value>>,

    /// Additional fields retained for forward compatibility.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Image content block accepted by Anthropic.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RequestImageBlock {
    /// Image source descriptor (base64, URL, or file reference).
    pub source: Value,

    /// Optional cache-control hints attached to the block.
    #[serde(default)]
    pub cache_control: Option<CacheControl>,

    /// Additional unknown properties carried through untouched.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Document block describing attachments or inline documents.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RequestDocumentBlock {
    /// Document source payload (text, PDF, file, etc.).
    pub source: Value,

    /// Optional cache-control hints applied to the document.
    #[serde(default)]
    pub cache_control: Option<CacheControl>,

    /// Optional citation configuration for the document.
    #[serde(default)]
    pub citations: Option<RequestCitationsConfig>,

    /// Optional contextual description associated with the document.
    #[serde(default)]
    pub context: Option<String>,

    /// Optional document title retained for the request.
    #[serde(default)]
    pub title: Option<String>,

    /// Unknown document fields preserved verbatim.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Controls whether citations are enabled for a block.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RequestCitationsConfig {
    /// Flag indicating if citations should be generated.
    pub enabled: bool,

    /// Unknown citation fields held for compatibility.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Search result block passed back to Anthropic.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RequestSearchResultBlock {
    /// Structured content extracted from the search result.
    pub content: Vec<Value>,

    /// Optional cache-control hints applied to the block.
    #[serde(default)]
    pub cache_control: Option<CacheControl>,

    /// Optional citations configuration for the result.
    #[serde(default)]
    pub citations: Option<RequestCitationsConfig>,

    /// Source identifier for the search result.
    pub source: String,

    /// Title associated with the search result.
    pub title: String,

    /// Unknown fields retained for future schema changes.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Thinking block provided by the caller.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RequestThinkingBlock {
    /// Signature reported alongside the thinking payload.
    pub signature: String,

    /// Raw thinking text supplied to the API.
    pub thinking: String,

    /// Unknown fields retained for future schema revisions.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Redacted thinking block.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RequestRedactedThinkingBlock {
    /// Redacted content payload.
    pub data: String,

    /// Unknown properties carried forward untouched.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Tool use block emitted by a caller when forcing tool calls.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RequestToolUseBlock {
    /// Unique identifier referencing the tool use.
    pub id: String,

    /// Tool input payload provided to Anthropic.
    pub input: Value,

    /// Name of the tool being invoked.
    pub name: String,

    /// Optional cache-control hints associated with the block.
    #[serde(default)]
    pub cache_control: Option<CacheControl>,

    /// Additional unknown fields preserved verbatim.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Tool result block describing the outcome of a tool invocation.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RequestToolResultBlock {
    /// Identifier of the tool use this result corresponds to.
    pub tool_use_id: String,

    /// Optional content returned by the tool (string or block array).
    #[serde(default)]
    pub content: Option<Value>,

    /// Indicates whether the tool invocation failed.
    #[serde(default)]
    pub is_error: Option<bool>,

    /// Optional cache-control configuration for the result.
    #[serde(default)]
    pub cache_control: Option<CacheControl>,

    /// Unknown fields retained for schema-forward compatibility.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Server-managed tool use block (web_search, web_fetch, etc.).
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RequestServerToolUseBlock {
    /// Unique identifier issued for the server tool use.
    pub id: String,

    /// Input payload forwarded to the managed tool.
    pub input: Value,

    /// Name of the server-managed tool.
    pub name: String,

    /// Optional cache-control configuration for the tool call.
    #[serde(default)]
    pub cache_control: Option<CacheControl>,

    /// Unknown fields preserved for forward compatibility.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Web search tool result block.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RequestWebSearchToolResultBlock {
    /// Identifier of the tool use that produced this result.
    pub tool_use_id: String,

    /// Result payload returned by the web search tool.
    pub content: Value,

    /// Optional cache-control hints.
    #[serde(default)]
    pub cache_control: Option<CacheControl>,

    /// Unknown fields retained for compatibility.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Web fetch tool result block.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RequestWebFetchToolResultBlock {
    /// Identifier linking the result to a tool use.
    pub tool_use_id: String,

    /// Payload returned by the web fetch tool.
    pub content: Value,

    /// Optional cache-control hints.
    #[serde(default)]
    pub cache_control: Option<CacheControl>,

    /// Unknown fields preserved verbatim.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Code execution tool result block.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RequestCodeExecutionToolResultBlock {
    /// Identifier of the corresponding tool use.
    pub tool_use_id: String,

    /// Payload returned by the execution environment.
    pub content: Value,

    /// Optional cache-control hints on the block.
    #[serde(default)]
    pub cache_control: Option<CacheControl>,

    /// Unknown fields retained for future schema updates.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Bash code execution tool result block.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RequestBashCodeExecutionToolResultBlock {
    /// Identifier of the related tool use.
    pub tool_use_id: String,

    /// Payload returned by the bash execution environment.
    pub content: Value,

    /// Optional cache-control hints.
    #[serde(default)]
    pub cache_control: Option<CacheControl>,

    /// Unknown fields retained verbatim.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Text editor code execution tool result block.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RequestTextEditorCodeExecutionToolResultBlock {
    /// Identifier of the corresponding tool use.
    pub tool_use_id: String,

    /// Payload returned by the text editor execution tool.
    pub content: Value,

    /// Optional cache-control hints on the block.
    #[serde(default)]
    pub cache_control: Option<CacheControl>,

    /// Unknown fields retained for forward compatibility.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// MCP tool use block supplied by the caller.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RequestMcpToolUseBlock {
    /// Unique identifier for the MCP tool use.
    pub id: String,

    /// Tool input payload delivered to the MCP server.
    pub input: Value,

    /// Tool name within the MCP server.
    pub name: String,

    /// Name of the MCP server handling the tool call.
    pub server_name: String,

    /// Optional cache-control hints for the block.
    #[serde(default)]
    pub cache_control: Option<CacheControl>,

    /// Additional fields preserved verbatim.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// MCP tool result block returned by the caller.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RequestMcpToolResultBlock {
    /// Identifier of the tool use this result belongs to.
    pub tool_use_id: String,

    /// Optional content returned by the tool (string or block array).
    #[serde(default)]
    pub content: Option<Value>,

    /// Indicates whether the tool invocation failed.
    #[serde(default)]
    pub is_error: Option<bool>,

    /// Optional cache-control hints attached to the block.
    #[serde(default)]
    pub cache_control: Option<CacheControl>,

    /// Unknown fields carried through untouched.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Container upload block used for code execution workflows.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RequestContainerUploadBlock {
    /// Identifier of the file to upload to the container context.
    pub file_id: String,

    /// Optional cache-control hints attached to the block.
    #[serde(default)]
    pub cache_control: Option<CacheControl>,

    /// Unknown fields preserved for forward compatibility.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}
