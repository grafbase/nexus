mod input;
mod output;

use async_trait::async_trait;
use axum::http::HeaderMap;
use config::OpenAiConfig;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use reqwest::{Client, header::AUTHORIZATION};
use secrecy::ExposeSecret;

use self::{
    input::OpenAIRequest,
    output::{OpenAIModelsResponse, OpenAIResponse, OpenAIStreamChunk},
};

use crate::{
    error::LlmError,
    messages::{ChatCompletionRequest, ChatCompletionResponse, Model},
    provider::{ChatCompletionStream, Provider},
};

const DEFAULT_OPENAI_API_URL: &str = "https://api.openai.com/v1";

pub(crate) struct OpenAIProvider {
    client: Client,
    base_url: String,
    name: String,
}

impl OpenAIProvider {
    pub fn new(name: String, config: OpenAiConfig) -> crate::Result<Self> {
        let mut headers = HeaderMap::new();

        headers.insert(
            AUTHORIZATION,
            format!("Bearer {}", config.api_key.expose_secret())
                .parse()
                .map_err(|e| {
                    log::error!("Failed to parse authorization header for OpenAI provider: {e}");
                    LlmError::InternalError(None)
                })?,
        );

        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .default_headers(headers)
            .build()
            .map_err(|e| {
                log::error!("Failed to create HTTP client for OpenAI provider: {e}");
                LlmError::InternalError(None)
            })?;

        // Use custom base URL if provided, otherwise use default
        let base_url = config
            .base_url
            .clone()
            .unwrap_or_else(|| DEFAULT_OPENAI_API_URL.to_string());

        Ok(Self { client, base_url, name })
    }
}

#[async_trait]
impl Provider for OpenAIProvider {
    async fn chat_completion(&self, request: ChatCompletionRequest) -> crate::Result<ChatCompletionResponse> {
        // Note: Streaming is handled by chat_completion_stream()

        let url = format!("{}/chat/completions", self.base_url);

        // Convert our request to OpenAI format
        let model_name = extract_model_from_full_name(&request.model);
        let original_model = request.model.clone();

        let mut openai_request = OpenAIRequest::from(request);
        openai_request.model = model_name;
        openai_request.stream = false; // Always false for now

        let response = self
            .client
            .post(&url)
            .json(&openai_request)
            .send()
            .await
            .map_err(|e| LlmError::ConnectionError(format!("Failed to send request to OpenAI: {e}")))?;

        let status = response.status();

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            log::error!("OpenAI API error ({status}): {error_text}");

            return Err(match status.as_u16() {
                401 => LlmError::AuthenticationFailed(error_text),
                403 => LlmError::InsufficientQuota(error_text),
                404 => LlmError::ModelNotFound(error_text),
                429 => LlmError::RateLimitExceeded(error_text),
                400 => LlmError::InvalidRequest(error_text),
                500 => LlmError::InternalError(Some(error_text)),
                _ => LlmError::ProviderApiError {
                    status: status.as_u16(),
                    message: error_text,
                },
            });
        }

        // First get the response as text to log if parsing fails
        let response_text = response.text().await.map_err(|e| {
            log::error!("Failed to read OpenAI response body: {e}");
            LlmError::InternalError(None)
        })?;

        // Try to parse the response
        let openai_response: OpenAIResponse = sonic_rs::from_str(&response_text).map_err(|e| {
            log::error!("Failed to parse OpenAI chat completion response: {e}");
            log::error!("Raw response that failed to parse: {response_text}");
            LlmError::InternalError(None)
        })?;

        let mut response = ChatCompletionResponse::from(openai_response);
        response.model = original_model;
        Ok(response)
    }

    async fn list_models(&self) -> crate::Result<Vec<Model>> {
        let url = format!("{}/models", self.base_url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| LlmError::ConnectionError(format!("Failed to fetch models from OpenAI: {e}")))?;

        let status = response.status();

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            log::error!("OpenAI API error ({status}): {error_text}");

            return Err(match status.as_u16() {
                400 => LlmError::InvalidRequest(error_text),
                401 => LlmError::AuthenticationFailed(error_text),
                _ => LlmError::ProviderApiError {
                    status: status.as_u16(),
                    message: error_text,
                },
            });
        }

        // First get the response as text to log if parsing fails
        let response_text = response.text().await.map_err(|e| {
            log::error!("Failed to read OpenAI models response body: {e}");
            LlmError::InternalError(None)
        })?;

        // Try to parse the response
        let models_response: OpenAIModelsResponse = sonic_rs::from_str(&response_text).map_err(|e| {
            log::error!("Failed to parse OpenAI models list response: {e}");
            log::error!("Raw response that failed to parse: {response_text}");
            LlmError::InternalError(None)
        })?;

        // Filter to only chat-compatible models that work with the chat/completions endpoint
        // Exclude models that:
        // - Don't support chat format (completion models, embeddings, etc.)
        // - Are for different modalities (audio, image)
        // - Have specialized purposes incompatible with chat (search, moderation)
        let total_models = models_response.data.len();
        let chat_models = models_response
            .data
            .into_iter()
            .filter(|model| {
                let id = &model.id;

                // Exclude non-chat model types
                if id.contains("embedding")
                    || id.contains("whisper")
                    || id.contains("tts")
                    || id.contains("dall-e")
                    || id.contains("davinci")
                    || id.contains("curie")
                    || id.contains("babbage")
                    || id.contains("ada")
                    || id.contains("search")
                    || id.contains("text-similarity")
                    || id.contains("code-search")
                    || id.contains("audio")
                    || id.contains("moderation")
                    || id.contains("realtime")
                {
                    log::debug!("Filtering out non-chat model: {id}");
                    return false;
                }

                // Include known chat models
                let is_chat =
                    id.starts_with("gpt") || id.starts_with("o1") || id.starts_with("chatgpt") || id.contains("turbo");

                if !is_chat {
                    log::debug!("Filtering out unknown model type: {id}");
                }

                is_chat
            })
            .map(Into::into)
            .collect::<Vec<Model>>();

        log::debug!(
            "OpenAI models: filtered {}/{} models to {} chat-compatible models",
            total_models - chat_models.len(),
            total_models,
            chat_models.len()
        );

        Ok(chat_models)
    }

    async fn chat_completion_stream(&self, request: ChatCompletionRequest) -> crate::Result<ChatCompletionStream> {
        let url = format!("{}/chat/completions", self.base_url);

        let mut openai_request = OpenAIRequest::from(request);
        openai_request.stream = true;

        let response = self
            .client
            .post(&url)
            .json(&openai_request)
            .send()
            .await
            .map_err(|e| LlmError::ConnectionError(format!("Failed to send streaming request to OpenAI: {e}")))?;

        let status = response.status();

        // Check for HTTP errors before attempting to stream
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            log::error!("OpenAI streaming API error ({status}): {error_text}");

            return Err(match status.as_u16() {
                401 => LlmError::AuthenticationFailed(error_text),
                403 => LlmError::InsufficientQuota(error_text),
                404 => LlmError::ModelNotFound(error_text),
                429 => LlmError::RateLimitExceeded(error_text),
                400 => LlmError::InvalidRequest(error_text),
                500 => LlmError::InternalError(Some(error_text)),
                _ => LlmError::ProviderApiError {
                    status: status.as_u16(),
                    message: error_text,
                },
            });
        }

        // Convert response bytes stream to SSE event stream
        let byte_stream = response.bytes_stream();
        let event_stream = byte_stream.eventsource();
        let provider_name = self.name.clone();

        // Transform the SSE event stream into ChatCompletionChunk stream
        let chunk_stream = event_stream.filter_map(move |event| {
            let provider = provider_name.clone();

            async move {
                // Handle SSE parsing errors
                let Ok(event) = event else {
                    // SSE parsing error - log and skip
                    log::warn!("SSE parsing error in OpenAI stream");
                    return None;
                };

                // Check for end marker
                if event.data == "[DONE]" {
                    return None;
                }

                // Parse the JSON chunk
                let Ok(chunk) = sonic_rs::from_str::<OpenAIStreamChunk<'_>>(&event.data) else {
                    log::warn!("Failed to parse OpenAI streaming chunk");
                    return None;
                };

                Some(Ok(chunk.into_chunk(&provider)))
            }
        });

        Ok(Box::pin(chunk_stream))
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// Extract the model name from a full provider/model string.
pub(super) fn extract_model_from_full_name(full_name: &str) -> String {
    full_name.split('/').next_back().unwrap_or(full_name).to_string()
}

// OpenAI API request/response types
