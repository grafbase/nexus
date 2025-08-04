# Integration Tests Guidelines

This crate contains end-to-end integration tests for the Nexus AI router system, including OAuth2 authentication flows, MCP server interactions, rate limiting, and various transport protocols.

## Core Requirements

### 1. ALWAYS Use TOML Strings for Configuration

**MANDATORY**: All test configurations must use TOML strings with the `indoc!` macro:

```rust
// CORRECT: Always use indoc! with TOML strings
let config = indoc! {r#"
    [server]
    host = "127.0.0.1"
    port = 0

    [server.oauth]
    url = "http://127.0.0.1:4444/.well-known/jwks.json"
    expected_issuer = "http://127.0.0.1:4444"

    [mcp]
    enabled = true
"#};

let server = TestServer::builder().build(config).await;
```

```rust
// WRONG: Never use struct construction
let config = Config {
    server: ServerConfig { ... },
    ..Default::default()
};
```

### 2. ALWAYS Use TestServer API

The Test API provides a complete testing framework for Nexus integration tests:

#### TestServer

The main test server that manages the lifecycle of a Nexus instance:

```rust
pub struct TestServer {
    pub client: TestClient,      // HTTP client for REST API testing
    pub address: SocketAddr,     // Server address for direct connections
    // Private fields handle server lifecycle and cleanup
}

impl TestServer {
    // Create a builder for complex setups with downstream services
    pub fn builder() -> TestServerBuilder;
    
    // Create MCP client that connects to the server's MCP endpoint
    pub async fn mcp_client(&self, path: &str) -> McpTestClient;
    
    // Create MCP client with OAuth2 authentication
    pub async fn mcp_client_with_auth(&self, path: &str, token: &str) -> McpTestClient;
}
```

**What it does:**
- Automatically finds an available port (using port 0)
- Starts the Nexus server with your TOML configuration
- Waits for the server to be ready (retries health endpoint)
- Provides HTTP and MCP clients for testing
- Automatically cleans up when dropped

#### TestClient

HTTP client for making REST API requests:

```rust
impl TestClient {
    // GET request to a path
    pub async fn get(&self, path: &str) -> reqwest::Response;
    
    // POST request with JSON body
    pub async fn post<T: Serialize>(&self, path: &str, body: &T) -> reqwest::Response;
    
    // Custom request builder for advanced scenarios (CORS, headers, etc.)
    pub fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder;
    
    // GET request that returns Result (for testing error cases)
    pub async fn try_get(&self, path: &str) -> reqwest::Result<reqwest::Response>;
}
```

**What it does:**
- Automatically prefixes paths with the server's base URL
- Handles TLS connections (accepts self-signed certs in tests)
- Provides convenient methods for common HTTP operations

#### McpTestClient

MCP protocol client for testing MCP-specific functionality:

```rust
impl McpTestClient {
    // Get server information (name, version, instructions)
    pub fn get_server_info(&self) -> &InitializeResult;
    
    // List all available tools
    pub async fn list_tools(&self) -> ListToolsResult;
    
    // Search for tools by keywords
    pub async fn search(&self, keywords: &[&str]) -> Vec<serde_json::Value>;
    
    // Execute a tool with arguments
    pub async fn execute(&self, tool: &str, arguments: Value) -> CallToolResult;
    
    // Execute a tool expecting an error
    pub async fn execute_expect_error(&self, tool: &str, arguments: Value) -> ServiceError;
    
    // List available prompts
    pub async fn list_prompts(&self) -> ListPromptsResult;
    
    // Get a specific prompt
    pub async fn get_prompt(&self, name: &str, arguments: Option<Map>) -> GetPromptResult;
    
    // List available resources
    pub async fn list_resources(&self) -> ListResourcesResult;
    
    // Read a resource by URI
    pub async fn read_resource(&self, uri: &str) -> ReadResourceResult;
    
    // Disconnect and cleanup
    pub async fn disconnect(self);
}
```

**What it does:**
- Establishes MCP connection over HTTP/HTTPS
- Handles OAuth2 authentication if provided
- Provides high-level methods for MCP operations
- Automatically handles the MCP protocol details

#### TestServerBuilder

Builder for complex test scenarios with multiple downstream services:

```rust
impl TestServerBuilder {
    // Add a downstream MCP service
    pub async fn spawn_service(&mut self, service: TestService);
    
    // Build the TestServer with the given base configuration
    pub async fn build(self, config: &str) -> TestServer;
}
```

**What it does:**
- Spawns downstream test services (mock MCP servers)
- Automatically configures Nexus to connect to them
- Manages service lifecycles and cleanup
- Merges service configurations with base config

### 3. ALWAYS Use Inline Snapshots

**MANDATORY**: Every test must use insta inline snapshots for assertions:

```rust
// CORRECT: Always use inline snapshots
#[tokio::test]
async fn health_endpoint_returns_json() {
    let config = indoc! {r#"
        [server.health]
        enabled = true

        [mcp]
        enabled = true
    "#};

    let server = TestServer::builder().build(config).await;
    let response = server.client.get("/health").await;

    assert_eq!(response.status(), 200);

    let body = response.text().await.unwrap();
    insta::assert_snapshot!(body, @r#"{"status":"healthy"}"#);
}
```

```rust
// WRONG: Never use manual assertions for response bodies
assert_eq!(body, r#"{"status":"healthy"}"#);
```

## Test Structure

### Test Organization

```
tests/
├── oauth2/              # OAuth2 authentication tests
├── cors/                # CORS policy tests
├── csrf/                # CSRF protection tests
├── stdio/               # STDIO MCP server tests
├── sse/                 # Server-Sent Events tests
├── streamable_http/     # Streamable HTTP transport tests
├── token_auth/          # Token authentication tests
├── tools/               # Tool discovery and execution tests
├── prompts_resources/   # Prompts and resources tests
└── integration_tests.rs # Main test file with common tests
```

### Test Naming

Follow clear naming without `test_` prefix:

```rust
#[tokio::test]
async fn stdio_basic_echo_tool() { ... }

#[tokio::test]
async fn oauth2_flow_completes_successfully() { ... }
```

## Test Service API

The `TestService` represents a mock MCP server for testing:

```rust
impl TestService {
    // Create SSE transport service
    pub fn sse(name: String) -> Self;
    
    // Create streamable HTTP transport service
    pub fn streamable_http(name: String) -> Self;
    
    // Create service with auto-detected transport
    pub fn sse_autodetect(name: String) -> Self;
    
    // Add a tool to this service
    pub fn add_tool(&mut self, tool: impl TestTool + 'static);
    
    // Enable TLS for this service
    pub fn with_tls(&mut self) -> &mut Self;
    
    // Set authentication token
    pub fn with_auth_token(&mut self, token: String) -> &mut Self;
    
    // Enable auth forwarding
    pub fn with_auth_forwarding(&mut self) -> &mut Self;
}
```

**What it does:**
- Creates mock MCP servers with different transport protocols
- Provides test tools (AdderTool, CalculatorTool, etc.)
- Supports TLS and authentication testing
- Automatically handles MCP protocol implementation

## TestServer Builder Pattern

For complex test setups with downstream services:

```rust
#[tokio::test]
async fn multiple_downstream_servers() {
    use tools::{AdderTool, CalculatorTool, TextProcessorTool};

    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    // Create downstream test services
    let mut math_server = TestService::sse("math_server".to_string());
    math_server.add_tool(AdderTool);
    math_server.add_tool(CalculatorTool);

    let mut text_server = TestService::streamable_http("text_server".to_string());
    text_server.add_tool(TextProcessorTool);

    // Build server with downstream services
    let mut builder = TestServer::builder();
    builder.spawn_service(math_server).await;
    builder.spawn_service(text_server).await;
    let server = builder.build(config).await;

    // Test aggregated tools
    let mcp_client = server.mcp_client("/mcp").await;
    let tools = mcp_client.list_tools().await;

    insta::assert_json_snapshot!(tools, @r#"..."#);
}
```

## MCP Client Testing

### Basic MCP Operations

```rust
#[tokio::test]
async fn search_and_execute_tools() {
    let config = indoc! {r#"
        [mcp.servers.test_stdio]
        cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
    "#};

    let server = TestServer::builder().build(config).await;
    let client = server.mcp_client("/mcp").await;

    // Search for tools
    let search_results = client.search(&["echo"]).await;
    insta::assert_json_snapshot!(search_results, @r#"
    [
      {
        "name": "test_stdio__echo",
        "description": "Echoes back the input text",
        "score": 3.6119184
      }
    ]
    "#);

    // Execute tool
    let result = client.execute(
        "test_stdio__echo",
        json!({ "text": "Hello!" })
    ).await;

    insta::assert_json_snapshot!(result, @r#"
    {
      "content": [
        {
          "type": "text",
          "text": "Echo: Hello!"
        }
      ]
    }
    "#);
}
```

### OAuth2 Protected MCP

```rust
#[tokio::test]
async fn mcp_with_oauth2_authentication() {
    let (server, token) = setup_hydra_test().await.unwrap();

    // Create authenticated MCP client
    let mcp_client = server.mcp_client_with_auth("/mcp", &token).await;

    // Test authenticated access
    let tools = mcp_client.list_tools().await;
    insta::assert_json_snapshot!(tools, @r#"..."#);
}
```

## Snapshot Types

### JSON Snapshots

Use for structured data:

```rust
insta::assert_json_snapshot!(response_body, @r#"
{
  "status": "success",
  "data": {
    "id": 123,
    "name": "test"
  }
}
"#);
```

### Debug Snapshots

Use for Rust structures:

```rust
let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
insta::assert_debug_snapshot!(tool_names, @r###"
[
    "search",
    "execute",
]
"###);
```

### String Snapshots

Use for plain text responses:

```rust
let body = response.text().await.unwrap();
insta::assert_snapshot!(body, @"Hello, World!");
```

## Docker Compose Integration

The test environment uses Docker Compose for external services:

```yaml
# compose.yaml
services:
  hydra:
    image: oryd/hydra:v2.3.0
    ports:
      - "4444:4444"  # Public API
      - "4445:4445"  # Admin API
  
  redis:
    image: redis:7-alpine
    ports:
      - "6379:6379"  # Redis for rate limiting tests
  
  redis-tls:
    image: redis:7-alpine
    ports:
      - "6380:6379"  # Redis with TLS for secure rate limiting tests
    # TLS configuration with certificates
```

Start services before running OAuth2 tests:

```bash
cd crates/integration-tests
docker compose up -d
```

## Test Helpers

### OAuth2 Configuration Helpers

```rust
// Basic OAuth configuration
pub fn oauth_config_basic() -> &'static str {
    indoc! {r#"
        [server.oauth]
        url = "http://127.0.0.1:4444/.well-known/jwks.json"
        expected_issuer = "http://127.0.0.1:4444"

        [mcp]
        enabled = true
    "#}
}

// OAuth with audience validation
pub fn oauth_config_with_audience(audience: &str) -> String {
    formatdoc! {r#"
        [server.oauth]
        url = "http://127.0.0.1:4444/.well-known/jwks.json"
        expected_issuer = "http://127.0.0.1:4444"
        expected_audience = "{audience}"

        [mcp]
        enabled = true
    "#}
}
```

### Request Builder Extensions

```rust
use crate::oauth2::RequestBuilderExt;

// Add Bearer token
let response = server.client
    .get("/protected")
    .authorization(&token)
    .send()
    .await?;

// Add MCP-style headers and body
let response = server.client
    .post("/mcp")
    .mcp_json(r#"{"method": "list_tools"}"#)
    .send()
    .await?;
```

## Complete Test Example

Here's how all the APIs work together in a real test:

```rust
use indoc::indoc;
use integration_tests::{TestServer, TestService};
use tools::{AdderTool, CalculatorTool};

#[tokio::test]
async fn complete_test_example() {
    // 1. Create mock MCP services
    let mut math_service = TestService::sse("math_server".to_string());
    math_service.add_tool(AdderTool);
    math_service.add_tool(CalculatorTool);
    
    // 2. Configure with TOML string (MANDATORY)
    let config = indoc! {r#"
        [server]
        host = "127.0.0.1"
        port = 0  # Always use 0 for automatic port assignment
        
        [mcp]
        enabled = true
        
        # Can also configure STDIO servers
        [mcp.servers.python_tools]
        cmd = ["python3", "mock-mcp-servers/tools.py"]
    "#};
    
    // 3. Build server with downstream services
    let mut builder = TestServer::builder();
    builder.spawn_service(math_service).await;
    let server = builder.build(config).await;
    
    // 4. Test HTTP endpoints
    let response = server.client.get("/health").await;
    assert_eq!(response.status(), 200);
    
    let body = response.text().await.unwrap();
    insta::assert_snapshot!(body, @r#"{"status":"healthy"}"#);
    
    // 5. Test MCP functionality
    let mcp = server.mcp_client("/mcp").await;
    
    // Get server info
    let info = mcp.get_server_info();
    insta::assert_snapshot!(info.server_info.name, @"Tool Aggregator (math_server, python_tools)");
    
    // Search for tools
    let results = mcp.search(&["add", "calculator"]).await;
    insta::assert_json_snapshot!(results, @r#"
    [
      {
        "name": "math_server__adder",
        "description": "Adds two numbers together",
        "score": 4.5
      },
      {
        "name": "math_server__calculator", 
        "description": "Basic calculator operations",
        "score": 3.2
      }
    ]
    "#);
    
    // Execute a tool
    let result = mcp.execute(
        "math_server__adder",
        json!({ "a": 5, "b": 3 })
    ).await;
    
    insta::assert_json_snapshot!(result, @r#"
    {
      "content": [{
        "type": "text",
        "text": "5 + 3 = 8"
      }]
    }
    "#);
    
    // 6. Cleanup happens automatically
    mcp.disconnect().await;
}
```

## Running Tests

```bash
# Run all integration tests
cargo nextest run -p integration-tests

# Run with debug output
TEST_LOG=debug cargo nextest run -p integration-tests

# Run specific test module
cargo nextest run -p integration-tests oauth2::

# Run single test
cargo nextest run -p integration-tests stdio_basic_echo_tool

# Approve snapshot changes
cargo insta approve
```

## Debugging Tips

1. **Enable debug logging**: Set `TEST_LOG=debug` to see server logs
2. **Check Docker logs**: `docker compose logs hydra` for OAuth2 issues
3. **Wait for services**: Add small delays after starting STDIO servers
4. **Use `dbg!()` macro**: Quick debugging for values

## Best Practices

1. **Always use TOML strings** with `indoc!` for configuration
2. **Always use inline snapshots** for all assertions
3. **Always use the TestServer API** - never create servers manually
4. **Test independence**: Each test should be self-contained
5. **Descriptive test names**: Clearly indicate what is being tested
6. **Group related tests**: Use modules for logical grouping
7. **Avoid hardcoded ports**: Use port 0 for automatic assignment

Remember: Integration tests verify end-to-end functionality. Focus on user-facing behavior and API contracts rather than implementation details.

## Keeping This Document Updated

**IMPORTANT**: Update this CLAUDE.md when test patterns evolve:

1. **New Test APIs**: Document new TestServer methods or test utilities
2. **Pattern Changes**: Update if the TOML configuration approach changes
3. **New Test Types**: Add sections for new test categories (e.g., performance tests)
4. **Snapshot Updates**: Document new snapshot assertion patterns
5. **Infrastructure Changes**: Update Docker Compose or service setup instructions

Update triggers:
- Adding new test helper functions or builders
- Changing how test services are configured
- Introducing new testing frameworks or tools
- Modifying the TestServer API
- Adding new mock services or test tools
