# RFC: AWS Bedrock Provider for Nexus

## Problem

Nexus aggregates multiple LLM providers behind a unified OpenAI-compatible API. Users want AWS Bedrock access through this same interface.

AWS Bedrock offers models from Anthropic, Meta, Amazon, and others. Organizations already using AWS want to leverage their existing AWS infrastructure and credentials.

## Constraints

Nexus has established patterns for LLM providers. Each provider implements a `Provider` trait, uses a `ModelManager` for model resolution, and transforms between OpenAI's format and provider-specific formats.

AWS Bedrock differs from existing providers in three ways. First, it uses AWS's credential chain instead of simple API keys. Second, different model families require different request formats. Third, it uses EventStream for streaming instead of Server-Sent Events.

The implementation must maintain backward compatibility. Existing provider configurations and behaviors cannot change.

## Authentication Design

AWS authentication is more complex than API keys. Organizations use IAM roles in production, developers use profiles locally, and CI/CD uses explicit credentials.

The AWS SDK handles this complexity through its credential chain. It tries environment variables, then credential files, then instance metadata, then container credentials. Using the SDK directly gives us this for free.

Our configuration will accept optional explicit credentials. When absent, the SDK uses its standard chain. This matches how other AWS tools work, reducing surprise.

```toml
[llm.providers.bedrock-us-east]
type = "bedrock"
region = "us-east-1"
# Optional: falls back to credential chain
access_key_id = "..."
secret_access_key = "..."
```

## Region Handling

Bedrock models vary by region. Claude might be in us-east-1 but not eu-west-1. Cross-region inference adds latency.

Each provider instance targets one region. Users wanting multiple regions create multiple providers. This matches how LiteLLM and other routers work.

This design is simpler than dynamic region switching. It makes costs predictable. It lets users explicitly control where data is processed.

## Model Family Routing

Bedrock hosts models from multiple vendors. Each vendor uses different request formats. Claude expects Anthropic's format. Titan expects Amazon's format.

The model ID reveals the vendor: `anthropic.claude-3-sonnet` versus `amazon.titan-express`. We parse the prefix to determine the transformation needed.

This routing happens inside the provider. The external API remains uniform. Clients send OpenAI format, unaware of the underlying complexity.

## Request Transformation

Each model family needs specific field mappings. OpenAI uses `max_tokens`, Anthropic uses `max_tokens`, but Titan uses `maxTokenCount`.

We implement a transformer per family. Each transformer is a pure function from `ChatCompletionRequest` to the vendor's format. This isolation makes testing straightforward.

```rust
match ModelFamily::from_model_id(model_id)? {
    ModelFamily::Anthropic => transform_anthropic(request),
    ModelFamily::Amazon => transform_titan(request),
    ModelFamily::Meta => transform_llama(request),
    // ...
}
```

## Streaming Architecture

### EventStream Protocol

For streaming responses, AWS Bedrock uses EventStream - a binary protocol with checksums, not Server-Sent Events (SSE) like OpenAI/Anthropic. This applies to ALL streaming-capable models.

Non-streaming requests use standard HTTP with JSON payloads. Streaming requests use EventStream as a transport layer, with model-specific JSON inside each event.

### Streaming Support by Model Family

Not all Bedrock models support streaming:

| Model Family | Streaming Support | Notes |
|--------------|-------------------|-------|
| **Anthropic** (Claude) | ✅ Yes | All Claude models support streaming |
| **Amazon** (Titan) | ✅ Yes | Titan Text models support streaming |
| **Meta** (Llama) | ✅ Yes | Llama 2 and Llama 3 models support streaming |
| **Mistral** | ✅ Yes | Mistral and Mixtral models support streaming |
| **Cohere** | ✅ Yes | Command models support streaming (not Embed) |
| **AI21** | ❌ No | Jurassic and Jamba models do not support streaming |
| **Stability** | N/A | Image generation uses different paradigm |

To check if a specific model supports streaming, call AWS Bedrock's `GetFoundationModel` API and check the `responseStreamingSupported` field.

### EventStream Processing

The AWS SDK provides `invoke_model_with_response_stream()` which returns a `ResponseStream`. This stream yields events in the EventStream format:

```rust
// AWS SDK handles EventStream protocol parsing
let response = bedrock_client
    .invoke_model_with_response_stream()
    .model_id(model_id)
    .body(request_body)
    .send()
    .await?;

let mut stream = response.body;
while let Some(event) = stream.recv().await? {
    match event {
        ResponseStream::Chunk(chunk) => {
            // Each chunk contains model-specific JSON
            let json_data = chunk.bytes.as_ref();
            // Parse based on model family
            match model_family {
                ModelFamily::Anthropic => parse_anthropic_chunk(json_data),
                ModelFamily::Amazon => parse_titan_chunk(json_data),
                // ...
            }
        },
        ResponseStream::InternalServerException(e) => {
            // Handle error
        },
        // ... other event types
    }
}
```

### Streaming Response Formats

Each model family has different JSON formats within the EventStream chunks:

#### Anthropic Streaming Chunk
```json
{
  "completion": " Hello",
  "stop_reason": null,
  "stop": null,
  "delta": {
    "type": "text_delta",
    "text": " Hello"
  }
}
```

#### Amazon Titan Streaming Chunk
```json
{
  "outputText": " Hello",
  "index": 0,
  "totalOutputTextTokenCount": 1,
  "completionReason": null
}
```

#### Meta Llama Streaming Chunk
```json
{
  "generation": " Hello",
  "prompt_token_count": null,
  "generation_token_count": 1,
  "stop_reason": null
}
```

#### Mistral Streaming Chunk
```json
{
  "outputs": [{
    "text": " Hello",
    "stop_reason": null
  }]
}
```

#### Cohere Streaming Chunk
```json
{
  "text": " Hello",
  "is_finished": false,
  "finish_reason": null,
  "response": {
    "response_id": "...",
    "generation_id": "..."
  }
}
```

### SSE Transformation

We transform EventStream chunks to SSE format for OpenAI compatibility:

```
data: {"id":"chatcmpl-...","object":"chat.completion.chunk","created":1234567890,"model":"bedrock.anthropic.claude-3","choices":[{"index":0,"delta":{"content":" Hello"},"finish_reason":null}]}

data: {"id":"chatcmpl-...","object":"chat.completion.chunk","created":1234567890,"model":"bedrock.anthropic.claude-3","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}

data: [DONE]
```

This transformation is stateful. Different model families chunk differently. We maintain a processor per stream that accumulates state and emits SSE events.

## Error Handling

AWS errors differ from HTTP status codes. `ThrottlingException` maps to rate limiting. `ResourceNotFoundException` means the model doesn't exist in that region.

We map SDK errors to existing `LlmError` variants. This preserves the uniform error interface. Clients handle errors the same way regardless of provider.

The AWS SDK includes retry logic for transient failures. We rely on this instead of implementing our own. This avoids duplicate retry layers.

## Testing Strategy

Testing requires mocking AWS responses. The aws-smithy-mocks crate exists but is experimental. We'll create our own `BedrockMock` matching existing provider mocks.

The mock simulates model responses and errors. It validates request format per model family. Integration tests use it to verify behavior without AWS credentials.

Manual testing against real Bedrock remains necessary. Model behaviors change. New models appear. Our tests verify our code, not Bedrock itself.

## Implementation Phases

Start with configuration and types. Extend the `ProviderType` enum. Add Bedrock configuration structures. This establishes the interface.

Next, implement non-streaming requests. Focus on one model family first (Anthropic/Claude). Get end-to-end flow working. Add other families incrementally.

Then add streaming support. EventStream parsing is complex. SSE transformation is stateful. This deserves isolated focus.

Finally, comprehensive testing. Unit tests per component. Integration tests per scenario. Manual testing with real credentials.

## Migration Path

Existing users are unaffected. Bedrock is a new provider type. No existing configurations change.

Users add Bedrock providers alongside existing ones. They can migrate gradually. The router handles provider selection transparently.

## Future Considerations

Cross-region inference profiles could reduce latency. Bedrock routes requests across regions automatically. Supporting this requires parsing inference profile ARNs[^1].

Model discovery could list available models per region. Bedrock provides APIs for this. It would help users configure valid models.

Cost tracking could monitor usage per model. Bedrock models have different pricing. This helps organizations optimize spending.

---

[^1]: Inference profiles use ARNs like `arn:aws:bedrock:us-east-1::foundation-model/anthropic.claude-3-sonnet`. The SDK accepts these directly instead of model IDs.