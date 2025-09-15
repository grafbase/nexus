use serde::Serialize;

use crate::messages::{ChatCompletionRequest, ChatMessage, ChatRole, Tool, ToolChoice};

/// Request body for Anthropic Messages API.
///
/// This struct represents the request format for creating messages with Claude models
/// as documented in the [Anthropic API Reference](https://docs.anthropic.com/en/api/messages).
#[derive(Debug, Serialize)]
pub struct AnthropicRequest {
    /// The model that will complete your prompt.
    /// See [models](https://docs.anthropic.com/en/docs/models-overview) for additional details.
    /// Examples: "claude-3-opus-20240229", "claude-3-sonnet-20240229", "claude-3-haiku-20240307"
    pub model: String,

    /// Input messages.
    ///
    /// Our models are trained to operate on alternating user and assistant conversational turns.
    /// Messages must alternate between user and assistant roles.
    pub messages: Vec<AnthropicMessage>,

    /// System prompt.
    ///
    /// A system prompt is a way of providing context and instructions to Claude,
    /// separate from the user's direct input.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,

    /// The maximum number of tokens to generate before stopping.
    ///
    /// Different models have different maximum values.
    /// Refer to [models](https://docs.anthropic.com/en/docs/models-overview) for details.
    pub max_tokens: u32,

    /// Amount of randomness injected into the response.
    ///
    /// Defaults to 1.0. Ranges from 0.0 to 1.0. Use temperature closer to 0.0
    /// for analytical / multiple choice, and closer to 1.0 for creative and generative tasks.
    ///
    /// Note that even with temperature of 0.0, the results will not be fully deterministic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,

    /// Use nucleus sampling.
    ///
    /// In nucleus sampling, we compute the cumulative distribution over all the options
    /// for each subsequent token in decreasing probability order and cut it off once it
    /// exceeds the value of top_p. You should either alter temperature or top_p, but not both.
    ///
    /// Recommended for advanced use cases only. You usually only need to use temperature.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,

    /// Only sample from the top K options for each subsequent token.
    ///
    /// Used to remove "long tail" low probability responses.
    /// [Learn more technical details here](https://towardsdatascience.com/how-to-sample-from-language-models-682bceb97277).
    ///
    /// Recommended for advanced use cases only. You usually only need to use temperature.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,

    /// Custom text sequences that will cause the model to stop generating.
    ///
    /// Our models will normally stop when they have naturally completed their turn,
    /// which will result in a response stop_reason of "end_turn".
    ///
    /// If you want the model to stop generating when it encounters custom strings of text,
    /// you can use the stop_sequences parameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,

    /// Whether to stream the response using server-sent events.
    ///
    /// When true, the response will be streamed incrementally as it's generated.
    /// Default is false for non-streaming responses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,

    /// Tools available for the model to use.
    ///
    /// A list of tools the model may call. Currently, only functions are supported as tools.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<AnthropicTool>>,

    /// Controls how the model uses tools.
    ///
    /// Can be "auto" (default), "none", or a specific tool choice.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<AnthropicToolChoice>,
}

/// Represents a message in the conversation with Claude.
///
/// Messages must alternate between user and assistant roles.
#[derive(Debug, Serialize)]
pub struct AnthropicMessage {
    /// The role of the message sender.
    /// Must be either "user" or "assistant".
    pub role: ChatRole,

    /// The content of the message.
    /// Can be a string or an array of content blocks for tool responses.
    pub content: AnthropicMessageContent,
}

/// Content of an Anthropic message.
///
/// Can be either a simple string or an array of content blocks
/// (for messages with tool use or tool results).
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum AnthropicMessageContent {
    /// Simple text content
    Text(String),
    /// Array of content blocks (for tool use/results)
    Blocks(Vec<AnthropicContentBlock>),
}

/// A content block in an Anthropic message.
///
/// Used for tool use and tool results.
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum AnthropicContentBlock {
    /// Text content block
    #[serde(rename = "text")]
    Text { text: String },

    /// Tool use block (when assistant calls a tool)
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },

    /// Tool result block (response from tool execution)
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
}

/// Anthropic tool definition.
///
/// Defines a tool that the model can use. Currently only functions are supported.
#[derive(Debug, Serialize)]
pub struct AnthropicTool {
    /// The name of the tool. Must be unique.
    pub name: String,

    /// A description of what the tool does.
    pub description: String,

    /// The parameters the tool accepts, described as a JSON Schema object.
    pub input_schema: serde_json::Value,
}

impl From<Tool> for AnthropicTool {
    fn from(tool: Tool) -> Self {
        Self {
            name: tool.function.name,
            description: tool.function.description,
            input_schema: tool.function.parameters,
        }
    }
}

/// Controls how the model uses tools.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnthropicToolChoice {
    /// Auto tool selection
    Auto,

    /// Any tool selection (required)
    Any,

    /// Force a specific tool
    Tool { name: String },
}

impl From<ToolChoice> for AnthropicToolChoice {
    fn from(choice: ToolChoice) -> Self {
        match choice {
            ToolChoice::Mode(mode) => {
                use crate::messages::ToolChoiceMode;
                match mode {
                    ToolChoiceMode::Auto => AnthropicToolChoice::Auto,
                    ToolChoiceMode::Required | ToolChoiceMode::Any => AnthropicToolChoice::Any,
                    ToolChoiceMode::None => AnthropicToolChoice::Auto, // Anthropic doesn't have "none", use "auto"
                    ToolChoiceMode::Other(_) => AnthropicToolChoice::Auto, // Default to auto for unknown values
                }
            }
            ToolChoice::Specific { function, tool_type: _ } => {
                // For Anthropic, we need to use the function name directly
                // regardless of the tool_type from OpenAI format
                AnthropicToolChoice::Tool { name: function.name }
            }
        }
    }
}

impl From<ChatMessage> for AnthropicMessage {
    fn from(msg: ChatMessage) -> Self {
        // Handle tool role messages and assistant messages with tool calls
        let content = match msg.role {
            ChatRole::Tool => {
                // Tool role: create a tool_result block
                if let Some(tool_call_id) = msg.tool_call_id {
                    AnthropicMessageContent::Blocks(vec![AnthropicContentBlock::ToolResult {
                        tool_use_id: tool_call_id,
                        content: msg.content,
                        is_error: None,
                    }])
                } else {
                    // Fallback if tool_call_id is missing
                    AnthropicMessageContent::Text(msg.content.unwrap_or_default())
                }
            }
            ChatRole::Assistant if msg.tool_calls.is_some() => {
                // Assistant with tool calls: create content blocks
                let mut blocks = Vec::new();

                // Add text content if present
                if let Some(text) = msg.content
                    && !text.is_empty()
                {
                    blocks.push(AnthropicContentBlock::Text { text });
                }

                // Add tool use blocks
                if let Some(tool_calls) = msg.tool_calls {
                    for tool_call in tool_calls {
                        // Parse the arguments from JSON string to Value
                        let input = serde_json::from_str(&tool_call.function.arguments)
                            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

                        blocks.push(AnthropicContentBlock::ToolUse {
                            id: tool_call.id,
                            name: tool_call.function.name,
                            input,
                        });
                    }
                }

                AnthropicMessageContent::Blocks(blocks)
            }
            _ => {
                // Regular text message
                AnthropicMessageContent::Text(msg.content.unwrap_or_default())
            }
        };

        Self {
            role: match msg.role {
                ChatRole::Tool => ChatRole::User, // Anthropic requires tool results to be from "user"
                role => role,
            },
            content,
        }
    }
}

impl From<ChatCompletionRequest> for AnthropicRequest {
    fn from(request: ChatCompletionRequest) -> Self {
        let ChatCompletionRequest {
            model,
            messages,
            temperature,
            max_tokens,
            top_p,
            frequency_penalty: _, // Not supported by Anthropic
            presence_penalty: _,  // Not supported by Anthropic
            stop,
            stream: _, // Set later in streaming calls
            tools,
            tool_choice,
            parallel_tool_calls: _, // Anthropic doesn't have explicit parallel tool calls setting
        } = request;

        let mut system_message = None;
        let mut anthropic_messages = Vec::new();

        for msg in messages {
            match msg.role {
                ChatRole::System => {
                    system_message = msg.content;
                }
                ChatRole::Assistant | ChatRole::User | ChatRole::Tool => {
                    anthropic_messages.push(AnthropicMessage::from(msg));
                }
                ChatRole::Other(ref role) => {
                    log::warn!("Unknown chat role from request: {role}, treating as user");
                    anthropic_messages.push(AnthropicMessage {
                        role: ChatRole::User,
                        content: AnthropicMessageContent::Text(msg.content.unwrap_or_default()),
                    });
                }
            }
        }

        // Convert tools if present
        let anthropic_tools = tools.map(|tools| tools.into_iter().map(AnthropicTool::from).collect());

        // Convert tool choice if present
        let anthropic_tool_choice = tool_choice.map(AnthropicToolChoice::from);

        AnthropicRequest {
            model,
            messages: anthropic_messages,
            system: system_message,
            max_tokens: max_tokens.unwrap_or(4096),
            temperature,
            top_p,
            top_k: None,
            stop_sequences: stop,
            stream: None,
            tools: anthropic_tools,
            tool_choice: anthropic_tool_choice,
        }
    }
}
