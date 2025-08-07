use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

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

use super::common::find_custom_response;
use super::provider::{LlmProviderConfig, TestLlmProvider};

/// Builder for Anthropic test server
pub struct AnthropicMock {
    name: String,
    models: Vec<String>,
    custom_responses: HashMap<String, String>,
    streaming_enabled: bool,
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

    pub fn with_streaming(mut self) -> Self {
        self.streaming_enabled = true;
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

    async fn spawn(self: Box<Self>) -> anyhow::Result<LlmProviderConfig> {
        let state = Arc::new(TestAnthropicState {
            models: self.models,
            custom_responses: self.custom_responses,
            streaming_enabled: self.streaming_enabled,
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
        })
    }
}

/// Test Anthropic server state
struct TestAnthropicState {
    models: Vec<String>,
    custom_responses: HashMap<String, String>,
    streaming_enabled: bool,
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

    let response_text = find_custom_response(&request.messages, &state.custom_responses, |m| &m.content)
        .unwrap_or_else(|| {
            format!(
                "Test response to: {}",
                request.messages.first().map(|m| m.content.as_str()).unwrap_or("empty")
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
            text: response_text,
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
}

#[derive(Debug, Deserialize)]
struct AnthropicMessage {
    #[allow(dead_code)]
    role: String,
    content: String,
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
    text: String,
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
