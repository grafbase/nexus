use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow;
use axum::{
    Router,
    extract::{Json, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response, Sse, sse::Event},
    routing::{get, post},
};
use futures::stream;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;

use super::common::find_custom_response_in_text;
use super::provider::{LlmProviderConfig, TestLlmProvider};

/// Builder for Google test server
pub struct GoogleMock {
    name: String,
    models: Vec<String>,
    custom_responses: HashMap<String, String>,
    streaming_enabled: bool,
    streaming_chunks: Option<Vec<String>>,
}

impl GoogleMock {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            models: vec![
                "gemini-1.5-flash".to_string(),
                "gemini-1.5-pro".to_string(),
                "gemini-pro".to_string(),
                "text-embedding-004".to_string(),
            ],
            custom_responses: HashMap::new(),
            streaming_enabled: false,
            streaming_chunks: None,
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

    pub fn with_streaming_text_with_newlines(mut self, text: &str) -> Self {
        // Split text to test escape sequence handling
        let mut chunks = Vec::new();

        // Split at paragraph breaks to test escape sequences
        if text.contains("\n\n") {
            let parts: Vec<&str> = text.split("\n\n").collect();
            for (i, part) in parts.iter().enumerate() {
                chunks.push(part.to_string());
                if i < parts.len() - 1 {
                    // Add paragraph break as separate chunk
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

    pub fn with_streaming_chunks(mut self, chunks: Vec<String>) -> Self {
        self.streaming_chunks = Some(chunks);
        self.streaming_enabled = true;
        self
    }
}

impl TestLlmProvider for GoogleMock {
    fn provider_type(&self) -> &str {
        "google"
    }

    fn name(&self) -> &str {
        &self.name
    }

    async fn spawn(self: Box<Self>) -> anyhow::Result<LlmProviderConfig> {
        let state = Arc::new(TestGoogleState {
            models: self.models,
            custom_responses: self.custom_responses,
            streaming_enabled: self.streaming_enabled,
            streaming_chunks: self.streaming_chunks,
        });

        let app = Router::new()
            .route("/v1beta/models/{*path}", post(generate_content))
            .route("/v1beta/models", get(list_models))
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
            provider_type: super::provider::ProviderType::Google,
        })
    }
}

/// Test Google server state
struct TestGoogleState {
    models: Vec<String>,
    custom_responses: HashMap<String, String>,
    streaming_enabled: bool,
    streaming_chunks: Option<Vec<String>>,
}

/// Legacy compatibility server
pub struct TestGoogleServer {
    pub address: SocketAddr,
}

impl TestGoogleServer {
    pub async fn spawn() -> anyhow::Result<Self> {
        let builder = GoogleMock::new("test_google");
        let config = Box::new(builder).spawn().await?;

        Ok(Self {
            address: config.address,
        })
    }

    pub fn url(&self) -> String {
        format!("http://{}/v1beta", self.address)
    }
}

/// Handle Google generateContent requests in native format
async fn generate_content(
    State(state): State<Arc<TestGoogleState>>,
    Path(path): Path<String>,
    Json(request): Json<GoogleGenerateRequest>,
) -> Response {
    // Ensure we're handling a generateContent or streamGenerateContent request
    if !path.ends_with(":generateContent") && !path.ends_with(":streamGenerateContent") {
        eprintln!(
            "Google mock received path: {path:?}, expected to end with :generateContent or :streamGenerateContent"
        );
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Not found"}))).into_response();
    }
    // Check for streaming based on endpoint path
    let is_streaming = path.ends_with(":streamGenerateContent");

    if is_streaming && !state.streaming_enabled {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": {
                    "code": 400,
                    "message": "Streaming is not yet supported",
                    "status": "INVALID_ARGUMENT"
                }
            })),
        )
            .into_response();
    }

    // Extract text from contents
    let user_text = request
        .contents
        .iter()
        .filter_map(|content| content.parts.iter().find_map(|part| part.text.as_ref()))
        .cloned()
        .join(" ");

    // Check for custom responses
    let response_text = find_custom_response_in_text(&user_text, &state.custom_responses);

    // Default response logic
    let response_text = response_text.unwrap_or_else(|| {
        if user_text.contains("error") {
            "This is an error response for testing".to_string()
        } else if user_text.contains("Hello") {
            "Hello! I'm Gemini, a test assistant. How can I help you today?".to_string()
        } else {
            // Include temperature in response if it was high
            let is_creative = request
                .generation_config
                .as_ref()
                .and_then(|c| c.temperature)
                .map(|t| t > 1.5)
                .unwrap_or(false);

            if is_creative {
                "This is a creative response due to high temperature".to_string()
            } else {
                format!("Test response to: {user_text}")
            }
        }
    });

    // Handle streaming if requested
    if is_streaming {
        // Extract model from path (e.g., "gemini-1.5-flash:generateContent" -> "gemini-1.5-flash")
        let model = path.split(':').next().unwrap_or("unknown");
        return generate_google_streaming_response(model.to_string(), response_text, state.streaming_chunks.clone())
            .into_response();
    }

    let response = GoogleGenerateResponse {
        candidates: vec![GoogleCandidate {
            content: GoogleContent {
                parts: vec![GooglePart {
                    text: Some(response_text),
                }],
                role: "model".to_string(),
            },
            finish_reason: "STOP".to_string(),
            index: 0,
            safety_ratings: vec![],
        }],
        usage_metadata: GoogleUsageMetadata {
            prompt_token_count: 10,
            candidates_token_count: 15,
            total_token_count: 25,
        },
    };

    (StatusCode::OK, Json(response)).into_response()
}

/// Handle list models requests in Google's native format
async fn list_models(State(state): State<Arc<TestGoogleState>>) -> Json<GoogleModelsResponse> {
    let models = state
        .models
        .iter()
        .map(|id| GoogleModel {
            name: format!("models/{id}"),
            base_model_id: Some(id.clone()),
            version: "1.0".to_string(),
            display_name: id.clone(),
            description: format!("Google {id} model"),
            input_token_limit: 1000000,
            output_token_limit: 8192,
            supported_generation_methods: vec!["generateContent".to_string()],
            temperature: Some(1.0),
            top_p: Some(1.0),
            top_k: Some(40),
        })
        .collect();

    Json(GoogleModelsResponse { models })
}

/// Generate SSE streaming response for Google in native Google format
fn generate_google_streaming_response(
    model: String,
    response_text: String,
    streaming_chunks: Option<Vec<String>>,
) -> Sse<impl futures::Stream<Item = Result<Event, std::convert::Infallible>> + 'static> {
    let mut events = Vec::new();

    // Use custom chunks if provided, otherwise use the full text
    let chunks = streaming_chunks.unwrap_or_else(|| vec![response_text]);

    // Generate a chunk for each text piece
    for chunk_text in &chunks {
        let chunk = serde_json::json!({
            "candidates": [{
                "content": {
                    "parts": [{
                        "text": chunk_text
                    }],
                    "role": "model"
                },
                "index": 0
            }],
            "modelVersion": model.clone()
        });
        events.push(Event::default().data(serde_json::to_string(&chunk).unwrap()));
    }

    let final_chunk = serde_json::json!({
        "candidates": [{
            "content": {
                "parts": [{}],
                "role": "model"
            },
            "finishReason": "STOP",
            "index": 0
        }],
        "usageMetadata": {
            "promptTokenCount": 10,
            "candidatesTokenCount": 15,
            "totalTokenCount": 25
        },
        "modelVersion": model
    });
    events.push(Event::default().data(serde_json::to_string(&final_chunk).unwrap()));

    events.push(Event::default().data("[DONE]"));

    let stream = stream::iter(events.into_iter().map(Ok));
    Sse::new(stream)
}

// Google API types based on the Gemini API specification

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GoogleGenerateRequest {
    contents: Vec<GoogleContent>,
    #[serde(rename = "generationConfig")]
    generation_config: Option<GoogleGenerationConfig>,
    #[serde(rename = "safetySettings")]
    safety_settings: Option<Vec<GoogleSafetySetting>>,
    tools: Option<Vec<GoogleTool>>,
    #[serde(rename = "toolConfig")]
    tool_config: Option<GoogleToolConfig>,
    #[serde(rename = "systemInstruction")]
    system_instruction: Option<GoogleContent>,
}

#[derive(Debug, Deserialize, Serialize)]
struct GoogleContent {
    parts: Vec<GooglePart>,
    role: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct GooglePart {
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GoogleGenerationConfig {
    #[serde(rename = "stopSequences")]
    stop_sequences: Option<Vec<String>>,
    #[serde(rename = "responseMimeType")]
    response_mime_type: Option<String>,
    #[serde(rename = "responseSchema")]
    response_schema: Option<serde_json::Value>,
    #[serde(rename = "candidateCount")]
    candidate_count: Option<i32>,
    #[serde(rename = "maxOutputTokens")]
    max_output_tokens: Option<i32>,
    temperature: Option<f32>,
    #[serde(rename = "topP")]
    top_p: Option<f32>,
    #[serde(rename = "topK")]
    top_k: Option<i32>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GoogleSafetySetting {
    category: String,
    threshold: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GoogleTool {
    #[serde(rename = "functionDeclarations")]
    function_declarations: Option<Vec<GoogleFunctionDeclaration>>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GoogleFunctionDeclaration {
    name: String,
    description: Option<String>,
    parameters: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GoogleToolConfig {
    #[serde(rename = "functionCallingConfig")]
    function_calling_config: Option<GoogleFunctionCallingConfig>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GoogleFunctionCallingConfig {
    mode: String,
    #[serde(rename = "allowedFunctionNames")]
    allowed_function_names: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
struct GoogleGenerateResponse {
    candidates: Vec<GoogleCandidate>,
    #[serde(rename = "usageMetadata")]
    usage_metadata: GoogleUsageMetadata,
}

#[derive(Debug, Serialize)]
struct GoogleCandidate {
    content: GoogleContent,
    #[serde(rename = "finishReason")]
    finish_reason: String,
    index: i32,
    #[serde(rename = "safetyRatings")]
    safety_ratings: Vec<GoogleSafetyRating>,
}

#[derive(Debug, Serialize)]
struct GoogleSafetyRating {
    category: String,
    probability: String,
}

#[derive(Debug, Serialize)]
struct GoogleUsageMetadata {
    #[serde(rename = "promptTokenCount")]
    prompt_token_count: i32,
    #[serde(rename = "candidatesTokenCount")]
    candidates_token_count: i32,
    #[serde(rename = "totalTokenCount")]
    total_token_count: i32,
}

#[derive(Debug, Serialize)]
struct GoogleModelsResponse {
    models: Vec<GoogleModel>,
}

#[derive(Debug, Serialize)]
struct GoogleModel {
    name: String,
    #[serde(rename = "baseModelId")]
    base_model_id: Option<String>,
    version: String,
    #[serde(rename = "displayName")]
    display_name: String,
    description: String,
    #[serde(rename = "inputTokenLimit")]
    input_token_limit: i32,
    #[serde(rename = "outputTokenLimit")]
    output_token_limit: i32,
    #[serde(rename = "supportedGenerationMethods")]
    supported_generation_methods: Vec<String>,
    temperature: Option<f32>,
    #[serde(rename = "topP")]
    top_p: Option<f32>,
    #[serde(rename = "topK")]
    top_k: Option<i32>,
}
