# LLM Code Editing Guidelines for Nexus

When editing code in this repository, you are working on **Nexus**, an AI router that aggregates MCP (Model Context Protocol) servers and LLMs. This system helps reduce tool proliferation by intelligently routing and indexing available tools.

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

### Tools

The Nexus server will always return two tools when listed: `search` and `execute`. If you want to find tools in tests you created, you have to search for them.

### File Organization
Prefer flat module structure:

```rust
// Good: src/user_service.rs
// Bad: src/user_service/mod.rs
```

### Code Quality Principles

- **Prioritize correctness and clarity** over speed and efficiency unless explicitly required
- **Minimal comments**: Only explain "why" something is done, not "what" is being done
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

### Snapshot Testing
Prefer insta snapshots over manual assertions:

```rust
// Good: Inline snapshot
insta::assert_json_snapshot!(response, @r###"
{
  "tools": ["search", "execute"],
  "status": "ready"
}
"###);

// Avoid: Manual assertions for complex data
assert_eq!(response.tools.len(), 2);
assert_eq!(response.tools[0], "search");
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
```bash
# Run all tests
cargo nextest run

# Run tests with debug output
env TEST_LOG=1 cargo nextest run

# Approve snapshot changes
cargo insta approve
```

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

Examples of when to update:
- Adding a new dependency management pattern
- Changing error handling approaches
- Introducing new testing strategies
- Modifying string formatting conventions
- Updating technology choices
