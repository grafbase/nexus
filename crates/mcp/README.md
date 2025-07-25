# MCP (Model Context Protocol) Crate

This crate provides a Model Context Protocol (MCP) implementation that acts as a gateway for routing HTTP requests to multiple downstream MCP servers. It aggregates tools from various MCP servers and provides a unified interface with search and execution capabilities.

## Overview

The MCP crate implements a server that:
- Connects to multiple downstream MCP servers (Streamable HTTP, SSE, and stdio)
- Aggregates tools from all connected servers
- Maintains a full-text search index for efficient tool discovery
- Routes tool execution requests to the appropriate downstream server

## Architecture

#### 1. McpServer (`server.rs`)
The main server implementation that:
- Implements the `ServerHandler` trait from the `rmcp` crate
- Manages two built-in tools: `search` and `execute`
- Handles MCP protocol negotiation and capabilities
- Routes incoming requests to appropriate handlers

#### 2. Downstream (`downstream/`)
Manages connections to multiple downstream MCP servers:
- **DownstreamClient**: Handles individual server connections (Streamable HTTP/SSE/stdio)
- **Downstream**: Aggregates multiple servers and their tools
- **Protocol Detection**: Automatically detects between streamable-http and SSE protocols
- **Tool Prefixing**: Prefixes tool names with server names (`server__tool`) for uniqueness

#### 3. Tool Index (`index.rs`)
A full-text search engine built on Tantivy that:
- Indexes all tools from downstream servers
- Provides search across tool names, descriptions, and parameters
- Scores results by relevance
- Tokenizes and normalizes search terms

#### 4. Built-in Tools (`tool/`)
Two primary tools exposed by the MCP server:
- **SearchTool**: Discovers relevant tools based on keywords
- **ExecuteTool**: Executes tools on downstream servers with parameters

## Key Features

### Protocol Support
- **Streamable HTTP**: Direct HTTP communication with MCP servers
- **Server-Sent Events (SSE)**: Event-driven communication (deprecated)
- **Protocol Detection**: Automatic fallback from streamable-http to SSE
- **TLS Support**: Full TLS configuration including client certificates

### Tool Discovery
- **Intelligent Search**: Full-text search across tool metadata
- **Fuzzy Matching**: Finds tools even with partial or misspelled keywords
- **Relevance Scoring**: Returns most relevant tools first
- **Deduplication**: Prevents duplicate results across servers

### Tool Execution
- **Unified Interface**: Single execution endpoint for all downstream tools
- **Error Handling**: Comprehensive error handling with suggestions
- **Parameter Validation**: Schema-based parameter validation

## Configuration

The MCP server is configured through TOML configuration:

```toml
[mcp]
enabled = true
path = "/mcp"

[mcp.servers.filesystem]
url = "http://localhost:3001/mcp"

[mcp.servers.browser]
cmd = ["npx", "@modelcontextprotocol/server-brave-search"]
```

### HTTP Configuration
```toml
[mcp.servers.secure_server]
url = "https://mcp-server.example.com/mcp"

[mcp.servers.secure_server.tls]
verify_certs = true
accept_invalid_hostnames = false
root_ca_cert_path = "/path/to/ca.pem"
client_cert_path = "/path/to/client.pem"
client_key_path = "/path/to/client.key"
```

## Usage

### Creating an MCP Router

```rust
use axum::Router;
use config::McpConfig;

async fn create_mcp_router(config: &McpConfig) -> anyhow::Result<Router> {
    mcp::router(config).await
}
```

### Tool Discovery Workflow

1. **Search for Tools**:
   ```json
   {
     "name": "search",
     "arguments": {
       "keywords": ["file", "read"]
     }
   }
   ```

2. **Execute Found Tools**:
   ```json
   {
     "name": "execute",
     "arguments": {
       "name": "filesystem__read_file",
       "arguments": {
         "path": "/path/to/file.txt"
       }
     }
   }
   ```

## Development Guide

### Adding New Tool Types

To add support for new tool types, implement the `Tool` trait:

```rust
use crate::tool::Tool;
use rmcp::model::{CallToolResult, ToolAnnotations};

struct MyTool;

impl Tool for MyTool {
    type Parameters = MyParameters; // Must implement JsonSchema + Deserialize

    fn name() -> &'static str {
        "my_tool"
    }

    fn description(&self) -> Cow<'_, str> {
        "Description of what this tool does".into()
    }

    /// Returns the tool's annotations, which specify metadata about the tool's behavior.
    fn annotations(&self) -> ToolAnnotations {
        ToolAnnotations::new().read_only(true)
    }

    async fn call(&self, _parts: Parts, parameters: Self::Parameters) -> anyhow::Result<CallToolResult> {
        // Implementation
        todo!()
    }
}
```

### Extending Downstream Support

To add support for new downstream protocols:

1. Create a new transport in `downstream/client.rs`
2. Add protocol detection logic
3. Update the `http_service` function to handle the new protocol

### Search Index Customization

The search index can be customized by modifying `index.rs`:

```rust
// Add new search fields
struct IndexFields {
    // ... existing fields
    custom_field: Field,
}

// Modify token generation
fn generate_search_tokens(&self, tool: &Tool) -> anyhow::Result<String> {
    // Custom tokenization logic
}
```

## Testing

### Integration Tests
```bash
cargo test -p integration-tests
```

### Manual Testing

To test MCP functionality with real downstream servers, you can use the included `hello_service` example:

1. **Start the hello_service example**:
   ```bash
   cargo run -p hello_service
   ```
   This starts an MCP server on `http://localhost:3000/mcp` with a simple "hello" tool.

2. **Create a configuration file** (`nexus.toml`):
   ```toml
   [server]
   listen_address = "127.0.0.1:8000"

   [mcp]
   enabled = true
   path = "/mcp"

   # Connect to the hello_service example
   [mcp.servers.hello_service]
   protocol = "streamable-http"
   url = "http://localhost:3000/mcp"
   ```

3. **Start the nexus server**:
   ```bash
   cargo run -p nexus -- --config nexus.toml
   ```

4. **Test the MCP functionality**:
   - Search for tools: `POST http://localhost:8080/mcp` with search tool
   - Execute tools: `POST http://localhost:8080/mcp` with execute tool

   Example search request:
   ```json
   {
     "method": "tools/call",
     "params": {
       "name": "search",
       "arguments": {
         "keywords": ["hello", "greeting"]
       }
     }
   }
   ```

   Example execute request:
   ```json
   {
     "method": "tools/call",
     "params": {
       "name": "execute",
       "arguments": {
         "name": "hello_service__hello",
         "arguments": {
           "name": "World"
         }
       }
     }
   }
   ```

## Logging

The crate uses the `log` crate for logging. Enable debug logging to see detailed information about:
- Server connections and protocol detection
- Tool indexing and search operations
- Request routing and execution

```bash
cargo run -- --log debug
```

Tests emit logs, but only if you run them with the special environment variable:

```bash
TEST_LOG=1 cargo test
```
