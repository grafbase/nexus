//! Protocol-agnostic unified message types for LLM interactions.
//!
//! This module provides a unified type system that serves as an internal representation
//! for all LLM protocols (OpenAI, Anthropic, Google, Bedrock). The unified types ensure:
//!
//! - **Lossless conversion**: All protocol-specific features are preserved
//! - **Zero-allocation**: Uses enums and Cow for efficient memory usage
//! - **Protocol independence**: Providers work with unified types, not protocol-specific ones
//! - **Single source of truth**: Eliminates data duplication and synchronization issues
//!
//! ## Architecture
//!
//! The conversion flow follows this pattern:
//! ```text
//! Protocol Request → UnifiedRequest → Provider → UnifiedResponse → Protocol Response
//! ```
//!
//! ## Key Design Decisions
//!
//! - **Content containers**: Support both simple strings (OpenAI) and complex blocks (Anthropic)
//! - **Tool calls**: Computed on-demand from content blocks to avoid duplication
//! - **Streaming**: Unified chunk format with incremental updates
//! - **Metadata preservation**: Protocol-specific fields are preserved through conversion

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::borrow::Cow;

use super::openai::JsonSchema;

pub(crate) mod from_anthropic;
pub(crate) mod from_openai;
pub(crate) mod to_anthropic;
pub(crate) mod to_openai;

/// Unified request representation for all LLM protocols.
///
/// This is the central request type that all providers work with internally.
/// It captures all features from OpenAI, Anthropic, Google, and Bedrock APIs
/// in a protocol-agnostic way.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedRequest {
    /// Model identifier.
    ///
    /// Format: `"model-id"` or `"provider/model-id"`
    ///
    /// Examples:
    /// - `"gpt-4"` (OpenAI)
    /// - `"claude-3-opus-20240229"` (Anthropic)
    /// - `"anthropic/claude-3-haiku"` (with provider prefix)
    pub model: String,

    /// Conversation messages.
    ///
    /// Messages should alternate between user and assistant roles for most providers.
    /// System messages may be extracted and handled separately depending on the provider.
    pub messages: Vec<UnifiedMessage>,

    /// System instruction/prompt.
    ///
    /// This field is used when the system message is provided separately from the messages array.
    /// Different providers handle system messages differently:
    ///
    /// - **OpenAI**: Converts to a message with role "system" at the beginning
    /// - **Anthropic**: Uses the dedicated "system" field in the API
    /// - **Google**: Converts to "systemInstruction" field
    /// - **Bedrock**: Converts to system content blocks
    pub system: Option<String>,

    /// Maximum tokens to generate in the response.
    ///
    /// Provider requirements:
    /// - **OpenAI**: Optional, defaults to model's maximum
    /// - **Anthropic**: Required, must be specified
    /// - **Google**: Maps to `maxOutputTokens`
    /// - **Bedrock**: Maps to `maxTokens` in inference config
    ///
    /// Common values: 1024, 2048, 4096
    pub max_tokens: Option<u32>,

    /// Temperature for randomness in generation.
    ///
    /// Controls the randomness of the output. Lower values make the output more
    /// deterministic and focused, higher values make it more creative and varied.
    ///
    /// Valid ranges by provider:
    /// - **OpenAI**: 0.0 to 2.0 (default: 1.0)
    /// - **Anthropic**: 0.0 to 1.0 (default: 1.0)
    /// - **Google**: 0.0 to 2.0
    /// - **Bedrock**: Provider-dependent
    ///
    /// Recommended values:
    /// - 0.0-0.3: Factual, deterministic tasks
    /// - 0.7-0.9: Creative writing
    /// - 1.0+: Maximum creativity
    pub temperature: Option<f32>,

    /// Top-p nucleus sampling cutoff.
    ///
    /// An alternative to temperature that controls diversity by limiting the
    /// cumulative probability of token choices. The model considers only tokens
    /// whose cumulative probability is below this threshold.
    ///
    /// Range: 0.0 to 1.0 (typically 0.9-1.0)
    ///
    /// Note: Use either temperature OR top_p, not both.
    pub top_p: Option<f32>,

    /// Top-k sampling (number of tokens to consider).
    ///
    /// Limits the model to considering only the k most likely tokens at each step.
    /// Lower values increase focus, higher values increase diversity.
    ///
    /// Primarily used by:
    /// - **Anthropic**: Supports top_k
    /// - **Google**: Maps to `topK`
    /// - **Bedrock**: Supported by some models
    ///
    /// Typical range: 1-100 (40 is a common default)
    pub top_k: Option<u32>,

    /// Frequency penalty for reducing repetition.
    ///
    /// Penalizes tokens based on how often they've appeared in the text so far.
    /// Positive values reduce repetition, negative values increase it.
    ///
    /// Range: -2.0 to 2.0 (OpenAI-specific)
    ///
    /// - **0.0**: No penalty
    /// - **0.1-0.5**: Light reduction of repetition
    /// - **0.5-1.0**: Moderate reduction
    /// - **1.0-2.0**: Strong reduction (may affect coherence)
    pub frequency_penalty: Option<f32>,

    /// Presence penalty for encouraging topic diversity.
    ///
    /// Penalizes tokens that have appeared at all in the text so far,
    /// encouraging the model to explore new topics.
    ///
    /// Range: -2.0 to 2.0 (OpenAI-specific)
    ///
    /// - **0.0**: No penalty
    /// - **0.1-0.5**: Slight encouragement of new topics
    /// - **0.5-1.0**: Moderate topic diversity
    /// - **1.0-2.0**: Strong push toward new topics
    pub presence_penalty: Option<f32>,

    /// Stop sequences that halt generation.
    ///
    /// When the model generates any of these sequences, it stops generating
    /// further tokens. Useful for controlling output format.
    ///
    /// Examples: `["\n\n", "END", "</answer>"]`
    ///
    /// Provider field names:
    /// - **OpenAI**: `stop`
    /// - **Anthropic**: `stop_sequences`
    /// - **Google**: `stopSequences`
    /// - **Bedrock**: `stopSequences`
    pub stop_sequences: Option<Vec<String>>,

    /// Whether to stream the response incrementally.
    ///
    /// When true, the response is sent as a series of chunks via Server-Sent Events (SSE).
    /// Each chunk contains incremental content that should be appended to build the full response.
    ///
    /// Streaming is useful for:
    /// - Providing real-time feedback to users
    /// - Handling long responses without timeout
    /// - Reducing perceived latency
    pub stream: Option<bool>,

    /// Available tools/functions the model can call.
    ///
    /// Tools allow the model to request execution of specific functions
    /// to gather information or perform actions. The model generates
    /// structured arguments that match the tool's parameter schema.
    ///
    /// Also known as:
    /// - **OpenAI**: "functions" (legacy) or "tools"
    /// - **Anthropic**: "tools"
    /// - **Google**: "functionDeclarations"
    /// - **Bedrock**: "tools" in tool configuration
    pub tools: Option<Vec<UnifiedTool>>,

    /// Controls how the model uses tools.
    ///
    /// Specifies whether the model must use tools, can choose to use them,
    /// or should use a specific tool.
    ///
    /// Common patterns:
    /// - `None`: Tools available but not required
    /// - `Mode(Auto)`: Model decides whether to use tools
    /// - `Mode(Required)`: Model must use at least one tool
    /// - `Specific{...}`: Model must use the specified tool
    pub tool_choice: Option<UnifiedToolChoice>,

    /// Whether to allow parallel tool calls.
    ///
    /// When true, the model can make multiple tool calls in a single response,
    /// allowing for more efficient execution of independent operations.
    ///
    /// Provider support:
    /// - **OpenAI**: Supported via `parallel_tool_calls`
    /// - **Anthropic**: Multiple tool_use blocks in response
    /// - **Others**: Provider-dependent behavior
    pub parallel_tool_calls: Option<bool>,

    /// Custom metadata for request tracking and filtering.
    ///
    /// Used for:
    /// - Tracking requests by user ID
    /// - Applying user-specific rate limits
    /// - Analytics and monitoring
    /// - Audit trails
    ///
    /// Currently only supported by Anthropic's API.
    pub metadata: Option<UnifiedMetadata>,
}

/// Unified message representation for conversations.
///
/// Messages are the fundamental unit of conversation between users and models.
/// This structure supports both simple text exchanges and complex multi-modal
/// interactions with tool usage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedMessage {
    /// Role of the message sender.
    ///
    /// Determines who sent this message and how it should be interpreted
    /// by the model.
    pub role: UnifiedRole,

    /// Message content.
    ///
    /// Can be either:
    /// - Simple text string (common for user input and basic responses)
    /// - Complex content blocks (for tool usage, multi-modal content)
    ///
    /// The container enum allows efficient handling of both simple and
    /// complex cases without allocation overhead for simple messages.
    pub content: UnifiedContentContainer,

    /// Tool calls made by the assistant.
    ///
    /// This field is primarily for OpenAI compatibility. In the unified model,
    /// tool calls are stored as ToolUse blocks within the content. This field
    /// can be computed on-demand using `compute_tool_calls()`.
    ///
    /// Note: Avoid setting this directly to prevent duplication.
    pub tool_calls: Option<Vec<UnifiedToolCall>>,

    /// ID referencing a previous tool call.
    ///
    /// Used in tool response messages to link the response back to the
    /// specific tool call that triggered it. This ensures proper correlation
    /// in multi-turn tool interactions.
    ///
    /// Example: When the assistant calls a "get_weather" tool with id "call_123",
    /// the tool response message should have `tool_call_id: Some("call_123")`.
    pub tool_call_id: Option<String>,
}

/// Container for message content with flexible representation.
///
/// This enum provides zero-cost abstraction for content that can be either
/// simple (just text) or complex (multiple blocks with different types).
/// The untagged serde attribute ensures clean JSON serialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum UnifiedContentContainer {
    /// Simple text content.
    ///
    /// Used for straightforward messages without special formatting,
    /// tool usage, or multi-modal content. This is the most common case
    /// and avoids allocation overhead of a vector.
    ///
    /// Example: `"What is the weather in Paris?"`
    Text(String),

    /// Complex content blocks.
    ///
    /// Used for messages that contain:
    /// - Multiple content types (text + images)
    /// - Tool usage (tool calls and results)
    /// - Structured content requiring preservation
    ///
    /// Example: A message with text and a tool call, or text and an image.
    Blocks(Vec<UnifiedContent>),
}

/// Message sender role in conversations.
///
/// Roles determine how the model interprets and responds to messages.
/// Different providers handle roles differently, but this enum provides
/// a unified representation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UnifiedRole {
    /// System instructions that guide model behavior.
    ///
    /// System messages set context, personality, or constraints for the conversation.
    ///
    /// Provider handling:
    /// - **OpenAI**: Sent as first message with role "system"
    /// - **Anthropic**: Extracted to separate "system" field
    /// - **Google**: Converted to "systemInstruction"
    /// - **Bedrock**: Converted to system content blocks
    System,

    /// User input messages.
    ///
    /// Messages from the end user asking questions or providing information.
    /// These messages drive the conversation forward.
    User,

    /// Assistant/model responses.
    ///
    /// Messages generated by the AI model in response to user input.
    /// May contain text, tool calls, or multi-modal content.
    Assistant,

    /// Tool response messages.
    ///
    /// Contains results from tool/function execution.
    ///
    /// Provider handling:
    /// - **OpenAI**: Separate "tool" role
    /// - **Anthropic**: Embedded as tool_result blocks in user message
    /// - **Google**: Embedded as functionResponse parts
    /// - **Bedrock**: Embedded as toolResult blocks
    Tool,
}

/// Content block types for complex messages.
///
/// These blocks allow messages to contain multiple types of content,
/// including text, images, tool interactions, and more. The tagged
/// enum ensures clear type discrimination in JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum UnifiedContent {
    /// Plain text content block.
    ///
    /// The most common content type, containing regular text.
    Text {
        /// The text content.
        text: String,
    },

    /// Image content for multi-modal interactions.
    ///
    /// Allows models to process and respond to visual information.
    /// Not all models support image input.
    Image {
        /// Image data source (base64 encoded or URL reference).
        source: UnifiedImageSource,
    },

    /// Tool use request from the assistant.
    ///
    /// Represents the assistant's request to execute a tool/function
    /// with specific arguments.
    ToolUse {
        /// Unique identifier for this tool call.
        /// Used to correlate with tool results.
        id: String,

        /// Name of the tool/function to execute.
        /// Must match a tool name from the available tools list.
        name: String,

        /// Arguments for the tool as JSON.
        /// Must conform to the tool's parameter schema.
        input: Value,
    },

    /// Tool execution result.
    ///
    /// Contains the output from executing a tool, linked back to
    /// the original tool call.
    ToolResult {
        /// ID of the tool call this result responds to.
        /// Must match the id from a previous ToolUse block.
        tool_use_id: String,

        /// The tool's output content.
        /// Can be simple text or multiple content items.
        content: UnifiedToolResultContent,

        /// Whether the tool execution resulted in an error.
        /// Used to signal failures to the model.
        is_error: Option<bool>,
    },
}

impl UnifiedContent {
    /// Get the text content if this is a text block.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            UnifiedContent::Text { text } => Some(text),
            _ => None,
        }
    }
}

/// Tool execution result content.
///
/// Efficiently represents tool output that can be either simple (single string)
/// or complex (multiple strings). The untagged serialization ensures clean JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum UnifiedToolResultContent {
    /// Simple text result.
    ///
    /// Used when the tool returns a single string output.
    /// Example: A calculator tool returning "42".
    Text(String),

    /// Multiple content items for complex results.
    ///
    /// Used when the tool returns structured or multi-part output.
    /// Example: A search tool returning multiple results.
    Multiple(Vec<String>),
}

/// Image source for multi-modal content.
///
/// Supports both inline image data and external references.
/// Not all models or providers support image inputs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum UnifiedImageSource {
    /// Base64-encoded image data.
    ///
    /// Image data encoded directly in the message.
    Base64 {
        /// MIME type of the image.
        /// Examples: "image/jpeg", "image/png", "image/webp"
        media_type: String,

        /// Base64-encoded image data.
        /// Should not include the data URL prefix.
        data: String,
    },

    /// URL reference to an image.
    ///
    /// External image that the model will fetch.
    /// Must be publicly accessible.
    Url {
        /// HTTP(S) URL to the image.
        /// Example: "https://example.com/image.jpg"
        url: String,
    },
}

/// Tool/function definition for model capabilities.
///
/// Tools extend the model's capabilities by allowing it to request
/// execution of specific functions. The model generates structured
/// arguments that match the tool's schema.
///
/// Tools are also known as "functions" in some APIs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedTool {
    /// The function specification.
    ///
    /// Contains all details about what the function does and
    /// what parameters it accepts.
    pub function: UnifiedFunction,
}

/// Function specification for tools.
///
/// Defines a callable function including its name, purpose, and parameters.
/// The model uses this information to determine when and how to call the function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedFunction {
    /// Function name (identifier).
    ///
    /// Must be unique among available tools. Should be a valid identifier
    /// (alphanumeric with underscores, no spaces).
    ///
    /// Examples: "get_weather", "search_web", "send_email"
    pub name: String,

    /// Human-readable description of what the function does.
    ///
    /// This helps the model understand when to use this tool.
    /// Should be clear and concise.
    ///
    /// Example: "Get current weather information for a specified location"
    pub description: String,

    /// Parameter schema as JSON Schema.
    ///
    /// Defines the structure and types of arguments the function accepts.
    /// The model will generate arguments that conform to this schema.
    ///
    /// Example schema for a weather function:
    /// ```json
    /// {
    ///   "type": "object",
    ///   "properties": {
    ///     "location": {"type": "string", "description": "City name"},
    ///     "units": {"type": "string", "enum": ["celsius", "fahrenheit"]}
    ///   },
    ///   "required": ["location"]
    /// }
    /// ```
    pub parameters: Box<JsonSchema>,

    /// Whether to enforce strict schema validation.
    ///
    /// When true (OpenAI strict mode):
    /// - All properties must be specified in the schema
    /// - No additional properties are allowed
    /// - Provides more predictable tool usage
    ///
    /// Provider support:
    /// - **OpenAI**: Supports strict mode
    /// - **Others**: May ignore this field
    pub strict: Option<bool>,
}

/// Configuration for how the model should use tools.
///
/// Controls whether tool usage is optional, required, or if a specific
/// tool must be used. The untagged attribute ensures clean JSON serialization
/// that matches the provider's expected format.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum UnifiedToolChoice {
    /// Mode-based tool choice.
    ///
    /// Specifies general behavior for tool usage without
    /// selecting a specific tool.
    Mode(UnifiedToolChoiceMode),

    /// Force use of a specific tool.
    ///
    /// The model must use the specified tool and cannot
    /// respond with plain text or other tools.
    Specific {
        /// The specific function to use.
        function: UnifiedFunctionChoice,
    },
}

/// Tool choice modes for general behavior.
///
/// These modes control how the model decides whether to use tools
/// without specifying a particular tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnifiedToolChoiceMode {
    /// Disable tool usage entirely.
    ///
    /// The model will only respond with text, even if tools are available.
    /// Useful when you want to temporarily disable tool usage.
    None,

    /// Let the model decide whether to use tools.
    ///
    /// The model will intelligently determine if any available tools
    /// would be helpful for responding to the user's request.
    /// This is the default behavior when tools are provided.
    Auto,

    /// Force the model to use at least one tool.
    ///
    /// The model must call one or more tools and cannot respond
    /// with only text. Useful when you know tool usage is necessary.
    ///
    /// Also known as:
    /// - "required" (OpenAI)
    /// - "any" (Anthropic)
    #[serde(alias = "required", alias = "any")]
    Required,
}

/// Specification for forcing use of a particular function.
///
/// When used in UnifiedToolChoice::Specific, this forces the model
/// to use the named function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedFunctionChoice {
    /// Name of the function to use.
    ///
    /// Must match exactly the name of an available tool.
    /// Case-sensitive.
    pub name: String,
}

/// Tool call request from the assistant.
///
/// Represents the assistant's decision to use a tool with specific arguments.
/// Each tool call has a unique ID for correlation with results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedToolCall {
    /// Unique identifier for this tool call.
    ///
    /// Used to correlate tool results with the original request.
    /// Format varies by provider:
    /// - OpenAI: "call_xxxx"
    /// - Anthropic: "toolu_xxxx"
    pub id: String,

    /// Function call details.
    ///
    /// Contains the function name and arguments to execute.
    pub function: UnifiedFunctionCall,
}

/// Function call specification with arguments.
///
/// Contains the function to call and the arguments to pass to it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedFunctionCall {
    /// Name of the function to call.
    ///
    /// Must match exactly the name of an available tool function.
    pub name: String,

    /// Arguments to pass to the function.
    ///
    /// The arguments must conform to the function's parameter schema.
    /// Can be provided as either a JSON string or a parsed JSON value.
    pub arguments: UnifiedArguments,
}

/// Function arguments in flexible format.
///
/// Supports both string (OpenAI) and parsed JSON (Anthropic) formats
/// to avoid unnecessary parsing/serialization during conversion.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum UnifiedArguments {
    /// Arguments as JSON string.
    ///
    /// Used by OpenAI format where arguments are provided as a JSON string
    /// that needs to be parsed by the tool executor.
    ///
    /// Example: `"{\"location\": \"Paris\", \"units\": \"celsius\"}"`
    String(String),

    /// Arguments as parsed JSON value.
    ///
    /// Used by Anthropic format where arguments are already parsed
    /// into a JSON structure.
    ///
    /// Example: `{"location": "Paris", "units": "celsius"}`
    Value(Value),
}

/// Metadata for request tracking and user attribution.
///
/// Allows associating requests with specific users for
/// rate limiting, analytics, and audit purposes.
///
/// Currently primarily supported by Anthropic's API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedMetadata {
    /// User identifier for tracking and rate limiting.
    ///
    /// Can be any unique identifier for the user:
    /// - User ID from your system
    /// - Session ID
    /// - API key identifier
    ///
    /// Used for:
    /// - Per-user rate limiting
    /// - Usage tracking and analytics
    /// - Audit trails
    /// - Debugging and support
    pub user_id: Option<String>,
}

/// Unified response from LLM completion.
///
/// Represents the complete response from any LLM provider,
/// containing the generated content, usage statistics, and metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedResponse {
    /// Unique identifier for this completion.
    ///
    /// Format varies by provider:
    /// - OpenAI: "chatcmpl-xxx"
    /// - Anthropic: "msg_xxx"
    pub id: String,

    /// Model that generated the response.
    ///
    /// May include provider prefix for clarity.
    /// Example: "gpt-4" or "anthropic/claude-3-opus"
    pub model: String,

    /// Response choices/candidates.
    ///
    /// Most providers return a single choice, but some support
    /// multiple candidates with different generation parameters.
    /// The first choice (index 0) is typically the primary response.
    pub choices: Vec<UnifiedChoice>,

    /// Token usage statistics.
    ///
    /// Provides token counts for billing and rate limiting purposes.
    pub usage: UnifiedUsage,

    /// Unix timestamp when the response was created.
    ///
    /// Seconds since Unix epoch (1970-01-01 00:00:00 UTC).
    pub created: u64,

    /// Anthropic-style stop reason.
    ///
    /// More detailed than finish_reason, preserved for fidelity
    /// when converting from Anthropic responses.
    pub stop_reason: Option<UnifiedStopReason>,

    /// The stop sequence that triggered completion.
    ///
    /// Only set if generation stopped due to a stop sequence match.
    /// Contains the actual sequence that was matched.
    pub stop_sequence: Option<String>,
}

/// Individual response choice/candidate.
///
/// Represents one possible completion. Most responses contain
/// a single choice, but some configurations can generate multiple.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedChoice {
    /// Index of this choice in the response.
    ///
    /// Starts at 0. Used to identify choices when multiple
    /// candidates are generated.
    pub index: u32,

    /// The generated message content.
    ///
    /// Contains the assistant's response including any text,
    /// tool calls, or multi-modal content.
    pub message: UnifiedMessage,

    /// Reason why generation stopped.
    ///
    /// Indicates whether the response is complete or was truncated.
    /// None typically means generation is still in progress (streaming).
    pub finish_reason: Option<UnifiedFinishReason>,
}

/// Token usage statistics for billing and rate limiting.
///
/// Tracks the number of tokens consumed by a request/response pair.
/// Token counts are model-specific and affect pricing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedUsage {
    /// Number of tokens in the input/prompt.
    ///
    /// Includes all messages, system prompt, and tool definitions.
    /// This is what you're charged for sending to the model.
    pub prompt_tokens: u32,

    /// Number of tokens in the output/completion.
    ///
    /// The tokens generated by the model in its response.
    /// Usually priced differently (often higher) than input tokens.
    pub completion_tokens: u32,

    /// Total tokens consumed (prompt + completion).
    ///
    /// Convenience field for total usage tracking.
    /// Used for rate limiting and billing calculations.
    pub total_tokens: u32,
}

/// Reason why the model stopped generating.
///
/// Indicates whether the response is complete or was truncated/filtered.
/// Used to determine if the response is usable and complete.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnifiedFinishReason {
    /// Natural stopping point reached.
    ///
    /// The model completed its response naturally.
    /// This is the ideal finish reason indicating a complete response.
    Stop,

    /// Maximum token limit reached.
    ///
    /// Generation stopped because it hit the max_tokens limit.
    /// The response may be incomplete or cut off mid-sentence.
    /// Consider increasing max_tokens if this occurs frequently.
    #[serde(alias = "max_tokens")]
    Length,

    /// Content filtered for safety/policy reasons.
    ///
    /// The model's response was blocked or filtered due to
    /// content policy violations or safety concerns.
    /// The response may be incomplete or replaced with a refusal.
    ContentFilter,

    /// Tool calls were made.
    ///
    /// The model decided to use one or more tools instead of
    /// (or in addition to) generating text. The response contains
    /// tool calls that need to be executed.
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

/// Detailed stop reason (Anthropic-style).
///
/// Provides more granular information about why generation stopped.
/// Preserved separately from UnifiedFinishReason for providers that
/// support more detailed stop reasons.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnifiedStopReason {
    /// Model reached the end of its turn.
    ///
    /// Natural completion of the assistant's response.
    /// Equivalent to UnifiedFinishReason::Stop.
    EndTurn,

    /// Maximum token limit was reached.
    ///
    /// Response truncated due to max_tokens setting.
    /// Equivalent to UnifiedFinishReason::Length.
    MaxTokens,

    /// A stop sequence was encountered.
    ///
    /// Generation stopped because one of the provided
    /// stop sequences was generated. The stop_sequence
    /// field will contain which sequence was matched.
    StopSequence,

    /// Model invoked a tool/function.
    ///
    /// Generation included tool calls.
    /// Equivalent to UnifiedFinishReason::ToolCalls.
    ToolUse,
}

/// Streaming chunk for incremental response delivery.
///
/// In streaming mode, responses are delivered as a series of chunks
/// via Server-Sent Events (SSE). Each chunk contains incremental
/// content that should be appended to build the complete response.
///
/// Uses `Cow` for efficient memory usage with static strings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedChunk {
    /// Chunk identifier.
    ///
    /// Same across all chunks in a streaming response.
    /// Identifies which completion this chunk belongs to.
    pub id: Cow<'static, str>,

    /// Model generating the chunk.
    ///
    /// Identifies which model is generating the response.
    /// Same across all chunks in a response.
    pub model: Cow<'static, str>,

    /// Incremental choice updates.
    ///
    /// Contains the new content to append for each choice.
    /// Usually contains a single choice at index 0.
    pub choices: Vec<UnifiedChoiceDelta>,

    /// Token usage statistics.
    ///
    /// Only present in the final chunk of a streaming response.
    /// Contains cumulative token counts for the entire response.
    pub usage: Option<UnifiedUsage>,

    /// Unix timestamp when this chunk was created.
    ///
    /// May be the same for all chunks or increment per chunk
    /// depending on the provider.
    pub created: u64,
}

/// Incremental update for a choice in streaming mode.
///
/// Each chunk contains a delta that should be applied to
/// the corresponding choice to build the complete response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedChoiceDelta {
    /// Index of the choice being updated.
    ///
    /// Corresponds to the choice index in the final response.
    /// Usually 0 for single-choice responses.
    pub index: u32,

    /// Incremental message content to append.
    ///
    /// Contains only the new content generated since the last chunk.
    /// Should be appended to the existing message content.
    pub delta: UnifiedMessageDelta,

    /// Reason why generation stopped.
    ///
    /// Only present in the final chunk for this choice.
    /// Indicates the response is complete when present.
    pub finish_reason: Option<UnifiedFinishReason>,
}

/// Incremental message content in streaming responses.
///
/// Contains partial content that should be appended to build
/// the complete message. Not all fields are present in every chunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedMessageDelta {
    /// Message role.
    ///
    /// Only present in the first chunk to establish the role.
    /// Subsequent chunks will have None.
    /// Almost always UnifiedRole::Assistant for model responses.
    pub role: Option<UnifiedRole>,

    /// Incremental text content to append.
    ///
    /// Contains new text generated since the last chunk.
    /// May be as small as a single character or word.
    /// Append to existing content to build the complete message.
    pub content: Option<String>,

    /// Incremental tool call updates.
    ///
    /// Contains updates to tool calls being generated.
    /// May include new tool calls starting or arguments being appended.
    pub tool_calls: Option<Vec<UnifiedStreamingToolCall>>,
}

/// Tool call updates in streaming responses.
///
/// Tool calls are built incrementally across multiple chunks.
/// First a Start variant establishes the tool call, then Delta
/// variants provide the arguments piece by piece.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum UnifiedStreamingToolCall {
    /// Start of a new tool call.
    ///
    /// Establishes a new tool call with its ID and function name.
    /// Arguments start empty and are built up by subsequent deltas.
    Start {
        /// Index of this tool call in the tool_calls array.
        index: usize,

        /// Unique identifier for this tool call.
        id: String,

        /// Initial function information.
        function: UnifiedFunctionStart,
    },

    /// Incremental arguments for an existing tool call.
    ///
    /// Appends additional argument content to a tool call
    /// that was previously started.
    Delta {
        /// Index matching the tool call to update.
        index: usize,

        /// Incremental function arguments.
        function: UnifiedFunctionDelta,
    },
}

/// Initial function information when starting a streaming tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedFunctionStart {
    /// Name of the function being called.
    ///
    /// Matches one of the available tool function names.
    pub name: String,

    /// Initial arguments content.
    ///
    /// Usually starts empty ("") and is built up by deltas.
    pub arguments: String,
}

/// Incremental function arguments in streaming tool calls.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedFunctionDelta {
    /// Additional argument content to append.
    ///
    /// Contains a fragment of JSON that should be appended
    /// to the existing arguments string. May be as small as
    /// a single character. The complete arguments are built
    /// by concatenating all deltas.
    pub arguments: String,
}

/// Object type identifier for API responses.
///
/// Used to identify the type of object in API responses,
/// following OpenAI's convention of including an "object" field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnifiedObjectType {
    /// Single model object.
    ///
    /// Used when returning information about a single model.
    Model,

    /// List container for multiple objects.
    ///
    /// Used when returning arrays of items (like model lists).
    List,

    /// Chat completion response object.
    ///
    /// Identifies a complete chat completion response.
    #[serde(rename = "chat.completion")]
    ChatCompletion,

    /// Streaming chat completion chunk.
    ///
    /// Identifies a single chunk in a streaming response.
    #[serde(rename = "chat.completion.chunk")]
    ChatCompletionChunk,

    /// Message object (Anthropic-style).
    ///
    /// Alternative identifier used by Anthropic for responses.
    Message,
}

/// Model information and capabilities.
///
/// Describes an available model including its identifier,
/// ownership, and metadata. Used in model listing responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedModel {
    /// Model identifier used in requests.
    ///
    /// This is the value to use in the "model" field of requests.
    /// Examples: "gpt-4", "claude-3-opus-20240229"
    pub id: String,

    /// Object type identifier.
    ///
    /// Always UnifiedObjectType::Model for individual models.
    /// The rename and alias support both "type" and "object" field names.
    #[serde(rename = "type", alias = "object")]
    pub object_type: UnifiedObjectType,

    /// Human-readable model name.
    ///
    /// May be the same as id or a more friendly name.
    /// Example: "GPT-4 Turbo" for id "gpt-4-turbo"
    pub display_name: String,

    /// Unix timestamp when the model was created.
    ///
    /// Seconds since Unix epoch. May be 0 for providers
    /// that don't track model creation time (like Anthropic).
    pub created: u64,

    /// Organization that owns/provides the model.
    ///
    /// Examples: "openai", "anthropic", "google", "amazon"
    /// Used to identify the model's provider.
    pub owned_by: String,
}

/// Response containing a list of available models.
///
/// Returned by the list models endpoint to show all available
/// models from configured providers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedModelsResponse {
    /// Object type identifier.
    ///
    /// Always UnifiedObjectType::List for model listings.
    #[serde(rename = "type", alias = "object")]
    pub object_type: UnifiedObjectType,

    /// List of available models from all configured providers.
    ///
    /// Each model includes its identifier and metadata.
    /// Models may be from different providers.
    pub models: Vec<UnifiedModel>,

    /// Indicates if pagination is available.
    ///
    /// True if there are more models that can be fetched
    /// with pagination parameters. Currently always false
    /// as all models are returned in a single response.
    pub has_more: bool,
}

impl UnifiedMessage {
    /// Extract tool calls from message content blocks.
    ///
    /// This method computes tool calls on-demand by extracting ToolUse blocks
    /// from the message content. This eliminates the need to store tool calls
    /// in both the content blocks and a separate tool_calls field, avoiding
    /// duplication and synchronization issues.
    ///
    /// Returns None if there are no tool calls in the message.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let tool_calls = message.compute_tool_calls();
    /// if let Some(calls) = tool_calls {
    ///     for call in calls {
    ///         execute_tool(call.function.name, call.function.arguments);
    ///     }
    /// }
    /// ```
    pub fn compute_tool_calls(&self) -> Option<Vec<UnifiedToolCall>> {
        if let UnifiedContentContainer::Blocks(blocks) = &self.content {
            let tool_calls: Vec<UnifiedToolCall> = blocks
                .iter()
                .filter_map(|block| match block {
                    UnifiedContent::ToolUse { id, name, input } => Some(UnifiedToolCall {
                        id: id.clone(),
                        function: UnifiedFunctionCall {
                            name: name.clone(),
                            arguments: UnifiedArguments::Value(input.clone()),
                        },
                    }),
                    _ => None,
                })
                .collect();

            if tool_calls.is_empty() { None } else { Some(tool_calls) }
        } else {
            None
        }
    }
}
