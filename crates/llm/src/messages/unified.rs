//! Protocol-agnostic unified message types.
//!
//! These types serve as an internal representation that can losslessly
//! convert to/from both OpenAI and Anthropic protocol formats.

use serde::{Deserialize, Serialize};
use std::borrow::Cow;

pub(crate) mod from_anthropic;
pub(crate) mod from_openai;
pub(crate) mod to_anthropic;
pub(crate) mod to_openai;

/// Unified request that can represent both OpenAI and Anthropic requests without information loss.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedRequest {
    /// Model identifier (may include provider prefix).
    pub model: String,

    /// Messages in the conversation.
    pub messages: Vec<UnifiedMessage>,

    /// System instruction/prompt.
    ///
    /// - OpenAI: Sent as a message with role "system"
    /// - Anthropic: Sent as separate "system" field
    pub system: Option<String>,

    /// Maximum tokens to generate.
    ///
    /// - OpenAI: Optional, uses `max_tokens`
    /// - Anthropic: Required, uses `max_tokens`
    pub max_tokens: Option<u32>,

    /// Temperature for randomness (0.0 to 2.0).
    ///
    /// - OpenAI: 0.0 to 2.0
    /// - Anthropic: 0.0 to 1.0
    pub temperature: Option<f32>,

    /// Top-p nucleus sampling.
    pub top_p: Option<f32>,

    /// Top-k sampling (Anthropic-specific).
    pub top_k: Option<u32>,

    /// Frequency penalty (OpenAI-specific, -2.0 to 2.0).
    pub frequency_penalty: Option<f32>,

    /// Presence penalty (OpenAI-specific, -2.0 to 2.0).
    pub presence_penalty: Option<f32>,

    /// Stop sequences that halt generation.
    ///
    /// - OpenAI: `stop`
    /// - Anthropic: `stop_sequences`
    pub stop_sequences: Option<Vec<String>>,

    /// Whether to stream the response.
    pub stream: Option<bool>,

    /// Available tools/functions.
    pub tools: Option<Vec<UnifiedTool>>,

    /// How the model should use tools.
    pub tool_choice: Option<UnifiedToolChoice>,

    /// Whether to allow parallel tool calls (OpenAI-specific).
    pub parallel_tool_calls: Option<bool>,

    /// Custom metadata (Anthropic-specific).
    pub metadata: Option<UnifiedMetadata>,
}

/// Unified message representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedMessage {
    /// Role of the message sender.
    pub role: UnifiedRole,

    /// Content blocks (supports both simple strings and complex content).
    pub content: UnifiedContentContainer,

    /// Tool calls made by the assistant (OpenAI format).
    pub tool_calls: Option<Vec<UnifiedToolCall>>,

    /// ID referencing a tool call (for tool responses).
    pub tool_call_id: Option<String>,
}

/// Container for content that can be either a simple string or complex blocks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum UnifiedContentContainer {
    /// Simple text content (OpenAI style).
    Text(String),
    /// Complex content blocks (Anthropic style).
    Blocks(Vec<UnifiedContent>),
}

/// Unified role enum that covers all roles from both protocols.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UnifiedRole {
    /// System instructions (OpenAI only as message, Anthropic as field).
    System,
    /// User input.
    User,
    /// Assistant/model response.
    Assistant,
    /// Tool response (OpenAI uses "tool", Anthropic embeds in user message).
    Tool,
}

/// Unified content block that can represent various content types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum UnifiedContent {
    /// Plain text content.
    Text { text: String },

    /// Image content (Anthropic supports this directly).
    Image {
        /// Image source (base64 or URL)
        source: UnifiedImageSource,
    },

    /// Tool use request (Anthropic format).
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },

    /// Tool result (Anthropic format).
    ToolResult {
        tool_use_id: String,
        /// Combined content from tool result
        content: UnifiedToolResultContent,
        is_error: Option<bool>,
    },
}

/// Tool result content that avoids allocation by using enum.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum UnifiedToolResultContent {
    /// Simple text result.
    Text(String),
    /// Multiple content items (for complex results).
    Multiple(Vec<String>),
}

/// Image source for multi-modal content.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum UnifiedImageSource {
    /// Base64-encoded image data.
    Base64 { media_type: String, data: String },
    /// URL reference to image.
    Url { url: String },
}

/// Unified tool/function definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedTool {
    /// Function definition.
    pub function: UnifiedFunction,
}

/// Unified function definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedFunction {
    /// Function name.
    pub name: String,

    /// Function description.
    pub description: String,

    /// Parameters as JSON Schema.
    pub parameters: serde_json::Value,

    /// Whether the function accepts additional properties (OpenAI strict mode).
    pub strict: Option<bool>,
}

/// Unified tool choice configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum UnifiedToolChoice {
    /// Mode-based choice (none, auto, any/required).
    Mode(UnifiedToolChoiceMode),

    /// Specific tool selection.
    Specific { function: UnifiedFunctionChoice },
}

/// Tool choice modes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnifiedToolChoiceMode {
    /// Don't use any tools.
    None,
    /// Model decides whether to use tools.
    Auto,
    /// Model must use at least one tool.
    #[serde(alias = "required", alias = "any")]
    Required,
}

/// Specific function choice.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedFunctionChoice {
    pub name: String,
}

/// Tool call made by the assistant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedToolCall {
    /// Unique identifier for this tool call.
    pub id: String,

    /// Function call details.
    pub function: UnifiedFunctionCall,
}

/// Function call details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedFunctionCall {
    /// Name of the function to call.
    pub name: String,

    /// Arguments - either as JSON string or value.
    pub arguments: UnifiedArguments,
}

/// Arguments that can be either a string or JSON value.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum UnifiedArguments {
    /// JSON string (OpenAI format).
    String(String),
    /// JSON value (Anthropic format).
    Value(serde_json::Value),
}

/// Custom metadata (Anthropic-specific).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedMetadata {
    /// User identifier.
    pub user_id: Option<String>,
}

/// Unified response that can represent both OpenAI and Anthropic responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedResponse {
    /// Unique identifier for this completion.
    pub id: String,

    /// Model that generated the response.
    pub model: String,

    /// Response choices/candidates.
    pub choices: Vec<UnifiedChoice>,

    /// Token usage statistics.
    pub usage: UnifiedUsage,

    /// Unix timestamp of creation.
    pub created: u64,

    /// Stop reason (for non-streaming responses).
    pub stop_reason: Option<UnifiedStopReason>,

    /// Stop sequence that was matched (Anthropic).
    pub stop_sequence: Option<String>,
}

/// Response choice/candidate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedChoice {
    /// Index of this choice.
    pub index: u32,

    /// Generated message.
    pub message: UnifiedMessage,

    /// Reason for stopping.
    pub finish_reason: Option<UnifiedFinishReason>,
}

/// Token usage statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedUsage {
    /// Input/prompt tokens.
    pub prompt_tokens: u32,

    /// Output/completion tokens.
    pub completion_tokens: u32,

    /// Total tokens (input + output).
    pub total_tokens: u32,
}

/// Unified stop/finish reason.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnifiedFinishReason {
    /// Natural stop point reached.
    Stop,
    /// Maximum tokens reached.
    #[serde(alias = "max_tokens")]
    Length,
    /// Content filtered for safety.
    ContentFilter,
    /// Tool calls were made.
    ToolCalls,
}

impl std::fmt::Display for UnifiedFinishReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UnifiedFinishReason::Stop => write!(f, "stop"),
            UnifiedFinishReason::Length => write!(f, "length"),
            UnifiedFinishReason::ContentFilter => write!(f, "content_filter"),
            UnifiedFinishReason::ToolCalls => write!(f, "tool_calls"),
        }
    }
}

/// Anthropic-style stop reason (kept separate for fidelity).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnifiedStopReason {
    /// Reached natural stopping point.
    EndTurn,
    /// Exceeded max_tokens.
    MaxTokens,
    /// Matched a stop sequence.
    StopSequence,
    /// Model invoked a tool.
    ToolUse,
}

/// Unified streaming chunk for incremental responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedChunk {
    /// Chunk identifier.
    pub id: Cow<'static, str>,

    /// Model generating the chunk.
    pub model: Cow<'static, str>,

    /// Incremental choice updates.
    pub choices: Vec<UnifiedChoiceDelta>,

    /// Usage (only in final chunk).
    pub usage: Option<UnifiedUsage>,

    /// Creation timestamp.
    pub created: u64,
}

/// Incremental choice update in streaming.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedChoiceDelta {
    /// Index of this choice.
    pub index: u32,

    /// Incremental message content.
    pub delta: UnifiedMessageDelta,

    /// Finish reason (only in final chunk).
    pub finish_reason: Option<UnifiedFinishReason>,
}

/// Incremental message content in streaming.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedMessageDelta {
    /// Role (only in first chunk).
    pub role: Option<UnifiedRole>,

    /// Incremental text content.
    pub content: Option<String>,

    /// Incremental tool calls.
    pub tool_calls: Option<Vec<UnifiedStreamingToolCall>>,
}

/// Streaming tool call updates.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum UnifiedStreamingToolCall {
    /// Start of a new tool call.
    Start {
        index: usize,
        id: String,
        function: UnifiedFunctionStart,
    },
    /// Incremental arguments for a tool call.
    Delta {
        index: usize,
        function: UnifiedFunctionDelta,
    },
}

/// Start of a function call in streaming.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedFunctionStart {
    pub name: String,
    pub arguments: String,
}

/// Incremental function arguments in streaming.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedFunctionDelta {
    pub arguments: String,
}

/// Unified object type for API responses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnifiedObjectType {
    /// Single model object
    Model,
    /// List of objects
    List,
    /// Chat completion response
    #[serde(rename = "chat.completion")]
    ChatCompletion,
    /// Streaming chat completion chunk
    #[serde(rename = "chat.completion.chunk")]
    ChatCompletionChunk,
    /// Message (Anthropic-style)
    Message,
}

/// Unified model information that can represent both OpenAI and Anthropic model formats.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedModel {
    /// The model identifier (e.g., "gpt-4", "claude-3-opus")
    pub id: String,

    /// The object type (always Model for single models)
    #[serde(rename = "type", alias = "object")]
    pub object_type: UnifiedObjectType,

    /// Display name for the model (may be same as id for OpenAI)
    pub display_name: String,

    /// Unix timestamp when the model was created (0 for Anthropic models)
    pub created: u64,

    /// Owner/organization of the model (e.g., "openai", "anthropic")
    pub owned_by: String,
}

/// Unified models listing response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedModelsResponse {
    /// The object type (always List for model lists)
    #[serde(rename = "type", alias = "object")]
    pub object_type: UnifiedObjectType,

    /// List of available models
    pub models: Vec<UnifiedModel>,

    /// Whether there are more models to fetch (for pagination)
    pub has_more: bool,
}
