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
- **Model Management**: List models, dynamic fetching, caching

## Implementation Checklist

### 1. Config (config crate)
```rust
#[derive(Debug, Clone, Deserialize)]
pub struct YourProviderConfig {
    pub api_key: SecretString,
    pub api_url: Option<String>,
}
```
Add to `LlmProviderConfig` enum, test with insta snapshots.

### 2. Provider Trait (llm crate)
```rust
#[async_trait]
impl Provider for YourProvider {
    async fn chat_completion(&self, request: UnifiedRequest, context: &RequestContext) -> Result<UnifiedResponse>;
    async fn chat_completion_stream(&self, request: UnifiedRequest, context: &RequestContext) -> Result<ChatCompletionStream>;
    fn list_models(&self) -> Vec<Model>;
    fn name(&self) -> &str;
    fn supports_streaming(&self) -> bool;
}
```

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

### Model Names
Format: `provider_name/model_id` (e.g., `openai/gpt-4`)

### Caching
- Cache model lists (5 min TTL)
- Use provider-level cache, not per-model

### Rate Limiting
Integrates with token-based limits via `ClientIdentity`

### Header Rules
Support header forwarding, removal, insertion per provider/model

## AWS Bedrock Notes
- Use unified Converse API, not family-specific implementations
- Single endpoint for all models
- Consistent tool calling across families

## Common Pitfalls
- Missing `finish_reason` in streaming
- Not handling rate limit headers
- Incorrect tool call streaming order
- Missing error context in responses