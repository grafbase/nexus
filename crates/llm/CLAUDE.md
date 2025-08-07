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
    provider::Provider,
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

    async fn list_models(&self) -> crate::Result<Vec<Model>> {
        // Fetch and return available models
    }

    fn name(&self) -> &str {
        &self.name
    }
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
}

impl From<ChatCompletionRequest> for YourProviderRequest {
    fn from(request: ChatCompletionRequest) -> Self {
        // Transform common request to provider format
        // Handle role mapping (e.g., system messages)
        // Map optional fields appropriately
    }
}
```

Create `output.rs` with provider-specific response types:

```rust
use serde::Deserialize;
use crate::messages::{ChatCompletionResponse, Model, FinishReason};

/// Response format from your provider's API
#[derive(Debug, Deserialize)]
pub(super) struct YourProviderResponse {
    // Provider-specific fields
    // Document each field with rustdoc comments based on the API documentation
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
#[async_trait]
pub(crate) trait Provider: Send + Sync {
    async fn chat_completion(&self, request: ChatCompletionRequest) -> crate::Result<ChatCompletionResponse>;
    async fn list_models(&self) -> crate::Result<Vec<Model>>;
    fn name(&self) -> &str;
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
    StreamingNotSupported,                 // 400 - Streaming requested but not supported
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

### 5. Integrate with Server

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
async fn chat_completions_streaming_not_supported() {
    let mut builder = TestServer::builder();
    builder.spawn_llm(YourProviderMock::new("test_provider")).await;

    let server = builder.build("").await;
    let llm = server.llm_client("/llm");

    let request = json!({
        "model": "test_provider/model-1",
        "messages": [{"role": "user", "content": "Test"}],
        "stream": true
    });

    let response = llm.completions_raw(request).await;

    // Should return 400 because streaming is an invalid request
    assert_eq!(response.status(), 400);
    let body = response.text().await.unwrap();
    assert!(body.contains("Streaming is not yet supported"));
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
- [ ] Test that streaming returns 400 (not supported)
- [ ] Test model listing with various scenarios
- [ ] Test chat completions with different parameters

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
3. **Handle streaming requests**: Always check for `stream: true` and return 400
4. **Test error scenarios**: Don't just test happy paths
