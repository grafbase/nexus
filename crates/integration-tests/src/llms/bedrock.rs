use super::openai::ModelConfig;
use super::provider::{LlmProviderConfig, ProviderType, TestLlmProvider};
use axum::{
    Router,
    body::Bytes,
    extract::{Path, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpListener;

/// Mock AWS Bedrock provider for testing
pub struct BedrockMock {
    name: String,
    models: Vec<String>,
    model_configs: Vec<ModelConfig>,
    custom_responses: HashMap<String, String>,
    error_responses: HashMap<String, (u16, String)>,
}

impl BedrockMock {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            models: vec![
                "anthropic.claude-3-sonnet-20240229-v1:0".to_string(),
                "amazon.titan-text-express-v1".to_string(),
                "meta.llama3-70b-instruct-v1:0".to_string(),
            ],
            model_configs: vec![],
            custom_responses: HashMap::new(),
            error_responses: HashMap::new(),
        }
    }

    pub fn with_models(mut self, models: Vec<String>) -> Self {
        self.models = models;
        self
    }

    pub fn with_model_configs(mut self, configs: Vec<ModelConfig>) -> Self {
        self.model_configs = configs;
        self
    }

    pub fn with_response(mut self, trigger: impl Into<String>, response: impl Into<String>) -> Self {
        self.custom_responses.insert(trigger.into(), response.into());
        self
    }

    pub fn with_error(mut self, trigger: impl Into<String>, status: u16, message: impl Into<String>) -> Self {
        self.error_responses.insert(trigger.into(), (status, message.into()));
        self
    }
}

impl TestLlmProvider for BedrockMock {
    fn provider_type(&self) -> &str {
        "bedrock"
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn model_configs(&self) -> Vec<ModelConfig> {
        self.model_configs.clone()
    }

    async fn spawn(self: Box<Self>) -> anyhow::Result<LlmProviderConfig> {
        let state = Arc::new(TestState {
            models: self.models.clone(),
            custom_responses: self.custom_responses.clone(),
            error_responses: self.error_responses.clone(),
        });

        let app = Router::new()
            .route("/model/{model_id}/invoke", post(invoke_model))
            .route(
                "/model/{model_id}/invoke-with-response-stream",
                post(invoke_model_streaming),
            )
            .route("/model/{model_id}/converse", post(converse_model))
            .route("/model/{model_id}/converse-stream", post(converse_model_streaming))
            .fallback(fallback_handler)
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
            provider_type: ProviderType::Bedrock,
            model_configs: self.model_configs.clone(),
            model_filter: None,
        })
    }
}

#[derive(Clone)]
struct TestState {
    #[allow(dead_code)]
    models: Vec<String>,
    custom_responses: HashMap<String, String>,
    error_responses: HashMap<String, (u16, String)>,
}

async fn invoke_model(Path(model_id): Path<String>, State(state): State<Arc<TestState>>, body: Bytes) -> Response {
    println!("Bedrock mock server non-streaming request for model: {}", model_id);

    // Parse the body as JSON
    let body: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            println!("Failed to parse body as JSON: {}", e);
            return (StatusCode::BAD_REQUEST, format!("Invalid JSON: {}", e)).into_response();
        }
    };

    // Check for error triggers
    if let Some(prompt) = extract_prompt(&body, &model_id) {
        if let Some((status, message)) = state.error_responses.get(&prompt) {
            return (
                StatusCode::from_u16(*status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                message.clone(),
            )
                .into_response();
        }

        if let Some(response) = state.custom_responses.get(&prompt) {
            return generate_response(&model_id, response).into_response();
        }
    }

    // Generate default response based on model family
    let default_response = "Hello! I'm a mock Bedrock response.";
    generate_response(&model_id, default_response).into_response()
}

fn extract_prompt(body: &Value, model_id: &str) -> Option<String> {
    // Extract prompt based on model family
    if model_id.starts_with("anthropic.") {
        body["messages"]
            .as_array()?
            .last()?
            .get("content")?
            .as_str()
            .map(String::from)
    } else if model_id.starts_with("amazon.") {
        body["inputText"].as_str().map(String::from)
    } else if model_id.starts_with("meta.")
        || model_id.starts_with("mistral.")
        || model_id.starts_with("cohere.")
        || model_id.starts_with("deepseek.")
        || model_id.contains(".deepseek.")
    // Handle regional prefixes like us.deepseek.
    {
        body["prompt"].as_str().map(String::from)
    } else {
        None
    }
}

fn generate_response(model_id: &str, content: &str) -> (StatusCode, String) {
    let response = if model_id.starts_with("anthropic.") {
        // Anthropic Claude response format
        serde_json::json!({
            "id": format!("msg_{}", uuid::Uuid::new_v4()),
            "type": "message",
            "role": "assistant",
            "content": [{
                "type": "text",
                "text": content
            }],
            "model": model_id,
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 10,
                "output_tokens": 15
            }
        })
    } else if model_id.starts_with("amazon.") {
        // Amazon Titan response format
        serde_json::json!({
            "inputTextTokenCount": 10,
            "results": [{
                "tokenCount": 15,
                "outputText": content,
                "completionReason": "FINISH"
            }]
        })
    } else if model_id.starts_with("meta.") {
        // Meta Llama response format
        serde_json::json!({
            "generation": content,
            "prompt_token_count": 10,
            "generation_token_count": 15,
            "stop_reason": "stop"
        })
    } else if model_id.starts_with("mistral.") {
        // Mistral response format
        serde_json::json!({
            "outputs": [{
                "text": content,
                "stop_reason": "stop"
            }]
        })
    } else if model_id.starts_with("cohere.") {
        // Cohere response format
        serde_json::json!({
            "generations": [{
                "text": content,
                "finish_reason": "COMPLETE"
            }]
        })
    } else if model_id.starts_with("deepseek.") || model_id.contains(".deepseek.") {
        // DeepSeek response format (handles both deepseek. and us.deepseek. prefixes)
        serde_json::json!({
            "choices": [{
                "text": content,
                "stop_reason": "stop"
            }],
            "usage": {
                "input_tokens": 10,
                "output_tokens": 15,
                "total_tokens": 25
            }
        })
    } else {
        // Generic response
        serde_json::json!({
            "response": content
        })
    };

    (StatusCode::OK, response.to_string())
}

async fn fallback_handler(request: Request) -> Response {
    let method = request.method();
    let uri = request.uri();

    println!("Bedrock mock server received: {} {}", method, uri);
    log::debug!("Bedrock mock server fallback: {} {}", method, uri);

    (StatusCode::NOT_FOUND, format!("Path not found: {} {}", method, uri)).into_response()
}

async fn converse_model(Path(model_id): Path<String>, State(state): State<Arc<TestState>>, body: Bytes) -> Response {
    println!("Bedrock mock server converse request for model: {}", model_id);

    // Parse the body as JSON
    let body: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            println!("Failed to parse body as JSON: {}", e);
            return (StatusCode::BAD_REQUEST, format!("Invalid JSON: {}", e)).into_response();
        }
    };

    // Extract the user message content from the Converse API format
    let mut prompt = String::new();
    if let Some(messages) = body.get("messages").and_then(|m| m.as_array()) {
        for message in messages {
            if let Some(content) = message.get("content").and_then(|c| c.as_array()) {
                for content_block in content {
                    if let Some(text) = content_block.get("text").and_then(|t| t.as_str()) {
                        if !prompt.is_empty() {
                            prompt.push(' ');
                        }
                        prompt.push_str(text);
                    }
                }
            }
        }
    }

    // Check for error triggers
    if !prompt.is_empty() {
        if let Some((status, message)) = state.error_responses.get(&prompt) {
            return (
                StatusCode::from_u16(*status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                message.clone(),
            )
                .into_response();
        }

        if let Some(response) = state.custom_responses.get(&prompt) {
            return generate_converse_response(response).into_response();
        }
    }

    // Extract specific prompts for DeepSeek models that expect "User: <content>\nAssistant:" format
    if (model_id.starts_with("deepseek.") || model_id.contains(".deepseek."))
        && let Some(custom_prompt) = extract_deepseek_prompt(&body)
        && let Some(response) = state.custom_responses.get(&custom_prompt)
    {
        return generate_converse_response(response).into_response();
    }

    // Generate default response
    let default_response = "Hello! I'm a mock Bedrock Converse response.";
    generate_converse_response(default_response).into_response()
}

async fn converse_model_streaming(
    Path(model_id): Path<String>,
    State(_state): State<Arc<TestState>>,
    _body: Bytes,
) -> Response {
    println!("Bedrock mock server converse streaming request for model: {}", model_id);

    // For now, return an error since streaming implementation is complex
    (
        StatusCode::NOT_IMPLEMENTED,
        "Converse streaming not implemented in mock",
    )
        .into_response()
}

async fn invoke_model_streaming(
    Path(model_id): Path<String>,
    State(_state): State<Arc<TestState>>,
    _body: Bytes,
) -> Response {
    println!("Bedrock mock server streaming request for model: {}", model_id);

    // For now, return an error since streaming implementation is complex
    (StatusCode::NOT_IMPLEMENTED, "Streaming not implemented in mock").into_response()
}

/// Extract DeepSeek-specific prompt format from Converse API request
fn extract_deepseek_prompt(body: &Value) -> Option<String> {
    let messages = body.get("messages")?.as_array()?;
    let mut formatted_prompt = String::new();

    for message in messages {
        let role = message.get("role")?.as_str()?;
        let content = message.get("content")?.as_array()?;

        for content_block in content {
            if let Some(text) = content_block.get("text")?.as_str() {
                match role {
                    "user" => {
                        if !formatted_prompt.is_empty() {
                            formatted_prompt.push('\n');
                        }
                        formatted_prompt.push_str("User: ");
                        formatted_prompt.push_str(text);
                    }
                    "assistant" => {
                        formatted_prompt.push_str("\nAssistant: ");
                        formatted_prompt.push_str(text);
                    }
                    _ => {}
                }
            }
        }
    }

    if !formatted_prompt.is_empty() {
        formatted_prompt.push_str("\nAssistant:");
        Some(formatted_prompt)
    } else {
        None
    }
}

/// Generate Bedrock Converse API response format
fn generate_converse_response(content: &str) -> (StatusCode, String) {
    let response = serde_json::json!({
        "output": {
            "message": {
                "role": "assistant",
                "content": [{
                    "text": content
                }]
            }
        },
        "stopReason": "end_turn",
        "usage": {
            "inputTokens": 10,
            "outputTokens": 15,
            "totalTokens": 25
        },
        "metrics": {
            "latencyMs": 100
        }
    });

    (StatusCode::OK, response.to_string())
}
