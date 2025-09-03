use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use anyhow;
use axum::{
    Router,
    extract::{Json, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response, Sse, sse::Event},
    routing::{get, post},
};
use futures::stream;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;

use super::provider::{LlmProviderConfig, TestLlmProvider};
use super::{common::find_custom_response_in_text, openai::ModelConfig};
use crate::headers::HeaderRecorder;

/// Builder for Google test server
pub struct GoogleMock {
    name: String,
    models: Vec<String>,
    custom_responses: HashMap<String, String>,
    streaming_enabled: bool,
    streaming_chunks: Option<Vec<String>>,
    tool_calls: Vec<(String, String)>, // (function_name, arguments)
    parallel_tool_calls: Vec<(String, String)>,
    streaming_tool_calls: Option<(String, String)>,
    captured_headers: Arc<Mutex<Vec<(String, String)>>>,
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
            tool_calls: Vec::new(),
            parallel_tool_calls: Vec::new(),
            streaming_tool_calls: None,
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

    pub fn with_tool_call(mut self, function_name: &str, arguments: &str) -> Self {
        self.tool_calls.push((function_name.to_string(), arguments.to_string()));
        self
    }

    pub fn with_parallel_tool_calls(mut self, calls: Vec<(&str, &str)>) -> Self {
        self.parallel_tool_calls = calls
            .into_iter()
            .map(|(name, args)| (name.to_string(), args.to_string()))
            .collect();
        self
    }

    pub fn with_streaming_tool_call(mut self, function_name: &str, arguments: &str) -> Self {
        self.streaming_tool_calls = Some((function_name.to_string(), arguments.to_string()));
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

    fn model_configs(&self) -> Vec<ModelConfig> {
        // Return model configs based on the models in the mock
        self.models.iter().map(ModelConfig::new).collect()
    }

    async fn spawn(self: Box<Self>) -> anyhow::Result<LlmProviderConfig> {
        let model_configs = self.model_configs();
        let state = Arc::new(TestGoogleState {
            models: self.models,
            custom_responses: self.custom_responses,
            streaming_enabled: self.streaming_enabled,
            streaming_chunks: self.streaming_chunks,
            tool_calls: self.tool_calls,
            parallel_tool_calls: self.parallel_tool_calls,
            streaming_tool_calls: self.streaming_tool_calls,
            captured_headers: self.captured_headers.clone(),
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
            model_configs,
        })
    }
}

/// Test Google server state
struct TestGoogleState {
    models: Vec<String>,
    custom_responses: HashMap<String, String>,
    streaming_enabled: bool,
    streaming_chunks: Option<Vec<String>>,
    tool_calls: Vec<(String, String)>,
    parallel_tool_calls: Vec<(String, String)>,
    streaming_tool_calls: Option<(String, String)>,
    captured_headers: Arc<Mutex<Vec<(String, String)>>>,
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

/// Validates that a JSON schema doesn't contain additionalProperties
/// This simulates Google's actual API behavior which rejects this JSON Schema feature
fn validate_no_additional_properties(value: &serde_json::Value) -> Result<(), String> {
    if let Some(obj) = value.as_object() {
        // Check for additionalProperties at this level
        if obj.contains_key("additionalProperties") {
            return Err("additionalProperties is not supported in Google function parameters".to_string());
        }

        // Recursively check nested properties
        if let Some(properties) = obj.get("properties")
            && let Some(props_obj) = properties.as_object()
        {
            for (prop_name, prop_value) in props_obj {
                validate_no_additional_properties(prop_value).map_err(|e| format!("In property '{prop_name}': {e}"))?;
            }
        }

        // Check items for array types
        if let Some(items) = obj.get("items") {
            validate_no_additional_properties(items).map_err(|e| format!("In items: {e}"))?;
        }
    }

    Ok(())
}

/// Handle Google generateContent requests in native format
async fn generate_content(
    State(state): State<Arc<TestGoogleState>>,
    Path(path): Path<String>,
    headers: HeaderMap,
    Json(request): Json<GoogleGenerateRequest>,
) -> Response {
    // Capture headers
    let mut captured = Vec::new();
    for (name, value) in &headers {
        if let Ok(value_str) = value.to_str() {
            captured.push((name.to_string(), value_str.to_string()));
        }
    }
    *state.captured_headers.lock().unwrap() = captured;

    // Validate tools don't contain additionalProperties (Google rejects this)
    if let Some(tools) = &request.tools {
        for (tool_i, tool) in tools.iter().enumerate() {
            let Some(func_decls) = &tool.function_declarations else {
                continue;
            };

            for (decl_i, func_decl) in func_decls.iter().enumerate() {
                let Some(parameters) = &func_decl.parameters else {
                    continue;
                };

                if let Err(e) = validate_no_additional_properties(parameters) {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({
                            "error": {
                                "code": 400,
                                "message": format!("Invalid function parameters for '{}': {}", func_decl.name, e),
                                "status": "INVALID_ARGUMENT",
                                "details": [{
                                    "@type": "type.googleapis.com/google.rpc.BadRequest",
                                    "field_violations": [{
                                        "field": format!("tools[{tool_i}].function_declarations[{decl_i}].parameters"),
                                        "description": e
                                    }]
                                }]
                            }
                        })),
                    )
                        .into_response();
                }
            }
        }
    }
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

    // Extract text from contents (including function responses)
    let mut extracted_texts = Vec::new();
    for content in &request.contents {
        for part in &content.parts {
            if let Some(text) = &part.text {
                extracted_texts.push(text.clone());
            }
            // Also check function response content for matching
            if let Some(function_response) = &part.function_response {
                // Handle both string responses and object-wrapped responses
                if let Some(response_text) = function_response.response.as_str() {
                    extracted_texts.push(response_text.to_string());
                } else if let Some(obj) = function_response.response.as_object() {
                    // If it's wrapped in an object with a "result" field, extract that
                    if let Some(result) = obj.get("result").and_then(|v| v.as_str()) {
                        extracted_texts.push(result.to_string());
                    }
                }
            }
        }
    }
    let user_text = extracted_texts.join(" ");

    // Check if we should return a tool call
    let should_use_tools =
        !state.tool_calls.is_empty() || !state.parallel_tool_calls.is_empty() || state.streaming_tool_calls.is_some();

    if should_use_tools && request.tools.is_some() {
        // Generate tool call response
        let tool_calls = if !state.parallel_tool_calls.is_empty() {
            &state.parallel_tool_calls
        } else {
            &state.tool_calls
        };

        if is_streaming && let Some((name, args)) = &state.streaming_tool_calls {
            let model = path.split(':').next().unwrap_or("unknown");
            return generate_google_streaming_tool_response(model.to_string(), name.clone(), args.clone())
                .into_response();
        }

        let mut parts = Vec::new();
        for (function_name, arguments) in tool_calls {
            let args: serde_json::Value = serde_json::from_str(arguments)
                .unwrap_or_else(|_| serde_json::json!({"error": "Failed to parse arguments"}));

            parts.push(GooglePart {
                text: None,
                function_call: Some(GoogleFunctionCall {
                    name: function_name.clone(),
                    args,
                }),
                function_response: None,
            });
        }

        let response = GoogleGenerateResponse {
            candidates: vec![GoogleCandidate {
                content: GoogleContent {
                    parts,
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

        return (StatusCode::OK, Json(response)).into_response();
    }

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
                    function_call: None,
                    function_response: None,
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

/// Generate SSE streaming response for Google tool calls
fn generate_google_streaming_tool_response(
    model: String,
    function_name: String,
    arguments: String,
) -> axum::response::Sse<
    impl futures::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>> + 'static,
> {
    let mut events = Vec::new();

    // First chunk with function call
    let args: serde_json::Value =
        serde_json::from_str(&arguments).unwrap_or_else(|_| serde_json::json!({"error": "Failed to parse arguments"}));

    let tool_chunk = serde_json::json!({
        "candidates": [{
            "content": {
                "parts": [{
                    "functionCall": {
                        "name": function_name.clone(),
                        "args": args
                    }
                }],
                "role": "model"
            },
            "index": 0
        }],
        "modelVersion": model.clone()
    });
    events.push(axum::response::sse::Event::default().data(serde_json::to_string(&tool_chunk).unwrap()));

    // Final chunk with finish reason and empty function call to maintain tool call context
    let final_chunk = serde_json::json!({
        "candidates": [{
            "content": {
                "parts": [{
                    "functionCall": {
                        "name": function_name,
                        "args": {}
                    }
                }],
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
    events.push(axum::response::sse::Event::default().data(serde_json::to_string(&final_chunk).unwrap()));

    events.push(axum::response::sse::Event::default().data("[DONE]"));

    let stream = futures::stream::iter(events.into_iter().map(Ok));
    axum::response::Sse::new(stream)
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
    #[serde(rename = "functionCall")]
    function_call: Option<GoogleFunctionCall>,
    #[serde(rename = "functionResponse")]
    function_response: Option<GoogleFunctionResponse>,
}

#[derive(Debug, Deserialize, Serialize)]
struct GoogleFunctionCall {
    name: String,
    args: serde_json::Value,
}

#[derive(Debug, Deserialize, Serialize)]
struct GoogleFunctionResponse {
    name: String,
    response: serde_json::Value,
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
