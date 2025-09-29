# LLM Provider Implementation Guide

Unified interface for LLM providers with protocol-agnostic internal types.

## Architecture Overview

The LLM crate now uses a **unified type system** that serves as an internal representation for all protocols (OpenAI, Anthropic). This approach:
- Eliminates complex protocol-specific conversions between providers
- Ensures no information loss across different protocols
- Simplifies provider implementations
- Enables zero-allocation conversions through data movement

## Unified Types System

### Core Types (in `messages/unified.rs`)
- `UnifiedRequest`: Protocol-agnostic request format
- `UnifiedResponse`: Protocol-agnostic response format
- `UnifiedChunk`: Streaming chunk format
- `UnifiedMessage`, `UnifiedRole`, `UnifiedContent`: Message components
- `UnifiedTool`, `UnifiedToolChoice`: Tool calling structures

### Conversion Flow
```
Protocol Request → UnifiedRequest → Provider → UnifiedResponse → Protocol Response
```

## Required Features
- **Tool Calling**: Function definitions, tool choice ("auto"/"none"/"required"/specific), parallel calls
- **Streaming**: SSE-based streaming with protocol-specific chunks
- **Model Management**: List models, dynamic fetching, caching, pattern-based routing

## Implementation Checklist

### 1. Config (config crate)
```rust
#[derive(Debug, Clone, Deserialize)]
pub struct YourProviderConfig {
    pub api_key: SecretString,
    pub api_url: Option<String>,
    pub model_pattern: Option<ModelPattern>,  // Optional regex for dynamic routing
}
```
Add to `LlmProviderConfig` enum, test with insta snapshots. Implement validation for `model_pattern` OR explicit models requirement.

### 2. Provider Trait (llm crate)
```rust
#[async_trait]
impl Provider for YourProvider {
    async fn chat_completion(&self, request: UnifiedRequest, context: &RequestContext) -> Result<UnifiedResponse>;
    async fn chat_completion_stream(&self, request: UnifiedRequest, context: &RequestContext) -> Result<ChatCompletionStream>;
    async fn list_models(&self) -> anyhow::Result<Vec<Model>>;  // Async for pattern-based discovery
    fn name(&self) -> &str;
    fn supports_streaming(&self) -> bool;
}
```

**Model Discovery for Pattern-Based Routing:**
- If `model_pattern` is configured, fetch models from provider API and filter by regex
- Return pattern-matched models WITHOUT provider prefix (e.g., `gpt-4` not `openai/gpt-4`)
- Return explicit models WITH provider prefix (e.g., `openai/gpt-3.5-turbo`)
- Handle API errors gracefully (logged but not fatal)

### 3. Type Conversion
- `input.rs`: Convert UnifiedRequest → provider-specific format
- `output.rs`: Convert provider-specific → UnifiedResponse format
- NO protocol-specific subdirectories (e.g., no `input/openai.rs`)
- Use unified types as the intermediate representation
- Preserve all protocol-specific features through unified types

### 4. Error Mapping
- 400 → `InvalidRequest`
- 401 → `AuthenticationFailed` 
- 403 → `InsufficientQuota`
- 404 → `ModelNotFound`
- 429 → `RateLimitExceeded`
- 500 → `InternalError(Some(msg))` for provider errors, `None` for internal

### 5. Streaming
```rust
response.bytes_stream()
    .eventsource()
    .filter_map(|event| /* Convert to ChatCompletionChunk */)
```

### 6. Testing Requirements
- Basic chat completion
- Tool calling (single, parallel, forced)
- Streaming with tool calls
- Error scenarios (auth, rate limits, invalid model)
- Integration tests with mock server

## Architecture Patterns

### Model Names and Routing
- **Legacy format**: `provider_name/model_id` (e.g., `openai/gpt-4`)
- **Pattern-matched**: Bare model name (e.g., `gpt-4`, `claude-3-opus`)
- **Resolution order**:
  1. Check if model contains `/` → route to specified provider
  2. Check pattern routes (case-insensitive, first match wins)
  3. Check explicit model configs
  4. Return "model not found"

### Model Discovery & Caching
- Cache model lists (5 min TTL) via `ModelDiscovery` in `server/model_discovery.rs`
- Pattern-based providers fetch from API and filter by regex
- Stale cache reused on provider API errors (as per RFC)
- Pattern-matched models: no prefix in `/v1/models`
- Explicit models: provider prefix in `/v1/models`

### Rate Limiting
- Integrates with token-based limits via `ClientIdentity`
- Both `model` and `provider/model` formats share the same rate limit bucket
- Model-specific limits override provider-level limits

### Header Rules
Support header forwarding, removal, insertion per provider/model

## AWS Bedrock Notes
- Use unified Converse API, not family-specific implementations
- Single endpoint for all models
- Consistent tool calling across families

## Pattern-Based Routing Implementation

When implementing `list_models()` for a provider with pattern support:

```rust
async fn list_models(&self) -> anyhow::Result<Vec<Model>> {
    let mut models = Vec::new();

    // If model_pattern is configured, fetch from API and filter
    if let Some(ref pattern) = self.config.model_pattern {
        if let Some(api_key) = self.config.api_key.as_ref() {
            match self.fetch_models_from_api(api_key).await {
                Ok(api_models) => {
                    // Filter and add WITHOUT provider prefix
                    models.extend(
                        api_models
                            .into_iter()
                            .filter(|m| pattern.is_match(&m.id))
                    );
                }
                Err(e) => {
                    log::debug!("Failed to fetch models: {e}");
                    // Continue - not fatal
                }
            }
        }
    }

    // Always include explicit models WITH provider prefix
    models.extend(
        self.model_manager
            .get_configured_models()
            .into_iter()
            .map(|mut m| {
                m.id = format!("{}/{}", self.name, m.id);
                m
            })
    );

    Ok(models)
}
```

## Common Pitfalls
- Missing `finish_reason` in streaming
- Not handling rate limit headers
- Incorrect tool call streaming order
- Missing error context in responses
- **Pattern routing**: Forgetting to filter API models by pattern
- **Model prefixes**: Adding prefix to pattern-matched models (should be bare names only)