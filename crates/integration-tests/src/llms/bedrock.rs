use super::openai::ModelConfig;
use super::provider::{LlmProviderConfig, ProviderType, TestLlmProvider};
use axum::{
    Router,
    extract::{Json, Path, Request, State},
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

async fn invoke_model(
    Path(model_id): Path<String>,
    State(state): State<Arc<TestState>>,
    Json(body): Json<Value>,
) -> Response {
    println!("Bedrock mock server non-streaming request for model: {}", model_id);
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
    } else if model_id.starts_with("meta.") {
        body["prompt"].as_str().map(String::from)
    } else if model_id.starts_with("mistral.") {
        body["prompt"].as_str().map(String::from)
    } else if model_id.starts_with("cohere.") {
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

async fn invoke_model_streaming(
    Path(model_id): Path<String>,
    State(_state): State<Arc<TestState>>,
    Json(_body): Json<Value>,
) -> Response {
    println!("Bedrock mock server streaming request for model: {}", model_id);

    // For now, return an error since streaming implementation is complex
    (StatusCode::NOT_IMPLEMENTED, "Streaming not implemented in mock").into_response()
}
