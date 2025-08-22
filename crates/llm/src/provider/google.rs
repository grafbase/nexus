mod input;
mod output;

use async_trait::async_trait;
use config::ApiProviderConfig;
use reqwest::{Client, Method};
use secrecy::ExposeSecret;

use self::{
    input::GoogleGenerateRequest,
    output::{GoogleGenerateResponse, GoogleStreamChunk},
};

use eventsource_stream::Eventsource;
use futures::StreamExt;

use crate::{
    error::LlmError,
    messages::{ChatCompletionRequest, ChatCompletionResponse, Model},
    provider::{HttpProvider, ModelManager, Provider, openai::extract_model_from_full_name, token},
    request::RequestContext,
};
use config::HeaderRule;

const DEFAULT_GOOGLE_API_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

pub(crate) struct GoogleProvider {
    client: Client,
    base_url: String,
    name: String,
    config: ApiProviderConfig,
    model_manager: ModelManager,
}

impl GoogleProvider {
    pub fn new(name: String, config: ApiProviderConfig) -> crate::Result<Self> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|e| {
                log::error!("Failed to create HTTP client for Google provider: {e}");
                LlmError::InternalError(None)
            })?;

        let base_url = config
            .base_url
            .clone()
            .unwrap_or_else(|| DEFAULT_GOOGLE_API_URL.to_string());

        // Convert ApiModelConfig to unified ModelConfig for ModelManager
        let models = config
            .models
            .clone()
            .into_iter()
            .map(|(k, v)| (k, config::ModelConfig::Api(v)))
            .collect();
        let model_manager = ModelManager::new(models, "google");

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
impl Provider for GoogleProvider {
    async fn chat_completion(
        &self,
        request: ChatCompletionRequest,
        context: &RequestContext,
    ) -> crate::Result<ChatCompletionResponse> {
        let model_name = extract_model_from_full_name(&request.model);

        // Check if the model is configured and get the actual model name to use
        let actual_model = self
            .model_manager
            .resolve_model(&model_name)
            .ok_or_else(|| LlmError::ModelNotFound(format!("Model '{}' is not configured", model_name)))?;

        // Get the model config to access headers
        let model_config = self.model_manager.get_model_config(&model_name);

        let temp_api_key = self.config.api_key.clone();
        let api_key = token::get(self.config.forward_token, &temp_api_key, context)?;

        let url = format!(
            "{}/models/{actual_model}:generateContent?key={}",
            self.base_url,
            api_key.expose_secret()
        );

        // Store the original model name
        let original_model = request.model.clone();

        // Convert to Google format
        let google_request = GoogleGenerateRequest::from(request);

        // Use create_post_request to ensure headers are applied
        let request_builder = self.request_builder(Method::POST, &url, context, model_config);

        let response = request_builder
            .json(&google_request)
            .send()
            .await
            .map_err(|e| LlmError::ConnectionError(format!("Failed to send request to Google: {e}")))?;

        let status = response.status();

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            log::error!("Google API error ({status}): {error_text}");

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
            log::error!("Failed to read Google response body: {e}");

            LlmError::InternalError(None)
        })?;

        // Try to parse the response
        let google_response: GoogleGenerateResponse = sonic_rs::from_str(&response_text).map_err(|e| {
            log::error!("Failed to parse Google chat completion response: {e}");
            log::error!("Raw response that failed to parse: {response_text}");

            LlmError::InternalError(None)
        })?;

        // Ensure we have at least one candidate
        if google_response.candidates.is_empty() {
            log::error!("Google API returned empty candidates array");
            return Err(LlmError::InternalError(None));
        }

        let mut response = ChatCompletionResponse::from(google_response);
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
    ) -> crate::Result<crate::provider::ChatCompletionStream> {
        let model_name = extract_model_from_full_name(&request.model);

        // Check if the model is configured and get the actual model name to use
        let actual_model = self
            .model_manager
            .resolve_model(&model_name)
            .ok_or_else(|| LlmError::ModelNotFound(format!("Model '{}' is not configured", model_name)))?;

        // Get the model config to access headers
        let model_config = self.model_manager.get_model_config(&model_name);

        let temp_api_key = self.config.api_key.clone();
        let api_key = token::get(self.config.forward_token, &temp_api_key, context)?;

        let url = format!(
            "{}/models/{actual_model}:streamGenerateContent?alt=sse&key={}",
            self.base_url,
            api_key.expose_secret()
        );

        let google_request = GoogleGenerateRequest::from(request);

        // Use create_post_request to ensure headers are applied
        let request_builder = self.request_builder(Method::POST, &url, context, model_config);

        let response = request_builder
            .json(&google_request)
            .send()
            .await
            .map_err(|e| LlmError::ConnectionError(format!("Failed to send streaming request to Google: {e}")))?;

        let status = response.status();

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            log::error!("Google streaming API error ({status}): {error_text}");

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

        let chunk_stream = event_stream.filter_map(move |event| {
            let provider = provider_name.clone();
            let model = model_name.clone();

            async move {
                let Ok(event) = event else {
                    log::warn!("SSE parsing error in Google stream");
                    return None;
                };

                let Ok(chunk) = sonic_rs::from_str::<GoogleStreamChunk<'_>>(&event.data) else {
                    log::warn!("Failed to parse Google streaming chunk: {}", event.data);
                    return None;
                };

                Some(Ok(chunk.into_chunk(&provider, &model)))
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

impl HttpProvider for GoogleProvider {
    fn get_provider_headers(&self) -> &[HeaderRule] {
        &self.config.headers
    }

    fn get_http_client(&self) -> &Client {
        &self.client
    }
}
