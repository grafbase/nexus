pub(super) mod input;
pub(super) mod output;

use anyhow::{Context, anyhow};
use async_trait::async_trait;
use axum::http::HeaderMap;
use config::ApiProviderConfig;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use reqwest::{Client, Method, header::CONTENT_TYPE};
use secrecy::ExposeSecret;

use self::{
    input::AnthropicRequest,
    output::{AnthropicResponse, AnthropicStreamEvent, AnthropicStreamProcessor},
};

use crate::{
    error::LlmError,
    messages::{
        anthropic::CountTokensResponse,
        openai::Model,
        unified::{UnifiedRequest, UnifiedResponse},
    },
    provider::{
        ChatCompletionStream, HttpProvider, ModelManager, Provider, http_client::default_http_client_builder, token,
    },
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

        let client = default_http_client_builder(headers).build().map_err(|e| {
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
impl Provider for AnthropicProvider {
    async fn chat_completion(
        &self,
        mut request: UnifiedRequest,
        context: &RequestContext,
    ) -> crate::Result<UnifiedResponse> {
        let url = format!("{}/messages", self.base_url);
        let api_key = token::get(self.config.forward_token, &self.config.api_key, context)?;

        let original_model = request.model.clone();

        // Get the model config BEFORE resolving, so we lookup by the original alias
        let model_config = self.model_manager.get_model_config(&request.model);

        // Resolve model for configured aliases when discovery doesn't cover the request
        self.resolve_request_model(&mut request)?;

        let anthropic_request = AnthropicRequest::from(request);

        // Use create_post_request to ensure headers are applied
        let mut request_builder = self.request_builder(Method::POST, &url, context, model_config);

        // Add API key header (can be overridden by header rules)
        request_builder = request_builder.header("x-api-key", api_key.expose_secret());

        let body = sonic_rs::to_vec(&anthropic_request).map_err(|e| {
            log::error!("Failed to serialize Anthropic request: {e}");
            LlmError::InternalError(None)
        })?;

        let response = request_builder
            .header(CONTENT_TYPE, "application/json")
            .body(body)
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

        let mut response = UnifiedResponse::from(anthropic_response);
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
        }

        let mut models = Vec::new();

        if let Some(api_key) = self.config.api_key.as_ref() {
            let response = self
                .client
                .get(format!("{}/models", self.base_url))
                .header("x-api-key", api_key.expose_secret())
                .header("anthropic-version", ANTHROPIC_VERSION)
                .send()
                .await
                .context("failed to request Anthropic models")?;

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_else(|_| "<empty response>".to_string());
                return Err(anyhow!("Anthropic models request failed with status {status}: {body}"));
            }

            let api_response: ModelsResponse = response
                .json()
                .await
                .context("failed to deserialize Anthropic models response")?;

            models.extend(api_response.data.into_iter().map(|model| Model {
                id: model.id,
                object: crate::messages::openai::ObjectType::Model,
                created: 0,
                owned_by: "anthropic".to_string(),
            }));
        }

        models.extend(self.model_manager.get_configured_models().into_iter().map(|mut model| {
            model.id = format!("{}/{}", self.name, model.id);
            model
        }));

        Ok(models)
    }

    async fn count_tokens(
        &self,
        mut request: UnifiedRequest,
        context: &RequestContext,
    ) -> crate::Result<CountTokensResponse> {
        let url = format!("{}/messages/count_tokens", self.base_url);
        let api_key = token::get(self.config.forward_token, &self.config.api_key, context)?;

        let model_config = self.model_manager.get_model_config(&request.model);

        self.resolve_request_model(&mut request)?;

        request.stream = Some(false);

        let anthropic_request = AnthropicRequest::from(request);

        let mut request_builder = self.request_builder(Method::POST, &url, context, model_config);
        request_builder = request_builder.header("x-api-key", api_key.expose_secret());

        let body = sonic_rs::to_vec(&anthropic_request).map_err(|e| {
            log::error!("Failed to serialize Anthropic count tokens request: {e}");
            LlmError::InternalError(None)
        })?;

        let response = request_builder
            .header(CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .map_err(|e| LlmError::ConnectionError(format!("Failed to send request to Anthropic: {e}")))?;

        let status = response.status();

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            log::error!("Anthropic count tokens API error ({status}): {error_text}");

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

        let response_text = response.text().await.map_err(|e| {
            log::error!("Failed to read Anthropic count tokens response body: {e}");
            LlmError::InternalError(None)
        })?;

        sonic_rs::from_str(&response_text).map_err(|e| {
            log::error!("Failed to parse Anthropic count tokens response: {e}");
            log::error!("Raw response that failed to parse: {response_text}");
            LlmError::InternalError(None)
        })
    }

    async fn chat_completion_stream(
        &self,
        mut request: UnifiedRequest,
        context: &RequestContext,
    ) -> crate::Result<ChatCompletionStream> {
        let url = format!("{}/messages", self.base_url);

        // Get the model config BEFORE resolving, so we lookup by the original alias
        let model_config = self.model_manager.get_model_config(&request.model);

        // Resolve model for configured aliases when discovery doesn't cover the request
        self.resolve_request_model(&mut request)?;

        let api_key = token::get(self.config.forward_token, &self.config.api_key, context)?;

        let mut anthropic_request = AnthropicRequest::from(request);
        anthropic_request.stream = Some(true);

        // Use create_post_request to ensure headers are applied
        let mut request_builder = self.request_builder(Method::POST, &url, context, model_config);

        // Add API key header (can be overridden by header rules)
        request_builder = request_builder.header("x-api-key", api_key.expose_secret());

        let body = sonic_rs::to_vec(&anthropic_request).map_err(|e| {
            log::error!("Failed to serialize Anthropic streaming request: {e}");
            LlmError::InternalError(None)
        })?;

        let response = request_builder
            .header(CONTENT_TYPE, "application/json")
            .body(body)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::unified::{UnifiedContentContainer, UnifiedMessage, UnifiedRequest, UnifiedRole};
    use axum::{
        Json, Router,
        extract::State,
        http::{HeaderMap, StatusCode},
        response::IntoResponse,
        routing::post,
    };
    use config::ApiModelConfig;
    use secrecy::SecretString;
    use serde_json::{Value, json};
    use std::{
        collections::BTreeMap,
        sync::{Arc, Mutex},
    };
    use tokio::net::TcpListener;

    #[derive(Clone)]
    struct CaptureState {
        captured: Arc<Mutex<Option<(HeaderMap, Value)>>>,
    }

    async fn handle_count_tokens(
        State(state): State<CaptureState>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> impl IntoResponse {
        *state.captured.lock().unwrap() = Some((headers.clone(), body.clone()));

        (
            StatusCode::OK,
            Json(json!({
                "type": "message_count_tokens_result",
                "input_tokens": 42,
                "cache_creation_input_tokens": 1,
                "cache_read_input_tokens": 2
            })),
        )
    }

    #[tokio::test]
    async fn count_tokens_raw_calls_endpoint_and_parses_response() {
        let state = CaptureState {
            captured: Arc::new(Mutex::new(None)),
        };

        let app = Router::new()
            .route("/v1/messages/count_tokens", post(handle_count_tokens))
            .with_state(state.clone());

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let mut models = BTreeMap::new();
        models.insert(
            "claude-3-sonnet-20240229".to_string(),
            ApiModelConfig {
                rename: None,
                rate_limits: None,
                headers: Vec::new(),
            },
        );

        let config = ApiProviderConfig {
            api_key: Some(SecretString::from("test-key".to_string())),
            base_url: Some(format!("http://{address}/v1")),
            forward_token: false,
            models,
            rate_limits: None,
            headers: Vec::new(),
            model_filter: None,
        };

        let provider = AnthropicProvider::new("anthropic".to_string(), config).unwrap();

        let request = UnifiedRequest {
            model: "claude-3-sonnet-20240229".to_string(),
            messages: vec![UnifiedMessage {
                role: UnifiedRole::User,
                content: UnifiedContentContainer::Text("Hello".to_string()),
                tool_calls: None,
                tool_call_id: None,
            }],
            system: None,
            max_tokens: Some(128),
            temperature: None,
            top_p: None,
            top_k: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop_sequences: None,
            stream: None,
            tools: None,
            tool_choice: None,
            parallel_tool_calls: None,
            metadata: None,
        };

        let response = provider
            .count_tokens(request, &RequestContext::default())
            .await
            .unwrap();

        assert_eq!(response.input_tokens, 42);
        assert_eq!(response.cache_creation_input_tokens, 1);
        assert_eq!(response.cache_read_input_tokens, 2);

        let captured = state.captured.lock().unwrap().clone().expect("captured request");
        let (headers, body) = captured;

        assert_eq!(headers.get("x-api-key").unwrap(), "test-key");
        assert_eq!(headers.get("anthropic-version").unwrap(), "2023-06-01");

        assert_eq!(
            body.get("model").and_then(Value::as_str),
            Some("claude-3-sonnet-20240229")
        );
        if let Some(value) = body.get("stream") {
            assert_eq!(value, &Value::Bool(false));
        }
    }

    #[tokio::test]
    async fn count_tokens_resolves_model_aliases() {
        let state = CaptureState {
            captured: Arc::new(Mutex::new(None)),
        };

        let app = Router::new()
            .route("/v1/messages/count_tokens", post(handle_count_tokens))
            .with_state(state.clone());

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let mut models = BTreeMap::new();
        models.insert(
            "workspace-sonnet".to_string(),
            ApiModelConfig {
                rename: Some("claude-3-sonnet-20240229".to_string()),
                rate_limits: None,
                headers: Vec::new(),
            },
        );

        let config = ApiProviderConfig {
            api_key: Some(SecretString::from("test-key".to_string())),
            base_url: Some(format!("http://{address}/v1")),
            forward_token: false,
            models,
            rate_limits: None,
            headers: Vec::new(),
            model_filter: None,
        };

        let provider = AnthropicProvider::new("anthropic".to_string(), config).unwrap();

        let request = UnifiedRequest {
            model: "workspace-sonnet".to_string(),
            messages: vec![UnifiedMessage {
                role: UnifiedRole::User,
                content: UnifiedContentContainer::Text("Alias route".to_string()),
                tool_calls: None,
                tool_call_id: None,
            }],
            system: None,
            max_tokens: Some(64),
            temperature: None,
            top_p: None,
            top_k: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop_sequences: None,
            stream: None,
            tools: None,
            tool_choice: None,
            parallel_tool_calls: None,
            metadata: None,
        };

        provider
            .count_tokens(request, &RequestContext::default())
            .await
            .unwrap();

        let captured = state.captured.lock().unwrap().clone().expect("captured request");
        let (_headers, body) = captured;

        assert_eq!(
            body.get("model").and_then(Value::as_str),
            Some("claude-3-sonnet-20240229")
        );
    }
}
