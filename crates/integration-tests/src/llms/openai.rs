use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::{
    Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response, Sse, sse::Event},
    routing::{get, post},
};
use futures::stream;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::net::TcpListener;

use super::common::find_custom_response;
use super::provider::{LlmProviderConfig, TestLlmProvider};
use crate::headers::HeaderRecorder;

/// Model configuration for tests
#[derive(Clone)]
pub struct ModelConfig {
    /// The user-facing model ID (used in config)
    pub id: String,
    /// Optional rename - the actual provider model name
    pub rename: Option<String>,
}

impl ModelConfig {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            rename: None,
        }
    }

    pub fn with_rename(mut self, rename: impl Into<String>) -> Self {
        self.rename = Some(rename.into());
        self
    }
}

/// Builder for OpenAI test server
pub struct OpenAIMock {
    name: String,
    models: Vec<String>,
    model_configs: Option<Vec<ModelConfig>>,
    custom_responses: HashMap<String, String>,
    error_type: Option<ErrorType>,
    streaming_enabled: bool,
    streaming_chunks: Option<Vec<String>>,
    streaming_error: Option<String>,
    tool_response: Option<ToolCallResponse>,
    parallel_tool_calls: Option<Vec<(String, String)>>,
    captured_headers: Arc<Mutex<Vec<(String, String)>>>,
}

/// Tool call response configuration for testing
#[derive(Clone)]
pub struct ToolCallResponse {
    pub tool_name: String,
    pub tool_arguments: String,
    pub finish_reason: String,
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
            model_configs: None,
            custom_responses: HashMap::new(),
            error_type: None,
            streaming_enabled: false,
            streaming_chunks: None,
            streaming_error: None,
            tool_response: None,
            parallel_tool_calls: None,
            captured_headers: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Get a header recorder that can be used to inspect headers after the mock is moved
    pub fn header_recorder(&self) -> HeaderRecorder {
        HeaderRecorder::new(self.captured_headers.clone())
    }

    pub fn with_models(mut self, models: Vec<String>) -> Self {
        self.models = models;
        self
    }

    pub fn with_model_configs(mut self, configs: Vec<ModelConfig>) -> Self {
        self.model_configs = Some(configs);
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

    pub fn with_tool_call(mut self, tool_name: impl Into<String>, arguments: impl Into<String>) -> Self {
        self.tool_response = Some(ToolCallResponse {
            tool_name: tool_name.into(),
            tool_arguments: arguments.into(),
            finish_reason: "tool_calls".to_string(),
        });
        self
    }

    pub fn with_parallel_tool_calls(mut self, tool_calls: Vec<(&str, &str)>) -> Self {
        // Store parallel tool calls for the response
        self.parallel_tool_calls = Some(
            tool_calls
                .into_iter()
                .map(|(name, args)| (name.to_string(), args.to_string()))
                .collect(),
        );
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

    pub fn with_streaming_tool_call(mut self, tool_name: impl Into<String>, arguments: impl Into<String>) -> Self {
        self.streaming_enabled = true;
        self.tool_response = Some(ToolCallResponse {
            tool_name: tool_name.into(),
            tool_arguments: arguments.into(),
            finish_reason: "tool_calls".to_string(),
        });
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

    fn model_configs(&self) -> Vec<ModelConfig> {
        self.model_configs.clone().unwrap_or_else(|| {
            // Default models if none specified
            vec![ModelConfig::new("gpt-3.5-turbo"), ModelConfig::new("gpt-4")]
        })
    }

    async fn spawn(self: Box<Self>) -> anyhow::Result<LlmProviderConfig> {
        let model_configs = self.model_configs();
        let state = Arc::new(TestLlmState {
            models: self.models,
            custom_responses: self.custom_responses,
            error_type: self.error_type,
            streaming_enabled: self.streaming_enabled,
            streaming_chunks: self.streaming_chunks,
            streaming_error: self.streaming_error,
            tool_response: self.tool_response,
            parallel_tool_calls: self.parallel_tool_calls,
            captured_headers: self.captured_headers.clone(),
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
            model_configs,
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
    tool_response: Option<ToolCallResponse>,
    parallel_tool_calls: Option<Vec<(String, String)>>,
    captured_headers: Arc<Mutex<Vec<(String, String)>>>,
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
            tool_response: None,
            parallel_tool_calls: None,
            captured_headers: Arc::new(Mutex::new(Vec::new())),
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
    headers: HeaderMap,
    Json(request): Json<ChatCompletionRequest>,
) -> Result<Response, ErrorResponse> {
    // Capture headers
    let mut captured = Vec::new();
    for (name, value) in &headers {
        if let Ok(value_str) = value.to_str() {
            captured.push((name.to_string(), value_str.to_string()));
        }
    }
    *state.captured_headers.lock().unwrap() = captured;
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
        let tool_response = state.tool_response.clone();
        return Ok(generate_streaming_response(request, streaming_chunks, tool_response).into_response());
    }

    Ok(Json(generate_success_response(request, &state)).into_response())
}

/// Generate SSE streaming response
fn generate_streaming_response(
    request: ChatCompletionRequest,
    streaming_chunks: Option<Vec<String>>,
    tool_response: Option<ToolCallResponse>,
) -> Sse<impl futures::Stream<Item = Result<Event, std::convert::Infallible>> + 'static> {
    let model = request.model.clone();

    let mut events = Vec::new();

    // Check if this is a tool call streaming response
    if request.tools.is_some() {
        if let Some(tool) = tool_response {
            let id = format!("chatcmpl-{}", uuid::Uuid::new_v4());

            // First chunk: role
            let first_chunk = serde_json::json!({
                "id": &id,
                "object": "chat.completion.chunk",
                "created": 1677651200,
                "model": &model,
                "choices": [{
                    "index": 0,
                    "delta": {
                        "role": "assistant"
                    }
                }]
            });
            events.push(Event::default().data(serde_json::to_string(&first_chunk).unwrap()));

            // Second chunk: tool call with arguments
            let tool_chunk = serde_json::json!({
                "id": &id,
                "object": "chat.completion.chunk",
                "created": 1677651200,
                "model": &model,
                "choices": [{
                    "index": 0,
                    "delta": {
                        "tool_calls": [{
                            "index": 0,
                            "id": format!("call_{}", uuid::Uuid::new_v4()),
                            "type": "function",
                            "function": {
                                "name": tool.tool_name,
                                "arguments": tool.tool_arguments
                            }
                        }]
                    }
                }]
            });
            events.push(Event::default().data(serde_json::to_string(&tool_chunk).unwrap()));

            // Final chunk with finish reason
            let final_chunk = serde_json::json!({
                "id": &id,
                "object": "chat.completion.chunk",
                "created": 1677651200,
                "model": &model,
                "choices": [{
                    "index": 0,
                    "delta": {},
                    "finish_reason": "tool_calls"
                }],
                "usage": {
                    "prompt_tokens": 10,
                    "completion_tokens": 15,
                    "total_tokens": 25
                }
            });
            events.push(Event::default().data(serde_json::to_string(&final_chunk).unwrap()));
        }
    } else {
        // Regular text streaming
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
    }

    events.push(Event::default().data("[DONE]"));

    let stream = stream::iter(events.into_iter().map(Ok));
    Sse::new(stream)
}

fn generate_success_response(request: ChatCompletionRequest, state: &TestLlmState) -> ChatCompletionResponse {
    // Check if we should return parallel tool calls
    if request.tools.is_some() && state.parallel_tool_calls.is_some() {
        let parallel_calls = state.parallel_tool_calls.as_ref().unwrap();
        let tool_calls = parallel_calls
            .iter()
            .map(|(name, args)| ToolCall {
                id: format!("call_{}", uuid::Uuid::new_v4()),
                type_: "function".to_string(),
                function: FunctionCall {
                    name: name.clone(),
                    arguments: args.clone(),
                },
            })
            .collect();

        return ChatCompletionResponse {
            id: format!("chatcmpl-test-{}", uuid::Uuid::new_v4()),
            object: "chat.completion".to_string(),
            created: 1677651200,
            model: request.model.clone(),
            choices: vec![ChatChoice {
                index: 0,
                message: ChatMessage {
                    role: "assistant".to_string(),
                    content: None,
                    tool_calls: Some(tool_calls),
                    tool_call_id: None,
                },
                finish_reason: "tool_calls".to_string(),
            }],
            usage: Usage {
                prompt_tokens: 10,
                completion_tokens: 15,
                total_tokens: 25,
            },
        };
    }

    // Check if we should return a single tool call response
    if request.tools.is_some() && state.tool_response.is_some() {
        let tool_response = state.tool_response.as_ref().unwrap();
        return ChatCompletionResponse {
            id: format!("chatcmpl-test-{}", uuid::Uuid::new_v4()),
            object: "chat.completion".to_string(),
            created: 1677651200,
            model: request.model.clone(),
            choices: vec![ChatChoice {
                index: 0,
                message: ChatMessage {
                    role: "assistant".to_string(),
                    content: None,
                    tool_calls: Some(vec![ToolCall {
                        id: format!("call_{}", uuid::Uuid::new_v4()),
                        type_: "function".to_string(),
                        function: FunctionCall {
                            name: tool_response.tool_name.clone(),
                            arguments: tool_response.tool_arguments.clone(),
                        },
                    }]),
                    tool_call_id: None,
                },
                finish_reason: tool_response.finish_reason.clone(),
            }],
            usage: Usage {
                prompt_tokens: 10,
                completion_tokens: 15,
                total_tokens: 25,
            },
        };
    }

    let response_text = find_custom_response(&request.messages, &state.custom_responses, |m| {
        m.content.as_deref().unwrap_or("")
    })
    .unwrap_or_else(|| {
        if request
            .messages
            .iter()
            .any(|m| m.content.as_ref().is_some_and(|c| c.contains("error")))
        {
            "This is an error response for testing".to_string()
        } else if request
            .messages
            .iter()
            .any(|m| m.content.as_ref().is_some_and(|c| c.contains("Hello")))
        {
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
                content: Some(response_text),
                tool_calls: None,
                tool_call_id: None,
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
    #[serde(default)]
    tools: Option<Vec<Value>>,
    #[serde(default)]
    #[allow(dead_code)]
    tool_choice: Option<Value>,
    #[serde(default)]
    #[allow(dead_code)]
    parallel_tool_calls: Option<bool>,
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
    #[serde(skip_serializing_if = "Option::is_none", default)]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    tool_call_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ToolCall {
    id: String,
    #[serde(rename = "type")]
    type_: String,
    function: FunctionCall,
}

#[derive(Debug, Serialize, Deserialize)]
struct FunctionCall {
    name: String,
    arguments: String,
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
