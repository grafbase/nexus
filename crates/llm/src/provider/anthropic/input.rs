use serde::Serialize;
use serde_json::Value;

use crate::messages::{anthropic::AnthropicMetadata, openai::JsonSchema, unified};

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

    /// Custom metadata to attach to the request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<AnthropicMetadata>,

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
    pub role: AnthropicRole,

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

impl AnthropicMessageContent {
    pub fn into_block_vec(self) -> Vec<AnthropicContentBlock> {
        match self {
            AnthropicMessageContent::Blocks(blocks) => blocks,
            AnthropicMessageContent::Text(text) => {
                if text.is_empty() {
                    Vec::new()
                } else {
                    vec![AnthropicContentBlock::Text { text }]
                }
            }
        }
    }
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
    ToolUse { id: String, name: String, input: Value },

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
    pub input_schema: Box<JsonSchema>,
}

/// Anthropic message role.
#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AnthropicRole {
    User,
    Assistant,
}

impl From<unified::UnifiedTool> for AnthropicTool {
    fn from(tool: unified::UnifiedTool) -> Self {
        Self {
            name: tool.function.name,
            description: tool.function.description,
            input_schema: tool.function.parameters, // Already Box<JsonSchema>
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

impl From<unified::UnifiedToolChoice> for AnthropicToolChoice {
    fn from(choice: unified::UnifiedToolChoice) -> Self {
        match choice {
            unified::UnifiedToolChoice::Mode(mode) => match mode {
                unified::UnifiedToolChoiceMode::None => AnthropicToolChoice::Auto, // Anthropic doesn't have "none"
                unified::UnifiedToolChoiceMode::Auto => AnthropicToolChoice::Auto,
                unified::UnifiedToolChoiceMode::Required => AnthropicToolChoice::Any,
            },
            unified::UnifiedToolChoice::Specific { function } => AnthropicToolChoice::Tool { name: function.name },
        }
    }
}

impl From<unified::UnifiedRole> for AnthropicRole {
    fn from(role: unified::UnifiedRole) -> Self {
        match role {
            unified::UnifiedRole::User => AnthropicRole::User,
            unified::UnifiedRole::Assistant => AnthropicRole::Assistant,
            // Anthropic flattens System/Tool roles into user messages
            unified::UnifiedRole::System | unified::UnifiedRole::Tool => AnthropicRole::User,
        }
    }
}

impl From<unified::UnifiedContentContainer> for AnthropicMessageContent {
    fn from(content: unified::UnifiedContentContainer) -> Self {
        match content {
            unified::UnifiedContentContainer::Text(text) => AnthropicMessageContent::Text(text),
            unified::UnifiedContentContainer::Blocks(blocks) => {
                let anthropic_blocks: Vec<AnthropicContentBlock> = blocks
                    .into_iter()
                    .map(|block| convert_content_block(block, None))
                    .collect();

                if anthropic_blocks.is_empty() {
                    AnthropicMessageContent::Text(String::new())
                } else {
                    AnthropicMessageContent::Blocks(anthropic_blocks)
                }
            }
        }
    }
}

impl From<unified::UnifiedMessage> for AnthropicMessage {
    fn from(msg: unified::UnifiedMessage) -> Self {
        let unified::UnifiedMessage {
            role: unified_role,
            content,
            tool_calls: _,
            tool_call_id,
        } = msg;

        // Anthropic flattens tool replies into user-role tool_result blocks.
        // Keep a single conversion path so request assembly matches the provider
        // expectations and test helpers.
        let content = match unified_role {
            unified::UnifiedRole::Tool => convert_tool_content(content, tool_call_id),
            _ => AnthropicMessageContent::from(content),
        };

        let role = AnthropicRole::from(unified_role);

        Self { role, content }
    }
}

fn convert_tool_content(
    content: unified::UnifiedContentContainer,
    tool_call_id: Option<String>,
) -> AnthropicMessageContent {
    match content {
        unified::UnifiedContentContainer::Text(text) => match tool_call_id {
            Some(id) => AnthropicMessageContent::Blocks(vec![AnthropicContentBlock::ToolResult {
                tool_use_id: id,
                content: if text.is_empty() { None } else { Some(text) },
                is_error: None,
            }]),
            None => AnthropicMessageContent::Text(text),
        },
        unified::UnifiedContentContainer::Blocks(blocks) => {
            // When the unified content does not materialize a tool_result block we
            // synthesize one so Anthropic sees the tool output directly after the
            // tool_use chunk that triggered it.

            // Quick scan to check if any explicit tool_result blocks exist
            let has_explicit_tool_results = blocks
                .iter()
                .any(|b| matches!(b, unified::UnifiedContent::ToolResult { .. }));

            // Convert blocks - only apply default_tool_id if NO explicit tool_results exist
            let mut anthropic_blocks = Vec::with_capacity(blocks.len());
            let mut has_tool_result = false;

            for block in blocks {
                let converted = convert_content_block(
                    block,
                    if has_explicit_tool_results {
                        None
                    } else {
                        tool_call_id.as_deref()
                    },
                );

                if matches!(converted, AnthropicContentBlock::ToolResult { .. }) {
                    has_tool_result = true;
                }

                anthropic_blocks.push(converted);
            }

            if let Some(ref id) = tool_call_id
                && !has_tool_result
            {
                anthropic_blocks.push(AnthropicContentBlock::ToolResult {
                    tool_use_id: id.clone(),
                    content: None,
                    is_error: None,
                });
            }

            if anthropic_blocks.is_empty() {
                // No blocks means the tool returned an empty payload. Anthropic still
                // demands a tool_result wrapper, so emit one with empty content.
                if let Some(id) = tool_call_id {
                    AnthropicMessageContent::Blocks(vec![AnthropicContentBlock::ToolResult {
                        tool_use_id: id,
                        content: None,
                        is_error: None,
                    }])
                } else {
                    AnthropicMessageContent::Text(String::new())
                }
            } else {
                AnthropicMessageContent::Blocks(anthropic_blocks)
            }
        }
    }
}

fn convert_content_block(block: unified::UnifiedContent, default_tool_id: Option<&str>) -> AnthropicContentBlock {
    match block {
        unified::UnifiedContent::Text { text } => match default_tool_id {
            // Text appearing inside a tool-role message should be surfaced as the
            // tool_result payload unless the block already declared its target id.
            Some(id) => AnthropicContentBlock::ToolResult {
                tool_use_id: id.to_string(),
                content: if text.is_empty() { None } else { Some(text) },
                is_error: None,
            },
            None => AnthropicContentBlock::Text { text },
        },
        unified::UnifiedContent::ToolUse { id, name, input } => AnthropicContentBlock::ToolUse { id, name, input },
        unified::UnifiedContent::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => AnthropicContentBlock::ToolResult {
            tool_use_id,
            content: content.into_text(),
            is_error,
        },
        unified::UnifiedContent::Image { .. } => AnthropicContentBlock::Text {
            text: "[Image content not supported]".to_string(),
        },
    }
}

fn merge_tool_result_content(target: &mut AnthropicMessage, source: AnthropicMessage) {
    // Tool messages arrive as separate Unified messages. Anthropic expects a single
    // user-role message containing all tool_result blocks, so fold consecutive tool
    // replies into one vector.
    let target_block = std::mem::replace(&mut target.content, AnthropicMessageContent::Blocks(Vec::new()));

    let mut target_blocks = target_block.into_block_vec();
    let mut source_blocks = source.content.into_block_vec();

    target_blocks.append(&mut source_blocks);
    target.content = AnthropicMessageContent::Blocks(target_blocks);
}

impl From<unified::UnifiedRequest> for AnthropicRequest {
    fn from(request: unified::UnifiedRequest) -> Self {
        let unified::UnifiedRequest {
            model,
            messages,
            system,
            temperature,
            max_tokens,
            top_p,
            top_k,
            frequency_penalty: _, // Not supported by Anthropic
            presence_penalty: _,  // Not supported by Anthropic
            stop_sequences,
            stream: _, // Set later in streaming calls
            tools,
            tool_choice,
            parallel_tool_calls: _, // Anthropic doesn't have explicit parallel tool calls setting
            metadata,               // Anthropic doesn't use metadata in this conversion
        } = request;

        // System message is already separated in unified format
        let system_message = system;

        // Convert messages from unified format while merging consecutive tool results
        let mut anthropic_messages: Vec<AnthropicMessage> = Vec::with_capacity(messages.len());
        let mut last_tool_result_index: Option<usize> = None;

        for message in messages {
            let is_tool_message = matches!(message.role, unified::UnifiedRole::Tool);
            let converted = AnthropicMessage::from(message);

            if is_tool_message {
                if let Some(idx) = last_tool_result_index
                    && let Some(existing) = anthropic_messages.get_mut(idx)
                {
                    merge_tool_result_content(existing, converted);
                    continue;
                }

                last_tool_result_index = Some(anthropic_messages.len());
                anthropic_messages.push(converted);
            } else {
                last_tool_result_index = None;
                anthropic_messages.push(converted);
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
            top_k,
            stop_sequences,
            stream: None,
            tools: anthropic_tools,
            tool_choice: anthropic_tool_choice,
            metadata: metadata.map(|metadata| AnthropicMetadata {
                user_id: metadata.user_id,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::unified::{self, UnifiedContent, UnifiedContentContainer, UnifiedMessage, UnifiedRole};
    use insta::assert_json_snapshot;
    use serde_json::json;

    #[test]
    fn converts_tool_role_message_into_tool_result_block() {
        let messages = vec![
            UnifiedMessage {
                role: UnifiedRole::User,
                content: UnifiedContentContainer::Text("List files in crates/config".to_string()),
                tool_calls: None,
                tool_call_id: None,
            },
            UnifiedMessage {
                role: UnifiedRole::Assistant,
                content: UnifiedContentContainer::Blocks(vec![
                    UnifiedContent::Text {
                        text: "Let me inspect the repository.".to_string(),
                    },
                    UnifiedContent::ToolUse {
                        id: "toolu_list_files".to_string(),
                        name: "list_files".to_string(),
                        input: json!({ "path": "crates/config" }),
                    },
                ]),
                tool_calls: None,
                tool_call_id: None,
            },
            UnifiedMessage {
                role: UnifiedRole::Tool,
                content: UnifiedContentContainer::Text("[\"Cargo.toml\", \"src/lib.rs\"]".to_string()),
                tool_calls: None,
                tool_call_id: Some("toolu_list_files".to_string()),
            },
        ];

        let request = unified::UnifiedRequest {
            model: "anthropic/claude-3-5-haiku-latest".to_string(),
            messages,
            system: None,
            max_tokens: Some(200),
            temperature: None,
            top_p: None,
            top_k: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop_sequences: None,
            stream: None,
            tools: None,
            tool_choice: None,
            parallel_tool_calls: None,
            metadata: None,
        };

        let anthropic_request = AnthropicRequest::from(request);
        let value = serde_json::to_value(&anthropic_request).unwrap();

        assert_json_snapshot!(value, @r###"
        {
          "model": "anthropic/claude-3-5-haiku-latest",
          "messages": [
            {
              "role": "user",
              "content": "List files in crates/config"
            },
            {
              "role": "assistant",
              "content": [
                {
                  "type": "text",
                  "text": "Let me inspect the repository."
                },
                {
                  "type": "tool_use",
                  "id": "toolu_list_files",
                  "name": "list_files",
                  "input": {
                    "path": "crates/config"
                  }
                }
              ]
            },
            {
              "role": "user",
              "content": [
                {
                  "type": "tool_result",
                  "tool_use_id": "toolu_list_files",
                  "content": "[\"Cargo.toml\", \"src/lib.rs\"]"
                }
              ]
            }
          ],
          "max_tokens": 200
        }
        "###);
    }

    #[test]
    fn converts_multiple_tool_messages_into_results() {
        let messages = vec![
            UnifiedMessage {
                role: UnifiedRole::User,
                content: UnifiedContentContainer::Text("Help me with file operations".to_string()),
                tool_calls: None,
                tool_call_id: None,
            },
            UnifiedMessage {
                role: UnifiedRole::Assistant,
                content: UnifiedContentContainer::Blocks(vec![
                    UnifiedContent::Text {
                        text: "I'll help you with file operations.".to_string(),
                    },
                    UnifiedContent::ToolUse {
                        id: "call_file_read_001".to_string(),
                        name: "Read".to_string(),
                        input: json!({ "file_path": "./config.toml" }),
                    },
                    UnifiedContent::ToolUse {
                        id: "call_file_write_002".to_string(),
                        name: "Write".to_string(),
                        input: json!({
                            "file_path": "./output.txt",
                            "content": "Hello World"
                        }),
                    },
                ]),
                tool_calls: None,
                tool_call_id: None,
            },
            UnifiedMessage {
                role: UnifiedRole::Tool,
                content: UnifiedContentContainer::Text("# Configuration\nport = 8080".to_string()),
                tool_calls: None,
                tool_call_id: Some("call_file_read_001".to_string()),
            },
            UnifiedMessage {
                role: UnifiedRole::Tool,
                content: UnifiedContentContainer::Text("File written successfully".to_string()),
                tool_calls: None,
                tool_call_id: Some("call_file_write_002".to_string()),
            },
            UnifiedMessage {
                role: UnifiedRole::User,
                content: UnifiedContentContainer::Text("Great!".to_string()),
                tool_calls: None,
                tool_call_id: None,
            },
        ];

        let request = unified::UnifiedRequest {
            model: "anthropic/claude-3-5-sonnet-20241022".to_string(),
            messages,
            system: None,
            max_tokens: Some(512),
            temperature: None,
            top_p: None,
            top_k: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop_sequences: None,
            stream: None,
            tools: None,
            tool_choice: None,
            parallel_tool_calls: None,
            metadata: None,
        };

        let anthropic_request = AnthropicRequest::from(request);

        let merged_message = anthropic_request
            .messages
            .iter()
            .find(|message| {
                matches!(&message.content, AnthropicMessageContent::Blocks(blocks)
                if blocks.iter().any(|block| matches!(block,
                    AnthropicContentBlock::ToolResult { .. }
                )))
            })
            .expect("expected merged tool result message");

        let blocks = match &merged_message.content {
            AnthropicMessageContent::Blocks(blocks) => blocks,
            _ => return,
        };

        assert!(blocks.iter().any(|block| matches!(
            block,
            AnthropicContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == "call_file_write_002"
        )));
    }
}
