use serde::Serialize;
use serde_json::{self, Value as SerdeValue, json};
use std::collections::HashMap;

use crate::messages::{openai, unified};
use crate::provider::google::output::{
    GoogleContent, GoogleFunctionCall, GoogleFunctionResponse, GooglePart, GoogleRole,
};

/// Request body for Google Gemini GenerateContent API.
///
/// This struct represents the request format for generating content with Gemini models
/// as documented in the [Google AI API Reference](https://ai.google.dev/api/generate-content).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleGenerateRequest {
    /// The content of the current conversation with the model.
    ///
    /// For single-turn queries, this is a single instance.
    /// For multi-turn queries, this is a repeated field that contains conversation history and the latest request.
    pub contents: Vec<GoogleContent>,

    /// Optional configuration for model generation and output.
    pub generation_config: Option<GoogleGenerationConfig>,

    /// Optional safety settings to block unsafe content.
    ///
    /// These settings control the threshold for blocking content based on
    /// probability of harmfulness across various categories.
    pub safety_settings: Option<Vec<GoogleSafetySetting>>,

    /// Optional tool configurations for function calling.
    ///
    /// A list of Tools the model may use to generate the next response.
    pub tools: Option<Vec<GoogleTool>>,

    /// Optional tool configuration for any tools specified in the request.
    pub tool_config: Option<GoogleToolConfig>,

    /// Optional system instruction (prompt).
    ///
    /// The system instruction is a more natural way to steer the behavior of the model
    /// than using examples in a prompt.
    pub system_instruction: Option<GoogleContent>,
}

/// Configuration options for model generation and output.
///
/// Controls various aspects of the generation process including sampling parameters
/// and output formatting.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleGenerationConfig {
    /// Set of character sequences that will stop output generation.
    /// If specified, the API will stop at the first appearance of a stop sequence.
    pub stop_sequences: Option<Vec<String>>,

    /// MIME type of the generated candidate text.
    ///
    /// Supported values include:
    /// - `text/plain`: (default) Text output
    /// - `application/json`: JSON response format
    pub response_mime_type: Option<String>,

    /// Output schema of the generated candidate text when response_mime_type is `application/json`.
    ///
    /// This field allows you to constrain the model's JSON output to match a specific schema.
    pub response_schema: Option<sonic_rs::OwnedLazyValue>,

    /// Number of generated responses to return.
    ///
    /// Currently, this value can only be set to 1.
    pub candidate_count: Option<i32>,

    /// The maximum number of tokens to include in a candidate.
    ///
    /// If unset, this will default to a value determined by the model.
    pub max_output_tokens: Option<i32>,

    /// Controls randomness in generation.
    ///
    /// Values can range from 0.0 to 2.0.
    /// Higher values produce more random outputs.
    pub temperature: Option<f32>,

    /// The maximum cumulative probability of tokens to consider when sampling.
    ///
    /// The model uses combined top-k and nucleus sampling.
    /// Tokens are sorted based on their assigned probabilities.
    pub top_p: Option<f32>,

    /// The maximum number of tokens to consider when sampling.
    ///
    /// The model uses combined top-k and nucleus sampling.
    /// Top-k sampling considers the set of top_k most probable tokens.
    pub top_k: Option<i32>,
}

/// Safety setting for blocking unsafe content.
///
/// Controls content filtering based on harmfulness probability.
#[derive(Debug, Serialize)]
pub struct GoogleSafetySetting {
    /// The category of harmful content to filter.
    ///
    /// Categories include:
    /// - HARM_CATEGORY_HARASSMENT
    /// - HARM_CATEGORY_HATE_SPEECH
    /// - HARM_CATEGORY_SEXUALLY_EXPLICIT
    /// - HARM_CATEGORY_DANGEROUS_CONTENT
    category: String,

    /// The threshold for blocking content.
    ///
    /// Values include:
    /// - BLOCK_NONE: Always show content
    /// - BLOCK_LOW_AND_ABOVE: Block when low, medium, or high probability
    /// - BLOCK_MEDIUM_AND_ABOVE: Block when medium or high probability
    /// - BLOCK_HIGH: Block only when high probability
    threshold: String,
}

/// Tool configuration for function calling.
///
/// Defines functions that the model can call to get additional information.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleTool {
    /// A list of function declarations that the model can call.
    function_declarations: Option<Vec<GoogleFunctionDeclaration>>,
}

/// Declaration of a function that the model can call.
///
/// Describes a function including its parameters that the model can invoke.
#[derive(Debug, Serialize)]
pub struct GoogleFunctionDeclaration {
    /// The name of the function to call.
    name: String,

    /// Optional description of what the function does.
    description: Option<String>,

    /// The parameters of this function in JSON Schema format.
    parameters: Option<sonic_rs::OwnedLazyValue>,
}

impl From<openai::Tool> for GoogleFunctionDeclaration {
    fn from(tool: openai::Tool) -> Self {
        // Google's API doesn't support certain JSON Schema fields
        // We need to strip them from the parameters
        let cleaned_schema = strip_unsupported_schema_fields(tool.function.parameters);

        // Convert the cleaned schema back to sonic_rs::Value for Google's API
        let json = sonic_rs::to_string(&cleaned_schema).unwrap_or_else(|_| "{}".to_string());
        let parameters = Some(
            sonic_rs::from_str::<sonic_rs::OwnedLazyValue>(&json).unwrap_or_else(|_| sonic_rs::from_str("{}").unwrap()),
        );

        Self {
            name: tool.function.name,
            description: Some(tool.function.description),
            parameters,
        }
    }
}

impl From<unified::UnifiedTool> for GoogleFunctionDeclaration {
    fn from(tool: unified::UnifiedTool) -> Self {
        let unified::UnifiedFunction {
            name,
            description,
            parameters,
            strict: _,
        } = tool.function;

        let cleaned_schema = strip_unsupported_schema_fields(*parameters);
        let json = sonic_rs::to_string(&cleaned_schema).unwrap_or_else(|_| "{}".to_string());
        let parameters = Some(parse_owned_lazy(&json));

        Self {
            name,
            description: if description.is_empty() {
                None
            } else {
                Some(description)
            },
            parameters,
        }
    }
}

/// Recursively removes unsupported JSON Schema fields from the schema
/// Google's API doesn't support fields like 'additionalProperties' and '$schema'
fn strip_unsupported_schema_fields(mut schema: openai::JsonSchema) -> openai::JsonSchema {
    // Remove unsupported fields at this level
    schema.additional_properties = None;
    schema.schema = None;
    schema.default = None; // Gemini doesn't support default values

    // Handle format field restrictions for string types
    // Gemini only supports "enum" and "date-time" formats
    if schema.r#type.as_deref() == Some("string")
        && let Some(ref format) = schema.format
        && format != "enum"
        && format != "date-time"
    {
        schema.format = None;
    }

    // Recursively process nested properties
    if let Some(mut properties) = schema.properties {
        for (_, prop_value) in properties.iter_mut() {
            *prop_value = strip_unsupported_schema_fields(prop_value.clone());
        }
        schema.properties = Some(properties);
    }

    // Process items for array types
    if let Some(items) = schema.items {
        schema.items = Some(Box::new(strip_unsupported_schema_fields(*items)));
    }

    schema
}

/// Google's function calling mode.
///
/// Controls how the model interacts with available functions.
#[derive(Debug, Serialize, Clone, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GoogleFunctionCallingMode {
    /// Model cannot call functions
    None,
    /// Model decides whether to call functions
    Auto,
    /// Model must call at least one function
    Any,
}

impl From<openai::ToolChoiceMode> for GoogleFunctionCallingMode {
    fn from(mode: openai::ToolChoiceMode) -> Self {
        match mode {
            openai::ToolChoiceMode::None => GoogleFunctionCallingMode::None,
            openai::ToolChoiceMode::Auto => GoogleFunctionCallingMode::Auto,
            openai::ToolChoiceMode::Required | openai::ToolChoiceMode::Any => GoogleFunctionCallingMode::Any,
            openai::ToolChoiceMode::Other(_) => GoogleFunctionCallingMode::Auto, // Default to auto for unknown
        }
    }
}

impl From<unified::UnifiedToolChoiceMode> for GoogleFunctionCallingMode {
    fn from(mode: unified::UnifiedToolChoiceMode) -> Self {
        match mode {
            unified::UnifiedToolChoiceMode::None => GoogleFunctionCallingMode::None,
            unified::UnifiedToolChoiceMode::Auto => GoogleFunctionCallingMode::Auto,
            unified::UnifiedToolChoiceMode::Required => GoogleFunctionCallingMode::Any,
        }
    }
}

fn empty_owned_object() -> sonic_rs::OwnedLazyValue {
    sonic_rs::from_str("{}").expect("static JSON object should parse")
}

fn parse_owned_lazy(json: &str) -> sonic_rs::OwnedLazyValue {
    sonic_rs::from_str(json).unwrap_or_else(|_| empty_owned_object())
}

fn owned_lazy_from_serde(value: SerdeValue) -> sonic_rs::OwnedLazyValue {
    let json_string = serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string());
    parse_owned_lazy(json_string.as_str())
}

fn arguments_to_owned_lazy(arguments: unified::UnifiedArguments) -> sonic_rs::OwnedLazyValue {
    match arguments {
        unified::UnifiedArguments::String(raw) => parse_owned_lazy(raw.as_str()),
        unified::UnifiedArguments::Value(value) => owned_lazy_from_serde(value),
    }
}

fn tool_result_value(content: unified::UnifiedToolResultContent, is_error: Option<bool>) -> SerdeValue {
    let mut base = match content {
        unified::UnifiedToolResultContent::Text(text) => match serde_json::from_str::<SerdeValue>(&text) {
            Ok(SerdeValue::Object(obj)) => SerdeValue::Object(obj),
            Ok(other) => json!({ "result": other }),
            Err(_) => json!({ "result": text }),
        },
        unified::UnifiedToolResultContent::Multiple(values) => json!({ "result": values }),
    };

    if let Some(flag) = is_error
        && let SerdeValue::Object(ref mut map) = base
    {
        map.insert("is_error".to_string(), SerdeValue::Bool(flag));
    }

    base
}

fn tool_result_to_owned_lazy(
    content: unified::UnifiedToolResultContent,
    is_error: Option<bool>,
) -> sonic_rs::OwnedLazyValue {
    owned_lazy_from_serde(tool_result_value(content, is_error))
}

fn tool_text_to_owned_lazy(text: String) -> sonic_rs::OwnedLazyValue {
    tool_result_to_owned_lazy(unified::UnifiedToolResultContent::Text(text), None)
}

fn push_text_part(parts: &mut Vec<GooglePart>, text: String) {
    if text.is_empty() {
        return;
    }

    parts.push(GooglePart {
        text: Some(text),
        function_call: None,
        function_response: None,
    });
}

fn extract_system_text(content: unified::UnifiedContentContainer) -> Option<String> {
    match content {
        unified::UnifiedContentContainer::Text(text) => Some(text),
        unified::UnifiedContentContainer::Blocks(blocks) => {
            let mut collected = Vec::new();
            for block in blocks {
                if let unified::UnifiedContent::Text { text } = block {
                    collected.push(text);
                }
            }

            if collected.is_empty() {
                None
            } else {
                Some(collected.join("\n"))
            }
        }
    }
}

fn push_function_call(
    parts: &mut Vec<GooglePart>,
    id: String,
    name: String,
    args: sonic_rs::OwnedLazyValue,
    tool_call_names: &mut HashMap<String, String>,
) {
    tool_call_names.insert(id, name.clone());
    parts.push(GooglePart {
        text: None,
        function_call: Some(GoogleFunctionCall {
            name,
            args,
            thought_signature: None,
        }),
        function_response: None,
    });
}

fn push_function_response(
    parts: &mut Vec<GooglePart>,
    tool_use_id: String,
    response: sonic_rs::OwnedLazyValue,
    tool_call_names: &HashMap<String, String>,
) {
    let function_name = tool_call_names.get(&tool_use_id).cloned().unwrap_or_else(|| {
        log::warn!("Could not find function name for tool_use_id: {tool_use_id}, using fallback");
        tool_use_id
    });

    parts.push(GooglePart {
        text: None,
        function_call: None,
        function_response: Some(GoogleFunctionResponse {
            name: function_name,
            response,
        }),
    });
}

fn convert_unified_messages(
    messages: Vec<unified::UnifiedMessage>,
    system_instruction: Option<GoogleContent>,
) -> (Vec<GoogleContent>, Option<GoogleContent>) {
    let mut contents = Vec::new();
    let mut system_instruction = system_instruction;
    let mut tool_call_names: HashMap<String, String> = HashMap::new();

    for message in messages {
        match message.role {
            unified::UnifiedRole::System => {
                if let Some(text) = extract_system_text(message.content) {
                    system_instruction = Some(GoogleContent {
                        parts: vec![GooglePart {
                            text: Some(text),
                            function_call: None,
                            function_response: None,
                        }],
                        role: GoogleRole::User,
                    });
                }
            }
            unified::UnifiedRole::User => handle_user_message(message, &mut contents, &mut tool_call_names),
            unified::UnifiedRole::Assistant => handle_assistant_message(message, &mut contents, &mut tool_call_names),
            unified::UnifiedRole::Tool => handle_tool_message(message, &mut contents, &mut tool_call_names),
        }
    }

    (contents, system_instruction)
}

fn handle_user_message(
    message: unified::UnifiedMessage,
    contents: &mut Vec<GoogleContent>,
    tool_call_names: &mut HashMap<String, String>,
) {
    let unified::UnifiedMessage {
        content,
        tool_calls,
        tool_call_id: _,
        ..
    } = message;

    let mut parts = Vec::new();

    match content {
        unified::UnifiedContentContainer::Text(text) => push_text_part(&mut parts, text),
        unified::UnifiedContentContainer::Blocks(blocks) => {
            for block in blocks {
                match block {
                    unified::UnifiedContent::Text { text } => push_text_part(&mut parts, text),
                    unified::UnifiedContent::ToolUse { id, name, input } => {
                        let args = owned_lazy_from_serde(input);
                        push_function_call(&mut parts, id, name, args, tool_call_names);
                    }
                    unified::UnifiedContent::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    } => {
                        let response = tool_result_to_owned_lazy(content, is_error);
                        push_function_response(&mut parts, tool_use_id, response, tool_call_names);
                    }
                    unified::UnifiedContent::Image { .. } => {
                        push_text_part(&mut parts, "[Image content not supported]".to_string());
                    }
                }
            }
        }
    }

    if let Some(calls) = tool_calls {
        for call in calls {
            let args = arguments_to_owned_lazy(call.function.arguments);
            push_function_call(&mut parts, call.id, call.function.name, args, tool_call_names);
        }
    }

    if !parts.is_empty() {
        contents.push(GoogleContent {
            parts,
            role: GoogleRole::User,
        });
    }
}

fn handle_assistant_message(
    message: unified::UnifiedMessage,
    contents: &mut Vec<GoogleContent>,
    tool_call_names: &mut HashMap<String, String>,
) {
    let unified::UnifiedMessage {
        content, tool_calls, ..
    } = message;

    let mut parts = Vec::new();

    match content {
        unified::UnifiedContentContainer::Text(text) => push_text_part(&mut parts, text),
        unified::UnifiedContentContainer::Blocks(blocks) => {
            for block in blocks {
                match block {
                    unified::UnifiedContent::Text { text } => push_text_part(&mut parts, text),
                    unified::UnifiedContent::ToolUse { id, name, input } => {
                        let args = owned_lazy_from_serde(input);
                        push_function_call(&mut parts, id, name, args, tool_call_names);
                    }
                    unified::UnifiedContent::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    } => {
                        let response = tool_result_to_owned_lazy(content, is_error);
                        push_function_response(&mut parts, tool_use_id, response, tool_call_names);
                    }
                    unified::UnifiedContent::Image { .. } => {
                        push_text_part(&mut parts, "[Image content not supported]".to_string());
                    }
                }
            }
        }
    }

    if let Some(calls) = tool_calls {
        for call in calls {
            let args = arguments_to_owned_lazy(call.function.arguments);
            push_function_call(&mut parts, call.id, call.function.name, args, tool_call_names);
        }
    }

    if !parts.is_empty() {
        contents.push(GoogleContent {
            parts,
            role: GoogleRole::Model,
        });
    }
}

fn handle_tool_message(
    message: unified::UnifiedMessage,
    contents: &mut Vec<GoogleContent>,
    tool_call_names: &mut HashMap<String, String>,
) {
    let unified::UnifiedMessage {
        content,
        tool_calls: _,
        tool_call_id,
        ..
    } = message;

    let mut parts = Vec::new();

    match content {
        unified::UnifiedContentContainer::Text(text) => {
            if let Some(id) = tool_call_id {
                let response = tool_text_to_owned_lazy(text);
                push_function_response(&mut parts, id, response, tool_call_names);
            } else {
                push_text_part(&mut parts, text);
            }
        }
        unified::UnifiedContentContainer::Blocks(blocks) => {
            for block in blocks {
                match block {
                    unified::UnifiedContent::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    } => {
                        let response = tool_result_to_owned_lazy(content, is_error);
                        push_function_response(&mut parts, tool_use_id, response, tool_call_names);
                    }
                    unified::UnifiedContent::Text { text } => {
                        let response = tool_text_to_owned_lazy(text);
                        let id = tool_call_id.clone().unwrap_or_else(|| "unknown_function".to_string());
                        push_function_response(&mut parts, id, response, tool_call_names);
                    }
                    unified::UnifiedContent::ToolUse { id, name, input } => {
                        let args = owned_lazy_from_serde(input);
                        push_function_call(&mut parts, id, name, args, tool_call_names);
                    }
                    unified::UnifiedContent::Image { .. } => {
                        push_text_part(&mut parts, "[Image content not supported]".to_string());
                    }
                }
            }
        }
    }

    if !parts.is_empty() {
        contents.push(GoogleContent {
            parts,
            role: GoogleRole::User,
        });
    }
}

/// Configuration for function calling behavior.
///
/// Controls how the model should use the provided functions.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleToolConfig {
    /// Configuration for function calling.
    function_calling_config: Option<GoogleFunctionCallingConfig>,
}

/// Specifies the mode and allowed functions for function calling.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleFunctionCallingConfig {
    /// The mode of function calling.
    mode: GoogleFunctionCallingMode,

    /// List of function names the model is allowed to call.
    /// If empty, the model can call any provided function.
    allowed_function_names: Option<Vec<String>>,
}

impl From<unified::UnifiedRequest> for GoogleGenerateRequest {
    fn from(request: unified::UnifiedRequest) -> Self {
        let unified::UnifiedRequest {
            model: _, // Model resolution handled by provider before conversion
            messages,
            system,
            max_tokens,
            temperature,
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

        let system_instruction = system.map(|text| GoogleContent {
            parts: vec![GooglePart {
                text: Some(text),
                function_call: None,
                function_response: None,
            }],
            role: GoogleRole::User,
        });

        // Convert conversation messages directly from unified representation
        let (contents, system_instruction) = convert_unified_messages(messages, system_instruction);

        // Convert tools
        let tools = tools.map(|tool_list| {
            let tool_count = tool_list.len();
            log::debug!("Converting {} tools to Google format", tool_count);

            let function_declarations: Vec<GoogleFunctionDeclaration> = tool_list
                .into_iter()
                .map(|tool| {
                    let declaration = GoogleFunctionDeclaration::from(tool);
                    log::debug!(
                        "Converted tool '{}' with parameters: {}",
                        declaration.name,
                        sonic_rs::to_string(&declaration.parameters)
                            .unwrap_or_else(|_| "<serialization failed>".to_string())
                    );
                    declaration
                })
                .collect();

            vec![GoogleTool {
                function_declarations: Some(function_declarations),
            }]
        });

        // Convert tool choice
        let tool_config = tool_choice.map(|choice| {
            let (mode, allowed_names) = match choice {
                unified::UnifiedToolChoice::Mode(mode) => {
                    let google_mode = GoogleFunctionCallingMode::from(mode);
                    log::debug!("Tool choice mode mapped to {:?}", google_mode);
                    (google_mode, None)
                }
                unified::UnifiedToolChoice::Specific { function } => {
                    log::debug!("Tool choice specific function: {}", function.name);
                    (GoogleFunctionCallingMode::Any, Some(vec![function.name]))
                }
            };

            GoogleToolConfig {
                function_calling_config: Some(GoogleFunctionCallingConfig {
                    mode,
                    allowed_function_names: allowed_names,
                }),
            }
        });

        let generation_config = GoogleGenerationConfig {
            temperature,
            top_p,
            top_k: None,
            max_output_tokens: max_tokens.map(|value| value as i32),
            stop_sequences,
            candidate_count: Some(1),
            response_mime_type: None,
            response_schema: None,
        };

        let result = Self {
            contents,
            generation_config: Some(generation_config),
            safety_settings: None,
            tools,
            tool_config,
            system_instruction,
        };

        log::debug!(
            "Final Google request - has tools: {}, has tool_config: {}, contents count: {}",
            result.tools.is_some(),
            result.tool_config.is_some(),
            result.contents.len()
        );

        if let Ok(json) = sonic_rs::to_string(&result) {
            let preview = if json.len() > 2000 {
                format!("{}... (truncated, {} bytes total)", &json[..2000], json.len())
            } else {
                json
            };
            log::debug!("Complete Google request JSON: {}", preview);
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_unsupported_schema_fields() {
        use std::collections::BTreeMap;

        // Create a JsonSchema struct with unsupported fields
        let schema = openai::JsonSchema {
            r#type: Some("object".to_string()),
            schema: Some("http://json-schema.org/draft-07/schema#".to_string()),
            additional_properties: Some(false),
            properties: Some({
                let mut props = BTreeMap::new();

                props.insert(
                    "field1".to_string(),
                    openai::JsonSchema {
                        r#type: Some("string".to_string()),
                        ..Default::default()
                    },
                );

                props.insert(
                    "nested".to_string(),
                    openai::JsonSchema {
                        r#type: Some("object".to_string()),
                        additional_properties: Some(false),
                        properties: Some({
                            let mut nested_props = BTreeMap::new();
                            nested_props.insert(
                                "subfield".to_string(),
                                openai::JsonSchema {
                                    r#type: Some("number".to_string()),
                                    ..Default::default()
                                },
                            );
                            nested_props
                        }),
                        ..Default::default()
                    },
                );

                props.insert(
                    "array_field".to_string(),
                    openai::JsonSchema {
                        r#type: Some("array".to_string()),
                        items: Some(Box::new(openai::JsonSchema {
                            r#type: Some("object".to_string()),
                            additional_properties: Some(true),
                            ..Default::default()
                        })),
                        ..Default::default()
                    },
                );

                props
            }),
            ..Default::default()
        };

        let cleaned = strip_unsupported_schema_fields(schema);

        // Check that $schema is removed from root
        assert!(cleaned.schema.is_none());

        // Check that additionalProperties is removed from root
        assert!(cleaned.additional_properties.is_none());

        // Check that nested additionalProperties is removed
        let nested = &cleaned.properties.as_ref().unwrap()["nested"];
        assert!(nested.additional_properties.is_none());

        // Check that array item additionalProperties is removed
        let array_field = &cleaned.properties.as_ref().unwrap()["array_field"];
        let items = array_field.items.as_ref().unwrap();
        assert!(items.additional_properties.is_none());

        // Check that valid fields are preserved
        assert_eq!(cleaned.r#type, Some("object".to_string()));
        assert_eq!(
            cleaned.properties.as_ref().unwrap()["field1"].r#type,
            Some("string".to_string())
        );
    }
}
