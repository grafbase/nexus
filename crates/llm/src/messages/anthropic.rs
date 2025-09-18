use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Request body for Anthropic Messages API.
///
/// This struct represents the request format for creating messages with Claude models
/// as documented in the [Anthropic API Reference](https://docs.anthropic.com/en/api/messages).
/// The format differs from OpenAI's format in several key ways:
/// - Messages have a different structure with content arrays
/// - System messages are separate from the messages array
/// - Tool use has a different format (tool_use/tool_result vs function calls)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AnthropicChatRequest {
    /// The model to use for the completion.
    ///
    /// Examples:
    /// - "claude-3-opus-20240229"
    /// - "claude-3-sonnet-20240229"
    /// - "claude-3-haiku-20240307"
    pub model: String,

    /// The messages to send to the model.
    ///
    /// Messages alternate between "user" and "assistant" roles.
    /// Each message contains an array of content blocks.
    pub messages: Vec<AnthropicMessage>,

    /// Maximum number of tokens to generate.
    ///
    /// Required for Anthropic API. Different models have different maximums.
    pub max_tokens: u32,

    /// System prompt to set context for the assistant.
    ///
    /// Optional. Sets the behavior and context for the assistant.
    /// Can be either a string or an array of content blocks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_system")]
    pub system: Option<String>,

    /// Controls randomness in the response.
    ///
    /// Range: 0.0 to 1.0
    /// Default: 1.0
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,

    /// Nucleus sampling parameter.
    ///
    /// Range: 0.0 to 1.0
    /// Only sample from the top tokens whose cumulative probability is >= top_p
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,

    /// Only sample from the top K tokens.
    ///
    /// Alternative to nucleus sampling.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,

    /// Sequences that will cause the model to stop generating.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,

    /// Whether to stream the response.
    ///
    /// When true, responses are sent as Server-Sent Events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,

    /// Custom metadata to attach to the request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<AnthropicMetadata>,

    /// Tools available for the model to use.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<AnthropicTool>>,

    /// Controls how the model uses tools.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<AnthropicToolChoice>,
}

/// An Anthropic message with role and content.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AnthropicMessage {
    /// The role of the message sender.
    pub role: AnthropicRole,

    /// The content of the message as an array of content blocks.
    #[serde(deserialize_with = "deserialize_content")]
    pub content: Vec<AnthropicContent>,
}

/// Role of a message sender in Anthropic's API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnthropicRole {
    /// User message
    User,
    /// Assistant message
    Assistant,
}

/// Content block in an Anthropic message.
///
/// Anthropic uses content arrays to support multi-modal messages.
/// Each block can be text, an image, tool use, or tool result.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum AnthropicContent {
    /// Plain text content.
    #[serde(rename = "text")]
    Text {
        /// The text content
        text: String,
    },

    /// Image content.
    #[serde(rename = "image")]
    Image {
        /// The image source
        source: AnthropicImageSource,
    },

    /// Tool use request from the assistant.
    #[serde(rename = "tool_use")]
    ToolUse {
        /// Unique identifier for this tool use
        id: String,
        /// Name of the tool to use
        name: String,
        /// Input parameters for the tool (arbitrary JSON data)
        input: Value,
    },

    /// Result from a tool execution.
    #[serde(rename = "tool_result")]
    ToolResult {
        /// The tool use ID this result corresponds to
        tool_use_id: String,
        /// The result content (can be text or error)
        #[serde(deserialize_with = "deserialize_tool_result_content")]
        content: Vec<AnthropicToolResultContent>,
    },
}

/// Content of a tool result.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum AnthropicToolResultContent {
    /// Text result from tool execution
    #[serde(rename = "text")]
    Text {
        /// The text content
        text: String,
    },

    /// Error result from tool execution
    #[serde(rename = "error")]
    Error {
        /// The error message
        error: String,
    },
}

/// Image source for image content blocks.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AnthropicImageSource {
    /// The type of image source (always "base64" for now)
    #[serde(rename = "type")]
    pub source_type: String,

    /// The media type of the image (e.g., "image/jpeg")
    pub media_type: String,

    /// Base64-encoded image data
    pub data: String,
}

/// Metadata for the request.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AnthropicMetadata {
    /// Optional user identifier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
}

/// Tool definition in Anthropic format.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AnthropicTool {
    /// The name of the tool
    pub name: String,

    /// Description of what the tool does
    pub description: String,

    /// JSON Schema for the tool's input parameters
    pub input_schema: Box<super::openai::JsonSchema>,
}

/// Tool choice configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnthropicToolChoice {
    /// Let the model decide whether to use tools
    Auto,

    /// Force the model to use any available tool
    Any,

    /// Force the model to use a specific tool
    Tool {
        /// The name of the tool to use
        name: String,
    },
}

/// Response from Anthropic Messages API.
///
/// This struct represents the response format from creating messages with Claude
/// as documented in the [Anthropic API Reference](https://docs.anthropic.com/en/api/messages).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicChatResponse {
    /// Unique identifier for this completion
    pub id: String,

    /// The type of response (always "message" for completions)
    pub r#type: String,

    /// The role of the response (always "assistant")
    pub role: AnthropicRole,

    /// The content of the response
    pub content: Vec<AnthropicContent>,

    /// The model that generated the response
    pub model: String,

    /// Stop reason for the completion
    pub stop_reason: Option<AnthropicStopReason>,

    /// Stop sequence that caused the model to stop, if any
    pub stop_sequence: Option<String>,

    /// Token usage statistics
    pub usage: AnthropicUsage,
}

/// The reason why the model stopped generating tokens.
///
/// Provides insight into why the generation ended.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AnthropicStopReason {
    /// The model reached a natural stopping point.
    /// This is the most common stop reason for conversational responses.
    EndTurn,
    /// The generation exceeded the maximum token limit specified in the request.
    MaxTokens,
    /// The model encountered a stop sequence specified in the request.
    StopSequence,
    /// The model invoked a tool.
    ToolUse,
}

impl fmt::Display for AnthropicStopReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AnthropicStopReason::EndTurn => write!(f, "end_turn"),
            AnthropicStopReason::MaxTokens => write!(f, "max_tokens"),
            AnthropicStopReason::StopSequence => write!(f, "stop_sequence"),
            AnthropicStopReason::ToolUse => write!(f, "tool_use"),
        }
    }
}

/// Token usage statistics in Anthropic format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicUsage {
    /// Number of input tokens
    pub input_tokens: i32,

    /// Number of output tokens
    pub output_tokens: i32,
}

/// Error response in Anthropic format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicError {
    /// The type of error (always "error")
    #[serde(rename = "type")]
    pub error_type: String,

    /// Error details
    pub error: AnthropicErrorDetails,
}

/// Error details in Anthropic format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicErrorDetails {
    /// The type of error that occurred
    #[serde(rename = "type")]
    pub error_type: String,

    /// Human-readable error message
    pub message: String,
}

/// Model information in Anthropic format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicModel {
    /// The model identifier
    pub id: String,

    /// The type (always "model")
    #[serde(rename = "type")]
    pub model_type: String,

    /// Display name for the model
    pub display_name: String,

    /// Unix timestamp when the model was created
    pub created_at: u64,
}

/// Response for listing available models in Anthropic format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicModelsResponse {
    /// List of available models
    pub data: Vec<AnthropicModel>,

    /// Whether there are more models to fetch
    pub has_more: bool,
}

/// Streaming event types for Anthropic SSE responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AnthropicStreamEvent {
    /// Start of a message
    #[serde(rename = "message_start")]
    MessageStart {
        /// The initial message metadata
        message: AnthropicStreamMessageStart,
    },

    /// Content block start
    #[serde(rename = "content_block_start")]
    ContentBlockStart {
        /// The index of the content block
        index: u32,
        /// The content block being started
        content_block: AnthropicContent,
    },

    /// Incremental content update
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta {
        /// The index of the content block
        index: u32,
        /// The delta update
        delta: AnthropicContentDelta,
    },

    /// Content block finished
    #[serde(rename = "content_block_stop")]
    ContentBlockStop {
        /// The index of the content block
        index: u32,
    },

    /// Message completed
    #[serde(rename = "message_delta")]
    MessageDelta {
        /// Delta update for the message
        delta: AnthropicMessageDelta,
        /// Updated usage statistics
        usage: AnthropicUsage,
    },

    /// End of message stream
    #[serde(rename = "message_stop")]
    MessageStop,

    /// Ping event to keep connection alive
    #[serde(rename = "ping")]
    Ping,

    /// Error event
    #[serde(rename = "error")]
    Error {
        /// The error that occurred
        error: AnthropicErrorDetails,
    },
}

/// Initial message metadata for streaming.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicStreamMessageStart {
    /// Unique message ID
    pub id: String,

    /// The type (always "message")
    #[serde(rename = "type")]
    pub message_type: String,

    /// The role (always "assistant")
    pub role: AnthropicRole,

    /// Initial empty content array
    pub content: Vec<AnthropicContent>,

    /// The model being used
    pub model: String,

    /// Initial usage statistics
    pub usage: AnthropicUsage,
}

/// Delta update for content blocks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AnthropicContentDelta {
    /// Text delta
    #[serde(rename = "text_delta")]
    TextDelta {
        /// Additional text content
        text: String,
    },

    /// Tool use input delta
    #[serde(rename = "input_json_delta")]
    InputJsonDelta {
        /// Partial JSON string for tool input
        partial_json: String,
    },
}

/// Message delta for streaming responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicMessageDelta {
    /// Stop reason if the message is complete
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<AnthropicStopReason>,

    /// Stop sequence if one was encountered
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
}

/// Custom deserializer for content field that handles both string and array formats.
///
/// Claude Code may send the content field as a string directly for simple messages,
/// while the standard Anthropic API expects an array of content blocks.
fn deserialize_content<'de, D>(deserializer: D) -> Result<Vec<AnthropicContent>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};

    struct ContentVisitor;

    impl<'de> Visitor<'de> for ContentVisitor {
        type Value = Vec<AnthropicContent>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a string or array of content blocks")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            // Convert string to a single text content block
            Ok(vec![AnthropicContent::Text {
                text: value.to_string(),
            }])
        }

        fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            // Convert string to a single text content block
            Ok(vec![AnthropicContent::Text { text: value }])
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            let mut contents = Vec::new();
            while let Some(content) = seq.next_element::<AnthropicContent>()? {
                contents.push(content);
            }
            Ok(contents)
        }
    }

    deserializer.deserialize_any(ContentVisitor)
}

/// Custom deserializer for the system field that handles both string and array formats.
///
/// Claude Code may send the system field as an array of content blocks,
/// while the standard Anthropic API expects a string.
fn deserialize_system<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};

    struct SystemVisitor;

    impl<'de> Visitor<'de> for SystemVisitor {
        type Value = Option<String>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a string or array of content blocks")
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            deserializer.deserialize_any(SystemContentVisitor)
        }
    }

    struct SystemContentVisitor;

    impl<'de> Visitor<'de> for SystemContentVisitor {
        type Value = Option<String>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a string or array of content blocks")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(value.to_string()))
        }

        fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(value))
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            // Extract text from content blocks and join them
            let mut text_parts = Vec::new();

            while let Some(value) = seq.next_element::<Value>()? {
                // Handle different content block types
                if let Some(block_type) = value.get("type").and_then(|v| v.as_str()) {
                    match block_type {
                        "text" => {
                            if let Some(text) = value.get("text").and_then(|v| v.as_str()) {
                                text_parts.push(text.to_string());
                            }
                        }
                        "image" => {
                            // Skip image blocks in system messages
                            continue;
                        }
                        _ => {
                            // Skip unknown block types
                            continue;
                        }
                    }
                } else if let Some(text) = value.as_str() {
                    // Handle plain string items
                    text_parts.push(text.to_string());
                }
            }

            if text_parts.is_empty() {
                Ok(None)
            } else {
                Ok(Some(text_parts.join("\n")))
            }
        }
    }

    deserializer.deserialize_option(SystemVisitor)
}

/// Custom deserializer for tool_result content field that handles both string and array formats.
///
/// Claude Code may send the content field as a string directly for simple tool results,
/// while the standard Anthropic API expects an array of content blocks.
fn deserialize_tool_result_content<'de, D>(deserializer: D) -> Result<Vec<AnthropicToolResultContent>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};

    struct ToolResultContentVisitor;

    impl<'de> Visitor<'de> for ToolResultContentVisitor {
        type Value = Vec<AnthropicToolResultContent>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a string or array of tool result content blocks")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            // Convert string to a single text content block
            Ok(vec![AnthropicToolResultContent::Text {
                text: value.to_string(),
            }])
        }

        fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            // Convert string to a single text content block
            Ok(vec![AnthropicToolResultContent::Text { text: value }])
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            let mut contents = Vec::new();
            while let Some(content) = seq.next_element::<AnthropicToolResultContent>()? {
                contents.push(content);
            }
            Ok(contents)
        }
    }

    deserializer.deserialize_any(ToolResultContentVisitor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serialize_basic_request() {
        let request = AnthropicChatRequest {
            model: "claude-3-opus-20240229".to_string(),
            messages: vec![AnthropicMessage {
                role: AnthropicRole::User,
                content: vec![AnthropicContent::Text {
                    text: "Hello, Claude!".to_string(),
                }],
            }],
            max_tokens: 1000,
            system: Some("You are a helpful assistant.".to_string()),
            temperature: Some(0.7),
            top_p: None,
            top_k: None,
            stop_sequences: None,
            stream: Some(false),
            metadata: None,
            tools: None,
            tool_choice: None,
        };

        let json = serde_json::to_value(&request).unwrap();

        assert_eq!(json["model"], "claude-3-opus-20240229");
        assert_eq!(json["max_tokens"], 1000);
        assert_eq!(json["system"], "You are a helpful assistant.");
        // Use approx comparison for floats
        assert!((json["temperature"].as_f64().unwrap() - 0.7).abs() < 0.0001);
        assert_eq!(json["stream"], false);
        assert_eq!(json["messages"][0]["role"], "user");
        assert_eq!(json["messages"][0]["content"][0]["type"], "text");
        assert_eq!(json["messages"][0]["content"][0]["text"], "Hello, Claude!");
    }

    #[test]
    fn deserialize_basic_response() {
        let json = json!({
            "id": "msg_123",
            "type": "message",
            "role": "assistant",
            "content": [
                {
                    "type": "text",
                    "text": "Hello! How can I help you today?"
                }
            ],
            "model": "claude-3-opus-20240229",
            "stop_reason": "end_turn",
            "stop_sequence": null,
            "usage": {
                "input_tokens": 10,
                "output_tokens": 20
            }
        });

        let response: AnthropicChatResponse = serde_json::from_value(json).unwrap();

        assert_eq!(response.id, "msg_123");
        assert_eq!(response.r#type, "message");
        assert_eq!(response.role, AnthropicRole::Assistant);
        assert_eq!(response.model, "claude-3-opus-20240229");
        assert_eq!(response.stop_reason, Some(AnthropicStopReason::EndTurn));
        assert_eq!(response.usage.input_tokens, 10);
        assert_eq!(response.usage.output_tokens, 20);

        let AnthropicContent::Text { text } = &response.content[0] else {
            unreachable!("Expected text content");
        };
        assert_eq!(text, "Hello! How can I help you today?");
    }

    #[test]
    fn serialize_tool_use() {
        let request = AnthropicChatRequest {
            model: "claude-3-opus-20240229".to_string(),
            messages: vec![AnthropicMessage {
                role: AnthropicRole::User,
                content: vec![AnthropicContent::Text {
                    text: "What's the weather in San Francisco?".to_string(),
                }],
            }],
            max_tokens: 1000,
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            stream: None,
            metadata: None,
            tools: Some(vec![AnthropicTool {
                name: "get_weather".to_string(),
                description: "Get the weather for a location".to_string(),
                input_schema: {
                    use std::collections::BTreeMap;
                    let mut properties = BTreeMap::new();
                    properties.insert(
                        "location".to_string(),
                        crate::messages::openai::JsonSchema {
                            r#type: Some("string".to_string()),
                            description: Some("The city and state".to_string()),
                            ..Default::default()
                        },
                    );

                    Box::new(crate::messages::openai::JsonSchema {
                        r#type: Some("object".to_string()),
                        properties: Some(properties),
                        required: Some(vec!["location".to_string()]),
                        ..Default::default()
                    })
                },
            }]),
            tool_choice: Some(AnthropicToolChoice::Auto),
        };

        let json = serde_json::to_value(&request).unwrap();

        assert_eq!(json["tools"][0]["name"], "get_weather");
        assert_eq!(json["tool_choice"]["type"], "auto");
    }

    #[test]
    fn deserialize_tool_use_response() {
        let json = json!({
            "id": "msg_123",
            "type": "message",
            "role": "assistant",
            "content": [
                {
                    "type": "text",
                    "text": "I'll check the weather for you."
                },
                {
                    "type": "tool_use",
                    "id": "tool_use_456",
                    "name": "get_weather",
                    "input": {
                        "location": "San Francisco, CA"
                    }
                }
            ],
            "model": "claude-3-opus-20240229",
            "stop_reason": "tool_use",
            "stop_sequence": null,
            "usage": {
                "input_tokens": 50,
                "output_tokens": 30
            }
        });

        let response: AnthropicChatResponse = serde_json::from_value(json).unwrap();

        assert_eq!(response.content.len(), 2);
        assert_eq!(response.stop_reason, Some(AnthropicStopReason::ToolUse));

        let AnthropicContent::ToolUse { id, name, input } = &response.content[1] else {
            unreachable!("Expected tool use content");
        };
        assert_eq!(id, "tool_use_456");
        assert_eq!(name, "get_weather");

        // Verify input contains the expected location data by serializing and parsing
        let json_str = serde_json::to_string(input).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert!(parsed.is_object());
        assert!(parsed.get("location").is_some());
        assert_eq!(parsed.get("location").unwrap().as_str().unwrap(), "San Francisco, CA");
    }

    #[test]
    fn serialize_streaming_events() {
        let event = AnthropicStreamEvent::MessageStart {
            message: AnthropicStreamMessageStart {
                id: "msg_123".to_string(),
                message_type: "message".to_string(),
                role: AnthropicRole::Assistant,
                content: vec![],
                model: "claude-3-opus-20240229".to_string(),
                usage: AnthropicUsage {
                    input_tokens: 10,
                    output_tokens: 0,
                },
            },
        };

        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "message_start");
        assert_eq!(json["message"]["id"], "msg_123");

        let delta_event = AnthropicStreamEvent::ContentBlockDelta {
            index: 0,
            delta: AnthropicContentDelta::TextDelta {
                text: "Hello".to_string(),
            },
        };

        let json = serde_json::to_value(&delta_event).unwrap();
        assert_eq!(json["type"], "content_block_delta");
        assert_eq!(json["delta"]["type"], "text_delta");
        assert_eq!(json["delta"]["text"], "Hello");
    }

    #[test]
    fn deserialize_error() {
        let json = json!({
            "type": "error",
            "error": {
                "type": "invalid_request_error",
                "message": "Invalid model specified"
            }
        });

        let error: AnthropicError = serde_json::from_value(json).unwrap();

        assert_eq!(error.error_type, "error");
        assert_eq!(error.error.error_type, "invalid_request_error");
        assert_eq!(error.error.message, "Invalid model specified");
    }

    #[test]
    fn deserialize_content_as_string() {
        // Test deserializing content as string (Claude Code format)
        let json = json!({
            "role": "user",
            "content": "Hello, Claude!"
        });

        let msg: AnthropicMessage = serde_json::from_value(json).unwrap();

        assert_eq!(msg.role, AnthropicRole::User);
        assert_eq!(msg.content.len(), 1);

        let AnthropicContent::Text { text } = &msg.content[0] else {
            unreachable!("Expected text content");
        };
        assert_eq!(text, "Hello, Claude!");
    }

    #[test]
    fn deserialize_mixed_content_formats() {
        // Test a conversation with mixed formats like Claude Code might send
        let messages_json = json!([
            {
                "role": "user",
                "content": "What's the weather?"
            },
            {
                "role": "assistant",
                "content": [
                    {"type": "text", "text": "I'll help you check the weather."}
                ]
            },
            {
                "role": "user",
                "content": "Thanks!"
            }
        ]);

        let messages: Vec<AnthropicMessage> = serde_json::from_value(messages_json).unwrap();

        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].content.len(), 1);
        assert_eq!(messages[1].content.len(), 1);
        assert_eq!(messages[2].content.len(), 1);
    }

    #[test]
    fn deserialize_tool_result_with_string_content() {
        // Test that tool_result content can be deserialized from a string
        // This is what Claude Code sends
        let message_json = json!({
            "role": "user",
            "content": [
                {
                    "type": "tool_result",
                    "tool_use_id": "test-id",
                    "content": "Cargo.toml\nsrc"
                }
            ]
        });

        let msg: AnthropicMessage = serde_json::from_value(message_json).unwrap();
        assert_eq!(msg.role, AnthropicRole::User);
        assert_eq!(msg.content.len(), 1);

        match &msg.content[0] {
            AnthropicContent::ToolResult { tool_use_id, content } => {
                assert_eq!(tool_use_id, "test-id");
                assert_eq!(content.len(), 1);
                match &content[0] {
                    AnthropicToolResultContent::Text { text } => {
                        assert_eq!(text, "Cargo.toml\nsrc");
                    }
                    _ => unreachable!("Expected Text content"),
                }
            }
            _ => unreachable!("Expected ToolResult content"),
        }
    }

    #[test]
    fn deserialize_tool_result_with_array_content() {
        // Test that tool_result content can still be deserialized from array format
        // This is the standard Anthropic format
        let message_json = json!({
            "role": "user",
            "content": [
                {
                    "type": "tool_result",
                    "tool_use_id": "test-id",
                    "content": [
                        {
                            "type": "text",
                            "text": "Result line 1"
                        },
                        {
                            "type": "text",
                            "text": "Result line 2"
                        }
                    ]
                }
            ]
        });

        let msg: AnthropicMessage = serde_json::from_value(message_json).unwrap();
        assert_eq!(msg.role, AnthropicRole::User);
        assert_eq!(msg.content.len(), 1);

        match &msg.content[0] {
            AnthropicContent::ToolResult { tool_use_id, content } => {
                assert_eq!(tool_use_id, "test-id");
                assert_eq!(content.len(), 2);
                match &content[0] {
                    AnthropicToolResultContent::Text { text } => {
                        assert_eq!(text, "Result line 1");
                    }
                    _ => unreachable!("Expected Text content"),
                }
                match &content[1] {
                    AnthropicToolResultContent::Text { text } => {
                        assert_eq!(text, "Result line 2");
                    }
                    _ => unreachable!("Expected Text content"),
                }
            }
            _ => unreachable!("Expected ToolResult content"),
        }
    }

    #[test]
    fn deserialize_request_with_tool_use_blocks() {
        let json = json!({
            "model": "anthropic/claude-3-5-haiku-latest",
            "messages": [
                {
                    "role": "user",
                    "content": "Help me find and run files"
                },
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "text",
                            "text": "I'll search for files and run a command."
                        },
                        {
                            "type": "tool_use",
                            "id": "toolu_01XyzAbc123",
                            "name": "Glob",
                            "input": {"pattern": "*.toml"}
                        }
                    ]
                }
            ],
            "max_tokens": 100,
            "stream": false
        });

        let request: AnthropicChatRequest = serde_json::from_value(json).unwrap();
        assert_eq!(request.messages.len(), 2);
        assert_eq!(request.messages[1].content.len(), 2);
    }
}
