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
use serde_json::Value as SerdeValue;
use sonic_rs::JsonValueTrait;
use std::collections::HashMap;

use crate::{
    error::LlmError,
    messages::{
        openai::{
            ChatCompletionRequest, ChatMessage, ChatRole, FunctionCall as OpenAIFunctionCall,
            FunctionDefinition as OpenAIFunctionDefinition, Tool as OpenAITool, ToolCall,
            ToolChoice as OpenAIToolChoice, ToolChoiceMode,
        },
        unified,
    },
};

use super::output::document_to_string;

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
        let ChatMessage {
            role: chat_role,
            content,
            tool_calls,
            tool_call_id,
        } = msg;

        let (role, content_blocks) = message_parts_to_role_and_blocks(chat_role, content, tool_calls, tool_call_id);

        BedrockMessage::builder()
            .role(role)
            .set_content(Some(content_blocks))
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
        let ChatMessage {
            role: chat_role,
            content,
            tool_calls,
            tool_call_id,
        } = msg;

        if let ChatRole::System = chat_role {
            system_messages.push(SystemContentBlock::Text(content.unwrap_or_default()));
            continue;
        }

        let (role, new_blocks) = message_parts_to_role_and_blocks(chat_role, content, tool_calls, tool_call_id);

        if current_role.as_ref().is_some_and(|prev| *prev != role)
            && !current_content.is_empty()
            && let Some(prev_role) = current_role.take()
            && let Ok(message) = BedrockMessage::builder()
                .role(prev_role)
                .set_content(Some(std::mem::take(&mut current_content)))
                .build()
        {
            conversation_messages.push(message);
        }

        if current_content.is_empty() {
            current_content = new_blocks;
        } else {
            current_content.extend(new_blocks);
        }

        current_role = Some(role);
    }

    if let Some(role) = current_role
        && !current_content.is_empty()
        && let Ok(message) = BedrockMessage::builder()
            .role(role)
            .set_content(Some(current_content))
            .build()
    {
        conversation_messages.push(message);
    }

    let system = if system_messages.is_empty() {
        None
    } else {
        Some(system_messages)
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
            let OpenAITool { tool_type: _, function } = tool;
            let OpenAIFunctionDefinition {
                name,
                description,
                parameters,
            } = function;

            let params_value = serde_json::to_value(parameters).unwrap_or(SerdeValue::Null);
            let params_doc = serde_value_to_document(params_value);
            let input_schema = ToolInputSchema::Json(params_doc);

            let tool_spec = ToolSpecification::builder()
                .name(name)
                .description(description)
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

/// Convert sonic_rs::Value to aws_smithy_types::Document
pub fn json_value_to_document(value: sonic_rs::Value) -> aws_smithy_types::Document {
    use sonic_rs::{JsonContainerTrait, JsonNumberTrait, JsonValueTrait};

    if value.is_null() {
        aws_smithy_types::Document::Null
    } else if let Some(b) = value.as_bool() {
        aws_smithy_types::Document::Bool(b)
    } else if let Some(n) = value.as_number() {
        if let Some(i) = n.as_i64() {
            aws_smithy_types::Document::Number(aws_smithy_types::Number::NegInt(i))
        } else if let Some(u) = n.as_u64() {
            aws_smithy_types::Document::Number(aws_smithy_types::Number::PosInt(u))
        } else if let Some(f) = n.as_f64() {
            aws_smithy_types::Document::Number(aws_smithy_types::Number::Float(f))
        } else {
            aws_smithy_types::Document::Null
        }
    } else if let Some(s) = value.as_str() {
        aws_smithy_types::Document::String(s.to_string())
    } else if let Some(arr) = value.as_array() {
        aws_smithy_types::Document::Array(arr.iter().map(|v| json_value_to_document(v.clone())).collect())
    } else if let Some(obj) = value.as_object() {
        aws_smithy_types::Document::Object(
            obj.iter()
                .map(|(k, v)| (k.to_string(), json_value_to_document(v.clone())))
                .collect(),
        )
    } else {
        aws_smithy_types::Document::Null
    }
}

fn serde_value_to_document(value: SerdeValue) -> aws_smithy_types::Document {
    use aws_smithy_types::Document as Doc;

    match value {
        SerdeValue::Null => Doc::Null,
        SerdeValue::Bool(b) => Doc::Bool(b),
        SerdeValue::Number(num) => {
            if let Some(i) = num.as_i64() {
                Doc::Number(aws_smithy_types::Number::NegInt(i))
            } else if let Some(u) = num.as_u64() {
                Doc::Number(aws_smithy_types::Number::PosInt(u))
            } else if let Some(f) = num.as_f64() {
                Doc::Number(aws_smithy_types::Number::Float(f))
            } else {
                Doc::Null
            }
        }
        SerdeValue::String(s) => Doc::String(s),
        SerdeValue::Array(items) => Doc::Array(items.into_iter().map(serde_value_to_document).collect()),
        SerdeValue::Object(map) => Doc::Object(map.into_iter().map(|(k, v)| (k, serde_value_to_document(v))).collect()),
    }
}

/// Ensure tool input document matches Bedrock expectations (string or object).
fn normalize_tool_input_document(doc: aws_smithy_types::Document) -> aws_smithy_types::Document {
    use aws_smithy_types::Document as Doc;

    match doc {
        Doc::Object(_) => doc,
        Doc::String(s) => {
            if let Ok(parsed) = sonic_rs::from_str::<sonic_rs::Value>(&s)
                && parsed.is_object()
            {
                return json_value_to_document(parsed);
            }

            Doc::String(s)
        }
        Doc::Null => Doc::Object(HashMap::new()),
        Doc::Bool(b) => Doc::String(b.to_string()),
        Doc::Number(n) => {
            let rendered = match n {
                aws_smithy_types::Number::PosInt(u) => u.to_string(),
                aws_smithy_types::Number::NegInt(i) => i.to_string(),
                aws_smithy_types::Number::Float(f) => {
                    if let Some(num) = serde_json::Number::from_f64(f) {
                        num.to_string()
                    } else {
                        "0".to_string()
                    }
                }
            };
            Doc::String(rendered)
        }
        Doc::Array(items) => {
            let json_items: Vec<String> = items.into_iter().map(|item| document_to_string(&item)).collect();
            Doc::String(format!("[{}]", json_items.join(",")))
        }
    }
}

fn document_kind(doc: &aws_smithy_types::Document) -> &'static str {
    use aws_smithy_types::Document as Doc;

    match doc {
        Doc::Object(_) => "object",
        Doc::String(_) => "string",
        Doc::Array(_) => "array",
        Doc::Bool(_) => "bool",
        Doc::Number(_) => "number",
        Doc::Null => "null",
    }
}

fn document_preview(doc: &aws_smithy_types::Document) -> String {
    const MAX_LEN: usize = 200;
    let mut rendered = document_to_string(doc);
    if rendered.len() > MAX_LEN {
        rendered.truncate(MAX_LEN);
        rendered.push('…');
    }
    rendered
}

fn message_parts_to_role_and_blocks(
    chat_role: ChatRole,
    mut content: Option<String>,
    tool_calls: Option<Vec<ToolCall>>,
    tool_call_id: Option<String>,
) -> (ConversationRole, Vec<ContentBlock>) {
    let role = match chat_role {
        ChatRole::User => ConversationRole::User,
        ChatRole::Assistant => ConversationRole::Assistant,
        ChatRole::Tool => ConversationRole::User,
        ChatRole::System => ConversationRole::User,
        ChatRole::Other(_) => ConversationRole::User,
    };

    let tool_call_capacity = tool_calls.as_ref().map_or(0, Vec::len);
    let mut content_blocks = Vec::with_capacity(tool_call_capacity + 1);

    if let Some(tool_call_id) = tool_call_id {
        let tool_content = content.take().unwrap_or_default();

        if let Ok(tool_result) = ToolResultBlock::builder()
            .tool_use_id(tool_call_id)
            .content(ToolResultContentBlock::Text(tool_content))
            .build()
        {
            content_blocks.push(ContentBlock::ToolResult(tool_result));
        }
    } else {
        if let Some(text_content) = content.take()
            && !text_content.is_empty()
        {
            content_blocks.push(ContentBlock::Text(text_content));
        }

        if let Some(tool_calls) = tool_calls {
            if !tool_calls.is_empty() {
                content_blocks.reserve(tool_calls.len());
            }
            append_tool_calls(&mut content_blocks, tool_calls);
        }
    }

    (role, content_blocks)
}

fn append_tool_calls(content_blocks: &mut Vec<ContentBlock>, tool_calls: Vec<ToolCall>) {
    content_blocks.extend(tool_calls.into_iter().filter_map(tool_call_to_content_block));
}

fn tool_call_to_content_block(tool_call: ToolCall) -> Option<ContentBlock> {
    let ToolCall {
        id,
        tool_type: _,
        function,
    } = tool_call;

    let OpenAIFunctionCall { name, arguments } = function;

    let (args_doc, parse_error) = match sonic_rs::from_str::<sonic_rs::Value>(&arguments) {
        Ok(value) => (normalize_tool_input_document(json_value_to_document(value)), None),
        Err(err) => (
            normalize_tool_input_document(aws_smithy_types::Document::String(arguments.clone())),
            Some(err),
        ),
    };

    if let Some(err) = parse_error {
        log::debug!(
            "Bedrock tool_use arguments fallback to string: id={} name={} error={err} raw={raw}",
            id,
            name,
            raw = arguments
        );
    }

    log::debug!(
        "Bedrock tool_use arguments normalized: id={} name={} kind={} preview={}",
        id,
        name,
        document_kind(&args_doc),
        document_preview(&args_doc)
    );

    ToolUseBlock::builder()
        .tool_use_id(id)
        .name(name)
        .input(args_doc)
        .build()
        .map(ContentBlock::ToolUse)
        .ok()
}

impl From<unified::UnifiedRequest> for ConverseInput {
    fn from(request: unified::UnifiedRequest) -> Self {
        // Convert unified to OpenAI first, then use existing conversion
        let openai_request = ChatCompletionRequest::from(request);
        Self::from(openai_request)
    }
}

impl From<unified::UnifiedRequest> for ConverseStreamInput {
    fn from(request: unified::UnifiedRequest) -> Self {
        // Convert unified to OpenAI first, then use existing conversion
        let openai_request = ChatCompletionRequest::from(request);
        Self::from(openai_request)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic)]

    use super::*;
    use crate::messages::openai::ToolCallType;
    use aws_smithy_types::Document;

    fn build_tool_call(arguments: &str) -> ToolCall {
        ToolCall {
            id: "tool-1".to_string(),
            tool_type: ToolCallType::Function,
            function: OpenAIFunctionCall {
                name: "Bash".to_string(),
                arguments: arguments.to_string(),
            },
        }
    }

    #[test]
    fn tool_use_arguments_parse_as_object() {
        let block = tool_call_to_content_block(build_tool_call(r#"{"command":"ls"}"#)).expect("content block");

        let ContentBlock::ToolUse(tool_use) = block else {
            panic!("expected tool use block");
        };

        assert!(matches!(tool_use.input(), Document::Object(_)));
    }

    #[test]
    fn tool_use_arguments_fall_back_to_string() {
        let raw = r#"{"command": "echo "hello""}"#;
        let block = tool_call_to_content_block(build_tool_call(raw)).expect("content block");

        let ContentBlock::ToolUse(tool_use) = block else {
            panic!("expected tool use block");
        };

        assert!(matches!(tool_use.input(), Document::String(s) if s == raw));
    }
}
