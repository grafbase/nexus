<div align="center">
<picture>
  <source width="600" height="244" srcset="https://github.com/user-attachments/assets/9ca64651-b601-45e3-8ba1-f7bfb91625ab" media="(prefers-color-scheme: dark)">
  <source width="600" height="244" srcset="https://github.com/user-attachments/assets/5ee33450-f9ee-4e47-b0ed-0d302110c4ce"" media="(prefers-color-scheme: light)">
  <img src="https://github.com/user-attachments/assets/9ca64651-b601-45e3-8ba1-f7bfb91625ab" alt="Nexus logo">
</picture>
</div>

<p align="center">
  Plug in all your MCP servers, APIs, and LLM providers. Route everything through a unified endpoint. <br />
  Aggregate, govern, and control your AI stack.
</p>

## Features

- **MCP Server Aggregation**: Connect multiple MCP servers (STDIO, SSE, HTTP) through a single endpoint
- **Context-Aware Tool Search**: Intelligent fuzzy search across all connected tools using natural language queries
- **Protocol Support**: Supports STDIO (subprocess), SSE (Server-Sent Events), and streamable HTTP MCP servers
- **Flexible Configuration**: TOML-based configuration with environment variable substitution
- **Security**: Built-in CORS, CSRF protection, OAuth2, and TLS support
- **Rate Limiting**: Multi-level rate limiting with in-memory or Redis backends
- **Docker Ready**: Available as a container image with minimal configuration needed

## Installation

### Quick Install (Linux/Windows (WSL)/macOS)

```bash
curl -fsSL https://nexusrouter.com/install | bash
```

### Docker

Pull the latest image:
```bash
docker pull ghcr.io/grafbase/nexus:latest
```

Or use a specific version:
```bash
docker pull ghcr.io/grafbase/nexus:0.2.0
```

### Build from Source

```bash
git clone https://github.com/grafbase/nexus
cd nexus
cargo build --release
```

## Running Nexus

### Using the Binary

```bash
nexus
```

### Using Docker

```bash
docker run -p 8000:8000 -v /path/to/config:/etc/nexus.toml ghcr.io/grafbase/nexus:latest
```

### Docker Compose Example

```yaml
services:
  nexus:
    image: ghcr.io/grafbase/nexus:latest
    ports:
      - "8000:8000"
    volumes:
      - ./nexus.toml:/etc/nexus.toml
    environment:
      - GITHUB_TOKEN=${GITHUB_TOKEN}
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8000/health"]
      interval: 30s
      timeout: 10s
      retries: 3
```

## Configuration

Create a `nexus.toml` file to configure Nexus:

```toml
[mcp.servers.github]
url = "https://api.githubcopilot.com/mcp/"
auth.token = "{{ env.GITHUB_TOKEN }}"

[mcp.servers.filesystem]
cmd = ["npx", "-y", "@modelcontextprotocol/server-filesystem", "/Users/YOUR_USERNAME/Desktop"]

[mcp.servers.python_server]
cmd = ["python", "-m", "mcp_server"]
env = { PYTHONPATH = "/opt/mcp" }
cwd = "/workspace"
```

### Configuration Options

#### Server Configuration

- `server.listen_address`: The address and port Nexus will listen on (default: `127.0.0.1:8000`)
- `server.health.enabled`: Enable health endpoint (default: `true`)
- `server.health.path`: Health check endpoint path (default: `/health`)

#### MCP Configuration

- `mcp.enabled`: Enable MCP functionality (default: `true`)
- `mcp.path`: MCP endpoint path (default: `/mcp`)

#### MCP Server Types

1. **STDIO Servers**: Launch local processes that communicate via standard input/output
   ```toml
   [mcp.servers.my_tool]
   cmd = ["path/to/executable", "--arg1", "--arg2"]

   # Optional: Set environment variables
   env = { DEBUG = "1", API_KEY = "{{ env.MY_API_KEY }}" }

   # Optional: Set working directory
   cwd = "/path/to/working/directory"

   # Optional: Configure stderr handling (default: "null")
   stderr = "inherit"  # Show in console
   # or
   stderr = { file = "/var/log/mcp/server.log" }  # Log to file
   ```

   **Note**: STDIO servers must output valid JSON-RPC messages on stdout. The `cmd` array must have at least one element (the executable).

2. **SSE Servers**: Connect to Server-Sent Events endpoints
   ```toml
   [mcp.servers.my_sse_server]
   protocol = "sse"
   url = "http://example.com/sse"
   message_url = "http://example.com/messages"  # Optional
   ```

3. **HTTP Servers**: Connect to streamable HTTP endpoints
   ```toml
   [mcp.servers.my_http_server]
   protocol = "streamable-http"
   url = "https://api.example.com/mcp"
   ```

For remote MCP servers, if you omit the protocol Nexus will first try streamable HTTP and then SSE.

#### Authentication

Add service token authentication to any server:

```toml
[mcp.servers.my_server.auth]
token = "your-token-here"
# Or use environment variables
token = "{{ env.MY_API_TOKEN }}"
```

If you enable OAuth2 authentication to your server, and your downstream servers all use the same authentication server, you can configure Nexus to forward the request access token to the downstream server.

```toml
[mcp.servers.my_server.auth]
type = "forward"
```

#### OAuth2 Authentication

Configure OAuth2 authentication to protect your Nexus endpoints:

```toml
[server.oauth]
url = "https://your-oauth-provider.com/.well-known/jwks.json"
poll_interval = "5m"
expected_issuer = "https://your-oauth-provider.com"
expected_audience = "your-service-audience"

[server.oauth.protected_resource]
resource = "https://your-nexus-instance.com"
authorization_servers = ["https://your-oauth-provider.com"]
```

OAuth2 configuration options:
- `url`: JWKs endpoint URL for token validation
- `poll_interval`: How often to refresh JWKs (optional, default: no polling)
- `expected_issuer`: Expected `iss` claim in JWT tokens (optional)
- `expected_audience`: Expected `aud` claim in JWT tokens (optional)
- `protected_resource.resource`: URL of this protected resource
- `protected_resource.authorization_servers`: List of authorization server URLs

When OAuth2 is enabled, all endpoints except `/health` and `/.well-known/oauth-protected-resource` require valid JWT tokens in the `Authorization: Bearer <token>` header.

#### Rate Limiting

Nexus supports comprehensive rate limiting to prevent abuse and ensure fair resource usage:

```toml
# Global rate limiting configuration
[server.rate_limits]
enabled = true

# Storage backend configuration
[server.rate_limits.storage]
type = "memory"  # or "redis" for distributed rate limiting
# For Redis backend:
# url = "redis://localhost:6379"
# key_prefix = "nexus:rate_limit:"

# Global rate limit (applies to all requests)
[server.rate_limits.global]
limit = 1000
interval = "60s"

# Per-IP rate limit
[server.rate_limits.per_ip]
limit = 100
interval = "60s"

# Per-MCP server rate limits
[mcp.servers.my_server.rate_limits]
limit = 50
interval = "60s"

# Tool-specific rate limits (override server defaults)
[mcp.servers.my_server.rate_limits.tools]
expensive_tool = { limit = 10, interval = "60s" }
cheap_tool = { limit = 100, interval = "60s" }
```

**Rate Limiting Features:**
- **Multiple levels**: Global, per-IP, per-server, and per-tool limits
- **Storage backends**: In-memory (single instance) or Redis (distributed)
- **Flexible intervals**: Configure time windows for each limit
- **Tool-specific overrides**: Set different limits for expensive operations

**Redis Backend Configuration:**
```toml
[server.rate_limits.storage]
type = "redis"
url = "redis://localhost:6379"
key_prefix = "nexus:rate_limit:"
response_timeout = "1s"
connection_timeout = "5s"

# Connection pool settings
[server.rate_limits.storage.pool]
max_size = 16
min_idle = 0
timeout_create = "5s"
timeout_wait = "5s"
timeout_recycle = "300s"

# TLS configuration for Redis (optional)
[server.rate_limits.storage.tls]
enabled = true
ca_cert_path = "/path/to/ca.crt"
client_cert_path = "/path/to/client.crt"  # For mutual TLS
client_key_path = "/path/to/client.key"
# insecure = true  # WARNING: Only for development/testing, skips certificate validation
```

**Note**: When configuring tool-specific rate limits, Nexus will warn if you reference tools that don't exist.

#### TLS Configuration

Configure TLS for downstream connections:

```toml
[mcp.servers.my_server.tls]
verify_certs = true
accept_invalid_hostnames = false
root_ca_cert_path = "/path/to/ca.pem"
client_cert_path = "/path/to/client.pem"
client_key_path = "/path/to/client.key"
```

## Adding to AI Assistants

### Cursor

Add to your Cursor settings:

1. Open Cursor Settings (Cmd+, on macOS)
2. Search for "Model Context Protocol"
3. Enable MCP support
4. Add to the MCP server configuration:

```json
{
  "nexus": {
    "transport": {
      "type": "http",
      "url": "http://localhost:8000/mcp"
    }
  }
}
```

Make sure Nexus is running on `localhost:8000` (or adjust the URL accordingly).

### Claude Code

Add to your Claude Code configuration:

1. Open Claude Code and run the command:
   ```bash
   claude mcp add --transport http nexus http://localhost:8000/mcp
   ```

2. Or add it to your project's `.mcp.json` file:
   ```json
   {
     "mcpServers": {
       "nexus": {
         "type": "http",
         "url": "http://localhost:8000/mcp"
       }
     }
   }
   ```

3. Verify the connection:
   ```bash
   claude mcp list
   ```

Make sure Nexus is running before starting Claude Code.

## How It Works

Nexus provides two main tools to AI assistants:

1. **`search`**: A context-aware tool search that uses fuzzy matching to find relevant tools across all connected MCP servers
2. **`execute`**: Executes a specific tool with the provided parameters

When an AI assistant connects to Nexus, it can:
1. Search for tools using natural language queries
2. Discover tool names, descriptions, and required parameters
3. Execute tools from any connected MCP server

All tools from downstream servers are namespaced with their server name (e.g., `github__search_code`, `filesystem__read_file`).

### STDIO Server Integration

STDIO servers are spawned as child processes and communicate via JSON-RPC over standard input/output:

1. **Process Management**: Nexus automatically manages the lifecycle of STDIO server processes
2. **Tool Discovery**: Tools from STDIO servers are discovered dynamically and indexed for search
3. **Error Handling**: If a STDIO process crashes or outputs invalid JSON, appropriate errors are returned
4. **Environment Isolation**: Each STDIO server runs in its own process with configurable environment

## Example Usage

Once configured, AI assistants can interact with Nexus like this:

1. **Search for tools**:
   ```
   User: "I need to search for code on GitHub"
   Assistant: Let me search for GitHub-related tools...
   [Calls search with keywords: ["github", "code", "search"]]
   ```

2. **Execute tools**:
   ```
   Assistant: I found the `github__search_code` tool. Let me search for your query...
   [Calls execute with name: "github__search_code" and appropriate arguments]
   ```

## Common STDIO Server Examples

### Python MCP Server
```toml
[mcp.servers.python_tools]
cmd = ["python", "-m", "my_mcp_server"]
env = { PYTHONPATH = "/opt/mcp", PYTHONUNBUFFERED = "1" }
stderr = "inherit"  # See Python output during development
```

### Node.js MCP Server
```toml
[mcp.servers.node_tools]
cmd = ["node", "mcp-server.js"]
cwd = "/path/to/project"
env = { NODE_ENV = "production" }
```

### Using npx packages
```toml
[mcp.servers.filesystem]
cmd = ["npx", "-y", "@modelcontextprotocol/server-filesystem", "/home/user"]
```

## Troubleshooting STDIO Servers

### Server doesn't start
- **Check executable path**: Ensure the command exists and is executable
- **View stderr output**: Set `stderr = "inherit"` temporarily to see error messages
- **Verify JSON-RPC output**: The server must output valid JSON-RPC on stdout
- **Check working directory**: Ensure `cwd` path exists if specified

### Tools not appearing
- **Wait for initialization**: STDIO servers may take a moment to start
- **Use search**: STDIO tools only appear in search results, not in the base tool list
- **Check server logs**: Enable stderr logging to see if the server is responding to tool list requests

## Security Considerations

- Always use environment variables for sensitive tokens
- Enable TLS verification for production deployments
- Use CORS configuration to restrict access
- Configure OAuth2 authentication for production deployments
- Ensure JWKs URLs use HTTPS in production
- Validate JWT token issuer and audience claims
- Keep your MCP servers and Nexus updated
- Be cautious when running STDIO servers with elevated privileges
- Validate and sanitize any user input passed to STDIO server commands

### OAuth2 Security

When using OAuth2 authentication:

1. **Use HTTPS**: Always use HTTPS for JWKs URLs and protected resources in production
2. **Validate Claims**: Configure `expected_issuer` and `expected_audience` to validate JWT claims
3. **Metadata Endpoint**: The `/.well-known/oauth-protected-resource` endpoint provides OAuth2 metadata and is publicly accessible
4. **Health Checks**: The `/health` endpoint bypasses OAuth2 authentication for monitoring systems

## License

Nexus is licensed under the Mozilla Public License 2.0 (MPL-2.0). See the LICENSE file for details.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines on how to contribute to Nexus.

## Support

- Documentation: [https://nexusrouter.com/docs](https://nexusrouter.com/docs)
- Issues: [https://github.com/grafbase/nexus/issues](https://github.com/grafbase/nexus/issues)
- Discord: [Grafbase Discord](https://discord.gg/grafbase)
