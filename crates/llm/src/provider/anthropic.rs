pub(super) mod input;
pub(super) mod output;

use async_trait::async_trait;
use axum::http::HeaderMap;
use config::ApiProviderConfig;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use reqwest::{Client, Method};
use secrecy::ExposeSecret;

use self::{
    input::AnthropicRequest,
    output::{AnthropicResponse, AnthropicStreamEvent, AnthropicStreamProcessor},
};

use crate::{
    error::LlmError,
    messages::{ChatCompletionRequest, ChatCompletionResponse, Model},
    provider::{ChatCompletionStream, HttpProvider, ModelManager, Provider, token},
    request::RequestContext,
};
use config::HeaderRule;

const DEFAULT_ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1";
const ANTHROPIC_VERSION: &str = "2023-06-01";

pub(crate) struct AnthropicProvider {
    client: Client,
    base_url: String,
    name: String,
    config: ApiProviderConfig,
    model_manager: ModelManager,
}

impl AnthropicProvider {
    pub fn new(name: String, config: ApiProviderConfig) -> crate::Result<Self> {
        let mut headers = HeaderMap::new();

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

        // Convert ApiModelConfig to unified ModelConfig for ModelManager
        let models = config
            .models
            .clone()
            .into_iter()
            .map(|(k, v)| (k, config::ModelConfig::Api(v)))
            .collect();
        let model_manager = ModelManager::new(models, "anthropic");

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
impl Provider for AnthropicProvider {
    async fn chat_completion(
        &self,
        mut request: ChatCompletionRequest,
        context: &RequestContext,
    ) -> crate::Result<ChatCompletionResponse> {
        let url = format!("{}/messages", self.base_url);
        let temp_api_key = self.config.api_key.clone();
        let api_key = token::get(self.config.forward_token, &temp_api_key, context)?;

        let original_model = request.model.clone();

        // Check if the model is configured and get the actual model name to use
        let actual_model = self
            .model_manager
            .resolve_model(&request.model)
            .ok_or_else(|| LlmError::ModelNotFound(format!("Model '{}' is not configured", request.model)))?;

        // Get the model config to access headers
        let model_config = self.model_manager.get_model_config(&request.model);

        request.model = actual_model;
        let anthropic_request = AnthropicRequest::from(request);

        // Use create_post_request to ensure headers are applied
        let mut request_builder = self.request_builder(Method::POST, &url, context, model_config);

        // Add API key header (can be overridden by header rules)
        request_builder = request_builder.header("x-api-key", api_key.expose_secret());

        let response = request_builder
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

    fn list_models(&self) -> Vec<Model> {
        // Phase 3: Return only explicitly configured models, error if none
        self.model_manager.get_configured_models()
    }

    async fn chat_completion_stream(
        &self,
        mut request: ChatCompletionRequest,
        context: &RequestContext,
    ) -> crate::Result<ChatCompletionStream> {
        let url = format!("{}/messages", self.base_url);

        // Check if the model is configured and get the actual model name to use
        let actual_model = self
            .model_manager
            .resolve_model(&request.model)
            .ok_or_else(|| LlmError::ModelNotFound(format!("Model '{}' is not configured", request.model)))?;

        // Get the model config to access headers
        let model_config = self.model_manager.get_model_config(&request.model);

        request.model = actual_model;
        let temp_api_key = self.config.api_key.clone();
        let api_key = token::get(self.config.forward_token, &temp_api_key, context)?;

        let mut anthropic_request = AnthropicRequest::from(request);
        anthropic_request.stream = Some(true);

        // Use create_post_request to ensure headers are applied
        let mut request_builder = self.request_builder(Method::POST, &url, context, model_config);

        // Add API key header (can be overridden by header rules)
        request_builder = request_builder.header("x-api-key", api_key.expose_secret());

        let response =
            request_builder.json(&anthropic_request).send().await.map_err(|e| {
                LlmError::ConnectionError(format!("Failed to send streaming request to Anthropic: {e}"))
            })?;

        let status = response.status();

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            log::error!("Anthropic streaming API error ({status}): {error_text}");

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

impl HttpProvider for AnthropicProvider {
    fn get_provider_headers(&self) -> &[HeaderRule] {
        &self.config.headers
    }

    fn get_http_client(&self) -> &Client {
        &self.client
    }
}
