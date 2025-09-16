use serde::Serialize;

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
    pub response_schema: Option<sonic_rs::Value>,

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
    parameters: Option<serde_json::Value>,
}

impl From<openai::Tool> for GoogleFunctionDeclaration {
    fn from(tool: openai::Tool) -> Self {
        // Google's API doesn't support certain JSON Schema fields
        // We need to strip them from the parameters
        let parameters = Some(strip_unsupported_schema_fields(tool.function.parameters));

        Self {
            name: tool.function.name,
            description: Some(tool.function.description),
            parameters,
        }
    }
}

/// Recursively removes unsupported JSON Schema fields from the schema
/// Google's API doesn't support fields like 'additionalProperties' and '$schema'
fn strip_unsupported_schema_fields(mut value: serde_json::Value) -> serde_json::Value {
    if let Some(obj) = value.as_object_mut() {
        // Remove unsupported fields at this level
        obj.remove("additionalProperties");
        obj.remove("$schema");
        obj.remove("default");  // Gemini doesn't support default values

        // Handle format field restrictions for string types
        // Gemini only supports "enum" and "date-time" formats
        if obj.get("type").and_then(|v| v.as_str()) == Some("string")
            && let Some(format) = obj.get("format").and_then(|v| v.as_str())
            && format != "enum" && format != "date-time"
        {
            obj.remove("format");
        }

        // Recursively process nested properties
        if let Some(properties) = obj.get_mut("properties")
            && let Some(props_obj) = properties.as_object_mut()
        {
            for (_, prop_value) in props_obj.iter_mut() {
                *prop_value = strip_unsupported_schema_fields(prop_value.take());
            }
        }

        // Process items for array types
        if let Some(items) = obj.get_mut("items") {
            *items = strip_unsupported_schema_fields(items.take());
        }
    }

    value
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

impl From<openai::ChatCompletionRequest> for GoogleGenerateRequest {
    fn from(request: openai::ChatCompletionRequest) -> Self {
        let mut google_contents = Vec::new();
        let mut system_instruction = None;

        // Track tool calls from assistant messages to map tool_call_id to function names
        let mut tool_call_mapping: std::collections::HashMap<String, String> = std::collections::HashMap::new();

        // Extract tools and tool_choice before consuming request
        let tools = request.tools.map(|tools| {
            let tool_count = tools.len();
            log::debug!("Converting {} tools to Google format", tool_count);

            let function_declarations: Vec<GoogleFunctionDeclaration> = tools
                .into_iter()
                .map(|tool| {
                    let declaration = GoogleFunctionDeclaration::from(tool);
                    log::debug!(
                        "Converted tool '{}' with parameters: {}",
                        declaration.name,
                        serde_json::to_string(&declaration.parameters).unwrap_or_else(|_| "<serialization failed>".to_string())
                    );
                    declaration
                })
                .collect();
            vec![GoogleTool {
                function_declarations: Some(function_declarations),
            }]
        });

        let tool_config = request.tool_choice.map(|choice| {
            let (mode, allowed_names) = match choice {
                openai::ToolChoice::Mode(mode) => {
                    let google_mode = GoogleFunctionCallingMode::from(mode.clone());
                    log::debug!("Tool choice mode: {:?} -> {:?}", mode, google_mode);
                    (google_mode, None)
                },
                openai::ToolChoice::Specific { function, .. } => {
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

        for msg in request.messages {
            match msg.role {
                openai::ChatRole::System => {
                    // Google uses systemInstruction for system messages
                    system_instruction = Some(GoogleContent {
                        parts: vec![GooglePart {
                            text: Some(msg.content.unwrap_or_default()),
                            function_call: None,
                            function_response: None,
                        }],
                        role: GoogleRole::User, // System instruction role is typically "user"
                    });
                }
                openai::ChatRole::User => {
                    google_contents.push(GoogleContent {
                        parts: vec![GooglePart {
                            text: Some(msg.content.unwrap_or_default()),
                            function_call: None,
                            function_response: None,
                        }],
                        role: GoogleRole::User,
                    });
                }
                openai::ChatRole::Assistant => {
                    let mut parts = Vec::new();

                    // Add text content if present
                    if let Some(content) = msg.content
                        && !content.is_empty()
                    {
                        parts.push(GooglePart {
                            text: Some(content),
                            function_call: None,
                            function_response: None,
                        });
                    }

                    // Add tool calls if present
                    if let Some(tool_calls) = msg.tool_calls {
                        for tool_call in tool_calls {
                            // Parse arguments as JSON
                            let args = serde_json::from_str(&tool_call.function.arguments)
                                .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

                            // Store the mapping from tool_call_id to function name
                            // We need to clone the name here since we're moving it into GoogleFunctionCall below
                            tool_call_mapping.insert(tool_call.id, tool_call.function.name.clone());

                            parts.push(GooglePart {
                                text: None,
                                function_call: Some(GoogleFunctionCall {
                                    name: tool_call.function.name,
                                    args,
                                    thought_signature: None,
                                }),
                                function_response: None,
                            });
                        }
                    }

                    // Only add if we have parts
                    if !parts.is_empty() {
                        google_contents.push(GoogleContent {
                            parts,
                            role: GoogleRole::Model, // Google uses "model" instead of "assistant"
                        });
                    }
                }
                openai::ChatRole::Tool => {
                    // Convert tool response to Google's function response format
                    if let Some(tool_call_id) = msg.tool_call_id {
                        // Look up the function name from our mapping
                        let function_name = tool_call_mapping.get(&tool_call_id)
                            .cloned()
                            .unwrap_or_else(|| {
                                log::warn!("Could not find function name for tool_call_id: {tool_call_id}, using 'unknown_function'");
                                "unknown_function".to_string()
                            });

                        let response_content = msg.content.unwrap_or_default();

                        // Google's API requires function_response.response to be a JSON object
                        // Parse response as JSON and ensure it's an object
                        let response_value = match serde_json::from_str::<serde_json::Value>(&response_content) {
                            Ok(value) if value.is_object() => {
                                // Already a JSON object, use as-is
                                log::debug!("Tool response is already a JSON object: {value}");
                                value
                            }
                            Ok(value) => {
                                // Valid JSON but not an object (string, number, array, etc.)
                                log::debug!(
                                    "Tool response is JSON but not an object (type: {}), wrapping it",
                                    if value.is_string() {
                                        "string"
                                    } else if value.is_number() {
                                        "number"
                                    } else if value.is_array() {
                                        "array"
                                    } else if value.is_boolean() {
                                        "boolean"
                                    } else {
                                        "null"
                                    }
                                );
                                serde_json::json!({
                                    "result": response_content
                                })
                            }
                            Err(e) => {
                                // Not valid JSON at all
                                log::debug!("Tool response is not valid JSON ({e}), wrapping as string");
                                serde_json::json!({
                                    "result": response_content
                                })
                            }
                        };

                        let function_response = GoogleFunctionResponse {
                            name: function_name.clone(),
                            response: response_value,
                        };

                        log::debug!(
                            "Creating function response for '{}': {:?}",
                            function_name,
                            serde_json::to_string(&function_response.response)
                                .unwrap_or_else(|_| "serialization failed".to_string())
                        );

                        google_contents.push(GoogleContent {
                            parts: vec![GooglePart {
                                text: None,
                                function_call: None,
                                function_response: Some(function_response),
                            }],
                            role: GoogleRole::User, // Function responses are sent as user messages
                        });
                    } else {
                        log::warn!("Tool message missing tool_call_id, treating as regular user message");
                        google_contents.push(GoogleContent {
                            parts: vec![GooglePart {
                                text: Some(msg.content.unwrap_or_default()),
                                function_call: None,
                                function_response: None,
                            }],
                            role: GoogleRole::User,
                        });
                    }
                }
                openai::ChatRole::Other(role) => {
                    log::warn!("Unknown chat role from request: {role}, treating as user");
                    google_contents.push(GoogleContent {
                        parts: vec![GooglePart {
                            text: Some(msg.content.unwrap_or_default()),
                            function_call: None,
                            function_response: None,
                        }],
                        role: GoogleRole::User,
                    });
                }
            }
        }

        let generation_config = GoogleGenerationConfig {
            temperature: request.temperature,
            top_p: request.top_p,
            top_k: None,
            max_output_tokens: request.max_tokens.map(|x| x as i32),
            stop_sequences: request.stop,
            candidate_count: Some(1),
            response_mime_type: None,
            response_schema: None,
        };

        let result = Self {
            contents: google_contents,
            generation_config: Some(generation_config),
            safety_settings: None,
            tools,
            tool_config,
            system_instruction,
        };

        // Log the final request structure for debugging
        log::debug!(
            "Final Google request - has tools: {}, has tool_config: {}, contents count: {}",
            result.tools.is_some(),
            result.tool_config.is_some(),
            result.contents.len()
        );

        if let Ok(json) = serde_json::to_string(&result) {
            // Truncate for readability if too long
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

impl From<unified::UnifiedRequest> for GoogleGenerateRequest {
    fn from(request: unified::UnifiedRequest) -> Self {
        // Convert unified to OpenAI first, then use existing conversion
        let openai_request = openai::ChatCompletionRequest::from(request);
        Self::from(openai_request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_strip_unsupported_schema_fields() {
        let schema = json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "properties": {
                "field1": {
                    "type": "string"
                },
                "nested": {
                    "type": "object",
                    "properties": {
                        "subfield": {
                            "type": "number"
                        }
                    },
                    "additionalProperties": false
                },
                "array_field": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "additionalProperties": true
                    }
                }
            },
            "additionalProperties": false
        });

        let cleaned = strip_unsupported_schema_fields(schema);

        // Check that $schema is removed from root
        assert!(cleaned.get("$schema").is_none());

        // Check that additionalProperties is removed from root
        assert!(cleaned.get("additionalProperties").is_none());

        // Check that nested additionalProperties is removed
        let nested = &cleaned["properties"]["nested"];
        assert!(nested.get("additionalProperties").is_none());

        // Check that array item additionalProperties is removed
        let items = &cleaned["properties"]["array_field"]["items"];
        assert!(items.get("additionalProperties").is_none());

        // Check that valid fields are preserved
        assert_eq!(cleaned["type"], "object");
        assert_eq!(cleaned["properties"]["field1"]["type"], "string");
    }
}
