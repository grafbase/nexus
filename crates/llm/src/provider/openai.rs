mod input;
mod output;

use anyhow::{Context, anyhow};
use async_trait::async_trait;
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
    http_client::http_client,
    messages::{
        openai::Model,
        unified::{UnifiedRequest, UnifiedResponse},
    },
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
        let client = http_client();

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

    fn resolve_request_model(&self, request: &mut UnifiedRequest) -> crate::Result<()> {
        if let Some(filter) = &self.config.model_filter
            && filter.is_match(&request.model)
        {
            return Ok(());
        }

        if let Some(resolved_model) = self.model_manager.resolve_model(&request.model) {
            request.model = resolved_model;
            return Ok(());
        }

        if self.config.model_filter.is_none() {
            return Ok(());
        }

        Err(LlmError::ModelNotFound(format!(
            "Model '{}' is not configured",
            request.model
        )))
    }
}

#[async_trait]
impl Provider for OpenAIProvider {
    async fn chat_completion(
        &self,
        mut request: UnifiedRequest,
        context: &RequestContext,
    ) -> crate::Result<UnifiedResponse> {
        let url = format!("{}/chat/completions", self.base_url);

        // request.model already contains the extracted model name from server.rs
        let original_model = request.model.clone();

        // Get the model config BEFORE resolving, so we lookup by the original alias
        let model_config = self.model_manager.get_model_config(&request.model);

        // Resolve model for configured aliases when discovery doesn't cover the request
        self.resolve_request_model(&mut request)?;

        let mut openai_request = OpenAIRequest::from(request);
        openai_request.stream = false; // Always false for now

        // Use create_post_request to ensure headers are applied
        let mut request_builder = self.request_builder(Method::POST, &url, context, model_config);

        // Add authorization header (can be overridden by header rules)
        let key = token::get(self.config.forward_token, &self.config.api_key, context)?;
        request_builder = request_builder.header(AUTHORIZATION, format!("Bearer {}", key.expose_secret()));

        // Serialize with sonic_rs to handle sonic_rs::Value fields properly
        let body = sonic_rs::to_vec(&openai_request)
            .map_err(|e| LlmError::InvalidRequest(format!("Failed to serialize request: {e}")))?;

        let response = request_builder
            .header("Content-Type", "application/json")
            .body(body)
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
            log::debug!("Response parsing failed, length: {} bytes", response_text.len());

            LlmError::InternalError(None)
        })?;

        let mut response = UnifiedResponse::from(openai_response);
        response.model = original_model;

        Ok(response)
    }

    async fn list_models(&self) -> anyhow::Result<Vec<Model>> {
        #[derive(serde::Deserialize)]
        struct ModelsResponse {
            data: Vec<ApiModel>,
        }

        #[derive(serde::Deserialize)]
        struct ApiModel {
            id: String,
            created: Option<u64>,
            owned_by: Option<String>,
        }

        let mut models = Vec::new();

        if let Some(api_key) = self.config.api_key.as_ref() {
            let response = self
                .client
                .get(format!("{}/models", self.base_url))
                .bearer_auth(api_key.expose_secret())
                .send()
                .await
                .context("failed to request OpenAI models")?;

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_else(|_| "<empty response>".to_string());
                return Err(anyhow!("OpenAI models request failed with status {status}: {body}"));
            }

            let api_response: ModelsResponse = response
                .json()
                .await
                .context("failed to deserialize OpenAI models response")?;

            models.extend(api_response.data.into_iter().map(|model| Model {
                id: model.id,
                object: crate::messages::openai::ObjectType::Model,
                created: model.created.unwrap_or(0),
                owned_by: model.owned_by.unwrap_or_else(|| "openai".to_string()),
            }));
        }

        // Always include explicitly configured models with provider prefix
        models.extend(self.model_manager.get_configured_models().into_iter().map(|mut model| {
            model.id = format!("{}/{}", self.name, model.id);
            model
        }));

        Ok(models)
    }

    async fn chat_completion_stream(
        &self,
        mut request: UnifiedRequest,
        context: &RequestContext,
    ) -> crate::Result<ChatCompletionStream> {
        let url = format!("{}/chat/completions", self.base_url);

        // request.model already contains the extracted model name from server.rs
        // Get the model config BEFORE resolving, so we lookup by the original alias
        let model_config = self.model_manager.get_model_config(&request.model);

        // Resolve model for configured aliases when discovery doesn't cover the request
        self.resolve_request_model(&mut request)?;

        let mut openai_request = OpenAIRequest::from(request);
        openai_request.stream = true;

        let key = token::get(self.config.forward_token, &self.config.api_key, context)?;

        // Use create_post_request to ensure headers are applied
        let mut request_builder = self.request_builder(Method::POST, &url, context, model_config);

        // Add authorization header (can be overridden by header rules)
        request_builder = request_builder.header(AUTHORIZATION, format!("Bearer {}", key.expose_secret()));

        // Serialize with sonic_rs to handle sonic_rs::Value fields properly
        let body = sonic_rs::to_vec(&openai_request)
            .map_err(|e| LlmError::InvalidRequest(format!("Failed to serialize request: {e}")))?;

        let response = request_builder
            .header("Content-Type", "application/json")
            .body(body)
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
