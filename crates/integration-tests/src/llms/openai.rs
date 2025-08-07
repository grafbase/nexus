use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json, Response, Sse, sse::Event},
    routing::{get, post},
};
use futures::stream;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;

use super::common::find_custom_response;
use super::provider::{LlmProviderConfig, TestLlmProvider};

/// Builder for OpenAI test server
pub struct OpenAIMock {
    name: String,
    models: Vec<String>,
    custom_responses: HashMap<String, String>,
    error_type: Option<ErrorType>,
    streaming_enabled: bool,
    streaming_chunks: Option<Vec<String>>,
    streaming_error: Option<String>,
}

#[derive(Clone)]
enum ErrorType {
    AuthError(String),
    ModelNotFound(String),
    RateLimit(String),
    QuotaExceeded(String),
    BadRequest(String),
    InternalError(String),
    ServiceUnavailable(String),
}

impl OpenAIMock {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            models: vec![
                "gpt-3.5-turbo".to_string(),
                "gpt-4".to_string(),
                "gpt-4-turbo".to_string(),
            ],
            custom_responses: HashMap::new(),
            error_type: None,
            streaming_enabled: false,
            streaming_chunks: None,
            streaming_error: None,
        }
    }

    pub fn with_models(mut self, models: Vec<String>) -> Self {
        self.models = models;
        self
    }

    pub fn with_response(mut self, trigger: impl Into<String>, response: impl Into<String>) -> Self {
        self.custom_responses.insert(trigger.into(), response.into());
        self
    }

    pub fn with_auth_error(mut self, message: impl Into<String>) -> Self {
        self.error_type = Some(ErrorType::AuthError(message.into()));
        self
    }

    pub fn with_model_not_found(mut self, model: impl Into<String>) -> Self {
        self.error_type = Some(ErrorType::ModelNotFound(model.into()));
        self
    }

    pub fn with_rate_limit(mut self, message: impl Into<String>) -> Self {
        self.error_type = Some(ErrorType::RateLimit(message.into()));
        self
    }

    pub fn with_quota_exceeded(mut self, message: impl Into<String>) -> Self {
        self.error_type = Some(ErrorType::QuotaExceeded(message.into()));
        self
    }

    pub fn with_bad_request(mut self, message: impl Into<String>) -> Self {
        self.error_type = Some(ErrorType::BadRequest(message.into()));
        self
    }

    pub fn with_internal_error(mut self, message: impl Into<String>) -> Self {
        self.error_type = Some(ErrorType::InternalError(message.into()));
        self
    }

    pub fn with_service_unavailable(mut self, message: impl Into<String>) -> Self {
        self.error_type = Some(ErrorType::ServiceUnavailable(message.into()));
        self
    }

    pub fn with_streaming(mut self) -> Self {
        self.streaming_enabled = true;
        self
    }

    pub fn with_streaming_chunks(mut self, chunks: Vec<&str>) -> Self {
        self.streaming_chunks = Some(chunks.into_iter().map(|s| s.to_string()).collect());
        self
    }

    pub fn with_streaming_error(mut self, error: &str) -> Self {
        self.streaming_error = Some(error.to_string());
        self
    }

    pub fn with_streaming_text_with_newlines(mut self, text: &str) -> Self {
        // Split text to include escape sequences that need to be handled
        let mut chunks = Vec::new();

        // Split at paragraph breaks to test escape sequence handling
        if text.contains("\n\n") {
            let parts: Vec<&str> = text.split("\n\n").collect();
            for (i, part) in parts.iter().enumerate() {
                chunks.push(part.to_string());
                if i < parts.len() - 1 {
                    // Add the paragraph break as a separate chunk to test escape handling
                    chunks.push("\n\n".to_string());
                }
            }
        } else {
            chunks.push(text.to_string());
        }

        self.streaming_chunks = Some(chunks);
        self.streaming_enabled = true;
        self
    }
}

impl TestLlmProvider for OpenAIMock {
    fn provider_type(&self) -> &str {
        "openai"
    }

    fn name(&self) -> &str {
        &self.name
    }

    async fn spawn(self: Box<Self>) -> anyhow::Result<LlmProviderConfig> {
        let state = Arc::new(TestLlmState {
            models: self.models,
            custom_responses: self.custom_responses,
            error_type: self.error_type,
            streaming_enabled: self.streaming_enabled,
            streaming_chunks: self.streaming_chunks,
            streaming_error: self.streaming_error,
        });

        let app = Router::new()
            .route("/v1/chat/completions", post(chat_completions))
            .route("/v1/models", get(list_models))
            .with_state(state);

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        // Give the server time to start
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        Ok(LlmProviderConfig {
            name: self.name.clone(),
            address,
            provider_type: super::provider::ProviderType::OpenAI,
        })
    }
}

/// Test LLM server that mimics OpenAI API for testing (legacy compatibility)
pub struct TestOpenAIServer {
    pub address: SocketAddr,
}

impl TestOpenAIServer {
    /// Create and start a new test LLM server (for backward compatibility)
    pub async fn start() -> Self {
        let builder = OpenAIMock::new("test_openai");
        let config = Box::new(builder).spawn().await.unwrap();

        Self {
            address: config.address,
        }
    }

    /// Get the base URL for this test server
    pub fn base_url(&self) -> String {
        format!("http://{}", self.address)
    }

    pub fn url(&self) -> String {
        format!("http://{}/v1", self.address)
    }
}

struct TestLlmState {
    models: Vec<String>,
    custom_responses: HashMap<String, String>,
    error_type: Option<ErrorType>,
    streaming_enabled: bool,
    streaming_chunks: Option<Vec<String>>,
    streaming_error: Option<String>,
}

impl Default for TestLlmState {
    fn default() -> Self {
        Self {
            models: vec![
                "gpt-3.5-turbo".to_string(),
                "gpt-4".to_string(),
                "gpt-4-turbo".to_string(),
            ],
            custom_responses: HashMap::new(),
            error_type: None,
            streaming_enabled: false,
            streaming_chunks: None,
            streaming_error: None,
        }
    }
}

/// Error response wrapper
struct ErrorResponse {
    status: StatusCode,
    message: String,
}

impl IntoResponse for ErrorResponse {
    fn into_response(self) -> Response {
        (self.status, self.message).into_response()
    }
}

/// Handle chat completion requests
async fn chat_completions(
    State(state): State<Arc<TestLlmState>>,
    Json(request): Json<ChatCompletionRequest>,
) -> Result<Response, ErrorResponse> {
    // Check for configured error responses
    if let Some(error_type) = &state.error_type {
        return Err(match error_type {
            ErrorType::AuthError(msg) => ErrorResponse {
                status: StatusCode::UNAUTHORIZED,
                message: msg.clone(),
            },
            ErrorType::ModelNotFound(model) => {
                if request.model.contains(model) {
                    ErrorResponse {
                        status: StatusCode::NOT_FOUND,
                        message: format!("The model '{model}' does not exist"),
                    }
                } else {
                    // Don't return error if this isn't the problematic model
                    return Ok(Json(generate_success_response(request, &state)).into_response());
                }
            }
            ErrorType::RateLimit(msg) => ErrorResponse {
                status: StatusCode::TOO_MANY_REQUESTS,
                message: msg.clone(),
            },
            ErrorType::QuotaExceeded(msg) => ErrorResponse {
                status: StatusCode::FORBIDDEN,
                message: msg.clone(),
            },
            ErrorType::BadRequest(msg) => ErrorResponse {
                status: StatusCode::BAD_REQUEST,
                message: msg.clone(),
            },
            ErrorType::InternalError(msg) => ErrorResponse {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: msg.clone(),
            },
            ErrorType::ServiceUnavailable(msg) => ErrorResponse {
                status: StatusCode::SERVICE_UNAVAILABLE,
                message: msg.clone(),
            },
        });
    }

    if request.stream.unwrap_or(false) {
        if !state.streaming_enabled {
            return Err(ErrorResponse {
                status: StatusCode::BAD_REQUEST,
                message: "Streaming is not yet supported".to_string(),
            });
        }

        if let Some(error_msg) = &state.streaming_error {
            return Err(ErrorResponse {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: error_msg.clone(),
            });
        }

        let streaming_chunks = state.streaming_chunks.clone();
        return Ok(generate_streaming_response(request, streaming_chunks).into_response());
    }

    Ok(Json(generate_success_response(request, &state)).into_response())
}

/// Generate SSE streaming response
fn generate_streaming_response(
    request: ChatCompletionRequest,
    streaming_chunks: Option<Vec<String>>,
) -> Sse<impl futures::Stream<Item = Result<Event, std::convert::Infallible>> + 'static> {
    let model = request.model.clone();

    let mut events = Vec::new();

    // Note: streaming_error is handled at HTTP level, not in streaming chunks
    let chunks = streaming_chunks.unwrap_or_else(|| vec!["Why don't scientists trust atoms? ".to_string()]);

    let first_chunk = serde_json::json!({
        "id": format!("chatcmpl-{}", uuid::Uuid::new_v4()),
        "object": "chat.completion.chunk",
        "created": 1677651200,
        "model": &model,
        "choices": [{
            "index": 0,
            "delta": {
                "role": "assistant",
                "content": chunks[0]
            },
            "finish_reason": null,
            "logprobs": null
        }],
        "system_fingerprint": null,
        "usage": null
    });
    events.push(Event::default().data(serde_json::to_string(&first_chunk).unwrap()));

    for chunk_text in chunks.iter().skip(1) {
        let chunk = serde_json::json!({
            "id": format!("chatcmpl-{}", uuid::Uuid::new_v4()),
            "object": "chat.completion.chunk",
            "created": 1677651200,
            "model": &model,
            "choices": [{
                "index": 0,
                "delta": {
                    "content": chunk_text
                },
                "finish_reason": null,
                "logprobs": null
            }],
            "system_fingerprint": null,
            "usage": null
        });
        events.push(Event::default().data(serde_json::to_string(&chunk).unwrap()));
    }

    let final_chunk = serde_json::json!({
        "id": format!("chatcmpl-{}", uuid::Uuid::new_v4()),
        "object": "chat.completion.chunk",
        "created": 1677651200,
        "model": &model,
        "choices": [{
            "index": 0,
            "delta": {},
            "finish_reason": "stop",
            "logprobs": null
        }],
        "system_fingerprint": null,
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 15,
            "total_tokens": 25
        }
    });
    events.push(Event::default().data(serde_json::to_string(&final_chunk).unwrap()));

    events.push(Event::default().data("[DONE]"));

    let stream = stream::iter(events.into_iter().map(Ok));
    Sse::new(stream)
}

fn generate_success_response(request: ChatCompletionRequest, state: &TestLlmState) -> ChatCompletionResponse {
    let response_text = find_custom_response(&request.messages, &state.custom_responses, |m| &m.content)
        .unwrap_or_else(|| {
            if request.messages.iter().any(|m| m.content.contains("error")) {
                "This is an error response for testing".to_string()
            } else if request.messages.iter().any(|m| m.content.contains("Hello")) {
                "Hello! I'm a test LLM assistant. How can I help you today?".to_string()
            } else {
                // Include temperature in response if it was high
                if request.temperature.unwrap_or(0.0) > 1.5 {
                    "This is a creative response due to high temperature".to_string()
                } else {
                    "This is a test response from the mock LLM server".to_string()
                }
            }
        });

    ChatCompletionResponse {
        id: format!("chatcmpl-test-{}", uuid::Uuid::new_v4()),
        object: "chat.completion".to_string(),
        created: 1677651200,
        model: request.model.clone(),
        choices: vec![ChatChoice {
            index: 0,
            message: ChatMessage {
                role: "assistant".to_string(),
                content: response_text,
            },
            finish_reason: "stop".to_string(),
        }],
        usage: Usage {
            prompt_tokens: 10,
            completion_tokens: 15,
            total_tokens: 25,
        },
    }
}

/// Handle list models requests
async fn list_models(State(state): State<Arc<TestLlmState>>) -> Result<Json<ModelsResponse>, ErrorResponse> {
    // Check for configured error responses
    if let Some(error_type) = &state.error_type {
        return Err(match error_type {
            ErrorType::AuthError(msg) => ErrorResponse {
                status: StatusCode::UNAUTHORIZED,
                message: msg.clone(),
            },
            ErrorType::RateLimit(msg) => ErrorResponse {
                status: StatusCode::TOO_MANY_REQUESTS,
                message: msg.clone(),
            },
            ErrorType::QuotaExceeded(msg) => ErrorResponse {
                status: StatusCode::FORBIDDEN,
                message: msg.clone(),
            },
            ErrorType::InternalError(msg) => ErrorResponse {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: msg.clone(),
            },
            ErrorType::ServiceUnavailable(msg) => ErrorResponse {
                status: StatusCode::SERVICE_UNAVAILABLE,
                message: msg.clone(),
            },
            _ => ErrorResponse {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "Error".to_string(),
            },
        });
    }

    let models = state
        .models
        .iter()
        .enumerate()
        .map(|(i, id)| Model {
            id: id.clone(),
            object: "model".to_string(),
            created: 1677651200 + i as u64,
            owned_by: "openai".to_string(),
        })
        .collect();

    Ok(Json(ModelsResponse {
        object: "list".to_string(),
        data: models,
    }))
}

// Request/Response types (matching OpenAI API structure)

#[derive(Debug, Deserialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(default)]
    temperature: Option<f32>,
    #[serde(default)]
    #[allow(dead_code)]
    max_tokens: Option<u32>,
    #[serde(default)]
    #[allow(dead_code)]
    top_p: Option<f32>,
    #[serde(default)]
    #[allow(dead_code)]
    frequency_penalty: Option<f32>,
    #[serde(default)]
    #[allow(dead_code)]
    presence_penalty: Option<f32>,
    #[serde(default)]
    #[allow(dead_code)]
    stop: Option<Vec<String>>,
    #[serde(default)]
    stream: Option<bool>,
}

#[derive(Debug, Serialize)]
struct ChatCompletionResponse {
    id: String,
    object: String,
    created: u64,
    model: String,
    choices: Vec<ChatChoice>,
    usage: Usage,
}

#[derive(Debug, Serialize)]
struct ChatChoice {
    index: u32,
    message: ChatMessage,
    finish_reason: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct Usage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

#[derive(Debug, Serialize)]
struct ModelsResponse {
    object: String,
    data: Vec<Model>,
}

#[derive(Debug, Serialize)]
struct Model {
    id: String,
    object: String,
    created: u64,
    owned_by: String,
}
