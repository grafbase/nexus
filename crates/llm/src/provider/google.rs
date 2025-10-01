mod input;
mod output;

use anyhow::{Context, anyhow};
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
    messages::{
        openai::Model,
        unified::{UnifiedChunk, UnifiedRequest, UnifiedResponse},
    },
    provider::{HttpProvider, ModelManager, Provider, http_client::default_http_client_builder, token},
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
        let client = default_http_client_builder(Default::default()).build().map_err(|e| {
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
impl Provider for GoogleProvider {
    async fn chat_completion(
        &self,
        mut request: UnifiedRequest,
        context: &RequestContext,
    ) -> crate::Result<UnifiedResponse> {
        // request.model already contains the extracted model name from server.rs
        // Get the model config BEFORE resolving, so we lookup by the original alias
        let model_config = self.model_manager.get_model_config(&request.model);

        // Resolve model for configured aliases when discovery doesn't cover the request
        self.resolve_request_model(&mut request)?;

        let api_key = token::get(self.config.forward_token, &self.config.api_key, context)?;

        let url = format!(
            "{}/models/{}:generateContent?key={}",
            self.base_url,
            request.model,
            api_key.expose_secret()
        );

        // Store the original model name
        let original_model = request.model.clone();

        // Convert to Google format
        let google_request = GoogleGenerateRequest::from(request);

        // Log the request for debugging Google 500 errors
        log::debug!("Sending request to Google API at URL: {}", url);
        if let Ok(json) = sonic_rs::to_string(&google_request) {
            // Only log first part to avoid noise
            let preview = if json.len() > 1000 {
                format!("{}... (truncated, {} bytes total)", &json[..1000], json.len())
            } else {
                json
            };
            log::debug!("Google API request: {}", preview);
        }

        // Use create_post_request to ensure headers are applied
        let request_builder = self.request_builder(Method::POST, &url, context, model_config);

        let body = sonic_rs::to_vec(&google_request).map_err(|e| {
            log::error!("Failed to serialize Google request: {e}");
            LlmError::InternalError(None)
        })?;

        let response = request_builder
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
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
            log::error!("Full response text that failed to parse: {}", response_text);

            LlmError::InternalError(None)
        })?;

        // Ensure we have at least one candidate
        if google_response.candidates.is_empty() {
            log::error!("Google API returned empty candidates array");
            return Err(LlmError::InternalError(None));
        }

        let mut response = UnifiedResponse::from(google_response);
        response.model = original_model;

        Ok(response)
    }

    async fn list_models(&self) -> anyhow::Result<Vec<Model>> {
        #[derive(serde::Deserialize)]
        struct ModelsResponse {
            models: Vec<ApiModel>,
        }

        #[derive(serde::Deserialize)]
        struct ApiModel {
            name: String,
        }

        let mut models = Vec::new();

        if let Some(api_key) = self.config.api_key.as_ref() {
            let response = self
                .client
                .get(format!("{}/models?key={}", self.base_url, api_key.expose_secret()))
                .send()
                .await
                .context("failed to request Google models")?;

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_else(|_| "<empty response>".to_string());
                return Err(anyhow!("Google models request failed with status {status}: {body}"));
            }

            let api_response: ModelsResponse = response
                .json()
                .await
                .context("failed to deserialize Google models response")?;

            models.extend(api_response.models.into_iter().map(|model| {
                let id = model.name.strip_prefix("models/").unwrap_or(&model.name).to_string();

                Model {
                    id,
                    object: crate::messages::openai::ObjectType::Model,
                    created: 0,
                    owned_by: "google".to_string(),
                }
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
    ) -> crate::Result<crate::provider::ChatCompletionStream> {
        // request.model already contains the extracted model name from server.rs
        let model_name = request.model.clone(); // Keep for closure later

        // Get the model config BEFORE resolving, so we lookup by the original alias
        let model_config = self.model_manager.get_model_config(&request.model);

        // Resolve model for configured aliases when discovery doesn't cover the request
        self.resolve_request_model(&mut request)?;

        let api_key = token::get(self.config.forward_token, &self.config.api_key, context)?;

        let url = format!(
            "{}/models/{}:streamGenerateContent?alt=sse&key={}",
            self.base_url,
            request.model,
            api_key.expose_secret()
        );

        let google_request = GoogleGenerateRequest::from(request);

        // Use create_post_request to ensure headers are applied
        let request_builder = self.request_builder(Method::POST, &url, context, model_config);

        let body = sonic_rs::to_vec(&google_request).map_err(|e| {
            log::error!("Failed to serialize Google streaming request: {e}");
            LlmError::InternalError(None)
        })?;

        let response = request_builder
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
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

                Some(Ok(UnifiedChunk::from(chunk.into_chunk(&provider, &model))))
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
