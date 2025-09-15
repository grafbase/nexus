use std::borrow::Cow;

use serde::Deserialize;

use crate::messages::{
    ChatChoice, ChatChoiceDelta, ChatCompletionChunk, ChatCompletionResponse, ChatMessage, ChatMessageDelta, ChatRole,
    FinishReason, FunctionDelta, FunctionStart, ObjectType, StreamingToolCall, Usage,
};

/// Describes the type of content in an Anthropic message.
///
/// Used to distinguish between different content blocks in the response.
#[derive(Debug, Deserialize, PartialEq)]
pub enum ContentType {
    /// Plain text content.
    #[serde(rename = "text")]
    Text,
    /// Tool use request from the model.
    #[serde(rename = "tool_use")]
    ToolUse,
    /// Result from a tool execution.
    #[serde(rename = "tool_result")]
    ToolResult,
    /// Image content (for multi-modal inputs).
    #[serde(rename = "image")]
    Image,
    /// Any other content type not yet known.
    /// Captures the actual string value for forward compatibility.
    #[serde(untagged)]
    Other(String),
}

/// The reason why the model stopped generating tokens.
///
/// Provides insight into why the generation ended.
#[derive(Debug, Deserialize, PartialEq)]
pub enum StopReason {
    /// The model reached a natural stopping point.
    /// This is the most common stop reason for conversational responses.
    #[serde(rename = "end_turn")]
    EndTurn,
    /// The generation exceeded the maximum token limit specified in the request.
    #[serde(rename = "max_tokens")]
    MaxTokens,
    /// The model encountered a stop sequence specified in the request.
    #[serde(rename = "stop_sequence")]
    StopSequence,
    /// The model invoked a tool.
    #[serde(rename = "tool_use")]
    ToolUse,
    /// The model paused its turn (for advanced use cases).
    #[serde(rename = "pause_turn")]
    PauseTurn,
    /// The model refused to generate content due to safety concerns.
    #[serde(rename = "refusal")]
    Refusal,
    /// Any other stop reason not yet known.
    /// Captures the actual string value for forward compatibility.
    #[serde(untagged)]
    Other(String),
}

/// The type of response from the Anthropic API.
#[derive(Debug, Deserialize, PartialEq)]
pub enum ResponseType {
    /// A standard message response.
    #[serde(rename = "message")]
    Message,
    /// Any other response type not yet known.
    /// Captures the actual string value for forward compatibility.
    #[serde(untagged)]
    Other(String),
}

/// Response from Anthropic Messages API.
///
/// This struct represents the response format from creating messages with Claude
/// as documented in the [Anthropic API Reference](https://docs.anthropic.com/en/api/messages).
#[derive(Debug, Deserialize)]
pub struct AnthropicResponse {
    /// Unique identifier for the message.
    pub id: String,

    /// Object type. Always "message" for message responses.
    #[allow(dead_code)]
    pub r#type: ResponseType,

    /// Conversational role of the generated message.
    /// This will always be "assistant".
    pub role: ChatRole,

    /// Content blocks in the response.
    /// Each block contains a portion of the response with its type.
    pub content: Vec<AnthropicContent>,

    /// The model that handled the request.
    #[allow(dead_code)]
    pub model: String,

    /// The reason the model stopped generating.
    /// See [`StopReason`] for possible values.
    pub stop_reason: Option<StopReason>,

    /// Which custom stop sequence was triggered, if any.
    #[allow(dead_code)]
    pub stop_sequence: Option<String>,

    /// Billing and rate limit usage information.
    pub usage: AnthropicUsage,
}

/// A content block in an Anthropic message response.
///
/// Represents a single piece of content which could be text, tool use, etc.
#[derive(Debug, Deserialize)]
pub struct AnthropicContent {
    /// The type of this content block.
    pub r#type: ContentType,

    /// Text content if this is a text block.
    /// Will be `None` for non-text content types.
    #[serde(default)]
    pub text: Option<String>,

    /// Unique identifier for tool use blocks.
    /// Format: "toolu_{alphanumeric}"
    #[serde(default)]
    pub id: Option<String>,

    /// Name of the tool/function being called.
    #[serde(default)]
    pub name: Option<String>,

    /// Input arguments for the tool as JSON.
    #[serde(default)]
    pub input: Option<serde_json::Value>,
}

/// Token usage information for an Anthropic API request.
///
/// Used for tracking consumption and billing.
#[derive(Debug, Deserialize, Clone, Copy)]
pub struct AnthropicUsage {
    /// Number of tokens in the input prompt.
    /// This includes the system prompt, messages, and any other input.
    /// In streaming message_delta events, this field may be omitted.
    #[serde(default)]
    pub input_tokens: i32,

    /// Number of tokens generated in the response.
    pub output_tokens: i32,
}

impl From<StopReason> for FinishReason {
    fn from(reason: StopReason) -> Self {
        match reason {
            StopReason::EndTurn => FinishReason::Stop,
            StopReason::MaxTokens => FinishReason::Length,
            StopReason::StopSequence => FinishReason::Stop,
            StopReason::ToolUse => FinishReason::ToolCalls,
            StopReason::PauseTurn => FinishReason::Other("pause".to_string()),
            StopReason::Refusal => FinishReason::ContentFilter,
            StopReason::Other(s) => {
                log::warn!("Unknown stop reason from Anthropic: {s}");
                FinishReason::Other(s)
            }
        }
    }
}

impl From<AnthropicResponse> for ChatCompletionResponse {
    fn from(response: AnthropicResponse) -> Self {
        use crate::messages::{FunctionCall, ToolCall};

        // Extract text content
        let message_content = response
            .content
            .iter()
            .filter_map(|c| match &c.r#type {
                ContentType::Text => c.text.clone(),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("");

        // Extract tool calls
        let tool_calls: Vec<ToolCall> = response
            .content
            .iter()
            .filter_map(|c| match &c.r#type {
                ContentType::ToolUse => Some(ToolCall {
                    id: c
                        .id
                        .clone()
                        .unwrap_or_else(|| format!("toolu_{}", uuid::Uuid::new_v4())),
                    tool_type: crate::messages::ToolCallType::Function,
                    function: FunctionCall {
                        name: c.name.clone().unwrap_or_default(),
                        arguments: c
                            .input
                            .as_ref()
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "{}".to_string()),
                    },
                }),
                _ => None,
            })
            .collect();

        // Determine if we have content or tool calls
        let content = if message_content.is_empty() {
            None
        } else {
            Some(message_content)
        };

        let tool_calls_opt = if tool_calls.is_empty() { None } else { Some(tool_calls) };

        Self {
            id: response.id,
            object: ObjectType::ChatCompletion,
            created: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            model: String::new(), // Will be set by the provider
            choices: vec![ChatChoice {
                index: 0,
                message: ChatMessage {
                    role: response.role,
                    content,
                    tool_calls: tool_calls_opt,
                    tool_call_id: None,
                },
                finish_reason: response.stop_reason.map(Into::into).unwrap_or(FinishReason::Stop),
            }],
            usage: Usage {
                prompt_tokens: response.usage.input_tokens as u32,
                completion_tokens: response.usage.output_tokens as u32,
                total_tokens: (response.usage.input_tokens + response.usage.output_tokens) as u32,
            },
        }
    }
}

// Streaming types for Anthropic SSE responses

/// Anthropic streaming event types with borrowed strings for zero-copy parsing.
///
/// Anthropic uses a more complex streaming format than OpenAI, with distinct event
/// types for different stages of message generation. Events arrive as Server-Sent Events
/// with both an event type and JSON data.
///
/// See: https://docs.anthropic.com/en/api/messages-streaming
///
/// Event flow for a typical streaming response:
/// 1. `message_start` - Initial message metadata with empty content
/// 2. `content_block_start` - Beginning of a content block (text or tool use)
/// 3. `content_block_delta` - Incremental content updates (multiple)
/// 4. `content_block_stop` - End of the current content block
/// 5. `message_delta` - Final message metadata (stop reason, usage)
/// 6. `message_stop` - End of streaming
#[allow(dead_code)] // Streaming types are defined but not yet fully implemented
#[derive(Debug, Deserialize)]
#[serde(tag = "type", bound = "'de: 'a")]
pub enum AnthropicStreamEvent<'a> {
    /// Sent at the start of a streaming response.
    ///
    /// Contains initial message metadata including ID, model, and token usage.
    /// The content array is empty at this stage.
    #[serde(rename = "message_start")]
    MessageStart { message: AnthropicMessageStart<'a> },

    /// Sent when a new content block begins.
    ///
    /// Content blocks can be:
    /// - `text`: Regular text response
    /// - `tool_use`: Tool/function call
    ///
    /// Each block has an index for ordering multiple blocks.
    #[serde(rename = "content_block_start")]
    ContentBlockStart {
        index: u32,
        content_block: AnthropicContentBlock,
    },

    /// Sent for each incremental update to a content block.
    ///
    /// For text blocks: Contains text fragments to append
    /// For tool use blocks: Contains partial JSON arguments
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta { index: u32, delta: AnthropicBlockDelta<'a> },

    /// Sent when a content block is complete.
    ///
    /// Indicates no more deltas will be sent for this block index.
    #[serde(rename = "content_block_stop")]
    ContentBlockStop { index: u32 },

    /// Sent with final message metadata.
    ///
    /// Contains the stop reason (why generation ended) and final token counts.
    /// Usually sent after all content blocks are complete.
    #[serde(rename = "message_delta")]
    MessageDelta {
        delta: AnthropicMessageDeltaData,
        usage: AnthropicUsage,
    },

    /// Sent at the end of the streaming response.
    ///
    /// Indicates the message is complete and the stream will close.
    #[serde(rename = "message_stop")]
    MessageStop,

    /// Periodic ping events to keep the connection alive.
    ///
    /// Sent every few seconds during long responses to prevent timeout.
    /// Can be safely ignored by clients.
    #[serde(rename = "ping")]
    Ping,

    /// Error event if something goes wrong during streaming.
    ///
    /// Contains error type and message. The stream ends after an error.
    #[serde(rename = "error")]
    Error { error: AnthropicStreamError<'a> },
}

/// Initial message metadata in a streaming response.
///
/// Sent in the `message_start` event at the beginning of streaming.
/// Contains the message structure that will be populated by subsequent events.
#[allow(dead_code)] // Streaming types are defined but not yet fully implemented
#[derive(Debug, Deserialize)]
pub struct AnthropicMessageStart<'a> {
    /// Unique message identifier.
    ///
    /// Format: "msg_{alphanumeric}"
    /// Example: "msg_01XFDUDYJgAACzvnptvVoYEL"
    pub id: &'a str,

    /// The model being used for this response.
    ///
    /// Examples: "claude-3-opus-20240229", "claude-3-sonnet-20240229"
    pub model: &'a str,

    /// Role of the message author.
    ///
    /// Always "assistant" for model responses.
    pub role: &'a str,

    /// Content array that will be populated by content blocks.
    ///
    /// Always empty (`[]`) in the message_start event.
    /// Gets filled through content_block_start/delta/stop events.
    /// We don't process this field as we build content from deltas instead.
    pub content: Vec<sonic_rs::Value>,

    /// The reason the model stopped generating.
    ///
    /// Always `null` in message_start, set in message_delta.
    /// Possible values: "end_turn", "max_tokens", "stop_sequence", "tool_use"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<&'a str>,

    /// The stop sequence that caused generation to stop.
    ///
    /// Only present if stop_reason is "stop_sequence".
    /// Contains the exact string that triggered the stop.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<&'a str>,

    /// Initial token usage statistics.
    ///
    /// Contains input_tokens count at start.
    /// output_tokens is 0 initially, updated in message_delta.
    pub usage: AnthropicUsage,
}

/// Content block metadata when starting a new block.
///
/// Sent in `content_block_start` events to indicate the type and initial
/// state of a new content block being generated.
///
/// Uses internally tagged enum based on the "type" field.
/// Uses owned strings since we convert to owned when creating tool calls anyway.
#[allow(dead_code)] // Streaming types are defined but not yet fully implemented
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnthropicContentBlock {
    /// Text content block - regular text response
    #[serde(rename = "text")]
    Text {
        /// Initial text content.
        /// Usually empty string "" at start, filled via deltas.
        text: String,
    },

    /// Tool use block - function/tool call
    #[serde(rename = "tool_use")]
    ToolUse {
        /// Unique identifier for this tool call.
        /// Format: "toolu_{alphanumeric}"
        /// Example: "toolu_01T1x1fJ34qAmk2tNTrN7Up6"
        id: String,

        /// Name of the tool/function being called.
        /// Example: "get_weather", "search_web"
        name: String,
    },
}

/// Delta content for a content block.
///
/// Sent in `content_block_delta` events with incremental updates
/// to append to the current content block.
///
/// Uses internally tagged enum based on the "type" field.
/// Uses Cow (Clone on Write) to handle both borrowed strings (when no escaping needed)
/// and owned strings (when escape sequences like \n need to be unescaped).
#[allow(dead_code)] // Streaming types are defined but not yet fully implemented
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnthropicBlockDelta<'a> {
    /// Text delta - incremental text content
    #[serde(rename = "text_delta")]
    TextDelta {
        /// Text fragment to append to the current text block.
        /// Can be any length from a single character to multiple words.
        /// Concatenate all text deltas to build the complete response.
        text: Cow<'a, str>,
    },

    /// Input JSON delta - incremental tool arguments
    #[serde(rename = "input_json_delta")]
    InputJsonDelta {
        /// Partial JSON string for tool/function arguments.
        /// Contains fragments of JSON that should be concatenated
        /// to build the complete tool arguments object.
        partial_json: Cow<'a, str>,
    },
}

/// Final message metadata delta.
///
/// Sent in `message_delta` events near the end of streaming
/// with final metadata about why generation stopped.
#[allow(dead_code)] // Streaming types are defined but not yet fully implemented
#[derive(Debug, Deserialize)]
pub struct AnthropicMessageDeltaData {
    /// The reason the model stopped generating.
    ///
    /// Possible values:
    /// - "end_turn": Model finished its response naturally
    /// - "max_tokens": Hit the max_tokens limit
    /// - "stop_sequence": Hit a stop sequence from the request
    /// - "tool_use": Model decided to use a tool
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,

    /// The specific stop sequence that triggered completion.
    ///
    /// Only present when stop_reason is "stop_sequence".
    /// Contains the exact string from the stop_sequences array
    /// that was encountered in the output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
}

/// Error information in streaming response.
///
/// Sent in `error` events when something goes wrong during streaming.
/// The stream ends immediately after an error event.
#[derive(Debug, Deserialize)]
pub struct AnthropicStreamError<'a> {
    /// Type of error that occurred.
    ///
    /// Common values:
    /// - "invalid_request_error": Problem with request parameters
    /// - "authentication_error": Invalid or missing API key
    /// - "permission_error": Lack of access to requested resource
    /// - "not_found_error": Requested resource doesn't exist
    /// - "rate_limit_error": Too many requests
    /// - "api_error": Server-side error
    /// - "overloaded_error": Servers are overloaded
    #[serde(rename = "type")]
    pub error_type: &'a str,

    /// Human-readable error message describing what went wrong.
    ///
    /// Examples:
    /// - "Invalid API key provided"
    /// - "Rate limit exceeded. Please wait before retrying."
    /// - "The model claude-3-opus is not available"
    pub message: &'a str,
}

/// State machine for converting Anthropic stream events to OpenAI-compatible chunks.
///
/// Anthropic's streaming format is significantly different from OpenAI's:
/// - Anthropic uses typed events with a state machine approach
/// - OpenAI uses simpler delta chunks
///
/// This processor maintains state across Anthropic events to generate
/// equivalent OpenAI-format chunks that our unified API can handle.
///
/// State tracked:
/// - Message ID from message_start
/// - Current text being accumulated from deltas
/// - Model name for response
/// - Usage statistics
/// - Current tool calls being constructed
pub struct AnthropicStreamProcessor {
    provider_name: String,
    message_id: Option<String>,
    model: Option<String>,
    current_text: String,
    usage: Option<AnthropicUsage>,
    created: u64,
    /// Tool calls being constructed (index -> tool call data)
    current_tool_calls: std::collections::HashMap<u32, ToolCallBuilder>,
}

/// Helper struct for building tool calls incrementally from streaming chunks.
#[derive(Debug, Clone)]
struct ToolCallBuilder {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

impl AnthropicStreamProcessor {
    pub fn new(provider_name: String) -> Self {
        Self {
            provider_name,
            message_id: None,
            model: None,
            current_text: String::new(),
            usage: None,
            created: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            current_tool_calls: std::collections::HashMap::new(),
        }
    }

    /// Process an Anthropic stream event and convert to OpenAI-compatible chunk if applicable.
    pub fn process_event(&mut self, event: AnthropicStreamEvent<'_>) -> Option<ChatCompletionChunk> {
        match event {
            AnthropicStreamEvent::MessageStart { message } => {
                // Store message metadata for later chunks with provider prefix
                self.message_id = Some(message.id.to_string());
                self.model = Some(format!("{}/{}", self.provider_name, message.model));
                self.usage = Some(message.usage);

                // Emit initial chunk with role
                Some(ChatCompletionChunk {
                    id: self.message_id.clone().unwrap_or_default(),
                    object: ObjectType::ChatCompletionChunk,
                    created: self.created,
                    model: self.model.clone().unwrap_or_default(),
                    choices: vec![ChatChoiceDelta {
                        index: 0,
                        delta: ChatMessageDelta {
                            role: Some(ChatRole::Assistant),
                            content: None,
                            tool_calls: None,
                            function_call: None,
                        },
                        finish_reason: None,
                        logprobs: None,
                    }],
                    system_fingerprint: None,
                    usage: None,
                })
            }

            AnthropicStreamEvent::ContentBlockStart { index, content_block } => {
                // Handle the start of a tool use block
                match content_block {
                    AnthropicContentBlock::ToolUse { id, name } => {
                        let tool_call = ToolCallBuilder {
                            id: Some(id),
                            name: Some(name),
                            arguments: String::new(),
                        };

                        self.current_tool_calls.insert(index, tool_call.clone());

                        // Emit tool call start chunk
                        let tool_call_value = StreamingToolCall::Start {
                            index: 0,
                            id: tool_call.id.unwrap_or_default(),
                            r#type: crate::messages::ToolCallType::Function,
                            function: FunctionStart {
                                name: tool_call.name.unwrap_or_default(),
                                arguments: String::new(),
                            },
                        };

                        Some(ChatCompletionChunk {
                            id: self.message_id.clone().unwrap_or_default(),
                            object: ObjectType::ChatCompletionChunk,
                            created: self.created,
                            model: self.model.clone().unwrap_or_default(),
                            choices: vec![ChatChoiceDelta {
                                index: 0,
                                delta: ChatMessageDelta {
                                    role: None,
                                    content: None,
                                    tool_calls: Some(vec![tool_call_value]),
                                    function_call: None,
                                },
                                finish_reason: None,
                                logprobs: None,
                            }],
                            system_fingerprint: None,
                            usage: None,
                        })
                    }
                    AnthropicContentBlock::Text { .. } => {
                        // For text blocks, we don't emit anything at start
                        None
                    }
                }
            }

            AnthropicStreamEvent::ContentBlockDelta { index, delta } => {
                match delta {
                    AnthropicBlockDelta::TextDelta { text } => {
                        // Handle text content
                        self.current_text.push_str(&text);

                        Some(ChatCompletionChunk {
                            id: self.message_id.clone().unwrap_or_default(),
                            object: ObjectType::ChatCompletionChunk,
                            created: self.created,
                            model: self.model.clone().unwrap_or_default(),
                            choices: vec![ChatChoiceDelta {
                                index: 0,
                                delta: ChatMessageDelta {
                                    role: None,
                                    content: Some(text.into_owned()),
                                    tool_calls: None,
                                    function_call: None,
                                },
                                finish_reason: None,
                                logprobs: None,
                            }],
                            system_fingerprint: None,
                            usage: None,
                        })
                    }

                    AnthropicBlockDelta::InputJsonDelta { partial_json } => {
                        // Handle tool call arguments accumulation
                        if let Some(tool_call) = self.current_tool_calls.get_mut(&index) {
                            tool_call.arguments.push_str(&partial_json);

                            // Emit tool call arguments chunk
                            let tool_call_value = StreamingToolCall::Delta {
                                index: 0,
                                function: FunctionDelta {
                                    arguments: partial_json.into_owned(),
                                },
                            };

                            Some(ChatCompletionChunk {
                                id: self.message_id.clone().unwrap_or_default(),
                                object: ObjectType::ChatCompletionChunk,
                                created: self.created,
                                model: self.model.clone().unwrap_or_default(),
                                choices: vec![ChatChoiceDelta {
                                    index: 0,
                                    delta: ChatMessageDelta {
                                        role: None,
                                        content: None,
                                        tool_calls: Some(vec![tool_call_value]),
                                        function_call: None,
                                    },
                                    finish_reason: None,
                                    logprobs: None,
                                }],
                                system_fingerprint: None,
                                usage: None,
                            })
                        } else {
                            None
                        }
                    }
                }
            }

            AnthropicStreamEvent::MessageDelta { delta, usage } => {
                // Final chunk with finish reason and usage
                self.usage = Some(usage);

                let finish_reason = delta.stop_reason.as_deref().map(|reason| match reason {
                    "end_turn" => FinishReason::Stop,
                    "max_tokens" => FinishReason::Length,
                    "stop_sequence" => FinishReason::Stop,
                    "tool_use" => FinishReason::ToolCalls,
                    other => FinishReason::Other(other.to_string()),
                });

                Some(ChatCompletionChunk {
                    id: self.message_id.clone().unwrap_or_default(),
                    object: ObjectType::ChatCompletionChunk,
                    created: self.created,
                    model: self.model.clone().unwrap_or_default(),
                    choices: vec![ChatChoiceDelta {
                        index: 0,
                        delta: ChatMessageDelta {
                            role: None,
                            content: None,
                            tool_calls: None,
                            function_call: None,
                        },
                        finish_reason,
                        logprobs: None,
                    }],
                    system_fingerprint: None,
                    usage: self.usage.as_ref().map(|u| Usage {
                        prompt_tokens: u.input_tokens as u32,
                        completion_tokens: u.output_tokens as u32,
                        total_tokens: (u.input_tokens + u.output_tokens) as u32,
                    }),
                })
            }

            AnthropicStreamEvent::Error { error } => {
                log::error!("Anthropic stream error: {} - {}", error.error_type, error.message);
                None
            }

            _ => None, // Ignore other events (Ping, ContentBlockStop, MessageStop)
        }
    }
}
