//! Direct input conversions for AWS Bedrock Converse API.
//!
//! This module handles direct transformation from ChatCompletionRequest
//! to AWS Bedrock's Converse API types with no intermediate types.

use aws_sdk_bedrockruntime::{
    operation::{converse::ConverseInput, converse_stream::ConverseStreamInput},
    types::{
        AnyToolChoice, AutoToolChoice, ContentBlock, ConversationRole, InferenceConfiguration,
        Message as BedrockMessage, SpecificToolChoice, SystemContentBlock, Tool, ToolChoice, ToolConfiguration,
        ToolInputSchema, ToolResultBlock, ToolResultContentBlock, ToolSpecification, ToolUseBlock,
    },
};

use crate::{
    error::LlmError,
    messages::openai::{
        ChatCompletionRequest, ChatMessage, ChatRole, Tool as OpenAITool, ToolChoice as OpenAIToolChoice,
        ToolChoiceMode,
    },
};

/// Direct conversion from ChatCompletionRequest to ConverseInput.
impl From<ChatCompletionRequest> for ConverseInput {
    fn from(request: ChatCompletionRequest) -> Self {
        let ChatCompletionRequest {
            model,
            messages,
            temperature,
            max_tokens,
            top_p,
            frequency_penalty: _, // Not supported by Bedrock
            presence_penalty: _,  // Not supported by Bedrock
            stop,
            stream: _, // Not used for ConverseInput
            tools,
            tool_choice,
            parallel_tool_calls: _, // Not supported by Bedrock
        } = request;

        // Convert inference parameters
        let inference_config = build_inference_config(temperature, max_tokens, top_p, stop);

        // Convert tools if present
        let tool_config = tools.and_then(|tools| {
            if tools.is_empty() {
                None
            } else {
                convert_tools(tools, tool_choice, &model).ok()
            }
        });

        // Convert messages (moves messages)
        let (system, bedrock_messages) = convert_messages(messages);

        ConverseInput::builder()
            .model_id(model)
            .set_messages(Some(bedrock_messages))
            .set_system(system)
            .set_inference_config(inference_config)
            .set_tool_config(tool_config)
            .build()
            .expect("ConverseInput should build successfully with valid inputs")
    }
}

/// Direct conversion from ChatCompletionRequest to ConverseStreamInput.
impl From<ChatCompletionRequest> for ConverseStreamInput {
    fn from(request: ChatCompletionRequest) -> Self {
        let ChatCompletionRequest {
            model,
            messages,
            temperature,
            max_tokens,
            top_p,
            frequency_penalty: _, // Not supported by Bedrock
            presence_penalty: _,  // Not supported by Bedrock
            stop,
            stream: _, // Always streaming for this type
            tools,
            tool_choice,
            parallel_tool_calls: _, // Not supported by Bedrock
        } = request;

        // Convert inference parameters
        let inference_config = build_inference_config(temperature, max_tokens, top_p, stop);

        // Convert tools if present
        let tool_config = tools.and_then(|tools| {
            if tools.is_empty() {
                None
            } else {
                convert_tools(tools, tool_choice, &model).ok()
            }
        });

        // Convert messages (moves messages)
        let (system, bedrock_messages) = convert_messages(messages);

        ConverseStreamInput::builder()
            .model_id(model)
            .set_messages(Some(bedrock_messages))
            .set_system(system)
            .set_inference_config(inference_config)
            .set_tool_config(tool_config)
            .build()
            .expect("ConverseStreamInput should build successfully with valid inputs")
    }
}

/// Convert a single ChatMessage to BedrockMessage.
impl From<ChatMessage> for BedrockMessage {
    fn from(msg: ChatMessage) -> Self {
        let role = match msg.role {
            ChatRole::User => ConversationRole::User,
            ChatRole::Assistant => ConversationRole::Assistant,
            ChatRole::Tool => ConversationRole::User, // Tool responses are handled as user messages
            ChatRole::System => ConversationRole::User, // Should not happen here
            ChatRole::Other(_) => ConversationRole::User, // Treat unknown roles as user
        };

        let mut content = Vec::new();

        // Handle tool calls from assistant
        if role == ConversationRole::Assistant
            && let Some(tool_calls) = msg.tool_calls
        {
            for tool_call in tool_calls {
                // Parse arguments as JSON Value first, then convert to Document
                let args_value: serde_json::Value = sonic_rs::from_str(&tool_call.function.arguments)
                    .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new()));
                let args_doc = json_value_to_document(args_value);

                if let Ok(tool_use) = ToolUseBlock::builder()
                    .tool_use_id(tool_call.id)
                    .name(tool_call.function.name)
                    .input(args_doc)
                    .build()
                {
                    content.push(ContentBlock::ToolUse(tool_use));
                }
            }
        }

        // Handle tool responses
        if let Some(tool_call_id) = msg.tool_call_id {
            if let Ok(tool_result) = ToolResultBlock::builder()
                .tool_use_id(tool_call_id)
                .content(ToolResultContentBlock::Text(msg.content.unwrap_or_default()))
                .build()
            {
                content.push(ContentBlock::ToolResult(tool_result));
            }
        } else if let Some(text_content) = msg.content {
            // Regular text content - only add if not empty
            if !text_content.is_empty() {
                content.push(ContentBlock::Text(text_content));
            }
        }

        BedrockMessage::builder()
            .role(role)
            .set_content(Some(content))
            .build()
            .expect("BedrockMessage should build successfully with valid inputs")
    }
}

/// Convert OpenAI messages to Bedrock Converse format.
///
/// This function handles message grouping - consecutive messages with the same role
/// are batched together into a single BedrockMessage with multiple content blocks.
fn convert_messages(messages: Vec<ChatMessage>) -> (Option<Vec<SystemContentBlock>>, Vec<BedrockMessage>) {
    let mut system_messages = Vec::new();
    let mut conversation_messages = Vec::new();
    let mut current_role: Option<ConversationRole> = None;
    let mut current_content = Vec::new();

    for msg in messages {
        // Handle system messages separately
        if matches!(msg.role, ChatRole::System) {
            system_messages.push(SystemContentBlock::Text(msg.content.unwrap_or_default()));
            continue;
        }

        // Convert message to get role and content
        let bedrock_msg: BedrockMessage = msg.into();
        let role = bedrock_msg.role();
        let content = bedrock_msg.content();

        // If role changes, save the accumulated content before processing new message
        if let Some(prev_role) = current_role
            && prev_role != *role
            && !current_content.is_empty()
        {
            if let Ok(message) = BedrockMessage::builder()
                .role(prev_role)
                .set_content(Some(current_content.clone()))
                .build()
            {
                conversation_messages.push(message);
            }
            current_content.clear();
        }

        // Add content from the converted message
        current_content.extend_from_slice(content);
        current_role = Some(role.clone());
    }

    // Add the last message if there's content
    if let Some(role) = current_role
        && !current_content.is_empty()
        && let Ok(message) = BedrockMessage::builder()
            .role(role)
            .set_content(Some(current_content))
            .build()
    {
        conversation_messages.push(message);
    }

    let system = if !system_messages.is_empty() {
        Some(system_messages)
    } else {
        None
    };

    (system, conversation_messages)
}

/// Build inference configuration from individual parameters.
fn build_inference_config(
    temperature: Option<f32>,
    max_tokens: Option<u32>,
    top_p: Option<f32>,
    stop: Option<Vec<String>>,
) -> Option<InferenceConfiguration> {
    let mut builder = InferenceConfiguration::builder();
    let mut has_config = false;

    if let Some(max_tokens) = max_tokens {
        builder = builder.max_tokens(max_tokens as i32);
        has_config = true;
    }

    if let Some(temperature) = temperature {
        builder = builder.temperature(temperature);
        has_config = true;
    }

    if let Some(top_p) = top_p {
        builder = builder.top_p(top_p);
        has_config = true;
    }

    if let Some(stop) = stop {
        builder = builder.set_stop_sequences(Some(stop));
        has_config = true;
    }

    if has_config { Some(builder.build()) } else { None }
}

/// Convert OpenAI tools to Bedrock format.
fn convert_tools(
    tools: Vec<OpenAITool>,
    tool_choice: Option<OpenAIToolChoice>,
    model_id: &str,
) -> crate::Result<ToolConfiguration> {
    let bedrock_tools: Result<Vec<Tool>, LlmError> = tools
        .into_iter()
        .map(|tool| {
            // Convert parameters to AWS Document format (moves tool.function.parameters)
            let params_doc = json_value_to_document(tool.function.parameters);
            let input_schema = ToolInputSchema::Json(params_doc);

            let tool_spec = ToolSpecification::builder()
                .name(tool.function.name) // moves
                .description(tool.function.description) // moves
                .input_schema(input_schema)
                .build()
                .map_err(|e| LlmError::InvalidRequest(format!("Failed to build tool specification: {e}")))?;

            Ok(Tool::ToolSpec(tool_spec))
        })
        .collect();

    let bedrock_tools = bedrock_tools?;

    let mut config_builder = ToolConfiguration::builder().set_tools(Some(bedrock_tools));

    // Add tool choice if specified
    if let Some(choice) = tool_choice.and_then(|tc| {
        let family = ModelFamily::from_model_id(model_id);
        family.convert_tool_choice(tc)
    }) {
        config_builder = config_builder.tool_choice(choice);
    }

    config_builder
        .build()
        .map_err(|e| LlmError::InvalidRequest(format!("Failed to build tool configuration: {e}")))
}

/// Model family capabilities for Bedrock Converse API.
#[derive(Debug)]
enum ModelFamily {
    Anthropic,
    AmazonNova,
    AmazonTitan,
    Cohere,
    MetaLlama,
    DeepSeek,
    Jamba,
    Unknown,
}

impl ModelFamily {
    /// Create a ModelFamily from a model ID.
    fn from_model_id(model_id: &str) -> Self {
        if model_id.starts_with("anthropic.") {
            ModelFamily::Anthropic
        } else if model_id.starts_with("amazon.nova") {
            ModelFamily::AmazonNova
        } else if model_id.starts_with("amazon.titan") {
            ModelFamily::AmazonTitan
        } else if model_id.starts_with("cohere.") {
            ModelFamily::Cohere
        } else if model_id.starts_with("meta.") || model_id.starts_with("us.meta.") {
            ModelFamily::MetaLlama
        } else if model_id.starts_with("us.deepseek.") {
            ModelFamily::DeepSeek
        } else if model_id.starts_with("ai21.jamba") {
            ModelFamily::Jamba
        } else {
            ModelFamily::Unknown
        }
    }

    /// Whether this family supports "any" tool choice (force tool use).
    fn supports_tool_choice_any(&self) -> bool {
        match self {
            // These families support forcing tool use
            ModelFamily::Anthropic => true,
            ModelFamily::AmazonNova => true,
            ModelFamily::MetaLlama => true,
            ModelFamily::DeepSeek => true,
            ModelFamily::Jamba => true,

            // These families don't support "any" tool choice
            ModelFamily::Cohere => false,
            ModelFamily::AmazonTitan => false,
            ModelFamily::Unknown => false,
        }
    }

    /// Whether this family supports specific tool choice (call a specific tool).
    fn supports_tool_choice_specific(&self) -> bool {
        match self {
            // Most families support specific tool choice
            ModelFamily::Anthropic => true,
            ModelFamily::AmazonNova => true,
            ModelFamily::Cohere => true,
            ModelFamily::MetaLlama => true,
            ModelFamily::DeepSeek => true,
            ModelFamily::Jamba => true,

            // Titan might not support it
            ModelFamily::AmazonTitan => false,
            ModelFamily::Unknown => false,
        }
    }

    /// Convert OpenAI tool choice to Bedrock format based on model family capabilities.
    fn convert_tool_choice(&self, tool_choice: OpenAIToolChoice) -> Option<ToolChoice> {
        match tool_choice {
            OpenAIToolChoice::Mode(mode) => match mode {
                ToolChoiceMode::None => None,
                ToolChoiceMode::Auto => Some(ToolChoice::Auto(AutoToolChoice::builder().build())),
                ToolChoiceMode::Required | ToolChoiceMode::Any => {
                    // Some families don't support "any" tool choice, fall back to "auto"
                    if self.supports_tool_choice_any() {
                        Some(ToolChoice::Any(AnyToolChoice::builder().build()))
                    } else {
                        // Fall back to auto for families that don't support "any"
                        Some(ToolChoice::Auto(AutoToolChoice::builder().build()))
                    }
                }
                ToolChoiceMode::Other(_) => None,
            },
            OpenAIToolChoice::Specific { function, .. } => {
                // Most families support specific tool choice
                if self.supports_tool_choice_specific() {
                    SpecificToolChoice::builder()
                        .name(function.name)
                        .build()
                        .ok()
                        .map(ToolChoice::Tool)
                } else {
                    // Fall back to auto if specific choice not supported
                    Some(ToolChoice::Auto(AutoToolChoice::builder().build()))
                }
            }
        }
    }
}

/// Convert serde_json::Value to aws_smithy_types::Document
pub fn json_value_to_document(value: serde_json::Value) -> aws_smithy_types::Document {
    match value {
        serde_json::Value::Null => aws_smithy_types::Document::Null,
        serde_json::Value::Bool(b) => aws_smithy_types::Document::Bool(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                aws_smithy_types::Document::Number(aws_smithy_types::Number::NegInt(i))
            } else if let Some(u) = n.as_u64() {
                aws_smithy_types::Document::Number(aws_smithy_types::Number::PosInt(u))
            } else if let Some(f) = n.as_f64() {
                aws_smithy_types::Document::Number(aws_smithy_types::Number::Float(f))
            } else {
                aws_smithy_types::Document::Null
            }
        }
        serde_json::Value::String(s) => aws_smithy_types::Document::String(s),
        serde_json::Value::Array(arr) => {
            aws_smithy_types::Document::Array(arr.into_iter().map(json_value_to_document).collect())
        }
        serde_json::Value::Object(obj) => {
            aws_smithy_types::Document::Object(obj.into_iter().map(|(k, v)| (k, json_value_to_document(v))).collect())
        }
    }
}
