mod input;
mod output;

use async_trait::async_trait;
use axum::http::HeaderMap;
use config::ApiProviderConfig;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use reqwest::{Client, Method, header::AUTHORIZATION};
use secrecy::ExposeSecret;

use self::{
    input::OpenAIRequest,
    output::{OpenAIResponse, OpenAIStreamChunk},
};

use crate::{
    error::LlmError,
    messages::{ChatCompletionRequest, ChatCompletionResponse, Model},
    provider::{ChatCompletionStream, HttpProvider, ModelManager, Provider, token},
    request::RequestContext,
};
use config::HeaderRule;

const DEFAULT_OPENAI_API_URL: &str = "https://api.openai.com/v1";

pub(crate) struct OpenAIProvider {
    client: Client,
    base_url: String,
    name: String,
    config: ApiProviderConfig,
    model_manager: ModelManager,
}

impl OpenAIProvider {
    pub fn new(name: String, config: ApiProviderConfig) -> crate::Result<Self> {
        let headers = HeaderMap::new();

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

        // Convert ApiModelConfig to unified ModelConfig for ModelManager
        let models = config
            .models
            .clone()
            .into_iter()
            .map(|(k, v)| (k, config::ModelConfig::Api(v)))
            .collect();
        let model_manager = ModelManager::new(models, "openai");

        Ok(Self {
            client,
            base_url,
            name,
            model_manager,
            config,
        })
    }
}

#[async_trait]
impl Provider for OpenAIProvider {
    async fn chat_completion(
        &self,
        request: ChatCompletionRequest,
        context: &RequestContext,
    ) -> crate::Result<ChatCompletionResponse> {
        let url = format!("{}/chat/completions", self.base_url);

        let model_name = extract_model_from_full_name(&request.model);
        let original_model = request.model.clone();

        // Check if the model is configured and get the actual model name to use
        let actual_model = self
            .model_manager
            .resolve_model(&model_name)
            .ok_or_else(|| LlmError::ModelNotFound(format!("Model '{}' is not configured", model_name)))?;

        // Get the model config to access headers
        let model_config = self.model_manager.get_model_config(&model_name);

        let mut openai_request = OpenAIRequest::from(request);
        openai_request.model = actual_model;
        openai_request.stream = false; // Always false for now

        // Use create_post_request to ensure headers are applied
        let mut request_builder = self.request_builder(Method::POST, &url, context, model_config);

        // Add authorization header (can be overridden by header rules)
        let temp_api_key = self.config.api_key.clone();
        let key = token::get(self.config.forward_token, &temp_api_key, context)?;
        request_builder = request_builder.header(AUTHORIZATION, format!("Bearer {}", key.expose_secret()));

        let response = request_builder
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
                429 => LlmError::RateLimitExceeded { message: error_text },
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

    fn list_models(&self) -> Vec<Model> {
        // Phase 3: Return only explicitly configured models, error if none
        self.model_manager.get_configured_models()
    }

    async fn chat_completion_stream(
        &self,
        request: ChatCompletionRequest,
        context: &RequestContext,
    ) -> crate::Result<ChatCompletionStream> {
        let url = format!("{}/chat/completions", self.base_url);

        // Check if the model is configured and get the actual model name to use
        let model_name = extract_model_from_full_name(&request.model);
        let actual_model = self
            .model_manager
            .resolve_model(&model_name)
            .ok_or_else(|| LlmError::ModelNotFound(format!("Model '{}' is not configured", model_name)))?;

        // Get the model config to access headers
        let model_config = self.model_manager.get_model_config(&model_name);

        let mut openai_request = OpenAIRequest::from(request);
        openai_request.model = actual_model;
        openai_request.stream = true;

        let temp_api_key = self.config.api_key.clone();
        let key = token::get(self.config.forward_token, &temp_api_key, context)?;

        // Use create_post_request to ensure headers are applied
        let mut request_builder = self.request_builder(Method::POST, &url, context, model_config);

        // Add authorization header (can be overridden by header rules)
        request_builder = request_builder.header(AUTHORIZATION, format!("Bearer {}", key.expose_secret()));

        let response = request_builder
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
                429 => LlmError::RateLimitExceeded { message: error_text },
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

impl HttpProvider for OpenAIProvider {
    fn get_provider_headers(&self) -> &[HeaderRule] {
        &self.config.headers
    }

    fn get_http_client(&self) -> &Client {
        &self.client
    }
}

/// Extract the model name from a full provider/model string.
pub(super) fn extract_model_from_full_name(full_name: &str) -> String {
    full_name.split('/').next_back().unwrap_or(full_name).to_string()
}

// OpenAI API request/response types
