# Implementing LLM Providers in Nexus

This guide describes how to add support for a new LLM provider to the Nexus system. The process involves creating configuration structures, implementing the provider trait, adding integration tests, and connecting everything to the server.

## Overview

The LLM crate provides a unified interface for interacting with different LLM providers (OpenAI, Anthropic, Google, etc.). Each provider implements the `Provider` trait, which standardizes chat completion and model listing operations.

## Implementation Steps

### 1. Add Configuration Structure (config crate)

First, define the configuration for your provider in `crates/config/src/llm.rs`:

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct YourProviderConfig {
    /// API key for authentication
    pub api_key: SecretString,

    /// Optional custom API URL (useful for proxies or self-hosted instances)
    #[serde(default)]
    pub api_url: Option<String>,

    // Add any provider-specific configuration fields
}
```

Add your config to the `LlmProviderConfig` enum:

```rust
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LlmProviderConfig {
    OpenAi(OpenAiConfig),
    Anthropic(AnthropicConfig),
    Google(GoogleConfig),
    YourProvider(YourProviderConfig),  // Add your provider here
}
```

Write tests for configuration parsing in `crates/config/src/llm.rs`:

```rust
#[test]
fn test_your_provider_config() {
    let config = indoc! {r#"
        [llm.providers.anthropic]
        type = "anthropic"
        api_key = "asdf"
    "#};

    let parsed: Config = toml::from_str(config).unwrap();

    // Assert the configuration is parsed correctly with insta snapshots
    insta::assert_debug_snapshot!(&parsed.llm, @r###"
        YourProvider {
            type: "your_provider",
            api_key: SecretString("test-key"),
        }
    "###);
}
```

### 2. Implement the Provider (llm crate)

Create a new module in `crates/llm/src/provider/your_provider.rs`:

```rust
mod input;
mod output;

use async_trait::async_trait;
use crate::{
    error::LlmError,
    messages::{ChatCompletionRequest, ChatCompletionResponse, Model},
    provider::{ChatCompletionStream, Provider},
};

pub(crate) struct YourProvider {
    client: reqwest::Client,
    base_url: String,
    name: String,
}

impl YourProvider {
    pub fn new(name: String, config: YourProviderConfig) -> crate::Result<Self> {
        // Initialize HTTP client with authentication headers
        // Set up base URL and other provider-specific setup
    }
}

#[async_trait]
impl Provider for YourProvider {
    async fn chat_completion(&self, request: ChatCompletionRequest) -> crate::Result<ChatCompletionResponse> {
        // Convert request to provider format using From trait
        let provider_request = YourProviderRequest::from(request);

        // Make API call ...

        // Convert response back using From trait
        let response = ChatCompletionResponse::from(api_response);

        Ok(response)
    }

    async fn chat_completion_stream(&self, request: ChatCompletionRequest) -> crate::Result<ChatCompletionStream> {
        // Convert request to provider format with streaming enabled
        let mut provider_request = YourProviderRequest::from(request);
        provider_request.stream = true;

        // Make streaming API call and return SSE stream
        // See OpenAI/Anthropic/Google providers for examples
        
        // Return Err(LlmError::StreamingNotSupported) if provider doesn't support streaming
    }

    async fn list_models(&self) -> crate::Result<Vec<Model>> {
        // Fetch and return available models
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn supports_streaming(&self) -> bool {
        true  // or false if provider doesn't support streaming
    }
```

### 3. Define Input/Output Types

Create `input.rs` with provider-specific request types:

```rust
use serde::Serialize;
use crate::messages::{ChatCompletionRequest, ChatMessage};

/// Request format for your provider's API
#[derive(Debug, Serialize)]
pub(super) struct YourProviderRequest {
    // Provider-specific fields
    // Document each field with rustdoc comments based on the API documentation
    
    /// Enable streaming responses (if supported)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

impl From<ChatCompletionRequest> for YourProviderRequest {
    fn from(request: ChatCompletionRequest) -> Self {
        // Transform common request to provider format
        // Handle role mapping (e.g., system messages)
        // Map optional fields appropriately
        // Note: Don't set stream here - it's set in chat_completion_stream
    }
}
```

Create `output.rs` with provider-specific response types:

```rust
use serde::Deserialize;
use crate::messages::{ChatCompletionResponse, ChatCompletionChunk, Model, FinishReason};

/// Response format from your provider's API
#[derive(Debug, Deserialize)]
pub(super) struct YourProviderResponse {
    // Provider-specific fields
    // Document each field with rustdoc comments based on the API documentation
}

/// Streaming chunk format from your provider's API
#[derive(Debug, Deserialize)]
pub(super) struct YourProviderStreamChunk<'a> {
    // Use borrowed strings (&'a str) for better performance
    // Document fields based on provider's streaming API
    
    #[serde(borrow)]
    pub id: Cow<'a, str>,
    
    // Other fields...
}

impl<'a> YourProviderStreamChunk<'a> {
    /// Convert provider's streaming chunk to OpenAI-compatible format
    pub(super) fn into_chunk(self, provider_name: &str) -> ChatCompletionChunk {
        ChatCompletionChunk {
            id: self.id.into_owned(),
            object: ObjectType::ChatCompletionChunk,
            created: self.created,
            model: format!("{}/{}", provider_name, self.model),
            choices: /* convert choices */,
            usage: /* include in final chunk */,
        }
    }
}

// Define provider-specific enums with Other variants for forward compatibility
#[derive(Debug, Deserialize)]
pub(super) enum YourProviderFinishReason {
    Stop,
    Length,
    // ... other known reasons
    #[serde(untagged)]
    Other(String),  // Captures unknown values
}

impl From<YourProviderResponse> for ChatCompletionResponse {
    fn from(response: YourProviderResponse) -> Self {
        // Transform provider response to common format
        // Map finish reasons appropriately
        // Handle usage statistics
    }
}

impl From<YourProviderFinishReason> for FinishReason {
    fn from(reason: YourProviderFinishReason) -> Self {
        match reason {
            YourProviderFinishReason::Stop => FinishReason::Stop,
            YourProviderFinishReason::Length => FinishReason::Length,
            YourProviderFinishReason::Other(s) => {
                log::warn!("Unknown finish reason from provider: {s}");
                FinishReason::Other(s)
            }
        }
    }
}
```

### 4. Important Design Patterns

#### Forward Compatibility with Enums

Use enums with `Other(String)` variants for API fields that might have new values in the future:

```rust
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiEnum {
    KnownValue1,
    KnownValue2,
    #[serde(untagged)]
    Other(String),  // Captures any unknown value
}
```

This prevents breaking changes when providers add new enum values.

#### Async Trait Compatibility

The `Provider` trait uses `#[async_trait]` to enable dynamic dispatch:

```rust
/// Type alias for streaming chat completion responses.
pub(crate) type ChatCompletionStream = Pin<Box<dyn Stream<Item = crate::Result<ChatCompletionChunk>> + Send>>;

#[async_trait]
pub(crate) trait Provider: Send + Sync {
    async fn chat_completion(&self, request: ChatCompletionRequest) -> crate::Result<ChatCompletionResponse>;
    
    /// Stream chat completion responses. Default returns StreamingNotSupported error.
    async fn chat_completion_stream(&self, request: ChatCompletionRequest) -> crate::Result<ChatCompletionStream> {
        Err(LlmError::StreamingNotSupported)
    }
    
    async fn list_models(&self) -> crate::Result<Vec<Model>>;
    fn name(&self) -> &str;
    
    /// Check if provider supports streaming. Default is false.
    fn supports_streaming(&self) -> bool {
        false
    }
}
```

This allows storing providers as `Box<dyn Provider>`.

Note: The trait uses `crate::Result` which is an alias for `Result<T, LlmError>`.

#### Error Handling

The LLM crate uses a structured error handling approach with the `LlmError` enum:

```rust
pub(crate) enum LlmError {
    InvalidModelFormat(String),           // 400 - Invalid model format
    InvalidRequest(String),                // 400 - Bad request
    StreamingNotSupported,                 // 501 - Provider doesn't support streaming
    AuthenticationFailed(String),          // 401 - Auth failure
    InsufficientQuota(String),            // 403 - Quota exceeded
    ProviderNotFound(String),              // 404 - Provider not found
    ModelNotFound(String),                 // 404 - Model not found
    RateLimitExceeded(String),            // 429 - Rate limited
    InternalError(Option<String>),        // 500 - Internal error (see below)
    ProviderApiError { status, message }, // 502 - Other provider errors
    ConnectionError(String),               // 502 - Network errors
}
```

**Critical: Internal Error Handling**

`InternalError(Option<String>)` has special semantics:
- `None`: Internal Nexus error - NEVER expose details to clients
- `Some(message)`: Provider 500 error - pass through the provider's message

```rust
// Internal Nexus error - log details, return generic message
.map_err(|e| {
    log::error!("Failed to parse response: {e}");
    LlmError::InternalError(None)
})?

// Provider 500 error - pass through their message
match status.as_u16() {
    500 => LlmError::InternalError(Some(error_text)),
    // ... other status codes
}
```

**Error Mapping in Providers**

```rust
if !status.is_success() {
    let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
    log::error!("Provider API error ({status}): {error_text}");

    return Err(match status.as_u16() {
        400 => LlmError::InvalidRequest(error_text),
        401 => LlmError::AuthenticationFailed(error_text),
        403 => LlmError::InsufficientQuota(error_text),
        404 => LlmError::ModelNotFound(error_text),
        429 => LlmError::RateLimitExceeded(error_text),
        500 => LlmError::InternalError(Some(error_text)),
        _ => LlmError::ProviderApiError {
            status: status.as_u16(),
            message: error_text,
        },
    });
}
```

**Logging Requirements**

- All 5xx errors are automatically logged in `IntoResponse`
- Log internal errors with full details before returning `InternalError(None)`
- Provider errors should be logged at the point of occurrence

### 5. Implementing Streaming Support

Streaming is implemented using Server-Sent Events (SSE) for all providers. **IMPORTANT**: Each provider must convert its native streaming format to the OpenAI-compatible `ChatCompletionChunk` format to ensure consistency across all providers.

#### SSE Stream Processing

```rust
use eventsource_stream::Eventsource;
use futures::StreamExt;
use crate::messages::{ChatCompletionChunk, ObjectType, ChunkChoice, Delta};

async fn chat_completion_stream(&self, request: ChatCompletionRequest) -> crate::Result<ChatCompletionStream> {
    let url = format!("{}/streaming-endpoint", self.base_url);
    
    // Enable streaming in the request
    let mut provider_request = YourProviderRequest::from(request);
    provider_request.stream = Some(true);
    
    // Make the streaming request
    let response = self.client
        .post(&url)
        .json(&provider_request)
        .send()
        .await
        .map_err(|e| LlmError::ConnectionError(e.to_string()))?;
    
    // Check for errors
    if !response.status().is_success() {
        // Handle error responses appropriately
        return Err(/* appropriate error */);
    }
    
    // Convert response to SSE stream
    let stream = response
        .bytes_stream()
        .eventsource()
        .filter_map(move |event| {
            // Process SSE events
            match event {
                Ok(event) if event.data == "[DONE]" => None,
                Ok(event) => {
                    // Parse the provider's native chunk format
                    let native_chunk = sonic_rs::from_str::<YourProviderStreamChunk>(&event.data)
                        .ok()?;
                    
                    // CRITICAL: Convert to OpenAI-compatible format
                    // This ensures all providers return the same chunk structure
                    let openai_chunk = ChatCompletionChunk {
                        id: native_chunk.id.into_owned(),
                        object: ObjectType::ChatCompletionChunk,
                        created: native_chunk.created,
                        model: format!("{}/{}", provider_name, native_chunk.model),
                        choices: vec![ChunkChoice {
                            index: 0,
                            delta: Delta {
                                role: /* map role if first chunk */,
                                content: /* extract incremental content */,
                            },
                            finish_reason: /* map finish reason if present */,
                        }],
                        usage: /* include in final chunk only */,
                    };
                    
                    Some(Ok(openai_chunk))
                }
                Err(e) => Some(Err(LlmError::ConnectionError(e.to_string()))),
            }
        });
    
    Ok(Box::pin(stream))
}
```

#### Converting Provider Formats to OpenAI Format

Each provider has a different streaming format that must be converted:

**Anthropic Example:**
```rust
// Anthropic sends: message_start, content_block_delta, message_delta, message_stop
// Convert to: OpenAI ChatCompletionChunk with delta.content
```

**Google Example:**
```rust
// Google sends: candidates[].content.parts[].text
// Convert to: OpenAI ChatCompletionChunk with delta.content
```

**Your Provider:**
```rust
impl<'a> YourProviderStreamChunk<'a> {
    /// Convert provider's native format to OpenAI-compatible ChatCompletionChunk
    pub(super) fn into_chunk(self, provider_name: &str) -> ChatCompletionChunk {
        // Map provider-specific fields to OpenAI format:
        // - Extract incremental text content
        // - Map finish reasons (stop, length, etc.)
        // - Include role in first chunk's delta
        // - Add usage statistics in final chunk
        // - Prefix model name with provider
        
        ChatCompletionChunk {
            id: self.id.into_owned(),
            object: ObjectType::ChatCompletionChunk,
            created: self.created,
            model: format!("{}/{}", provider_name, self.model),
            choices: /* convert provider's choice format */,
            usage: /* only in final chunk */,
        }
    }
}
```

#### Key Points for Streaming Implementation

1. **Convert to OpenAI format**: ALL providers must return `ChatCompletionChunk` format
2. **Use `eventsource_stream`**: For parsing SSE responses
3. **Use `sonic_rs`**: For fast JSON parsing with borrowed strings
4. **Handle `[DONE]` marker**: Most providers send this to signal stream end
5. **Include usage in final chunk**: Add token usage statistics only in the last chunk
6. **Prefix model names**: Always add provider prefix to model names in chunks
7. **First chunk has role**: Include `delta.role = "assistant"` in the first content chunk
8. **Error handling**: Convert stream errors to appropriate `LlmError` variants

#### Testing Streaming

Use the test helpers provided:

```rust
// Test basic streaming - returns parsed JSON chunks
let chunks = llm.stream_completions(request).await;
assert!(chunks.len() > 1);

// Verify chunk structure with snapshots
let first_chunk = &chunks[0];
insta::assert_json_snapshot!(first_chunk, {
    ".id" => "[id]",
    ".created" => "[created]"
});

// Test content accumulation - concatenates all delta.content
let content = llm.stream_completions_content(request).await;
assert_eq!(content, "Expected response text");
```

### 6. Integrate with Server

Add your provider to `crates/llm/src/server.rs`:

```rust
for (name, provider_config) in config.providers.into_iter() {
    log::debug!("Initializing provider: {name}");

    match provider_config {
        LlmProvider::Openai(config) => {
            let provider = Box::new(OpenAIProvider::new(name.clone(), config)?);
            providers.push(provider as Box<dyn Provider>)
        }
        LlmProvider::Anthropic(config) => {
            let provider = Box::new(AnthropicProvider::new(name.clone(), config)?);
            providers.push(provider as Box<dyn Provider>)
        }
        LlmProvider::Google(config) => {
            let provider = Box::new(GoogleProvider::new(name.clone(), config)?);
            providers.push(provider as Box<dyn Provider>)
        }
        ...
    }
}
```

### 6. Add Integration Tests

Create mock provider in `crates/integration-tests/src/llms/your_provider.rs`:

```rust
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use axum::{Router, extract::{Json, State}, http::StatusCode, routing::{get, post}};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use super::provider::{LlmProviderConfig, TestLlmProvider};

/// Builder for YourProvider test server
pub struct YourProviderMock {
    name: String,
    models: Vec<String>,
    custom_responses: HashMap<String, String>,
    streaming_enabled: bool,
    streaming_chunks: Option<Vec<String>>,
}

impl YourProviderMock {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            models: vec![
                "model-1".to_string(),
                "model-2".to_string(),
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
    
    pub fn with_streaming_chunks(mut self, chunks: Vec<String>) -> Self {
        self.streaming_chunks = Some(chunks);
        self.streaming_enabled = true;
        self
    }
}

impl TestLlmProvider for YourProviderMock {
    fn provider_type(&self) -> &str {
        "your_provider"
    }

    fn name(&self) -> &str {
        &self.name
    }

    async fn spawn(self: Box<Self>) -> anyhow::Result<LlmProviderConfig> {
        let state = Arc::new(TestState {
            models: self.models,
            custom_responses: self.custom_responses,
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
            provider_type: super::provider::ProviderType::YourProvider,
        })
    }
}

struct TestState {
    models: Vec<String>,
    custom_responses: HashMap<String, String>,
}

// Handle chat completion and list models requests
// See openai.rs, anthropic.rs, google.rs for full examples
```

Create tests in `crates/integration-tests/tests/llm/your_provider.rs`:

```rust
use indoc::indoc;
use integration_tests::{TestServer, llms::YourProviderMock};
use sonic_rs::json;

#[tokio::test]
async fn list_models() {
    let mut builder = TestServer::builder();
    builder.spawn_llm(YourProviderMock::new("test_provider")).await;

    let server = builder.build("").await;
    let llm = server.llm_client("/llm");
    let body = llm.list_models().await;

    insta::assert_json_snapshot!(body, {
        ".data[].created" => "[created]"
    }, @r#"
    {
      "object": "list",
      "data": [
        {
          "id": "test_provider/model-1",
          "object": "model",
          "created": "[created]",
          "owned_by": "your_provider"
        }
      ]
    }
    "#);
}

#[tokio::test]
async fn chat_completions_simple() {
    let mut builder = TestServer::builder();
    builder.spawn_llm(
        YourProviderMock::new("test_provider")
            .with_response("Hello", "Hello! How can I help?")
    ).await;

    let server = builder.build("").await;
    let llm = server.llm_client("/llm");

    let body = llm.simple_completion("test_provider/model-1", "Hello").await;

    insta::assert_json_snapshot!(body, {
        ".id" => "[uuid]"
    }, @r#"
    {
      "id": "[uuid]",
      "object": "chat.completion",
      "model": "test_provider/model-1",
      "choices": [{
        "index": 0,
        "message": {
          "role": "assistant",
          "content": "Hello! How can I help?"
        },
        "finish_reason": "stop"
      }],
      "usage": {
        "prompt_tokens": 10,
        "completion_tokens": 15,
        "total_tokens": 25
      }
    }
    "#);
}

#[tokio::test]
async fn chat_completions_streaming() {
    let mut builder = TestServer::builder();
    builder.spawn_llm(
        YourProviderMock::new("test_provider")
            .with_streaming()  // Enable streaming support
            .with_response("Hello", "Hello! How can I help?")
    ).await;

    let server = builder.build("").await;
    let llm = server.llm_client("/llm");

    let request = json!({
        "model": "test_provider/model-1",
        "messages": [{"role": "user", "content": "Hello"}],
        "stream": true
    });

    // Test streaming with stream_completions helper
    let chunks = llm.stream_completions(request).await;
    
    // Should have multiple chunks
    assert!(chunks.len() >= 2);
    
    // Verify chunk structure with snapshot
    let first_chunk = &chunks[0];
    insta::assert_json_snapshot!(first_chunk, {
        ".id" => "[id]",
        ".created" => "[created]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion.chunk",
      "created": "[created]",
      "model": "test_provider/model-1",
      "choices": [{
        "index": 0,
        "delta": {
          "role": "assistant",
          "content": "Hello"
        }
      }]
    }
    "#);
    
    // Test accumulated content
    let content = llm.stream_completions_content(request).await;
    assert_eq!(content, "Hello! How can I help?");
}
```

### 7. Documentation Requirements

Every type and function should have comprehensive rustdoc comments:

```rust
/// Represents a chat completion request to YourProvider API.
///
/// This structure follows the format documented at [provider docs URL].
///
/// # Example
/// ```
/// let request = YourProviderRequest {
///     model: "model-name".to_string(),
///     // ...
/// };
/// ```
pub struct YourProviderRequest {
    /// The model identifier to use for completion.
    /// See [models documentation](URL) for available models.
    pub model: String,

    // Document every field
}
```

### 8. Error Handling Tests

Always include tests for error scenarios:

```rust
#[tokio::test]
async fn handles_authentication_error() {
    let mock = YourProviderMock::new("provider")
        .with_auth_error("Invalid API key");

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;
    let server = builder.build("").await;

    let response = /* make request */;
    assert_eq!(response.status(), 401);
}

#[tokio::test]
async fn handles_rate_limiting() {
    let mock = YourProviderMock::new("provider")
        .with_rate_limit("Rate limit exceeded");

    // Test returns 429 status code
}
```

## Testing Checklist

- [ ] Configuration parsing tests in config crate
- [ ] Integration tests with mock servers using TestServer
- [ ] Error handling tests for all status codes
- [ ] Test streaming completions with `stream_completions()` helper
- [ ] Test streaming content accumulation with `stream_completions_content()`
- [ ] Test streaming error handling (network errors, malformed chunks)
- [ ] Test model listing with various scenarios
- [ ] Test chat completions with different parameters
- [ ] Test that last streaming chunk includes usage statistics

## Architecture Notes

### Model Name Format

Models are identified using the format `provider/model`:
- User requests: `"openai/gpt-4"`
- Provider receives: `"gpt-4"` (prefix stripped)
- Response includes: `"openai/gpt-4"` (prefix restored)

### Caching

Model lists are cached for 5 minutes to reduce API calls. The cache is automatically invalidated after this period.

### Concurrent Model Fetching

When listing models, all providers are queried concurrently using `FuturesUnordered`. Failed providers are logged but don't block the response - models from successful providers are still returned.

### Rate Limiting Integration

The LLM endpoints respect the global rate limiting configuration if enabled. Provider-specific rate limits should be handled by returning `LlmError::RateLimitExceeded`.

## Maintenance

When updating providers:
1. Check for API changes in official documentation
2. Update enum variants if new values are documented
3. Add new optional fields with `#[serde(default)]`
4. Update integration tests for new functionality
5. Review error handling - ensure proper status codes
6. Update this CLAUDE.md file if necessary

## Common Pitfalls

1. **Never expose internal errors**: Always use `InternalError(None)` for Nexus errors
2. **Always log 5xx errors**: These are critical for debugging
3. **Handle streaming properly**: Implement SSE streaming or return `StreamingNotSupported` error
4. **Test error scenarios**: Don't just test happy paths
5. **Include usage in final chunk**: The last streaming chunk should contain usage statistics
6. **Escape SSE data**: Ensure newlines in content are properly escaped in SSE format
