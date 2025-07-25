# Nexus - The AI Router

Plug in all your MCP servers, APIs, and LLM providers. Route everything through a unified endpoint. Aggregate, govern, and dominate your AI stack.

## Features

- **MCP Server Aggregation**: Connect multiple MCP servers (stdio, SSE, HTTP) through a single endpoint
- **Context-Aware Tool Search**: Intelligent fuzzy search across all connected tools using natural language queries
- **Protocol Support**: Supports stdio, SSE (Server-Sent Events), and streamable HTTP MCP servers
- **Flexible Configuration**: TOML-based configuration with environment variable substitution
- **Security**: Built-in CORS, CSRF protection, OAuth2, and TLS support.
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

1. **Stdio Servers**: Launch local processes
   ```toml
   [mcp.servers.my_tool]
   cmd = ["path/to/executable", "--arg1", "--arg2"]
   ```

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

## Security Considerations

- Always use environment variables for sensitive tokens
- Enable TLS verification for production deployments
- Use CORS configuration to restrict access
- Configure OAuth2 authentication for production deployments
- Ensure JWKs URLs use HTTPS in production
- Validate JWT token issuer and audience claims
- Keep your MCP servers and Nexus updated

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
