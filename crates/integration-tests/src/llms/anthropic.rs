use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use anyhow;
use axum::{
    Router,
    extract::{Json, State},
    http::StatusCode,
    response::{IntoResponse, Response, Sse, sse::Event},
    routing::{get, post},
};
use futures::stream;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;

use super::provider::{LlmProviderConfig, TestLlmProvider};
use super::{common::find_custom_response, openai::ModelConfig};
use crate::headers::HeaderRecorder;

/// Builder for Anthropic test server
pub struct AnthropicMock {
    name: String,
    models: Vec<String>,
    custom_responses: HashMap<String, String>,
    streaming_enabled: bool,
    tool_response: Option<ToolCallResponse>,
    captured_headers: Arc<Mutex<Vec<(String, String)>>>,
}

/// Tool call response configuration for testing
#[derive(Clone)]
pub struct ToolCallResponse {
    pub tool_name: String,
    pub tool_arguments: String,
}

impl AnthropicMock {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            models: vec![
                "claude-3-5-sonnet-20241022".to_string(),
                "claude-3-5-haiku-20241022".to_string(),
                "claude-3-opus-20240229".to_string(),
                "claude-3-sonnet-20240229".to_string(),
                "claude-3-haiku-20240307".to_string(),
            ],
            custom_responses: HashMap::new(),
            streaming_enabled: false,
            tool_response: None,
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

    pub fn with_response(mut self, trigger: impl Into<String>, response: impl Into<String>) -> Self {
        self.custom_responses.insert(trigger.into(), response.into());
        self
    }

    pub fn with_streaming(mut self) -> Self {
        self.streaming_enabled = true;
        self
    }

    pub fn with_tool_call(mut self, tool_name: impl Into<String>, arguments: impl Into<String>) -> Self {
        self.tool_response = Some(ToolCallResponse {
            tool_name: tool_name.into(),
            tool_arguments: arguments.into(),
        });
        self
    }

    pub fn with_parallel_tool_calls(mut self, tool_calls: Vec<(&str, &str)>) -> Self {
        // For parallel tool calls, we'll store them as a single tool response with multiple calls
        // This will be handled differently in the response generation
        if let Some((name, args)) = tool_calls.first() {
            self.tool_response = Some(ToolCallResponse {
                tool_name: name.to_string(),
                tool_arguments: args.to_string(),
            });
        }
        self
    }
}

impl TestLlmProvider for AnthropicMock {
    fn provider_type(&self) -> &str {
        "anthropic"
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn model_configs(&self) -> Vec<ModelConfig> {
        // Return model configs based on the models in the mock
        self.models.iter().map(ModelConfig::new).collect()
    }

    async fn spawn(self: Box<Self>) -> anyhow::Result<LlmProviderConfig> {
        let model_configs = self.model_configs();
        let state = Arc::new(TestAnthropicState {
            models: self.models,
            custom_responses: self.custom_responses,
            streaming_enabled: self.streaming_enabled,
            tool_response: self.tool_response,
            captured_headers: self.captured_headers.clone(),
        });

        let app = Router::new()
            .route("/v1/messages", post(create_message))
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
            provider_type: super::provider::ProviderType::Anthropic,
            model_configs,
        })
    }
}

/// Test Anthropic server state
struct TestAnthropicState {
    models: Vec<String>,
    custom_responses: HashMap<String, String>,
    streaming_enabled: bool,
    tool_response: Option<ToolCallResponse>,
    captured_headers: Arc<Mutex<Vec<(String, String)>>>,
}

/// Spawn a test Anthropic server on a random port (legacy compatibility)
pub struct TestAnthropicServer {
    pub address: SocketAddr,
}

impl TestAnthropicServer {
    pub async fn spawn() -> anyhow::Result<Self> {
        let builder = AnthropicMock::new("test_anthropic");
        let config = Box::new(builder).spawn().await?;

        Ok(Self {
            address: config.address,
        })
    }

    pub fn url(&self) -> String {
        format!("http://{}/v1", self.address)
    }
}

/// Handle Anthropic message creation requests
async fn create_message(
    State(state): State<Arc<TestAnthropicState>>,
    headers: axum::http::HeaderMap,
    Json(request): Json<AnthropicMessageRequest>,
) -> Response {
    // Capture headers
    let mut captured = Vec::new();
    for (name, value) in &headers {
        if let Ok(value_str) = value.to_str() {
            captured.push((name.to_string(), value_str.to_string()));
        }
    }
    *state.captured_headers.lock().unwrap() = captured;

    // Validate required headers
    if !headers.contains_key("x-api-key") {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": {
                    "type": "authentication_error",
                    "message": "Missing x-api-key header"
                }
            })),
        )
            .into_response();
    }

    if !headers.contains_key("anthropic-version") {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": {
                    "type": "invalid_request_error",
                    "message": "Missing anthropic-version header"
                }
            })),
        )
            .into_response();
    }

    // Check if we should return a tool call
    if request.tools.is_some() && state.tool_response.is_some() {
        let tool_response = state.tool_response.as_ref().unwrap();

        // Parse the tool arguments as JSON
        let input: serde_json::Value = serde_json::from_str(&tool_response.tool_arguments)
            .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new()));

        // Check if streaming was requested
        if request.stream.unwrap_or(false) {
            if !state.streaming_enabled {
                let response = (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": {
                            "type": "invalid_request_error",
                            "message": "Streaming is not yet supported"
                        }
                    })),
                );

                return response.into_response();
            }

            return generate_anthropic_streaming_tool_response(request.model.clone(), tool_response.clone(), input)
                .into_response();
        }

        let response = AnthropicMessageResponse {
            id: format!("msg_{}", uuid::Uuid::new_v4()),
            response_type: "message".to_string(),
            role: "assistant".to_string(),
            content: vec![AnthropicContent {
                content_type: "tool_use".to_string(),
                text: None,
                id: Some(format!("toolu_{}", uuid::Uuid::new_v4())),
                name: Some(tool_response.tool_name.clone()),
                input: Some(input),
            }],
            model: request.model.clone(),
            stop_reason: Some("tool_use".to_string()),
            stop_sequence: None,
            usage: AnthropicUsage {
                input_tokens: 10,
                output_tokens: 15,
            },
        };

        return (StatusCode::OK, Json(response)).into_response();
    }

    // Extract all message content for trigger matching
    let message_contents: Vec<String> = request.messages.iter().map(|m| m.content.as_text()).collect();

    let response_text = find_custom_response(&message_contents, &state.custom_responses, |s| s).unwrap_or_else(|| {
        format!(
            "Test response to: {}",
            message_contents.first().unwrap_or(&"empty".to_string())
        )
    });

    // Check if streaming was requested
    if request.stream.unwrap_or(false) {
        if !state.streaming_enabled {
            let response = (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": {
                        "type": "invalid_request_error",
                        "message": "Streaming is not yet supported"
                    }
                })),
            );

            return response.into_response();
        }

        return generate_anthropic_streaming_response(request.model.clone(), response_text).into_response();
    }

    let response = AnthropicMessageResponse {
        id: format!("msg_{}", uuid::Uuid::new_v4()),
        response_type: "message".to_string(),
        role: "assistant".to_string(),
        content: vec![AnthropicContent {
            content_type: "text".to_string(),
            text: Some(response_text),
            id: None,
            name: None,
            input: None,
        }],
        model: request.model.clone(),
        stop_reason: Some("end_turn".to_string()),
        stop_sequence: None,
        usage: AnthropicUsage {
            input_tokens: 10,
            output_tokens: 15,
        },
    };

    (StatusCode::OK, Json(response)).into_response()
}

/// Handle list models requests (Anthropic doesn't have this endpoint, but we mock it for testing)
async fn list_models(State(state): State<Arc<TestAnthropicState>>) -> Json<AnthropicModelsResponse> {
    let models = state
        .models
        .iter()
        .enumerate()
        .map(|(i, id)| {
            AnthropicModel {
                id: id.clone(),
                created: Some(1709164800 + i as u64 * 86400), // Incremental timestamps
            }
        })
        .collect();

    Json(AnthropicModelsResponse { data: models })
}

/// Generate SSE streaming response for Anthropic (in native Anthropic format)
fn generate_anthropic_streaming_response(
    model: String,
    response_text: String,
) -> Sse<impl futures::Stream<Item = Result<Event, std::convert::Infallible>> + 'static> {
    let mut events = Vec::new();

    let message_id = format!("msg_{}", uuid::Uuid::new_v4());

    // 1. message_start event
    let message_start = serde_json::json!({
        "type": "message_start",
        "message": {
            "id": message_id,
            "type": "message",
            "role": "assistant",
            "model": model,
            "content": [],
            "stop_reason": null,
            "stop_sequence": null,
            "usage": {
                "input_tokens": 10,
                "output_tokens": 0
            }
        }
    });
    events.push(Event::default().data(serde_json::to_string(&message_start).unwrap()));

    // 2. content_block_start event
    let content_block_start = serde_json::json!({
        "type": "content_block_start",
        "index": 0,
        "content_block": {
            "type": "text",
            "text": ""
        }
    });
    events.push(Event::default().data(serde_json::to_string(&content_block_start).unwrap()));

    // 3. content_block_delta event with the actual text
    let content_block_delta = serde_json::json!({
        "type": "content_block_delta",
        "index": 0,
        "delta": {
            "type": "text_delta",
            "text": response_text
        }
    });
    events.push(Event::default().data(serde_json::to_string(&content_block_delta).unwrap()));

    // 4. content_block_stop event
    let content_block_stop = serde_json::json!({
        "type": "content_block_stop",
        "index": 0
    });
    events.push(Event::default().data(serde_json::to_string(&content_block_stop).unwrap()));

    // 5. message_delta event with usage and stop reason
    let message_delta = serde_json::json!({
        "type": "message_delta",
        "delta": {
            "stop_reason": "end_turn",
            "stop_sequence": null
        },
        "usage": {
            "input_tokens": 10,
            "output_tokens": 15
        }
    });
    events.push(Event::default().data(serde_json::to_string(&message_delta).unwrap()));

    // 6. message_stop event
    let message_stop = serde_json::json!({
        "type": "message_stop"
    });
    events.push(Event::default().data(serde_json::to_string(&message_stop).unwrap()));

    let stream = stream::iter(events.into_iter().map(Ok));
    Sse::new(stream)
}

/// Generate SSE streaming response for Anthropic tool calls
fn generate_anthropic_streaming_tool_response(
    model: String,
    tool_response: ToolCallResponse,
    _input: serde_json::Value,
) -> Sse<impl futures::Stream<Item = Result<Event, std::convert::Infallible>> + 'static> {
    let mut events = Vec::new();

    let message_id = format!("msg_{}", uuid::Uuid::new_v4());
    let tool_id = format!("toolu_{}", uuid::Uuid::new_v4());

    // 1. message_start event
    let message_start = serde_json::json!({
        "type": "message_start",
        "message": {
            "id": message_id,
            "type": "message",
            "role": "assistant",
            "model": model,
            "content": [],
            "stop_reason": null,
            "stop_sequence": null,
            "usage": {
                "input_tokens": 10,
                "output_tokens": 0
            }
        }
    });
    events.push(Event::default().data(serde_json::to_string(&message_start).unwrap()));

    // 2. content_block_start event for tool use
    let content_block_start = serde_json::json!({
        "type": "content_block_start",
        "index": 0,
        "content_block": {
            "type": "tool_use",
            "id": tool_id,
            "name": tool_response.tool_name,
            "input": {}
        }
    });
    events.push(Event::default().data(serde_json::to_string(&content_block_start).unwrap()));

    // 3. content_block_delta event with the tool arguments
    let content_block_delta = serde_json::json!({
        "type": "content_block_delta",
        "index": 0,
        "delta": {
            "type": "input_json_delta",
            "partial_json": tool_response.tool_arguments
        }
    });
    events.push(Event::default().data(serde_json::to_string(&content_block_delta).unwrap()));

    // 4. content_block_stop event
    let content_block_stop = serde_json::json!({
        "type": "content_block_stop",
        "index": 0
    });
    events.push(Event::default().data(serde_json::to_string(&content_block_stop).unwrap()));

    // 5. message_delta event with tool_use stop reason
    let message_delta = serde_json::json!({
        "type": "message_delta",
        "delta": {
            "stop_reason": "tool_use",
            "stop_sequence": null
        },
        "usage": {
            "input_tokens": 10,
            "output_tokens": 15
        }
    });
    events.push(Event::default().data(serde_json::to_string(&message_delta).unwrap()));

    // 6. message_stop event
    let message_stop = serde_json::json!({
        "type": "message_stop"
    });
    events.push(Event::default().data(serde_json::to_string(&message_stop).unwrap()));

    let stream = stream::iter(events.into_iter().map(Ok));
    Sse::new(stream)
}

// Anthropic API types

#[derive(Debug, Deserialize)]
struct AnthropicMessageRequest {
    model: String,
    messages: Vec<AnthropicMessage>,
    #[serde(default)]
    #[allow(dead_code)]
    system: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    max_tokens: Option<u32>,
    #[serde(default)]
    #[allow(dead_code)]
    temperature: Option<f32>,
    #[serde(default)]
    #[allow(dead_code)]
    top_p: Option<f32>,
    #[serde(default)]
    #[allow(dead_code)]
    top_k: Option<u32>,
    #[serde(default)]
    #[allow(dead_code)]
    stop_sequences: Option<Vec<String>>,
    #[serde(default)]
    stream: Option<bool>,
    #[serde(default)]
    tools: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    #[allow(dead_code)]
    tool_choice: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct AnthropicMessage {
    #[allow(dead_code)]
    role: String,
    content: AnthropicMessageContent,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum AnthropicMessageContent {
    /// Simple text content
    Text(String),
    /// Array of content blocks (for tool use/results)
    Blocks(Vec<AnthropicContentBlock>),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum AnthropicContentBlock {
    /// Text content block
    #[serde(rename = "text")]
    Text { text: String },

    /// Tool use block (when assistant calls a tool)
    #[serde(rename = "tool_use")]
    ToolUse {
        #[allow(dead_code)]
        id: String,
        #[allow(dead_code)]
        name: String,
        #[allow(dead_code)]
        input: serde_json::Value,
    },

    /// Tool result block (response from tool execution)
    #[serde(rename = "tool_result")]
    ToolResult {
        #[allow(dead_code)]
        tool_use_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[allow(dead_code)]
        is_error: Option<bool>,
    },
}

impl AnthropicMessageContent {
    /// Extract text content for trigger matching
    fn as_text(&self) -> String {
        match self {
            AnthropicMessageContent::Text(text) => text.clone(),
            AnthropicMessageContent::Blocks(blocks) => {
                // Extract text from all text blocks and tool result blocks
                blocks
                    .iter()
                    .filter_map(|block| match block {
                        AnthropicContentBlock::Text { text } => Some(text.clone()),
                        AnthropicContentBlock::ToolResult { content, .. } => content.clone(),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            }
        }
    }
}

#[derive(Debug, Serialize)]
struct AnthropicMessageResponse {
    id: String,
    #[serde(rename = "type")]
    response_type: String,
    role: String,
    content: Vec<AnthropicContent>,
    model: String,
    stop_reason: Option<String>,
    stop_sequence: Option<String>,
    usage: AnthropicUsage,
}

#[derive(Debug, Serialize)]
struct AnthropicContent {
    #[serde(rename = "type")]
    content_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    input: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct AnthropicUsage {
    input_tokens: i32,
    output_tokens: i32,
}

#[derive(Debug, Serialize)]
struct AnthropicModelsResponse {
    data: Vec<AnthropicModel>,
}

#[derive(Debug, Serialize)]
struct AnthropicModel {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    created: Option<u64>,
}
