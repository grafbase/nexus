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
- **Model Management**: `list_models` must return provider inventory (discovered + explicit) for the watch-channel cache

## Implementation Checklist

### 1. Config (config crate)
```rust
#[derive(Debug, Clone, Deserialize)]
pub struct YourProviderConfig {
    pub api_key: SecretString,
    pub api_url: Option<String>,
    pub model_filter: Option<ModelFilter>,  // Optional regex to restrict discovery
}
```
Add to `LlmProviderConfig` enum, test with insta snapshots. Implement validation for `model_filter` OR explicit models requirement.

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

**Model discovery contract:**
- Fetch every available model from the provider API (pagination included) and propagate `anyhow::Error` on failure
- Return discovered models without a provider prefix (e.g., `gpt-4`); the server applies `model_filter` and deduplication
- Append explicitly configured models with a provider prefix (e.g., `openai/gpt-3.5-turbo`)
- Preserve provider metadata (`created`, `owned_by`, display name) so the server can populate `ModelInfo`

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
- **Provider-prefixed**: `provider_name/model_id` (e.g., `openai/gpt-4`) bypasses discovery and routes directly
- **Discovered models**: Bare names (e.g., `gpt-4`, `claude-3-opus`) resolved through the shared watch-channel map
- **Resolution order**:
  1. If the requested model contains `/`, route to the specified provider
  2. Otherwise look up the bare name in the watch-channel model map
  3. Return `ModelNotFound` when neither path resolves the model

### Model Discovery & Caching
- `crates/llm/src/server/model_discovery.rs` hosts the background task and `ModelMap`
- Startup performs a blocking fetch—any provider failure aborts launch with context
- Background refresh runs every five minutes; failures keep the previous snapshot and emit error logs
- `model_filter` runs server-side against bare IDs, after providers return their model list
- Provider order controls deduplication; the first provider that reports a model claims it
- `/v1/models` rebuilds responses from the shared map: bare discovered models first, explicit prefixed models second

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

## `list_models` Implementation Template

Providers must return every discoverable model plus explicit configuration without applying filters:

```rust
async fn list_models(&self) -> anyhow::Result<Vec<Model>> {
    let mut models = Vec::new();

    // Fetch discovered models without prefixes; propagate errors so startup can fail fast
    let discovered = self.fetch_discovered_models().await?;
    models.extend(discovered);

    // Append explicit models with provider prefixes for legacy routing and overrides
    models.extend(
        self.model_manager
            .get_configured_models()
            .into_iter()
            .map(|mut model| {
                model.id = format!("{}/{}", self.name, model.id);
                model
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
- Returning provider-prefixed IDs for discovered models (prevents bare-name routing)
- Dropping provider metadata (`created`, `owned_by`, display name) from discovered entries
- Swallowing discovery errors instead of propagating them (startup must fail on invalid configs)
