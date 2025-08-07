use serde::Serialize;

use super::output::{GoogleContent, GooglePart};
use crate::messages::{ChatCompletionRequest, ChatRole};

/// Request body for Google Gemini GenerateContent API.
///
/// This struct represents the request format for generating content with Gemini models
/// as documented in the [Google AI API Reference](https://ai.google.dev/api/generate-content).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GoogleGenerateRequest {
    /// The content of the current conversation with the model.
    ///
    /// For single-turn queries, this is a single instance.
    /// For multi-turn queries, this is a repeated field that contains conversation history and the latest request.
    pub(super) contents: Vec<GoogleContent>,

    /// Optional configuration for model generation and output.
    pub(super) generation_config: Option<GoogleGenerationConfig>,

    /// Optional safety settings to block unsafe content.
    ///
    /// These settings control the threshold for blocking content based on
    /// probability of harmfulness across various categories.
    pub(super) safety_settings: Option<Vec<GoogleSafetySetting>>,

    /// Optional tool configurations for function calling.
    ///
    /// A list of Tools the model may use to generate the next response.
    pub(super) tools: Option<Vec<GoogleTool>>,

    /// Optional tool configuration for any tools specified in the request.
    pub(super) tool_config: Option<GoogleToolConfig>,

    /// Optional system instruction (prompt).
    ///
    /// The system instruction is a more natural way to steer the behavior of the model
    /// than using examples in a prompt.
    pub(super) system_instruction: Option<GoogleContent>,
}

/// Configuration options for model generation and output.
///
/// Controls various aspects of the generation process including sampling parameters
/// and output formatting.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GoogleGenerationConfig {
    /// Set of character sequences that will stop output generation.
    /// If specified, the API will stop at the first appearance of a stop sequence.
    pub(super) stop_sequences: Option<Vec<String>>,

    /// MIME type of the generated candidate text.
    ///
    /// Supported values include:
    /// - `text/plain`: (default) Text output
    /// - `application/json`: JSON response format
    pub(super) response_mime_type: Option<String>,

    /// Output schema of the generated candidate text when response_mime_type is `application/json`.
    ///
    /// This field allows you to constrain the model's JSON output to match a specific schema.
    pub(super) response_schema: Option<sonic_rs::Value>,

    /// Number of generated responses to return.
    ///
    /// Currently, this value can only be set to 1.
    pub(super) candidate_count: Option<i32>,

    /// The maximum number of tokens to include in a candidate.
    ///
    /// If unset, this will default to a value determined by the model.
    pub(super) max_output_tokens: Option<i32>,

    /// Controls randomness in generation.
    ///
    /// Values can range from 0.0 to 2.0.
    /// Higher values produce more random outputs.
    pub(super) temperature: Option<f32>,

    /// The maximum cumulative probability of tokens to consider when sampling.
    ///
    /// The model uses combined top-k and nucleus sampling.
    /// Tokens are sorted based on their assigned probabilities.
    pub(super) top_p: Option<f32>,

    /// The maximum number of tokens to consider when sampling.
    ///
    /// The model uses combined top-k and nucleus sampling.
    /// Top-k sampling considers the set of top_k most probable tokens.
    pub(super) top_k: Option<i32>,
}

/// Safety setting for blocking unsafe content.
///
/// Controls content filtering based on harmfulness probability.
#[derive(Debug, Serialize)]
pub(super) struct GoogleSafetySetting {
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
pub(super) struct GoogleTool {
    /// A list of function declarations that the model can call.
    function_declarations: Option<Vec<GoogleFunctionDeclaration>>,
}

/// Declaration of a function that the model can call.
///
/// Describes a function including its parameters that the model can invoke.
#[derive(Debug, Serialize)]
pub(super) struct GoogleFunctionDeclaration {
    /// The name of the function to call.
    name: String,

    /// Optional description of what the function does.
    description: Option<String>,

    /// The parameters of this function in JSON Schema format.
    parameters: Option<sonic_rs::Value>,
}

/// Configuration for function calling behavior.
///
/// Controls how the model should use the provided functions.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GoogleToolConfig {
    /// Configuration for function calling.
    function_calling_config: Option<GoogleFunctionCallingConfig>,
}

/// Specifies the mode and allowed functions for function calling.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GoogleFunctionCallingConfig {
    /// The mode of function calling.
    ///
    /// Values include:
    /// - AUTO: Model decides whether to call functions
    /// - ANY: Model is forced to call at least one function
    /// - NONE: Model cannot call functions
    mode: String,

    /// List of function names the model is allowed to call.
    /// If empty, the model can call any provided function.
    allowed_function_names: Option<Vec<String>>,
}

impl From<ChatCompletionRequest> for GoogleGenerateRequest {
    fn from(request: ChatCompletionRequest) -> Self {
        let mut google_contents = Vec::new();
        let mut system_instruction = None;

        for msg in request.messages {
            match &msg.role {
                ChatRole::System => {
                    // Google uses systemInstruction for system messages
                    system_instruction = Some(GoogleContent {
                        parts: vec![GooglePart {
                            text: Some(msg.content),
                        }],
                        role: "user".to_string(), // System instruction role is typically "user"
                    });
                }
                ChatRole::User => {
                    google_contents.push(GoogleContent {
                        parts: vec![GooglePart {
                            text: Some(msg.content),
                        }],
                        role: "user".to_string(),
                    });
                }
                ChatRole::Assistant => {
                    google_contents.push(GoogleContent {
                        parts: vec![GooglePart {
                            text: Some(msg.content),
                        }],
                        role: "model".to_string(), // Google uses "model" instead of "assistant"
                    });
                }
                ChatRole::Other(role) => {
                    log::warn!("Unknown chat role from request: {role}, treating as user");
                    google_contents.push(GoogleContent {
                        parts: vec![GooglePart {
                            text: Some(msg.content),
                        }],
                        role: "user".to_string(),
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

        Self {
            contents: google_contents,
            generation_config: Some(generation_config),
            safety_settings: None,
            tools: None,
            tool_config: None,
            system_instruction,
        }
    }
}
