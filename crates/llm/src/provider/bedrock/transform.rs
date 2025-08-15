//! Request/response transformation for different Bedrock model families.
//!
//! AWS Bedrock hosts models from multiple vendors, each with its own request/response format.
//! This module provides transformation logic to convert between the unified Nexus format
//! and the vendor-specific formats required by each model family.

use anyhow::{Result, anyhow};
use aws_sdk_bedrockruntime::primitives::Blob;
use serde::{Deserialize, Serialize};

use crate::messages::{
    ChatChoice, ChatCompletionRequest, ChatCompletionResponse, ChatMessage, ChatRole, FinishReason, ObjectType, Usage,
};

use super::families::ModelFamily;

/// Transform a unified chat completion request to the appropriate vendor format for Bedrock.
pub(super) fn transform_request(
    request: &ChatCompletionRequest,
    family: ModelFamily,
    resolved_model_id: &str,
) -> Result<Blob> {
    match family {
        ModelFamily::Anthropic => transform_anthropic_request(request, resolved_model_id),
        ModelFamily::Amazon => transform_titan_request(request, resolved_model_id),
        ModelFamily::Meta => transform_llama_request(request, resolved_model_id),
        ModelFamily::Mistral => transform_mistral_request(request, resolved_model_id),
        ModelFamily::Cohere => transform_cohere_request(request, resolved_model_id),
        ModelFamily::AI21 => Err(anyhow!(
            "AI21 model family not yet implemented for model: {resolved_model_id}"
        )),
        ModelFamily::Stability => Err(anyhow!(
            "Stability AI models are for image generation, not chat completion. Model: {resolved_model_id}"
        )),
    }
}

/// Transform a Bedrock response back to the unified format.
pub(super) fn transform_response(
    response_body: &[u8],
    family: ModelFamily,
    model_name: &str,
) -> Result<ChatCompletionResponse> {
    match family {
        ModelFamily::Anthropic => transform_anthropic_response(response_body, model_name),
        ModelFamily::Amazon => transform_titan_response(response_body, model_name),
        ModelFamily::Meta => transform_llama_response(response_body, model_name),
        ModelFamily::Mistral => transform_mistral_response(response_body, model_name),
        ModelFamily::Cohere => transform_cohere_response(response_body, model_name),
        ModelFamily::AI21 => Err(anyhow!("AI21 model family not yet implemented for model: {model_name}")),
        ModelFamily::Stability => Err(anyhow!(
            "Stability AI models are for image generation, not chat completion. Model: {model_name}"
        )),
    }
}

// Anthropic-specific types for Bedrock
#[derive(Debug, Serialize)]
struct BedrockAnthropicRequest {
    model: String,
    messages: Vec<BedrockAnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop_sequences: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
struct BedrockAnthropicMessage {
    role: ChatRole,
    content: String,
}

#[derive(Debug, Deserialize)]
struct BedrockAnthropicResponse {
    id: String,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    response_type: String,
    role: ChatRole,
    content: Vec<BedrockAnthropicContent>,
    #[allow(dead_code)]
    model: String,
    stop_reason: Option<String>,
    usage: BedrockAnthropicUsage,
}

#[derive(Debug, Deserialize)]
struct BedrockAnthropicContent {
    #[serde(rename = "type")]
    content_type: String,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BedrockAnthropicUsage {
    #[serde(default)]
    input_tokens: i32,
    output_tokens: i32,
}

/// Transform request for Anthropic models (Claude).
///
/// Anthropic models in Bedrock use the same request format as the direct Anthropic API.
fn transform_anthropic_request(request: &ChatCompletionRequest, resolved_model_id: &str) -> Result<Blob> {
    let mut system_message = None;
    let mut anthropic_messages = Vec::new();

    for msg in &request.messages {
        match &msg.role {
            ChatRole::System => {
                system_message = Some(msg.content.clone());
            }
            ChatRole::Assistant | ChatRole::User => {
                anthropic_messages.push(BedrockAnthropicMessage {
                    role: msg.role.clone(),
                    content: msg.content.clone(),
                });
            }
            ChatRole::Other(role) => {
                log::warn!("Unknown chat role from request: {role}, treating as user");
                anthropic_messages.push(BedrockAnthropicMessage {
                    role: ChatRole::User,
                    content: msg.content.clone(),
                });
            }
        }
    }

    let bedrock_request = BedrockAnthropicRequest {
        model: resolved_model_id.to_string(),
        messages: anthropic_messages,
        system: system_message,
        max_tokens: request.max_tokens.unwrap_or(4096),
        temperature: request.temperature,
        top_p: request.top_p,
        stop_sequences: request.stop.clone(),
    };

    // Serialize to JSON and wrap in Blob
    let json_bytes = sonic_rs::to_vec(&bedrock_request)?;
    Ok(Blob::new(json_bytes))
}

/// Transform response from Anthropic models (Claude).
///
/// Anthropic models in Bedrock return the same response format as the direct Anthropic API.
fn transform_anthropic_response(response_body: &[u8], model_name: &str) -> Result<ChatCompletionResponse> {
    // Parse the response using Bedrock Anthropic types
    let bedrock_response: BedrockAnthropicResponse = sonic_rs::from_slice(response_body)?;

    // Extract text content from content blocks
    let message_content = bedrock_response
        .content
        .iter()
        .filter_map(|c| if c.content_type == "text" { c.text.clone() } else { None })
        .collect::<Vec<_>>()
        .join("");

    // Map stop reason to finish reason
    let finish_reason = bedrock_response
        .stop_reason
        .as_deref()
        .map(|sr| match sr {
            "end_turn" => FinishReason::Stop,
            "max_tokens" => FinishReason::Length,
            "stop_sequence" => FinishReason::Stop,
            "tool_use" => FinishReason::ToolCalls,
            other => {
                log::warn!("Unknown stop reason from Bedrock Anthropic: {other}");
                FinishReason::Other(other.to_string())
            }
        })
        .unwrap_or(FinishReason::Stop);

    let response = ChatCompletionResponse {
        id: bedrock_response.id,
        object: ObjectType::ChatCompletion,
        created: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        model: model_name.to_string(),
        choices: vec![ChatChoice {
            index: 0,
            message: ChatMessage {
                role: bedrock_response.role,
                content: message_content,
            },
            finish_reason,
        }],
        usage: Usage {
            prompt_tokens: bedrock_response.usage.input_tokens as u32,
            completion_tokens: bedrock_response.usage.output_tokens as u32,
            total_tokens: (bedrock_response.usage.input_tokens + bedrock_response.usage.output_tokens) as u32,
        },
    };

    Ok(response)
}

// Amazon Titan-specific types for Bedrock
#[derive(Debug, Serialize)]
struct BedrockTitanRequest {
    #[serde(rename = "inputText")]
    input_text: String,
    #[serde(rename = "textGenerationConfig")]
    text_generation_config: TitanTextGenerationConfig,
}

#[derive(Debug, Serialize)]
struct TitanTextGenerationConfig {
    #[serde(rename = "maxTokenCount")]
    max_token_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(rename = "topP", skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(rename = "stopSequences", skip_serializing_if = "Option::is_none")]
    stop_sequences: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct BedrockTitanResponse {
    #[serde(rename = "inputTextTokenCount")]
    input_text_token_count: u32,
    results: Vec<TitanResult>,
}

#[derive(Debug, Deserialize)]
struct TitanResult {
    #[serde(rename = "tokenCount")]
    token_count: u32,
    #[serde(rename = "outputText")]
    output_text: String,
    #[serde(rename = "completionReason")]
    completion_reason: Option<String>,
}

/// Transform request for Amazon Titan models.
fn transform_titan_request(request: &ChatCompletionRequest, _resolved_model_id: &str) -> Result<Blob> {
    // Titan expects a single text prompt, so we concatenate messages
    let mut prompt = String::new();
    for msg in &request.messages {
        match &msg.role {
            ChatRole::System => {
                prompt.push_str(&format!("System: {}\n", msg.content));
            }
            ChatRole::User => {
                prompt.push_str(&format!("User: {}\n", msg.content));
            }
            ChatRole::Assistant => {
                prompt.push_str(&format!("Assistant: {}\n", msg.content));
            }
            ChatRole::Other(role) => {
                prompt.push_str(&format!("{}: {}\n", role, msg.content));
            }
        }
    }
    prompt.push_str("Assistant: ");

    let bedrock_request = BedrockTitanRequest {
        input_text: prompt,
        text_generation_config: TitanTextGenerationConfig {
            max_token_count: request.max_tokens.unwrap_or(4096),
            temperature: request.temperature,
            top_p: request.top_p,
            stop_sequences: request.stop.clone(),
        },
    };

    let json_bytes = sonic_rs::to_vec(&bedrock_request)?;
    Ok(Blob::new(json_bytes))
}

/// Transform response from Amazon Titan models.
fn transform_titan_response(response_body: &[u8], model_name: &str) -> Result<ChatCompletionResponse> {
    let bedrock_response: BedrockTitanResponse = sonic_rs::from_slice(response_body)?;

    let first_result = bedrock_response.results.first().ok_or_else(|| {
        anyhow!(
            "No results in Titan response for model '{model_name}'. Response had {} results",
            bedrock_response.results.len()
        )
    })?;

    let finish_reason = first_result
        .completion_reason
        .as_deref()
        .map(|cr| match cr {
            "FINISH" => FinishReason::Stop,
            "LENGTH" => FinishReason::Length,
            "STOP_CRITERIA_MET" => FinishReason::Stop,
            "CONTENT_FILTERED" => FinishReason::ContentFilter,
            other => {
                log::warn!("Unknown completion reason from Bedrock Titan: {other}");
                FinishReason::Other(other.to_string())
            }
        })
        .unwrap_or(FinishReason::Stop);

    let response = ChatCompletionResponse {
        id: format!("titan-{}", uuid::Uuid::new_v4()),
        object: ObjectType::ChatCompletion,
        created: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        model: model_name.to_string(),
        choices: vec![ChatChoice {
            index: 0,
            message: ChatMessage {
                role: ChatRole::Assistant,
                content: first_result.output_text.clone(),
            },
            finish_reason,
        }],
        usage: Usage {
            prompt_tokens: bedrock_response.input_text_token_count,
            completion_tokens: first_result.token_count,
            total_tokens: bedrock_response.input_text_token_count + first_result.token_count,
        },
    };

    Ok(response)
}

// Meta Llama-specific types for Bedrock
#[derive(Debug, Serialize)]
struct BedrockLlamaRequest {
    prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_gen_len: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct BedrockLlamaResponse {
    generation: String,
    prompt_token_count: Option<u32>,
    generation_token_count: Option<u32>,
    stop_reason: Option<String>,
}

/// Transform request for Meta Llama models.
fn transform_llama_request(request: &ChatCompletionRequest, _resolved_model_id: &str) -> Result<Blob> {
    // Llama uses a specific prompt format
    let mut prompt = String::new();

    // Add system message if present
    let default_system = "You are a helpful assistant.".to_string();
    let system_msg = request
        .messages
        .iter()
        .find(|m| matches!(m.role, ChatRole::System))
        .map(|m| &m.content)
        .unwrap_or(&default_system);

    prompt.push_str(&format!(
        "<|begin_of_text|><|start_header_id|>system<|end_header_id|>\n\n{system_msg}<|eot_id|>"
    ));

    // Add conversation history
    for msg in &request.messages {
        match &msg.role {
            ChatRole::System => {} // Already handled
            ChatRole::User => {
                prompt.push_str(&format!(
                    "<|start_header_id|>user<|end_header_id|>\n\n{content}<|eot_id|>",
                    content = msg.content
                ));
            }
            ChatRole::Assistant => {
                prompt.push_str(&format!(
                    "<|start_header_id|>assistant<|end_header_id|>\n\n{content}<|eot_id|>",
                    content = msg.content
                ));
            }
            ChatRole::Other(role) => {
                log::warn!("Unknown role {role} in Llama request, treating as user");
                prompt.push_str(&format!(
                    "<|start_header_id|>user<|end_header_id|>\n\n{content}<|eot_id|>",
                    content = msg.content
                ));
            }
        }
    }

    prompt.push_str("<|start_header_id|>assistant<|end_header_id|>\n\n");

    let bedrock_request = BedrockLlamaRequest {
        prompt,
        temperature: request.temperature,
        top_p: request.top_p,
        max_gen_len: request.max_tokens,
    };

    let json_bytes = sonic_rs::to_vec(&bedrock_request)?;
    Ok(Blob::new(json_bytes))
}

/// Transform response from Meta Llama models.
fn transform_llama_response(response_body: &[u8], model_name: &str) -> Result<ChatCompletionResponse> {
    let bedrock_response: BedrockLlamaResponse = sonic_rs::from_slice(response_body)?;

    let finish_reason = bedrock_response
        .stop_reason
        .as_deref()
        .map(|sr| match sr {
            "stop" => FinishReason::Stop,
            "length" => FinishReason::Length,
            other => {
                log::warn!("Unknown stop reason from Bedrock Llama: {other}");
                FinishReason::Other(other.to_string())
            }
        })
        .unwrap_or(FinishReason::Stop);

    let response = ChatCompletionResponse {
        id: format!("llama-{}", uuid::Uuid::new_v4()),
        object: ObjectType::ChatCompletion,
        created: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        model: model_name.to_string(),
        choices: vec![ChatChoice {
            index: 0,
            message: ChatMessage {
                role: ChatRole::Assistant,
                content: bedrock_response.generation,
            },
            finish_reason,
        }],
        usage: Usage {
            prompt_tokens: bedrock_response.prompt_token_count.unwrap_or(0),
            completion_tokens: bedrock_response.generation_token_count.unwrap_or(0),
            total_tokens: bedrock_response.prompt_token_count.unwrap_or(0)
                + bedrock_response.generation_token_count.unwrap_or(0),
        },
    };

    Ok(response)
}

// Mistral-specific types for Bedrock
#[derive(Debug, Serialize)]
struct BedrockMistralRequest {
    prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct BedrockMistralResponse {
    outputs: Vec<MistralOutput>,
}

#[derive(Debug, Deserialize)]
struct MistralOutput {
    text: String,
    stop_reason: Option<String>,
}

/// Transform request for Mistral models.
fn transform_mistral_request(request: &ChatCompletionRequest, _resolved_model_id: &str) -> Result<Blob> {
    // Mistral uses instruction format
    let mut prompt = String::new();

    for msg in &request.messages {
        match &msg.role {
            ChatRole::System => {
                prompt.push_str(&format!("[INST] {} [/INST]\n", msg.content));
            }
            ChatRole::User => {
                prompt.push_str(&format!("[INST] {} [/INST]\n", msg.content));
            }
            ChatRole::Assistant => {
                prompt.push_str(&format!("{}\n", msg.content));
            }
            ChatRole::Other(role) => {
                log::warn!("Unknown role {role} in Mistral request, treating as user");
                prompt.push_str(&format!("[INST] {} [/INST]\n", msg.content));
            }
        }
    }

    let bedrock_request = BedrockMistralRequest {
        prompt,
        max_tokens: request.max_tokens,
        temperature: request.temperature,
        top_p: request.top_p,
        stop: request.stop.clone(),
    };

    let json_bytes = sonic_rs::to_vec(&bedrock_request)?;
    Ok(Blob::new(json_bytes))
}

/// Transform response from Mistral models.
fn transform_mistral_response(response_body: &[u8], model_name: &str) -> Result<ChatCompletionResponse> {
    let bedrock_response: BedrockMistralResponse = sonic_rs::from_slice(response_body)?;

    let first_output = bedrock_response.outputs.first().ok_or_else(|| {
        anyhow!(
            "No outputs in Mistral response for model '{model_name}'. Response had {} outputs",
            bedrock_response.outputs.len()
        )
    })?;

    let finish_reason = first_output
        .stop_reason
        .as_deref()
        .map(|sr| match sr {
            "stop" => FinishReason::Stop,
            "length" => FinishReason::Length,
            other => {
                log::warn!("Unknown stop reason from Bedrock Mistral: {other}");
                FinishReason::Other(other.to_string())
            }
        })
        .unwrap_or(FinishReason::Stop);

    let response = ChatCompletionResponse {
        id: format!("mistral-{}", uuid::Uuid::new_v4()),
        object: ObjectType::ChatCompletion,
        created: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        model: model_name.to_string(),
        choices: vec![ChatChoice {
            index: 0,
            message: ChatMessage {
                role: ChatRole::Assistant,
                content: first_output.text.clone(),
            },
            finish_reason,
        }],
        usage: Usage {
            prompt_tokens: 0, // Mistral doesn't provide token counts
            completion_tokens: 0,
            total_tokens: 0,
        },
    };

    Ok(response)
}

// Cohere-specific types for Bedrock
#[derive(Debug, Serialize)]
struct BedrockCohereRequest {
    prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop_sequences: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    return_likelihoods: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BedrockCohereResponse {
    generations: Vec<CohereGeneration>,
    #[serde(default)]
    #[allow(dead_code)]
    prompt: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CohereGeneration {
    text: String,
    #[serde(default)]
    finish_reason: Option<String>,
}

/// Transform request for Cohere models.
fn transform_cohere_request(request: &ChatCompletionRequest, _resolved_model_id: &str) -> Result<Blob> {
    // Cohere expects a simple prompt
    let mut prompt = String::new();

    for msg in &request.messages {
        match &msg.role {
            ChatRole::System => {
                prompt.push_str(&format!("System: {}\n", msg.content));
            }
            ChatRole::User => {
                prompt.push_str(&format!("User: {}\n", msg.content));
            }
            ChatRole::Assistant => {
                prompt.push_str(&format!("Assistant: {}\n", msg.content));
            }
            ChatRole::Other(role) => {
                prompt.push_str(&format!("{}: {}\n", role, msg.content));
            }
        }
    }
    prompt.push_str("Assistant: ");

    let bedrock_request = BedrockCohereRequest {
        prompt,
        max_tokens: request.max_tokens,
        temperature: request.temperature,
        p: request.top_p,
        stop_sequences: request.stop.clone(),
        return_likelihoods: None,
    };

    let json_bytes = sonic_rs::to_vec(&bedrock_request)?;
    Ok(Blob::new(json_bytes))
}

/// Transform response from Cohere models.
fn transform_cohere_response(response_body: &[u8], model_name: &str) -> Result<ChatCompletionResponse> {
    let bedrock_response: BedrockCohereResponse = sonic_rs::from_slice(response_body)?;

    let first_generation = bedrock_response.generations.first().ok_or_else(|| {
        anyhow!(
            "No generations in Cohere response for model '{model_name}'. Response had {} generations",
            bedrock_response.generations.len()
        )
    })?;

    let finish_reason = first_generation
        .finish_reason
        .as_deref()
        .map(|fr| match fr {
            "COMPLETE" => FinishReason::Stop,
            "MAX_TOKENS" => FinishReason::Length,
            "STOP_SEQUENCE" => FinishReason::Stop,
            other => {
                log::warn!("Unknown finish reason from Bedrock Cohere: {other}");
                FinishReason::Other(other.to_string())
            }
        })
        .unwrap_or(FinishReason::Stop);

    let response = ChatCompletionResponse {
        id: format!("cohere-{}", uuid::Uuid::new_v4()),
        object: ObjectType::ChatCompletion,
        created: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        model: model_name.to_string(),
        choices: vec![ChatChoice {
            index: 0,
            message: ChatMessage {
                role: ChatRole::Assistant,
                content: first_generation.text.clone(),
            },
            finish_reason,
        }],
        usage: Usage {
            prompt_tokens: 0, // Cohere doesn't provide token counts in this format
            completion_tokens: 0,
            total_tokens: 0,
        },
    };

    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sonic_rs::{JsonContainerTrait, JsonValueTrait};

    #[test]
    fn anthropic_request_transformation() {
        let request = ChatCompletionRequest {
            model: "anthropic.claude-3-sonnet-20240229-v1:0".to_string(),
            messages: vec![ChatMessage {
                role: ChatRole::User,
                content: "Hello, how are you?".to_string(),
            }],
            temperature: Some(0.7),
            max_tokens: Some(100),
            top_p: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            stream: None,
        };

        let result = transform_anthropic_request(&request, "anthropic.claude-3-sonnet-20240229-v1:0");
        assert!(result.is_ok());

        let blob = result.unwrap();
        let json_str = std::str::from_utf8(blob.as_ref()).unwrap();

        // Verify it's valid JSON and contains expected fields
        let parsed: sonic_rs::Value = sonic_rs::from_str(json_str).unwrap();
        assert_eq!(
            parsed["model"].as_str().unwrap(),
            "anthropic.claude-3-sonnet-20240229-v1:0"
        );
        assert_eq!(
            parsed["messages"][0]["content"].as_str().unwrap(),
            "Hello, how are you?"
        );
        assert_eq!(parsed["max_tokens"].as_u64().unwrap(), 100);
        assert_eq!(parsed["temperature"].as_f64().unwrap(), 0.7);
    }

    #[test]
    fn anthropic_response_transformation() {
        let mock_response = r#"{
            "id": "msg_01XFDUDYJgAACzvnptvVoYEL",
            "type": "message",
            "role": "assistant",
            "content": [
                {
                    "type": "text",
                    "text": "Hello! I'm doing well, thank you for asking."
                }
            ],
            "model": "claude-3-sonnet-20240229",
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 12,
                "output_tokens": 15
            }
        }"#;

        let result = transform_anthropic_response(
            mock_response.as_bytes(),
            "bedrock/anthropic.claude-3-sonnet-20240229-v1:0",
        );
        assert!(result.is_ok());

        let response = result.unwrap();
        assert_eq!(response.model, "bedrock/anthropic.claude-3-sonnet-20240229-v1:0");
        assert_eq!(response.choices.len(), 1);
        assert_eq!(
            response.choices[0].message.content,
            "Hello! I'm doing well, thank you for asking."
        );
        assert_eq!(response.usage.prompt_tokens, 12);
        assert_eq!(response.usage.completion_tokens, 15);
    }

    #[test]
    fn system_message_handling() {
        let request = ChatCompletionRequest {
            model: "anthropic.claude-3-sonnet-20240229-v1:0".to_string(),
            messages: vec![
                ChatMessage {
                    role: ChatRole::System,
                    content: "You are a helpful assistant.".to_string(),
                },
                ChatMessage {
                    role: ChatRole::User,
                    content: "Hello!".to_string(),
                },
            ],
            temperature: None,
            max_tokens: None,
            top_p: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            stream: None,
        };

        let result = transform_anthropic_request(&request, "anthropic.claude-3-sonnet-20240229-v1:0");
        assert!(result.is_ok());

        let blob = result.unwrap();
        let json_str = std::str::from_utf8(blob.as_ref()).unwrap();

        let parsed: sonic_rs::Value = sonic_rs::from_str(json_str).unwrap();
        assert_eq!(parsed["system"].as_str().unwrap(), "You are a helpful assistant.");
        assert_eq!(parsed["messages"].as_array().unwrap().len(), 1);
        assert_eq!(parsed["messages"][0]["content"].as_str().unwrap(), "Hello!");
    }

    #[test]
    fn titan_request_transformation() {
        let request = ChatCompletionRequest {
            model: "amazon.titan-text-express-v1".to_string(),
            messages: vec![ChatMessage {
                role: ChatRole::User,
                content: "Hello, world!".to_string(),
            }],
            temperature: Some(0.5),
            max_tokens: Some(200),
            top_p: Some(0.9),
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            stream: None,
        };

        let result = transform_titan_request(&request, "amazon.titan-text-express-v1");
        assert!(result.is_ok());

        let blob = result.unwrap();
        let json_str = std::str::from_utf8(blob.as_ref()).unwrap();
        let parsed: sonic_rs::Value = sonic_rs::from_str(json_str).unwrap();

        // Verify Titan-specific structure
        assert!(parsed["inputText"].is_str());
        assert!(parsed["textGenerationConfig"].is_object());
        assert_eq!(parsed["textGenerationConfig"]["temperature"].as_f64().unwrap(), 0.5);
        assert_eq!(parsed["textGenerationConfig"]["maxTokenCount"].as_u64().unwrap(), 200);
        assert_eq!(parsed["textGenerationConfig"]["topP"].as_f64().unwrap(), 0.9);
    }

    #[test]
    fn titan_response_transformation() {
        let response_json = r#"
        {
            "inputTextTokenCount": 10,
            "results": [
                {
                    "tokenCount": 25,
                    "outputText": "Hello! How can I help you today?",
                    "completionReason": "FINISH"
                }
            ]
        }
        "#;

        let result = transform_titan_response(response_json.as_bytes(), "amazon/titan-express");
        assert!(result.is_ok());

        let response = result.unwrap();
        assert_eq!(response.model, "amazon/titan-express");
        assert_eq!(response.choices.len(), 1);
        assert_eq!(response.choices[0].message.content, "Hello! How can I help you today?");
        assert_eq!(response.choices[0].finish_reason, FinishReason::Stop);
        assert_eq!(response.usage.prompt_tokens, 10);
        assert_eq!(response.usage.completion_tokens, 25);
        assert_eq!(response.usage.total_tokens, 35);
    }

    #[test]
    fn llama_request_transformation() {
        let request = ChatCompletionRequest {
            model: "meta.llama3-70b-instruct-v1:0".to_string(),
            messages: vec![
                ChatMessage {
                    role: ChatRole::System,
                    content: "You are a helpful assistant.".to_string(),
                },
                ChatMessage {
                    role: ChatRole::User,
                    content: "What's 2+2?".to_string(),
                },
            ],
            temperature: Some(0.1),
            max_tokens: Some(512),
            top_p: Some(0.95),
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            stream: None,
        };

        let result = transform_llama_request(&request, "meta.llama3-70b-instruct-v1:0");
        assert!(result.is_ok());

        let blob = result.unwrap();
        let json_str = std::str::from_utf8(blob.as_ref()).unwrap();
        let parsed: sonic_rs::Value = sonic_rs::from_str(json_str).unwrap();

        // Verify Llama-specific structure
        assert!(parsed["prompt"].is_str());
        let prompt = parsed["prompt"].as_str().unwrap();
        assert!(prompt.contains("<|begin_of_text|>"));
        assert!(prompt.contains("<|start_header_id|>system<|end_header_id|>"));
        assert!(prompt.contains("You are a helpful assistant."));
        assert!(prompt.contains("<|start_header_id|>user<|end_header_id|>"));
        assert!(prompt.contains("What's 2+2?"));

        assert_eq!(parsed["temperature"].as_f64().unwrap(), 0.1);
        assert_eq!(parsed["max_gen_len"].as_u64().unwrap(), 512);
        assert_eq!(parsed["top_p"].as_f64().unwrap(), 0.95);
    }

    #[test]
    fn llama_response_transformation() {
        let response_json = r#"
        {
            "generation": "2+2 equals 4.",
            "prompt_token_count": 25,
            "generation_token_count": 8,
            "stop_reason": "stop"
        }
        "#;

        let result = transform_llama_response(response_json.as_bytes(), "meta/llama3-70b");
        assert!(result.is_ok());

        let response = result.unwrap();
        assert_eq!(response.model, "meta/llama3-70b");
        assert_eq!(response.choices.len(), 1);
        assert_eq!(response.choices[0].message.content, "2+2 equals 4.");
        assert_eq!(response.choices[0].finish_reason, FinishReason::Stop);
        assert_eq!(response.usage.prompt_tokens, 25);
        assert_eq!(response.usage.completion_tokens, 8);
        assert_eq!(response.usage.total_tokens, 33);
    }

    #[test]
    fn mistral_request_transformation() {
        let request = ChatCompletionRequest {
            model: "mistral.mistral-7b-instruct-v0:2".to_string(),
            messages: vec![ChatMessage {
                role: ChatRole::User,
                content: "Explain quantum computing".to_string(),
            }],
            temperature: Some(0.8),
            max_tokens: Some(1000),
            top_p: Some(0.7),
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            stream: None,
        };

        let result = transform_mistral_request(&request, "mistral.mistral-7b-instruct-v0:2");
        assert!(result.is_ok());

        let blob = result.unwrap();
        let json_str = std::str::from_utf8(blob.as_ref()).unwrap();
        let parsed: sonic_rs::Value = sonic_rs::from_str(json_str).unwrap();

        // Verify Mistral-specific structure
        assert!(parsed["prompt"].is_str());
        let prompt = parsed["prompt"].as_str().unwrap();
        assert!(prompt.contains("[INST]"));
        assert!(prompt.contains("Explain quantum computing"));
        assert!(prompt.contains("[/INST]"));

        assert_eq!(parsed["temperature"].as_f64().unwrap(), 0.8);
        assert_eq!(parsed["max_tokens"].as_u64().unwrap(), 1000);
        assert_eq!(parsed["top_p"].as_f64().unwrap(), 0.7);
    }

    #[test]
    fn mistral_response_transformation() {
        let response_json = r#"
        {
            "outputs": [
                {
                    "text": "Quantum computing is a revolutionary technology...",
                    "stop_reason": "stop"
                }
            ]
        }
        "#;

        let result = transform_mistral_response(response_json.as_bytes(), "mistral/mistral-7b");
        assert!(result.is_ok());

        let response = result.unwrap();
        assert_eq!(response.model, "mistral/mistral-7b");
        assert_eq!(response.choices.len(), 1);
        assert_eq!(
            response.choices[0].message.content,
            "Quantum computing is a revolutionary technology..."
        );
        assert_eq!(response.choices[0].finish_reason, FinishReason::Stop);
    }

    #[test]
    fn cohere_request_transformation() {
        let request = ChatCompletionRequest {
            model: "cohere.command-text-v14".to_string(),
            messages: vec![ChatMessage {
                role: ChatRole::User,
                content: "Write a haiku about clouds".to_string(),
            }],
            temperature: Some(0.9),
            max_tokens: Some(100),
            top_p: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop: Some(vec!["END".to_string()]),
            stream: None,
        };

        let result = transform_cohere_request(&request, "cohere.command-text-v14");
        assert!(result.is_ok());

        let blob = result.unwrap();
        let json_str = std::str::from_utf8(blob.as_ref()).unwrap();
        let parsed: sonic_rs::Value = sonic_rs::from_str(json_str).unwrap();

        // Verify Cohere-specific structure
        assert!(parsed["prompt"].is_str());
        let prompt = parsed["prompt"].as_str().unwrap();
        assert!(prompt.contains("User: Write a haiku about clouds"));

        assert_eq!(parsed["temperature"].as_f64().unwrap(), 0.9);
        assert_eq!(parsed["max_tokens"].as_u64().unwrap(), 100);
        assert_eq!(parsed["stop_sequences"].as_array().unwrap()[0].as_str().unwrap(), "END");
    }

    #[test]
    fn cohere_response_transformation() {
        let response_json = r#"
        {
            "generations": [
                {
                    "text": "Clouds drift above,\nSoft whispers in the blue sky,\nNature's gentle dance.",
                    "finish_reason": "COMPLETE"
                }
            ]
        }
        "#;

        let result = transform_cohere_response(response_json.as_bytes(), "cohere/command-text");
        assert!(result.is_ok());

        let response = result.unwrap();
        assert_eq!(response.model, "cohere/command-text");
        assert_eq!(response.choices.len(), 1);
        assert_eq!(
            response.choices[0].message.content,
            "Clouds drift above,\nSoft whispers in the blue sky,\nNature's gentle dance."
        );
        assert_eq!(response.choices[0].finish_reason, FinishReason::Stop);
    }

    #[test]
    fn empty_message_array() {
        let request = ChatCompletionRequest {
            model: "anthropic.claude-3-sonnet-20240229-v1:0".to_string(),
            messages: vec![], // Empty messages
            temperature: None,
            max_tokens: None,
            top_p: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            stream: None,
        };

        let result = transform_anthropic_request(&request, "anthropic.claude-3-sonnet-20240229-v1:0");
        // Should handle gracefully - Anthropic requires at least one message
        // The actual behavior may vary, but it should not panic
        let _result = result; // Don't assert specific behavior, just ensure no panic
    }

    #[test]
    fn unknown_message_role() {
        let request = ChatCompletionRequest {
            model: "anthropic.claude-3-sonnet-20240229-v1:0".to_string(),
            messages: vec![ChatMessage {
                role: ChatRole::Other("custom_role".to_string()),
                content: "Hello with custom role".to_string(),
            }],
            temperature: None,
            max_tokens: None,
            top_p: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            stream: None,
        };

        let result = transform_anthropic_request(&request, "anthropic.claude-3-sonnet-20240229-v1:0");
        // Should handle unknown roles gracefully (likely mapping to 'user')
        assert!(result.is_ok());
    }

    #[test]
    fn malformed_response_handling() {
        let malformed_json = r#"{"invalid": "json structure"#; // Missing closing brace

        let result = transform_titan_response(malformed_json.as_bytes(), "amazon/titan-express");
        // Should return an error for malformed JSON
        assert!(result.is_err());
    }

    #[test]
    fn missing_required_response_fields() {
        // Titan response with empty results array
        let incomplete_json = r#"
        {
            "inputTextTokenCount": 10,
            "results": []
        }
        "#;

        let result = transform_titan_response(incomplete_json.as_bytes(), "amazon/titan-express");
        // Should return an error due to missing required field
        assert!(result.is_err());

        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("No results in Titan response"));
        assert!(error_msg.contains("amazon/titan-express"));
    }

    #[test]
    fn parameter_boundary_values() {
        let request = ChatCompletionRequest {
            model: "anthropic.claude-3-sonnet-20240229-v1:0".to_string(),
            messages: vec![ChatMessage {
                role: ChatRole::User,
                content: "Test".to_string(),
            }],
            temperature: Some(0.0), // Minimum temperature
            max_tokens: Some(1),    // Minimum max_tokens
            top_p: Some(1.0),       // Maximum top_p
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            stream: None,
        };

        let result = transform_anthropic_request(&request, "anthropic.claude-3-sonnet-20240229-v1:0");
        assert!(result.is_ok());

        let blob = result.unwrap();
        let json_str = std::str::from_utf8(blob.as_ref()).unwrap();
        let parsed: sonic_rs::Value = sonic_rs::from_str(json_str).unwrap();

        assert_eq!(parsed["temperature"].as_f64().unwrap(), 0.0);
        assert_eq!(parsed["max_tokens"].as_u64().unwrap(), 1);
        assert_eq!(parsed["top_p"].as_f64().unwrap(), 1.0);
    }
}
