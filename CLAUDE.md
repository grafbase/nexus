# LLM Code Editing Guidelines for Nexus

When editing code in this repository, you are working on **Nexus**, an AI router that aggregates MCP (Model Context Protocol) servers and LLMs. This system helps reduce tool proliferation by intelligently routing and indexing available tools.

## Commit Messages

- Follow the [Conventional Commits 1.0.0](https://www.conventionalcommits.org/en/v1.0.0/) specification for every commit message.
- Use recent commits (`git log`) as a reference for tone and formatting if you need examples.
- Structure messages as `type(issue number): subject`, where
- Common `type` values include `feat`, `fix`, `chore`, `docs`, `refactor`, `test`, and `build`; pick the one that best matches the change.
- The issue number is a linear issue number, in the format of `gb-1234`. The user should give you this in the prompt, but if you don't have it, you can ask the user for it.
- Keep the subject in the imperative mood, ≤72 characters, and avoid punctuation at the end.

## Domain Context

- **MCP (Model Context Protocol)**: A protocol for connecting AI models with external tools and data sources
- **AI Router**: Nexus acts as an intelligent intermediary between LLMs and multiple MCP servers
- **Tool Indexing**: Uses Tantivy (full-text search engine) to create searchable indexes of available tools
- **Dynamic vs Static Tools**: Static tools are shared across users; dynamic tools require user authentication

## Key Technologies

- **Rust**: The primary programming language
- **Axum**: Web framework for HTTP routing and middleware
- **Tantivy**: Full-text search engine for tool indexing
- **Tokio**: Async runtime
- **Serde**: Serialization/deserialization for JSON/TOML
- **RMCP**: Rust MCP client/server implementation
- **Anyhow**: Error handling with context and backtrace
- **Reqwest**: HTTP client for external API calls
- **JWT-Compact**: JWT token handling for authentication
- **Tower/Tower-HTTP**: Middleware and service layers
- **Rustls**: TLS implementation for secure connections
- **Insta**: Snapshot testing framework
- **Clap**: Command-line argument parsing
- **Logforth**: Structured logging with tracing support
- **Docker Compose**: For integration testing with Hydra OAuth2 server
- **Governor**: Rate limiting with token bucket algorithm
- **Mini-moka**: In-memory caching for rate limit buckets
- **Redis**: Redis support for distributed rate limiting
- **Deadpool**: Connection pooling
- **OpenTelemetry**: Observability with metrics collection and OTLP export

## Rust Coding Guidelines

### Error Handling
Always handle errors appropriately - never silently discard them:

```rust
// Good: Propagate errors
let result = some_operation().await?;

// Good: Custom error handling
match some_operation().await {
    Ok(value) => process(value),
    Err(e) => handle_specific_error(e),
}

// Bad: Silent error discarding
let _ = some_operation().await;

// Bad: Panic on errors
let result = some_operation().await.unwrap();
```

### String Formatting
Use modern Rust string interpolation:

```rust
// Good
let message = format!("User {username} has {count} items");

// Bad
let message = format!("User {} has {} items", username, count);

// Good
assert!(
    startup_duration < Duration::from_secs(5),
    "STDIO server startup took too long: {startup_duration:?}",
);

// Bad
assert!(
    startup_duration < Duration::from_secs(5),
    "STDIO server startup took too long: {:?}",
    startup_duration
);

// Good
log::debug!("creating stdio downstream service for {name}");

// Bad
log::debug!("creating stdio downstream service for {}", name);
```

When accessing fields or calling methods, interpolation is not needed:

```rust
// Good: Direct field/method access
let message = format!("Status: {}", server.status());
let info = format!("User: {}", user.name);

// Bad: Unnecessary named interpolation
let message = format!("Status: {status}", status = server.status());
let info = format!("User: {name}", name = user.name);
```

And so on. You will find many places where these rules apply, not only for format! or log macros.

### Control Flow and Readability
Avoid nested if-lets and matches. Use let-else with early return to reduce indentation. Horizontal space is sacred and nested structures are hard to read:

```rust
// Good: Early return with let-else
let Some(user) = get_user() else {
    return Err(anyhow!("User not found"));
};

let Some(profile) = user.profile() else {
    return Ok(Response::default());
};

process_profile(profile);

// Bad: Nested if-let
if let Some(user) = get_user() {
    if let Some(profile) = user.profile() {
        process_profile(profile);
    }
}

// Good: Flat match with early returns
let config = match load_config() {
    Ok(cfg) => cfg,
    Err(e) => return Err(e.into()),
};

let parsed = match config.parse() {
    Some(p) => p,
    None => return Ok(Default::default()),
};

// Bad: Nested matches
match load_config() {
    Ok(cfg) => {
        match cfg.parse() {
            Some(p) => {
                // deeply nested logic
            }
            None => { /* ... */ }
        }
    }
    Err(e) => { /* ... */ }
}
```

### Tools

The Nexus server will always return two tools when listed: `search` and `execute`. If you want to find tools in tests you created, you have to search for them.

### Configuration Validation

Nexus requires at least one downstream service (MCP servers or LLM providers) to be configured when the respective feature is enabled:

- **MCP**: When `mcp.enabled = true`, at least one server must be configured in `mcp.servers`
- **LLM**: When `llm.enabled = true`, at least one provider must be configured in `llm.providers`
  - Each LLM provider MUST have either:
    - At least one model explicitly configured under `[llm.providers.<name>.models.<model-id>]`, OR
    - A `model_filter` regex for dynamic model routing
  - Model IDs containing dots must be quoted: `[llm.providers.google.models."gemini-1.5-flash"]`
  - **Endpoints**: LLM endpoints are configured using the `[llm.protocols]` section
    - OpenAI protocol: `[llm.protocols.openai]` with `enabled` and `path` fields
    - Anthropic protocol: `[llm.protocols.anthropic]` with `enabled` and `path` fields
    - Example: `[llm.protocols.openai]` with `enabled = true` and `path = "/llm"`

#### Model Discovery and Filters

Nexus discovers models for every provider at startup and refreshes the list every five minutes. A background task fetches all provider models in parallel, applies any configured filters, and publishes the results through a `tokio::sync::watch` channel. Request handlers read from the watch receiver, giving lock-free lookups for routing and `/v1/models` responses.

**Discovery flow:**
- Startup performs an initial fetch; if any provider fails, Nexus exits so configuration issues surface immediately
- Background refresh continues independently—failures log errors but keep the previous successful snapshot
- Discovered models are de-duplicated using provider order (first configured provider wins) and stored in a shared `ModelMap`

**Model filters:**
- `model_filter` is an optional regex that runs against bare model IDs returned by discovery
- Filters are case-insensitive, cannot be empty, and must not contain `/`
- Filtering only affects discovered models—explicitly configured models always remain available

```toml
[llm.providers.openai]
type = "openai"
api_key = "{{ env.OPENAI_API_KEY }}"
model_filter = "^gpt-"  # Expose only GPT-family models from discovery

# Explicit models can still add aliases, renames, or rate limits
[llm.providers.openai.models.gpt-3-5-turbo]
rename = "fast-model"
```

**Discovery guarantees:**
- Discovered models appear as bare names (e.g. `gpt-4`); explicit models appear with `provider/model`
- Provider ordering determines duplicate resolution; skipped duplicates emit a warn-level log entry
- The watch channel ensures request handlers always see a consistent snapshot without locking

#### Client Identification for Rate Limiting

When using token-based rate limits with groups:
- `server.client_identification.enabled` must be `true`
- `server.client_identification.validation.group_values` must list all groups referenced in rate limit configurations
- Groups not in `group_values` will be rejected with 400 Bad Request
- Missing groups default to the provider/model's `per_user` rate limit

For integration tests that need to test endpoints without actual downstream servers, use dummy configurations:

```toml
[mcp]
enabled = true

# Dummy server to ensure MCP endpoint is exposed
[mcp.servers.dummy]
cmd = ["echo", "dummy"]
```

For LLM providers in tests:

```toml
# LLM endpoints configuration (required when providers are configured)
[llm]
enabled = true

[llm.protocols.openai]
enabled = true
path = "/llm"

# Provider configuration
[llm.providers.test]
type = "openai"
api_key = "test-key"

# At least one model is required
[llm.providers.test.models.gpt-4]
```

The MCP service will log warnings if configured servers fail to initialize but will continue to expose the endpoint. The LLM service will return an error if no providers can be initialized.

### File Organization
Prefer flat module structure:

```rust
// Good: src/user_service.rs
// Bad: src/user_service/mod.rs
```

### Code Quality Principles

- **Prioritize correctness and clarity** over speed and efficiency unless explicitly required
- **Minimal comments**: Do not write comments just describing the code. Only write comments describing _why_ that code is written in a certain way, or to point out non-obvious details or caveats. Or to help break up long blocks of code into logical chunks.
- **Prefer existing files**: Add functionality to existing files unless creating a new logical component
- **Debug logging**: Use debug level for most logging; avoid info/warn/error unless absolutely necessary

## Project Structure

### Nexus Binary (`./nexus`)
The main application entry point:
- `src/main.rs`: Binary entry point and application bootstrapping
- `src/logger.rs`: Centralized logging configuration
- `src/args.rs`: Command-line argument parsing and validation

### Server (`./crates/server`)
Shared HTTP server components used by both the main binary and integration tests:
- Axum routing and middleware
- Request/response handling
- Authentication integration

### Config (`./crates/config`)
Configuration management for the entire system:
- TOML-based configuration with serde traits
- Type-safe configuration loading and validation
- Environment-specific settings

### MCP Router (`./crates/mcp`)
The core MCP routing and tool management system:
- **Tool Discovery**: Connects to multiple MCP servers and catalogs their tools
- **Search**: Keyword search using Tantivy index to find relevant tools for LLM queries
- **Execute**: Routes tool execution requests to appropriate downstream MCP servers
- **Static Tools**: Shared tools initialized at startup
- **Dynamic Tools**: User-specific tools requiring authentication tokens

### Integration Tests (`./crates/integration-tests`)
Comprehensive testing setup:
- Docker Compose configuration with Hydra OAuth2 server
- End-to-end testing scenarios
- Authentication flow testing

### Telemetry (`./crates/telemetry`)
OpenTelemetry integration and observability:
- **Metrics**: Counter, histogram, and gauge creation with OTLP export
- **OTLP Support**: Export to OpenTelemetry collectors via gRPC or HTTP
- **Custom Headers**: Configure per-exporter OTLP headers using HTTP-compliant names/values
- **SDK Management**: Proper initialization and shutdown via TelemetryGuard
- **Standard Names**: Consistent metric naming following OTel semantic conventions

#### MCP Metrics Behavior
**Important**: Metrics are collected via middleware and only execute when `telemetry.exporters.otlp.enabled = true`. Zero overhead when disabled.

MCP operations emit deterministic metrics:
- `mcp.tool.call.duration`: Includes `tool_name`, `tool_type` (builtin/downstream), `status`, `client_id`
- Search operations include `keyword_count` and `result_count` attributes
- Execute operations include `server_name` for downstream tools
- Error metrics include `error_type` (e.g., `method_not_found`, `rate_limit_exceeded`)

**Testing**: MCP metrics are deterministic - exact counts must match test expectations. HTTP-level metrics may vary due to batching.

### Rate Limit (`./crates/rate-limit`)
Rate limiting functionality for the entire system:
- **Global Rate Limits**: System-wide request limits
- **Per-IP Rate Limits**: Individual IP address throttling
- **Token-based Rate Limits**: Per-user and per-group token consumption limits for LLMs
- **MCP Server/Tool Limits**: Per-server and per-tool rate limits
- **Storage Backends**: In-memory (governor) and Redis (distributed)
- **Averaging Fixed Window Algorithm**: For Redis-based rate limiting

## Testing Guidelines

### Test Naming
Don't prefix test functions with `test_`.

```rust
// Good: Clean and short test name
#[tokio::test]
async fn user_can_search_tools() { ... }

// Bad: The name of the test is too verbose
#[tokio::test]
async fn test_user_can_search_tools() { ... }
```

### LLM Testing Patterns

For LLM integration tests, **ALWAYS** use the builder pattern instead of manual HTTP calls:

```rust
// GOOD: Use TestServer builder methods
let request = json!({
    "model": "provider/model",
    "messages": [{"role": "user", "content": "Hello"}]
});

let response = server
    .openai_completions(request)
    .header("X-Provider-API-Key", "test-key")
    .send()
    .await;

// For error testing
let (status, body) = server
    .openai_completions(request)
    .send_raw()
    .await;
assert_eq!(status, 401);

// BAD: Manual HTTP client calls
let client = reqwest::Client::new();
let response = client.post(format!("http://{}/llm/openai/v1/chat/completions", server.address))
    .json(&request).send().await;
```

**Available methods**: `openai_completions()`, `openai_completions_stream()`, `anthropic_completions()`, `anthropic_completions_stream()`

### Snapshot Testing (INLINE ONLY - CRITICAL)

**STRICT RULES**:
1. Use `assert_eq!` ONLY for primitives (bool, int, status codes)
2. Use insta snapshots for ALL complex types (structs, vecs, JSON)
3. Snapshots MUST be inline (`@r###"..."###`) - NO external files
4. NEVER use `assert_eq!` to compare complex objects

```rust
// GOOD: Inline snapshot for complex data
insta::assert_json_snapshot!(response, @r###"
{
  "tools": ["search", "execute"],
  "status": "ready"
}
"###);

// GOOD: Simple assertions for primitives only
assert_eq!(response.status(), 200);
assert!(config.enabled);

// BAD: NEVER do this for complex types
assert_eq!(response.tools.len(), 2);  // NO! Use snapshot
assert_eq!(response.tools[0], "search");  // NO! Use snapshot
```

Prefer approve over review with cargo-insta:

```bash
# Good: approves all snapshot changes
cargo insta approve

# Bad: opens a pager and you'll get stuck
cargo insta review
```

### Multi-line strings

When writing strings that contain multiple lines, prefer the `indoc` crate and its `indoc!` and `formatdoc!` macros.

```rust
use indoc::{indoc, formatdoc};

// Good: Use indoc for multi-line strings
let message = indoc! {r#"
    This is a string.
    This is another string.
"#};

// Bad: Use raw strings without indoc
let message = r#"
    This is a string.
    This is another string.
"#;

// Good: use formatdoc with string interpolation
let name = "Alice";
let message = formatdoc! {r#"
    Hello, {name}!
    Welcome to our platform.
"#};

// Bad: use format directly
let name = "Alice";
let message = format!(r#"
    Hello, {name}!
    Welcome to our platform.
"#);
```

## Development Workflow

### Starting Services
```bash
# Start OAuth2 server for authentication tests
cd ./crates/integration-tests && docker compose up -d
```

### Testing

**CRITICAL: Always use `cargo nextest run`, NEVER use `cargo test`**

The integration tests MUST use nextest because they initialize global OpenTelemetry state. Running tests with `cargo test` causes tests to share global state across threads, leading to flaky failures and incorrect metrics attribution.

```bash
# ALWAYS use nextest for all testing
cargo nextest run

# The .cargo/config.toml aliases 'cargo test' to 'cargo nextest run' for safety
# But always explicitly use nextest in commands and documentation

# Run tests with debug logs visible
cargo nextest run --no-capture

# Or with specific log level
RUST_LOG=debug cargo nextest run --no-capture

# Run integration tests specifically
cargo nextest run -p integration-tests

# Run unit tests only
cargo nextest run --workspace --exclude integration-tests

# Approve snapshot changes
cargo insta approve
```

**Why nextest is required:**
- Each test runs in its own process (not thread), providing isolation
- Global telemetry state doesn't leak between tests
- Tests can safely initialize their own service names and metrics
- Prevents flaky test failures due to shared OpenTelemetry providers

### Code Quality
```bash
# Check for issues
cargo clippy

# Format code
cargo fmt

# Check formatting without changes
cargo fmt --check
```

## Examples

Do NOT write any examples or new markdown files if not explicitly requested. You can modify the existing ones.

## Architecture Patterns

### Error Propagation
Use the `?` operator liberally for clean error propagation:

```rust
// Good: Clean error chain
async fn handle_tool_search(query: String) -> anyhow::Result<Vec<Tool>> {
    let index = get_search_index().await?;
    let results = index.search(&query).await?;
    parse_results(results)
}
```

Prefer `anyhow::Result` over `Result` for error handling:

```rust
// Good: Clean error chain
async fn handle_tool_search(query: String) -> anyhow::Result<Vec<Tool>> {
    ...
}

// Bad: Error handling is verbose
async fn handle_tool_search(query: String) -> Result<Vec<Tool>, anyhow::Error> {
    ...
}
```

## LLM Provider Configuration Patterns

### Unified Types Architecture

The LLM crate uses a **unified type system** that serves as a protocol-agnostic internal representation for all LLM protocols (OpenAI, Anthropic, Google, Bedrock). This architecture:

- **Eliminates complex protocol-specific conversions** between providers
- **Ensures no information loss** across different protocols
- **Simplifies provider implementations** with consistent internal types
- **Enables zero-allocation conversions** through data movement instead of cloning

#### Conversion Flow
```
Protocol Request → UnifiedRequest → Provider → UnifiedResponse → Protocol Response
```

#### Key Unified Types
- `UnifiedRequest`: Protocol-agnostic request format
- `UnifiedResponse`: Protocol-agnostic response format
- `UnifiedChunk`: Streaming chunk format
- `UnifiedMessage`, `UnifiedRole`, `UnifiedContent`: Message components
- `UnifiedTool`, `UnifiedToolChoice`: Tool calling structures

Providers convert between protocol-specific formats and unified types in their `input.rs` and `output.rs` modules. See `/crates/llm/CLAUDE.md` for detailed implementation guidelines.

### Token Rate Limiting Configuration

Nexus supports hierarchical token-based rate limiting for LLM providers. The configuration uses a `per_user` namespace to make it explicit that these are individual user limits (not shared pools).

**Important**: Rate limits only count input tokens. The field is named `input_token_limit` to make this explicit. Output tokens and the `max_tokens` parameter are NOT considered in rate limit calculations:

```toml
# Provider-level default rate limit (applies to all models in this provider)
[llm.providers.openai.rate_limits.per_user]
input_token_limit = 1000        # Maximum input tokens per user
interval = "60s"                # Time window

# Provider-level group-specific rate limits
[llm.providers.openai.rate_limits.per_user.groups.premium]
input_token_limit = 5000
interval = "60s"

[llm.providers.openai.rate_limits.per_user.groups.basic]
input_token_limit = 500
interval = "60s"

# Model-level rate limits (override provider-level limits)
[llm.providers.openai.models."gpt-4".rate_limits.per_user]
input_token_limit = 2000
interval = "60s"

# Model-level group-specific rate limits (highest priority)
[llm.providers.openai.models."gpt-4".rate_limits.per_user.groups.premium]
input_token_limit = 10000
interval = "60s"
```

#### Rate Limit Hierarchy

Rate limits are resolved in the following priority order (highest to lowest):
1. **Model + Group**: Specific model with specific group
2. **Model Default**: Specific model without group
3. **Provider + Group**: Provider-level with specific group
4. **Provider Default**: Provider-level without group

### Model Configuration Requirements

Every LLM provider MUST have explicit model configuration:

```rust
// In config parsing - models are required
#[derive(Deserialize)]
pub struct ProviderConfig {
    #[serde(rename = "type")]
    pub provider_type: ProviderType,
    pub api_key: Option<String>,
    #[serde(default, deserialize_with = "deserialize_non_empty_models_with_default")]
    pub models: BTreeMap<String, ModelConfig>,  // Must have at least one entry
}
```

### Model Resolution Pattern

Use the `ModelManager` for consistent model resolution across providers:

```rust
// Good: Use ModelManager for model resolution
let model_manager = ModelManager::new(config.models, "provider_name");
let actual_model = model_manager.resolve_model(requested_model)
    .ok_or_else(|| LlmError::ModelNotFound(format!("Model '{}' is not configured", requested_model)))?;

// Bad: Manual model resolution logic in each provider
if let Some(model_config) = config.models.get(requested_model) {
    // ... duplicated logic
}
```

### Testing with Model Configurations

Integration tests should always include model configurations:

```rust
// Good: Explicit model configuration in tests
let mock = OpenAIMock::new("test")
    .with_models(vec!["gpt-4".to_string()])
    .with_model_configs(vec![
        ModelConfig::new("fast").with_rename("gpt-3.5-turbo"),
        ModelConfig::new("smart").with_rename("gpt-4"),
    ]);

// Bad: Tests without model configuration (will fail)
let mock = OpenAIMock::new("test");  // Missing models configuration
```

## Dependency Management

### Workspace Dependencies
All dependencies must be added to the **workspace** `Cargo.toml` (`./Cargo.toml`), not individual crate `Cargo.toml` files:

```toml
# In Cargo.toml [workspace.dependencies]
new-crate = "1.0.0"

# In crates/*/Cargo.toml or nexus/Cargo.toml [dependencies]
new-crate.workspace = true
```

### Adding New Dependencies
1. Add the dependency to `[workspace.dependencies]` in `nexus/Cargo.toml`
2. Reference it in individual crates using `dependency.workspace = true`
3. Enable specific features in individual crates as needed:

```toml
# Workspace defines base dependency
tokio = { version = "1.46.1", default-features = false }

# Individual crates enable needed features
tokio = { workspace = true, features = ["macros", "rt"] }
```

### Feature Management
- Keep `default-features = false` in workspace for minimal builds
- Enable only required features in individual crates
- Group related features logically (e.g., `["derive", "serde"]`)

Remember: This codebase values **correctness and maintainability** over premature optimization. Write clear, safe code that properly handles errors and follows Rust best practices.

## Keeping This Document Updated

**IMPORTANT**: This CLAUDE.md file must be kept in sync with the codebase. When making changes:

1. **Update Guidelines**: If you change coding patterns, update the relevant section
2. **Add New Patterns**: If you introduce new patterns or conventions, document them
3. **Remove Obsolete Info**: If you remove or deprecate features, update the docs
4. **Review Periodically**: When working on the codebase, check if the guidelines still match reality
5. **Update README.md**: When adding or modifying configuration options or CLI arguments, ALWAYS update the README.md documentation

Examples of when to update:
- Adding a new dependency management pattern
- Changing error handling approaches
- Introducing new testing strategies
- Modifying string formatting conventions
- Updating technology choices
- **Adding configuration options** (update both CLAUDE.md and README.md)
- **Changing CLI arguments** (update both CLAUDE.md and README.md)
- **Modifying default values** (update README.md configuration section)
