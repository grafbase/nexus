mod input;
mod output;

use async_trait::async_trait;
use axum::http::HeaderMap;
use config::AnthropicConfig;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use reqwest::Client;
use secrecy::ExposeSecret;

use self::{
    input::AnthropicRequest,
    output::{AnthropicModelsResponse, AnthropicResponse, AnthropicStreamEvent, AnthropicStreamProcessor},
};

use crate::{
    error::LlmError,
    messages::{ChatCompletionRequest, ChatCompletionResponse, Model},
    provider::Provider,
};

const DEFAULT_ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1";
const ANTHROPIC_VERSION: &str = "2023-06-01";

pub(crate) struct AnthropicProvider {
    client: Client,
    base_url: String,
    name: String,
}

impl AnthropicProvider {
    pub fn new(name: String, config: AnthropicConfig) -> crate::Result<Self> {
        let mut headers = HeaderMap::new();

        headers.insert(
            "x-api-key",
            config.api_key.expose_secret().parse().map_err(|e| {
                log::error!("Failed to parse API key header for Anthropic provider: {e}");
                LlmError::InternalError(None)
            })?,
        );

        headers.insert(
            "anthropic-version",
            ANTHROPIC_VERSION.parse().map_err(|e| {
                log::error!("Failed to parse Anthropic version header: {e}");
                LlmError::InternalError(None)
            })?,
        );

        headers.insert(
            "content-type",
            "application/json".parse().map_err(|e| {
                log::error!("Failed to parse content-type header for Anthropic provider: {e}");
                LlmError::InternalError(None)
            })?,
        );

        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .default_headers(headers)
            .build()
            .map_err(|e| {
                log::error!("Failed to create HTTP client for Anthropic provider: {e}");
                LlmError::InternalError(None)
            })?;

        let base_url = config
            .base_url
            .clone()
            .unwrap_or_else(|| DEFAULT_ANTHROPIC_API_URL.to_string());

        Ok(Self { client, base_url, name })
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    async fn chat_completion(&self, request: ChatCompletionRequest) -> crate::Result<ChatCompletionResponse> {
        let url = format!("{}/messages", self.base_url);

        // Store the original model name
        let original_model = request.model.clone();

        // Convert to Anthropic format
        let anthropic_request = AnthropicRequest::from(request);

        let response = self
            .client
            .post(&url)
            .json(&anthropic_request)
            .send()
            .await
            .map_err(|e| LlmError::ConnectionError(format!("Failed to send request to Anthropic: {e}")))?;

        let status = response.status();

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            log::error!("Anthropic API error ({status}): {error_text}");

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
            log::error!("Failed to read Anthropic response body: {e}");
            LlmError::InternalError(None)
        })?;

        // Try to parse the response
        let anthropic_response: AnthropicResponse = sonic_rs::from_str(&response_text).map_err(|e| {
            log::error!("Failed to parse Anthropic chat completion response: {e}");
            log::error!("Raw response that failed to parse: {response_text}");
            LlmError::InternalError(None)
        })?;

        let mut response = ChatCompletionResponse::from(anthropic_response);
        response.model = original_model;

        Ok(response)
    }

    async fn list_models(&self) -> Result<Vec<Model>, LlmError> {
        let url = format!("{}/models", self.base_url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| LlmError::ConnectionError(format!("Failed to fetch models from Anthropic: {e}")))?;

        let status = response.status();

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            log::error!("Anthropic API error ({status}): {error_text}");

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
            log::error!("Failed to read Anthropic models response body: {e}");
            LlmError::InternalError(None)
        })?;

        // Try to parse the response
        let models_response: AnthropicModelsResponse = sonic_rs::from_str(&response_text).map_err(|e| {
            log::error!("Failed to parse Anthropic models list response: {e}");
            log::error!("Raw response that failed to parse: {response_text}");
            LlmError::InternalError(None)
        })?;

        Ok(models_response.data.into_iter().map(Into::into).collect())
    }

    async fn chat_completion_stream(
        &self,
        request: ChatCompletionRequest,
    ) -> crate::Result<crate::provider::ChatCompletionStream> {
        let url = format!("{}/messages", self.base_url);

        // Convert to Anthropic format with streaming enabled
        let mut anthropic_request = AnthropicRequest::from(request);
        anthropic_request.stream = Some(true);

        let response = self
            .client
            .post(&url)
            .json(&anthropic_request)
            .send()
            .await
            .map_err(|e| LlmError::ConnectionError(format!("Failed to send streaming request to Anthropic: {e}")))?;

        let status = response.status();

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            log::error!("Anthropic streaming API error ({status}): {error_text}");

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

        // Use unfold to maintain state with AnthropicStreamProcessor
        let chunk_stream = futures::stream::unfold(
            (Box::pin(event_stream), AnthropicStreamProcessor::new(provider_name)),
            |(mut stream, mut processor)| async move {
                loop {
                    let event = stream.next().await?;

                    let Ok(event) = event else {
                        log::warn!("SSE parsing error in Anthropic stream");
                        continue;
                    };

                    let Ok(anthropic_event) = sonic_rs::from_str::<AnthropicStreamEvent<'_>>(&event.data) else {
                        log::warn!("Failed to parse Anthropic streaming event");
                        continue;
                    };

                    if let AnthropicStreamEvent::Error { error } = &anthropic_event {
                        log::error!("Anthropic stream error event: {} - {}", error.error_type, error.message);
                    }

                    if let Some(chunk) = processor.process_event(anthropic_event) {
                        return Some((Ok(chunk), (stream, processor)));
                    }
                }
            },
        );

        Ok(Box::pin(chunk_stream))
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        &self.name
    }
}
