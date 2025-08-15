//! Streaming support for AWS Bedrock models.
//!
//! This module handles the conversion from AWS EventStream format to Server-Sent Events (SSE)
//! format that is compatible with the OpenAI API.

use anyhow::{Result, anyhow};
use aws_sdk_bedrockruntime::operation::invoke_model_with_response_stream::InvokeModelWithResponseStreamOutput;
use aws_sdk_bedrockruntime::primitives::Blob;
use aws_sdk_bedrockruntime::types::ResponseStream;
use serde::Deserialize;
use sonic_rs::JsonValueMutTrait;

use crate::messages::{
    ChatChoiceDelta, ChatCompletionChunk, ChatCompletionRequest, ChatMessageDelta, ChatRole, FinishReason, ObjectType,
    Usage,
};

/// Metadata for creating a chunk
struct ChunkMetadata<'a> {
    message_id: &'a str,
    model_name: &'a str,
    provider_name: &'a str,
    created: u64,
}

/// Helper function to create a ChatCompletionChunk with common fields
fn create_chunk(
    metadata: &ChunkMetadata<'_>,
    role: Option<ChatRole>,
    content: Option<String>,
    finish_reason: Option<FinishReason>,
    usage: Option<Usage>,
) -> ChatCompletionChunk {
    ChatCompletionChunk {
        id: metadata.message_id.to_string(),
        object: ObjectType::ChatCompletionChunk,
        created: metadata.created,
        model: format!("{}/{}", metadata.provider_name, metadata.model_name),
        system_fingerprint: None,
        choices: vec![ChatChoiceDelta {
            index: 0,
            delta: ChatMessageDelta {
                role,
                content,
                function_call: None,
                tool_calls: None,
            },
            finish_reason,
            logprobs: None,
        }],
        usage,
    }
}
use crate::provider::ChatCompletionStream;

use super::families::ModelFamily;
use super::transform::transform_request;

/// Create a streaming request for Bedrock based on the model family.
pub(super) async fn create_streaming_request(
    client: &aws_sdk_bedrockruntime::Client,
    request: &ChatCompletionRequest,
    family: ModelFamily,
    resolved_model_id: &str,
) -> Result<InvokeModelWithResponseStreamOutput> {
    // Transform the request to the appropriate vendor format
    let mut request_body = transform_request(request, family, resolved_model_id)?;

    // Enable streaming in the request based on model family
    enable_streaming_in_request(&mut request_body, family, resolved_model_id)?;

    // Make the streaming API call to Bedrock
    let result = client
        .invoke_model_with_response_stream()
        .model_id(resolved_model_id)
        .body(request_body)
        .send()
        .await
        .map_err(|e| anyhow!("Failed to invoke streaming model: {e}"))?;

    Ok(result)
}

/// Enable streaming in the request body based on the model family.
fn enable_streaming_in_request(request_body: &mut Blob, family: ModelFamily, resolved_model_id: &str) -> Result<()> {
    let mut json: sonic_rs::Value = sonic_rs::from_slice(request_body.as_ref())?;

    match family {
        ModelFamily::Anthropic => {
            // Anthropic doesn't have a stream field, uses different endpoint
            // No modification needed
        }
        ModelFamily::Amazon => {
            // Titan uses stream field in textGenerationConfig
            #[allow(clippy::collapsible_if)]
            if let Some(config) = json.as_object_mut() {
                if let Some(text_generation_config) = config.get_mut(&"textGenerationConfig".to_string()) {
                    if let Some(text_config_obj) = text_generation_config.as_object_mut() {
                        text_config_obj.insert("stream", sonic_rs::Value::from(true));
                    }
                }
            }
        }
        ModelFamily::Meta => {
            // Llama doesn't have explicit stream field, handled by endpoint
        }
        ModelFamily::Mistral => {
            // Mistral uses stream field at root level
            if let Some(obj) = json.as_object_mut() {
                obj.insert("stream", sonic_rs::Value::from(true));
            }
        }
        ModelFamily::Cohere => {
            // Cohere uses stream field at root level
            if let Some(obj) = json.as_object_mut() {
                obj.insert("stream", sonic_rs::Value::from(true));
            }
        }
        ModelFamily::AI21 => {
            return Err(anyhow!(
                "AI21 models do not support streaming. Use non-streaming chat completion for model: {resolved_model_id}"
            ));
        }
        ModelFamily::Stability => {
            return Err(anyhow!(
                "Stability models are for image generation, not streaming text. Model: {resolved_model_id}"
            ));
        }
    }

    *request_body = Blob::new(sonic_rs::to_vec(&json)?);
    Ok(())
}

/// Convert Bedrock EventStream to OpenAI-compatible SSE stream.
pub(super) fn convert_event_stream_to_sse(
    mut stream: aws_sdk_bedrockruntime::primitives::event_stream::EventReceiver<
        ResponseStream,
        aws_sdk_bedrockruntime::types::error::ResponseStreamError,
    >,
    family: ModelFamily,
    model_name: String,
    provider_name: String,
) -> ChatCompletionStream {
    let stream = async_stream::stream! {
        let message_id = format!("bedrock-{}", uuid::Uuid::new_v4());
        let mut _accumulated_text = String::new();
        let mut is_first_chunk = true;
        let created = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        loop {
            match stream.recv().await {
                Ok(Some(event)) => {
                    match event {
                        ResponseStream::Chunk(chunk) => {
                            let json_data = chunk.bytes.as_ref().map(|b| b.as_ref()).unwrap_or(&[]);

                            match process_chunk(json_data, family, &model_name, &provider_name, &message_id, created, &mut is_first_chunk) {
                                Ok(Some(chunk)) => yield Ok(chunk),
                                Ok(None) => {}, // Skip empty chunks
                                Err(e) => {
                                    log::error!("Error processing Bedrock chunk: {e}");
                                    yield Err(crate::error::LlmError::InternalError(None));
                                    break;
                                }
                            }
                        }
                        _ => {
                            // Unknown event type, skip
                            log::debug!("Unknown Bedrock event type, skipping");
                        }
                    }
                }
                Ok(None) => {
                    // Stream ended
                    break;
                }
                Err(e) => {
                    log::error!("Error receiving from Bedrock stream: {e:?}");
                    yield Err(crate::error::LlmError::ConnectionError(format!("Stream error: {e:?}")));
                    break;
                }
            }
        }

        // Send final chunk with usage stats if available
        // Note: Most Bedrock models don't provide token usage in streaming mode
    };

    Box::pin(stream)
}

/// Process a single chunk from the EventStream based on model family.
fn process_chunk(
    json_data: &[u8],
    family: ModelFamily,
    model_name: &str,
    provider_name: &str,
    message_id: &str,
    created: u64,
    is_first_chunk: &mut bool,
) -> Result<Option<ChatCompletionChunk>> {
    match family {
        ModelFamily::Anthropic => process_anthropic_chunk(
            json_data,
            model_name,
            provider_name,
            message_id,
            created,
            is_first_chunk,
        ),
        ModelFamily::Amazon => process_titan_chunk(
            json_data,
            model_name,
            provider_name,
            message_id,
            created,
            is_first_chunk,
        ),
        ModelFamily::Meta => process_llama_chunk(
            json_data,
            model_name,
            provider_name,
            message_id,
            created,
            is_first_chunk,
        ),
        ModelFamily::Mistral => process_mistral_chunk(
            json_data,
            model_name,
            provider_name,
            message_id,
            created,
            is_first_chunk,
        ),
        ModelFamily::Cohere => process_cohere_chunk(
            json_data,
            model_name,
            provider_name,
            message_id,
            created,
            is_first_chunk,
        ),
        _ => Err(anyhow!(
            "Streaming not supported for model family: {family:?}. Model: {model_name}, Provider: {provider_name}"
        )),
    }
}

// Anthropic streaming chunk structures
#[derive(Debug, Deserialize)]
struct AnthropicStreamChunk {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    delta: Option<AnthropicDelta>,
    #[serde(default)]
    #[allow(dead_code)]
    message: Option<AnthropicMessage>,
    #[serde(default)]
    usage: Option<AnthropicUsage>,
}

#[derive(Debug, Deserialize)]
struct AnthropicDelta {
    #[serde(rename = "type")]
    #[allow(dead_code)]
    delta_type: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicMessage {
    #[serde(default)]
    #[allow(dead_code)]
    usage: Option<AnthropicUsage>,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: Option<u32>,
    output_tokens: Option<u32>,
}

fn process_anthropic_chunk(
    json_data: &[u8],
    model_name: &str,
    provider_name: &str,
    message_id: &str,
    created: u64,
    is_first_chunk: &mut bool,
) -> Result<Option<ChatCompletionChunk>> {
    let metadata = ChunkMetadata {
        message_id,
        model_name,
        provider_name,
        created,
    };

    let chunk: AnthropicStreamChunk = sonic_rs::from_slice(json_data)?;

    match chunk.event_type.as_str() {
        "message_start" => {
            // First chunk with role
            let chunk = create_chunk(&metadata, Some(ChatRole::Assistant), None, None, None);
            *is_first_chunk = false;
            Ok(Some(chunk))
        }
        "content_block_delta" => {
            // Content chunk
            if let Some(delta) = chunk.delta {
                if let Some(text) = delta.text {
                    let chunk = create_chunk(
                        &metadata,
                        if *is_first_chunk {
                            Some(ChatRole::Assistant)
                        } else {
                            None
                        },
                        Some(text),
                        None,
                        None,
                    );
                    *is_first_chunk = false;
                    Ok(Some(chunk))
                } else {
                    Ok(None)
                }
            } else {
                Ok(None)
            }
        }
        "message_delta" => {
            // Final chunk with usage and stop reason
            if let Some(delta) = chunk.delta {
                let finish_reason = delta.stop_reason.map(|sr| match sr.as_str() {
                    "end_turn" => FinishReason::Stop,
                    "max_tokens" => FinishReason::Length,
                    _ => FinishReason::Other(sr),
                });

                let usage = chunk.usage.map(|u| Usage {
                    prompt_tokens: u.input_tokens.unwrap_or(0),
                    completion_tokens: u.output_tokens.unwrap_or(0),
                    total_tokens: u.input_tokens.unwrap_or(0) + u.output_tokens.unwrap_or(0),
                });

                let chunk = create_chunk(&metadata, None, None, finish_reason, usage);
                Ok(Some(chunk))
            } else {
                Ok(None)
            }
        }
        _ => Ok(None), // Skip other event types
    }
}

// Amazon Titan streaming chunk
#[derive(Debug, Deserialize)]
struct TitanStreamChunk {
    #[serde(rename = "outputText")]
    output_text: Option<String>,
    #[serde(rename = "completionReason")]
    completion_reason: Option<String>,
    #[serde(rename = "totalOutputTextTokenCount")]
    #[allow(dead_code)]
    total_output_text_token_count: Option<u32>,
}

fn process_titan_chunk(
    json_data: &[u8],
    model_name: &str,
    provider_name: &str,
    message_id: &str,
    created: u64,
    is_first_chunk: &mut bool,
) -> Result<Option<ChatCompletionChunk>> {
    let metadata = ChunkMetadata {
        message_id,
        model_name,
        provider_name,
        created,
    };

    let chunk: TitanStreamChunk = sonic_rs::from_slice(json_data)?;

    let finish_reason = chunk.completion_reason.as_ref().map(|cr| match cr.as_str() {
        "FINISH" => FinishReason::Stop,
        "LENGTH" => FinishReason::Length,
        "CONTENT_FILTERED" => FinishReason::ContentFilter,
        _ => FinishReason::Other(cr.clone()),
    });

    if let Some(text) = chunk.output_text {
        let chunk = create_chunk(
            &metadata,
            if *is_first_chunk {
                Some(ChatRole::Assistant)
            } else {
                None
            },
            Some(text),
            finish_reason,
            None, // Titan doesn't provide incremental usage in streaming
        );
        *is_first_chunk = false;
        Ok(Some(chunk))
    } else if finish_reason.is_some() {
        // Final chunk with just finish reason
        let chunk = create_chunk(&metadata, None, None, finish_reason, None);
        Ok(Some(chunk))
    } else {
        Ok(None)
    }
}

// Meta Llama streaming chunk
#[derive(Debug, Deserialize)]
struct LlamaStreamChunk {
    generation: Option<String>,
    #[serde(rename = "stop_reason")]
    stop_reason: Option<String>,
    #[serde(rename = "generation_token_count")]
    #[allow(dead_code)]
    generation_token_count: Option<u32>,
}

fn process_llama_chunk(
    json_data: &[u8],
    model_name: &str,
    provider_name: &str,
    message_id: &str,
    created: u64,
    is_first_chunk: &mut bool,
) -> Result<Option<ChatCompletionChunk>> {
    let metadata = ChunkMetadata {
        message_id,
        model_name,
        provider_name,
        created,
    };

    let chunk: LlamaStreamChunk = sonic_rs::from_slice(json_data)?;

    let finish_reason = chunk.stop_reason.as_ref().map(|sr| match sr.as_str() {
        "stop" => FinishReason::Stop,
        "length" => FinishReason::Length,
        _ => FinishReason::Other(sr.clone()),
    });

    if let Some(text) = chunk.generation {
        let chunk = create_chunk(
            &metadata,
            if *is_first_chunk {
                Some(ChatRole::Assistant)
            } else {
                None
            },
            Some(text),
            finish_reason,
            None,
        );
        *is_first_chunk = false;
        Ok(Some(chunk))
    } else if finish_reason.is_some() {
        let chunk = create_chunk(&metadata, None, None, finish_reason, None);
        Ok(Some(chunk))
    } else {
        Ok(None)
    }
}

// Mistral streaming chunk
#[derive(Debug, Deserialize)]
struct MistralStreamChunk {
    outputs: Vec<MistralOutput>,
}

#[derive(Debug, Deserialize)]
struct MistralOutput {
    text: Option<String>,
    stop_reason: Option<String>,
}

fn process_mistral_chunk(
    json_data: &[u8],
    model_name: &str,
    provider_name: &str,
    message_id: &str,
    created: u64,
    is_first_chunk: &mut bool,
) -> Result<Option<ChatCompletionChunk>> {
    let metadata = ChunkMetadata {
        message_id,
        model_name,
        provider_name,
        created,
    };

    let chunk: MistralStreamChunk = sonic_rs::from_slice(json_data)?;

    if let Some(output) = chunk.outputs.first() {
        let finish_reason = output.stop_reason.as_ref().map(|sr| match sr.as_str() {
            "stop" => FinishReason::Stop,
            "length" => FinishReason::Length,
            _ => FinishReason::Other(sr.clone()),
        });

        if let Some(text) = &output.text {
            let chunk = create_chunk(
                &metadata,
                if *is_first_chunk {
                    Some(ChatRole::Assistant)
                } else {
                    None
                },
                Some(text.clone()),
                finish_reason,
                None,
            );
            *is_first_chunk = false;
            Ok(Some(chunk))
        } else if finish_reason.is_some() {
            let chunk = create_chunk(&metadata, None, None, finish_reason, None);
            Ok(Some(chunk))
        } else {
            Ok(None)
        }
    } else {
        Ok(None)
    }
}

// Cohere streaming chunk
#[derive(Debug, Deserialize)]
struct CohereStreamChunk {
    text: Option<String>,
    is_finished: Option<bool>,
    finish_reason: Option<String>,
    #[serde(default)]
    response: Option<CohereResponse>,
}

#[derive(Debug, Deserialize)]
struct CohereResponse {
    #[serde(default)]
    token_count: Option<CohereTokenCount>,
}

#[derive(Debug, Deserialize)]
struct CohereTokenCount {
    prompt_tokens: Option<u32>,
    response_tokens: Option<u32>,
    total_tokens: Option<u32>,
}

fn process_cohere_chunk(
    json_data: &[u8],
    model_name: &str,
    provider_name: &str,
    message_id: &str,
    created: u64,
    is_first_chunk: &mut bool,
) -> Result<Option<ChatCompletionChunk>> {
    let metadata = ChunkMetadata {
        message_id,
        model_name,
        provider_name,
        created,
    };

    let chunk: CohereStreamChunk = sonic_rs::from_slice(json_data)?;

    let finish_reason = if chunk.is_finished.unwrap_or(false) {
        chunk.finish_reason.as_ref().map(|fr| match fr.as_str() {
            "COMPLETE" => FinishReason::Stop,
            "MAX_TOKENS" => FinishReason::Length,
            _ => FinishReason::Other(fr.clone()),
        })
    } else {
        None
    };

    let usage = chunk.response.and_then(|r| r.token_count).map(|tc| Usage {
        prompt_tokens: tc.prompt_tokens.unwrap_or(0),
        completion_tokens: tc.response_tokens.unwrap_or(0),
        total_tokens: tc.total_tokens.unwrap_or(0),
    });

    if let Some(text) = chunk.text {
        let chunk = create_chunk(
            &metadata,
            if *is_first_chunk {
                Some(ChatRole::Assistant)
            } else {
                None
            },
            Some(text),
            finish_reason,
            usage,
        );
        *is_first_chunk = false;
        Ok(Some(chunk))
    } else if finish_reason.is_some() {
        let chunk = create_chunk(&metadata, None, None, finish_reason, usage);
        Ok(Some(chunk))
    } else {
        Ok(None)
    }
}
