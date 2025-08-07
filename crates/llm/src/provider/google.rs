mod input;
mod output;

use async_trait::async_trait;
use config::GoogleConfig;
use reqwest::Client;
use secrecy::ExposeSecret;

use self::{
    input::GoogleGenerateRequest,
    output::{GoogleGenerateResponse, GoogleModelsResponse, GoogleStreamChunk},
};

use eventsource_stream::Eventsource;
use futures::StreamExt;

use crate::{
    error::LlmError,
    messages::{ChatCompletionRequest, ChatCompletionResponse, Model},
    provider::{Provider, openai::extract_model_from_full_name},
};

const DEFAULT_GOOGLE_API_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

pub(crate) struct GoogleProvider {
    client: Client,
    base_url: String,
    api_key: String,
    name: String,
}

impl GoogleProvider {
    pub fn new(name: String, config: GoogleConfig) -> crate::Result<Self> {
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

        let api_key = config.api_key.expose_secret().to_string();

        Ok(Self {
            client,
            base_url,
            api_key,
            name,
        })
    }
}

#[async_trait]
impl Provider for GoogleProvider {
    async fn chat_completion(&self, request: ChatCompletionRequest) -> crate::Result<ChatCompletionResponse> {
        // Note: Streaming is handled by chat_completion_stream()

        // Extract the model name and construct the URL
        let model_name = extract_model_from_full_name(&request.model);
        let url = format!(
            "{}/models/{}:generateContent?key={}",
            self.base_url, model_name, self.api_key
        );

        // Store the original model name
        let original_model = request.model.clone();

        // Convert to Google format
        let google_request = GoogleGenerateRequest::from(request);

        let response = self
            .client
            .post(&url)
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

    async fn list_models(&self) -> crate::Result<Vec<Model>> {
        let url = format!("{}/models?key={}", self.base_url, self.api_key);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| LlmError::ConnectionError(format!("Failed to fetch models from Google: {e}")))?;

        let status = response.status();

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            log::error!("Google API error ({status}): {error_text}");

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
            log::error!("Failed to read Google models response body: {e}");
            LlmError::InternalError(None)
        })?;

        // Try to parse the response
        let models_response: GoogleModelsResponse = sonic_rs::from_str(&response_text).map_err(|e| {
            log::error!("Failed to parse Google models list response: {e}");
            log::error!("Raw response that failed to parse: {response_text}");
            LlmError::InternalError(None)
        })?;

        // Filter to only chat-capable models
        let chat_models = models_response
            .models
            .into_iter()
            .filter(|model| {
                model
                    .supported_generation_methods
                    .contains(&"generateContent".to_string())
            })
            .filter(|model| {
                // Filter to known chat models based on the name
                // Google model names are in format "models/gemini-1.5-pro"
                model.name.contains("gemini") || model.name.contains("chat") || model.name.contains("palm")
            })
            .map(Into::into)
            .collect::<Vec<Model>>();

        Ok(chat_models)
    }

    async fn chat_completion_stream(
        &self,
        request: ChatCompletionRequest,
    ) -> crate::Result<crate::provider::ChatCompletionStream> {
        // Extract the model name and construct the URL with streaming endpoint
        let model_name = extract_model_from_full_name(&request.model);
        let url = format!(
            "{}/models/{}:streamGenerateContent?alt=sse&key={}",
            self.base_url, model_name, self.api_key
        );

        // Convert to Google format
        let google_request = GoogleGenerateRequest::from(request);

        let response = self
            .client
            .post(&url)
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
            let model = model_name.clone();

            async move {
                // Handle SSE parsing errors
                let Ok(event) = event else {
                    log::warn!("SSE parsing error in Google stream");
                    return None;
                };

                // Parse the JSON chunk
                let Ok(chunk) = sonic_rs::from_str::<GoogleStreamChunk<'_>>(&event.data) else {
                    log::warn!("Failed to parse Google streaming chunk");
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
