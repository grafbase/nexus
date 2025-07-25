# Nexus Integration Tests

This crate provides comprehensive integration testing infrastructure for the Nexus MCP Router, including utilities for testing MCP servers, clients, and various middleware configurations.

## Overview

The integration tests validate Nexus from the server onwards against real working downstream MCP servers.

## Test Infrastructure

### TestServer

The main test harness that spins up a Nexus server instance with configurable settings:

```rust
use integration_tests::TestServer;
use indoc::indoc;

#[tokio::test]
async fn my_test() {
    let config = indoc! {r#"
        [mcp]
        enabled = true

        [server]
        [server.health]
        enabled = true
    "#};

    let server = TestServer::builder().build(config).await;

    // Make HTTP requests
    let response = server.client.get("/health").await;
    assert_eq!(response.status(), 200);

    // Create MCP client
    let mcp_client = server.mcp_client("/mcp").await;
    let tools = mcp_client.list_tools().await;
}
```

### TestService

Mock downstream MCP servers with configurable tools:

```rust
use integration_tests::{TestServer, TestService};
use crate::tools::AdderTool;

#[tokio::test]
async fn test_with_downstream_services() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    // Create downstream service
    let mut math_service = TestService::sse("math_server".to_string());
    math_service.add_tool(AdderTool).await;

    // Build server with downstream service
    let mut builder = TestServer::builder();
    builder.spawn_service(math_service).await;
    let server = builder.build(config).await;

    // Test tool execution
    let mcp_client = server.mcp_client("/mcp").await;
    let result = mcp_client
        .execute("math_server__adder", json!({ "a": 5, "b": 3 }))
        .await;
}
```

### Downstream Service Configuration

The test infrastructure automatically generates service configurations:

```toml
# SSE service with auto-detection
[mcp.servers.my_service]
url = "http://127.0.0.1:12345/mcp"

# Explicit SSE protocol
[mcp.servers.sse_service]
protocol = "sse"
url = "http://127.0.0.1:12345/mcp"

# Streamable HTTP protocol
[mcp.servers.http_service]
protocol = "streamable-http"
url = "http://127.0.0.1:12345/mcp"

# With TLS
[mcp.servers.secure_service]
url = "https://127.0.0.1:12345/mcp"

[mcp.servers.secure_service.tls]
verify_certs = false
accept_invalid_hostnames = true
root_ca_cert_path = "test-certs/cert.pem"
client_cert_path = "test-certs/cert.pem"
client_key_path = "test-certs/key.pem"

# With authentication
[mcp.servers.auth_service]
url = "http://127.0.0.1:12345/mcp"

[mcp.servers.auth_service.auth]
token = "my_secret_token"
```

The configuration is based on the test tools and their respective configurations.

## OAuth2 Testing Infrastructure

The integration tests include comprehensive OAuth2 testing using real Hydra OAuth2 containers:

### OAuth2 Test Setup

OAuth2 tests use Docker Compose to run Hydra OAuth2 providers:

```yaml
# compose.yaml in integration-tests directory
services:
  hydra:
    image: oryd/hydra:latest
    ports:
      - "4444:4444"  # Public API
      - "4445:4445"  # Admin API
```

## Running Tests

```bash
# Run all integration tests
cargo test -p integration-tests

# Run with logging enabled
TEST_LOG=1 cargo test -p integration-tests

# Run specific test
cargo test -p integration-tests test_name

# Run OAuth2-specific tests
cargo test -p integration-tests oauth2

# Run tests with Docker containers
docker compose up -d  # Start OAuth2 containers
cargo test -p integration-tests oauth2
docker compose down   # Clean up
```

## Contributing

When adding new tests:

1. Use TOML format for all configurations
2. Clean up resources (disconnect clients)
3. Add snapshot tests for complex responses

## Common Issues

### Port Conflicts
Tests automatically find available ports, but may occasionally conflict. Re-run if you see bind errors.

### TLS Certificate Issues
Test certificates are included in `test-certs/`. Ensure they're available when running tests.

### Timing Issues
Some tests may need timing adjustments on slower systems. Look for `tokio::time::sleep` calls if tests are flaky.

### OAuth2 Container Issues

#### Hydra Containers Not Starting
- Ensure Docker is running and has permission to bind to ports 4444/4445
- Check if ports are already in use: `netstat -an | grep 4444`
- Run `docker compose down` to clean up any existing containers

#### OAuth2 Token Issues
- Tokens have short expiration times (1 hour by default)
- Some tests may fail if system clock is significantly off
- Hydra containers need time to initialize (tests include wait logic)

#### JWKs Endpoint Issues
- Tests expect Hydra to be available at `http://127.0.0.1:4444`
- Ensure no firewall is blocking container access
- Check container logs: `docker compose logs hydra`
