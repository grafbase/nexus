//! Direct input conversions for AWS Bedrock Converse API.
//!
//! This module handles direct transformation from unified chat requests
//! to AWS Bedrock's Converse API types with no intermediate formats.

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

use crate::{error::LlmError, messages::unified};

use super::output::document_to_string;

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

/// Convert unified tools to Bedrock format.
fn convert_tools(
    tools: Vec<unified::UnifiedTool>,
    tool_choice: Option<unified::UnifiedToolChoice>,
    model_id: &str,
) -> crate::Result<ToolConfiguration> {
    let bedrock_tools: Result<Vec<Tool>, LlmError> = tools
        .into_iter()
        .map(|tool| {
            let unified::UnifiedFunction {
                name,
                description,
                parameters,
                strict: _,
            } = tool.function;

            let params_value = serde_json::to_value(*parameters).unwrap_or(SerdeValue::Null);
            let params_doc = serde_value_to_document(params_value);
            let input_schema = ToolInputSchema::Json(params_doc);

            let mut builder = ToolSpecification::builder().name(name).input_schema(input_schema);

            if !description.is_empty() {
                builder = builder.description(description);
            }

            let tool_spec = builder
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
    fn convert_tool_choice(&self, tool_choice: unified::UnifiedToolChoice) -> Option<ToolChoice> {
        match tool_choice {
            unified::UnifiedToolChoice::Mode(mode) => match mode {
                unified::UnifiedToolChoiceMode::None => None,
                unified::UnifiedToolChoiceMode::Auto => Some(ToolChoice::Auto(AutoToolChoice::builder().build())),
                unified::UnifiedToolChoiceMode::Required => {
                    // Some families don't support "any" tool choice, fall back to "auto"
                    if self.supports_tool_choice_any() {
                        Some(ToolChoice::Any(AnyToolChoice::builder().build()))
                    } else {
                        // Fall back to auto for families that don't support "any"
                        Some(ToolChoice::Auto(AutoToolChoice::builder().build()))
                    }
                }
            },
            unified::UnifiedToolChoice::Specific { function } => {
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

impl From<unified::UnifiedRequest> for ConverseInput {
    fn from(request: unified::UnifiedRequest) -> Self {
        let unified::UnifiedRequest {
            model,
            messages,
            system,
            temperature,
            max_tokens,
            top_p,
            top_k: _,
            frequency_penalty: _,
            presence_penalty: _,
            stop_sequences,
            stream: _,
            tools,
            tool_choice,
            parallel_tool_calls: _,
            metadata: _,
        } = request;

        let inference_config = build_inference_config(temperature, max_tokens, top_p, stop_sequences);

        let (extracted_system, bedrock_messages) = convert_unified_messages_to_bedrock(messages);

        // Convert the system string from request to SystemContentBlock if provided
        let system_from_request = system.map(|s| vec![SystemContentBlock::Text(s)]);

        // Use the system from the request if provided, otherwise use extracted system from messages
        let final_system = system_from_request.or(extracted_system);

        let tool_config = if let Some(tool_list) = tools {
            if tool_list.is_empty() {
                None
            } else {
                convert_tools(tool_list, tool_choice, &model).ok()
            }
        } else {
            None
        };

        ConverseInput::builder()
            .model_id(model)
            .set_messages(Some(bedrock_messages))
            .set_system(final_system)
            .set_inference_config(inference_config)
            .set_tool_config(tool_config)
            .build()
            .expect("ConverseInput should build successfully with valid inputs")
    }
}

impl From<unified::UnifiedRequest> for ConverseStreamInput {
    fn from(request: unified::UnifiedRequest) -> Self {
        let unified::UnifiedRequest {
            model,
            messages,
            system,
            temperature,
            max_tokens,
            top_p,
            top_k: _,
            frequency_penalty: _,
            presence_penalty: _,
            stop_sequences,
            stream: _,
            tools,
            tool_choice,
            parallel_tool_calls: _,
            metadata: _,
        } = request;

        let inference_config = build_inference_config(temperature, max_tokens, top_p, stop_sequences);

        let (extracted_system, bedrock_messages) = convert_unified_messages_to_bedrock(messages);

        // Convert the system string from request to SystemContentBlock if provided
        let system_from_request = system.map(|s| vec![SystemContentBlock::Text(s)]);

        // Use the system from the request if provided, otherwise use extracted system from messages
        let final_system = system_from_request.or(extracted_system);

        let tool_config = if let Some(tool_list) = tools {
            if tool_list.is_empty() {
                None
            } else {
                convert_tools(tool_list, tool_choice, &model).ok()
            }
        } else {
            None
        };

        ConverseStreamInput::builder()
            .model_id(model)
            .set_messages(Some(bedrock_messages))
            .set_system(final_system)
            .set_inference_config(inference_config)
            .set_tool_config(tool_config)
            .build()
            .expect("ConverseStreamInput should build successfully with valid inputs")
    }
}

enum MessageConversion {
    System(Vec<SystemContentBlock>),
    Conversation {
        role: ConversationRole,
        blocks: Vec<ContentBlock>,
    },
    Empty,
}

fn arguments_to_document(arguments: unified::UnifiedArguments) -> (aws_smithy_types::Document, bool) {
    match arguments {
        unified::UnifiedArguments::String(raw) => match sonic_rs::from_str::<sonic_rs::Value>(&raw) {
            Ok(value) => (normalize_tool_input_document(json_value_to_document(value)), false),
            Err(err) => {
                log::debug!(
                    "Bedrock tool_use arguments fallback to string: error={err} raw={raw}",
                    raw = raw
                );
                (
                    normalize_tool_input_document(aws_smithy_types::Document::String(raw)),
                    true,
                )
            }
        },
        unified::UnifiedArguments::Value(value) => {
            (normalize_tool_input_document(serde_value_to_document(value)), false)
        }
    }
}

fn build_tool_use_content_block(id: String, name: String, input: aws_smithy_types::Document) -> Option<ContentBlock> {
    log::debug!(
        "Bedrock tool_use arguments normalized: id={} name={} kind={} preview={}",
        id,
        name,
        document_kind(&input),
        document_preview(&input)
    );

    ToolUseBlock::builder()
        .tool_use_id(id)
        .name(name)
        .input(input)
        .build()
        .map(ContentBlock::ToolUse)
        .ok()
}

fn tool_result_text(content: unified::UnifiedToolResultContent) -> String {
    match content {
        unified::UnifiedToolResultContent::Text(text) => text,
        unified::UnifiedToolResultContent::Multiple(parts) => parts.join("\n"),
    }
}

fn build_tool_result_block(id: String, text: String) -> Option<ContentBlock> {
    ToolResultBlock::builder()
        .tool_use_id(id)
        .content(ToolResultContentBlock::Text(text))
        .build()
        .map(ContentBlock::ToolResult)
        .ok()
}

fn system_blocks_from_content(content: unified::UnifiedContentContainer) -> Vec<SystemContentBlock> {
    match content {
        unified::UnifiedContentContainer::Text(text) => {
            if text.is_empty() {
                Vec::new()
            } else {
                vec![SystemContentBlock::Text(text)]
            }
        }
        unified::UnifiedContentContainer::Blocks(blocks) => blocks
            .into_iter()
            .filter_map(|block| match block {
                unified::UnifiedContent::Text { text } if !text.is_empty() => Some(SystemContentBlock::Text(text)),
                _ => None,
            })
            .collect(),
    }
}

fn conversation_blocks_from_message(message: unified::UnifiedMessage) -> (ConversationRole, Vec<ContentBlock>) {
    let unified::UnifiedMessage {
        role: unified_role,
        content,
        tool_calls,
        tool_call_id,
    } = message;

    let role = match unified_role {
        unified::UnifiedRole::User => ConversationRole::User,
        unified::UnifiedRole::Assistant => ConversationRole::Assistant,
        unified::UnifiedRole::Tool => ConversationRole::User,
        unified::UnifiedRole::System => ConversationRole::User,
    };

    let mut blocks = Vec::new();

    match content {
        unified::UnifiedContentContainer::Text(text) => {
            if unified_role == unified::UnifiedRole::Tool {
                if let Some(id) = tool_call_id.clone() {
                    if let Some(block) = build_tool_result_block(id, text) {
                        blocks.push(block);
                    }
                } else if !text.is_empty() {
                    blocks.push(ContentBlock::Text(text));
                }
            } else if !text.is_empty() {
                blocks.push(ContentBlock::Text(text));
            }
        }
        unified::UnifiedContentContainer::Blocks(items) => {
            for item in items {
                match item {
                    unified::UnifiedContent::Text { text } => {
                        if !text.is_empty() {
                            blocks.push(ContentBlock::Text(text));
                        }
                    }
                    unified::UnifiedContent::ToolUse { id, name, input } => {
                        let input_doc = normalize_tool_input_document(serde_value_to_document(input));
                        if let Some(block) = build_tool_use_content_block(id, name, input_doc) {
                            blocks.push(block);
                        }
                    }
                    unified::UnifiedContent::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    } => {
                        let mut text = tool_result_text(content);
                        if matches!(is_error, Some(true)) {
                            text = format!("ERROR: {text}");
                        }
                        if let Some(block) = build_tool_result_block(tool_use_id, text) {
                            blocks.push(block);
                        }
                    }
                    unified::UnifiedContent::Image { .. } => {
                        blocks.push(ContentBlock::Text("[Image content not supported]".to_string()));
                    }
                }
            }
        }
    }

    if let Some(tool_calls) = tool_calls {
        for call in tool_calls {
            let (input_doc, used_fallback) = arguments_to_document(call.function.arguments);
            if used_fallback {
                log::debug!(
                    "Bedrock tool_use arguments fallback to string: id={} raw arguments",
                    call.id
                );
            }
            if let Some(block) = build_tool_use_content_block(call.id, call.function.name, input_doc) {
                blocks.push(block);
            }
        }
    }

    // For tool role without explicit tool result blocks, ensure tool_call_id is respected
    if unified_role == unified::UnifiedRole::Tool
        && blocks.is_empty()
        && let Some(id) = tool_call_id
        && let Some(block) = build_tool_result_block(id, String::new())
    {
        blocks.push(block);
    }

    (role, blocks)
}

fn convert_unified_message(message: unified::UnifiedMessage) -> MessageConversion {
    match message.role {
        unified::UnifiedRole::System => {
            let blocks = system_blocks_from_content(message.content);
            if blocks.is_empty() {
                MessageConversion::Empty
            } else {
                MessageConversion::System(blocks)
            }
        }
        _ => {
            let (role, blocks) = conversation_blocks_from_message(message);
            if blocks.is_empty() {
                MessageConversion::Empty
            } else {
                MessageConversion::Conversation { role, blocks }
            }
        }
    }
}

fn convert_unified_messages_to_bedrock(
    messages: Vec<unified::UnifiedMessage>,
) -> (Option<Vec<SystemContentBlock>>, Vec<BedrockMessage>) {
    let mut system_blocks = Vec::new();
    let mut conversation_messages = Vec::new();
    let mut current_role: Option<ConversationRole> = None;
    let mut current_blocks: Vec<ContentBlock> = Vec::new();

    for message in messages {
        match convert_unified_message(message) {
            MessageConversion::System(blocks) => system_blocks.extend(blocks),
            MessageConversion::Conversation { role, blocks } => {
                if current_role.as_ref().is_some_and(|prev| *prev != role)
                    && !current_blocks.is_empty()
                    && let Some(prev_role) = current_role.take()
                    && let Ok(built) = BedrockMessage::builder()
                        .role(prev_role)
                        .set_content(Some(std::mem::take(&mut current_blocks)))
                        .build()
                {
                    conversation_messages.push(built);
                }

                if current_blocks.is_empty() {
                    current_blocks = blocks;
                } else {
                    current_blocks.extend(blocks);
                }

                current_role = Some(role);
            }
            MessageConversion::Empty => {}
        }
    }

    if let Some(role) = current_role
        && !current_blocks.is_empty()
        && let Ok(message) = BedrockMessage::builder()
            .role(role)
            .set_content(Some(current_blocks))
            .build()
    {
        conversation_messages.push(message);
    }

    let system = if system_blocks.is_empty() {
        None
    } else {
        Some(system_blocks)
    };

    (system, conversation_messages)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic)]

    use super::*;
    use aws_smithy_types::Document;

    #[test]
    fn tool_use_arguments_parse_as_object() {
        let doc = normalize_tool_input_document(serde_value_to_document(serde_json::json!({
            "command": "ls"
        })));

        let block = build_tool_use_content_block("tool-1".to_string(), "Bash".to_string(), doc).expect("content block");

        let ContentBlock::ToolUse(tool_use) = block else {
            panic!("expected tool use block");
        };

        assert!(matches!(tool_use.input(), Document::Object(_)));
    }

    #[test]
    fn tool_use_arguments_fallback_to_string() {
        let (doc, _) = arguments_to_document(unified::UnifiedArguments::String("not json".to_string()));
        let block = build_tool_use_content_block("tool-1".to_string(), "Bash".to_string(), doc).expect("content block");

        let ContentBlock::ToolUse(tool_use) = block else {
            panic!("expected tool use block");
        };

        assert!(matches!(tool_use.input(), Document::String(_)));
    }
}
