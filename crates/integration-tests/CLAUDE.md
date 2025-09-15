# Integration Tests Guide

## Core Requirements

### 1. Use TOML Strings for Config
```rust
let config = indoc! {r#"
    [server]
    listen_address = "127.0.0.1:0"
"#};
```

### 2. Use TestServer API
```rust
let test = TestServer::spawn(config).await;
let client = test.client();
let response = client.post("/mcp").json(&request).send().await?;
```

### 3. Use Insta Snapshots (INLINE ONLY)
**Required for**: JSON responses, structured data, error messages, ANY complex type
**Regular asserts for**: Status codes, headers, simple booleans ONLY

**CRITICAL**: 
- Use `assert_eq!` ONLY for primitives (bool, int, status codes)
- Use insta snapshots for EVERYTHING else
- Snapshots MUST be inline (`@r###"..."###`) 
- NEVER use external snapshot files

```rust
assert_eq!(response.status(), 200);  // OK: Simple primitive
assert_json_snapshot!(body, @r###"   // REQUIRED: Inline snapshot
{
  "field": "value"
}
"###);
```

## Test Patterns

### Basic Structure
```rust
#[tokio::test]
async fn feature_works() {
    let config = indoc! {r#"config here"#};
    let test = TestServer::spawn(config).await;
    let response = test.client().get("/path").send().await.unwrap();
    assert_json_snapshot!(response.json::<Value>().await.unwrap());
}
```

### LLM Testing with Builder Pattern
**REQUIRED**: Use builder methods instead of manual HTTP calls

```rust
// GOOD: Use builder pattern with fluent API
let request = json!({
    "model": "provider/model",
    "messages": [{"role": "user", "content": "Hello"}]
});

let response = server
    .openai_completions(request)
    .header("X-Provider-API-Key", "test-key")
    .send()
    .await;

assert_json_snapshot!(response);

// For error testing - get status and body separately
let (status, body) = server
    .openai_completions(request)
    .send_raw()
    .await;
assert_eq!(status, 401);
assert_json_snapshot!(body);

// Streaming completions
let chunks = server
    .openai_completions_stream(request)
    .header("Authorization", "Bearer token")
    .send()
    .await;

// BAD: Never use manual HTTP client calls
let client = reqwest::Client::new();
let response = client
    .post(format!("http://{}/llm/openai/v1/chat/completions", server.address))
    .json(&request)
    .send()
    .await;
```

#### Available Builder Methods
- `server.openai_completions(request)` - OpenAI chat completions
- `server.openai_completions_stream(request)` - OpenAI streaming completions
- `server.anthropic_completions(request)` - Anthropic chat completions
- `server.anthropic_completions_stream(request)` - Anthropic streaming completions

#### Builder Methods
- `.header(key, value)` - Add request header (chainable)
- `.send()` - Send request, expect 200 status, return JSON body
- `.send_raw()` - Send request, return `(status_code, json_body)` tuple

### MCP Testing
```rust
let mcp = test.mcp_client("server_name");
let tools = mcp.list_tools().await?;
assert_json_snapshot!(tools);
```

### OAuth2 Testing
```rust
TestServerBuilder::new()
    .config(config)
    .spawn_with_oauth()
    .await;
```

## Live Provider Tests
Tests against real providers are **skipped by default**. Enable with env vars:
- `TEST_OPENAI_API_KEY` - OpenAI tests
- AWS credentials + `AWS_REGION` - Bedrock tests

## Docker Setup
```bash
cd crates/integration-tests
docker compose up -d  # Start OAuth2 server
```

## Test Organization
- File per feature: `oauth2.rs`, `rate_limiting.rs`
- Descriptive names: `user_can_search_tools()`
- No `test_` prefix

## Debugging

The server crate automatically uses debug log level for tests. To see debug logs:

```bash
# Run specific test with logs visible
cargo nextest run test_name --no-capture

# Run with custom log level
RUST_LOG=debug cargo nextest run test_name --no-capture

# Run with logs for specific crates only
RUST_LOG=mcp=debug,server=debug cargo nextest run test_name --no-capture
```

## Snapshot Management
```bash
cargo insta review  # Review changes  
cargo insta approve  # Accept all (prefer this over review)
```