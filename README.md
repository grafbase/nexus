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
- **LLM Provider Routing**: Unified interface for OpenAI, Anthropic, Google, and AWS Bedrock LLM providers with full tool calling support
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

Or use the stable version:
```bash
docker pull ghcr.io/grafbase/nexus:stable
```

Or use a specific version:
```bash
docker pull ghcr.io/grafbase/nexus:X.Y.Z
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
# LLM Provider configuration
[llm.providers.openai]
type = "openai"
api_key = "{{ env.OPENAI_API_KEY }}"
forward_token = true

# Model configuration (at least one model required per provider)
[llm.providers.openai.models.gpt-4]
[llm.providers.openai.models.gpt-3-5-turbo]

[llm.providers.anthropic]
type = "anthropic"
api_key = "{{ env.ANTHROPIC_API_KEY }}"

[llm.providers.anthropic.models.claude-3-5-sonnet-20241022]

# MCP Server configuration
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

#### LLM Configuration

- `llm.enabled`: Enable LLM functionality (default: `true`)
- `llm.path`: LLM endpoint path (default: `/llm`)

For detailed LLM provider configuration, see the LLM Provider Configuration section below.

#### MCP Configuration

- `mcp.enabled`: Enable MCP functionality (default: `true`)
- `mcp.path`: MCP endpoint path (default: `/mcp`)
- `mcp.enable_structured_content`: Control MCP search tool response format (default: `true`)
  - When `true`: Uses modern `structuredContent` field for better performance and type safety
  - When `false`: Uses legacy `content` field with `Content::json` objects for compatibility with older MCP clients

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

#### Header Insertion for MCP Servers

Nexus supports inserting static headers when making requests to MCP servers. Headers can be configured globally (for all MCP servers) or per-server.

**Note**: MCP currently only supports header insertion with static values. Headers from incoming requests are not forwarded.

##### Global MCP Headers

Configure headers that apply to all MCP servers:

```toml
# Global headers for all MCP servers
[[mcp.headers]]
rule = "insert"
name = "X-Application"
value = "nexus-router"

[[mcp.headers]]
rule = "insert"
name = "X-API-Version"
value = "v1"
```

##### Server-Specific Headers

Configure headers for individual HTTP-based MCP servers:

```toml
[mcp.servers.my_server]
url = "https://api.example.com/mcp"

# Insert headers for this specific server
[[mcp.servers.my_server.headers]]
rule = "insert"
name = "X-API-Key"
value = "{{ env.MY_API_KEY }}"  # Environment variable substitution

[[mcp.servers.my_server.headers]]
rule = "insert"
name = "X-Service-Name"
value = "my-service"
```

##### MCP Header Features

- **Static Values Only**: Headers are set at client initialization time with static values
- **Environment Variables**: Use `{{ env.VAR_NAME }}` syntax for environment variable substitution
- **HTTP Servers Only**: Headers only apply to HTTP-based MCP servers (not STDIO servers)
- **Insert Rule**: Currently only the `insert` rule is supported for MCP

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

Nexus supports rate limiting to prevent abuse and ensure fair resource usage:

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

#### LLM Token Rate Limiting

Nexus provides token-based rate limiting for LLM providers to help control costs and prevent abuse. Unlike request-based rate limits, token rate limits count an estimate of actual tokens consumed.

##### Prerequisites

**IMPORTANT**: LLM rate limiting requires client identification to be enabled:

```toml
[server.client_identification]
enabled = true

# Choose identification methods (at least one required)
client_id.jwt_claim = "sub"                    # Extract ID from JWT 'sub' claim
# or
client_id.http_header = "X-Client-ID"          # Extract ID from HTTP header

# Optional: Limit groups per user (at most one allowed)
group_id.jwt_claim = "groups"                  # JWT claim containing user's group
# or
group_id.http_header = "X-Group-ID"            # Extract ID from HTTP header

# You must provide a list of allowed groups
[server.client_identification.validation]
group_values = ["free", "pro", "max"]
```

Without client identification, rate limits cannot be enforced and requests will fail with a configuration error.

##### Configuration Hierarchy

Token rate limits can be configured at four levels, from most to least specific:

1. **Model per user + group**: Specific model for specific each user in a group
2. **Model per user**: Specific model for each user
3. **Provider per user + group**: All models from provider for each user in a group
4. **Provider per user**: All models from provider for each user

The most specific applicable limit is always used.

##### Basic Configuration

```toml
# Provider-level default rate limit (applies to all models)
[llm.providers.openai.rate_limits.per_user]
input_token_limit = 100000        # 100K input tokens
interval = "1m"                   # Per minute

# Model-specific rate limit (overrides provider default)
[llm.providers.openai.models.gpt-4.rate_limits.per_user]
input_token_limit = 50000         # More restrictive for expensive model
interval = "30s"
```

##### Group-Based Rate Limits

Configure different limits for user groups (requires `group_id` and `group_values` in client identification):

```toml
# Provider-level group limits
[llm.providers.openai.rate_limits.per_user.groups]
free = { input_token_limit = 10000, interval = "60s" }
pro = { input_token_limit = 100000, interval = "60s" }
enterprise = { input_token_limit = 1000000, interval = "60s" }

# Model-specific group limits (override provider groups)
[llm.providers.openai.models.gpt-4.rate_limits.per_user.groups]
free = { input_token_limit = 5000, interval = "60s" }
pro = { input_token_limit = 50000, interval = "60s" }
enterprise = { input_token_limit = 500000, interval = "60s" }
```

The limits are per user, but you can define different limits if the user is part of a specific group. If the user does not belong to any group, they will be assigned to the per-user limits.

##### Complete Example

```toml
# Client identification (REQUIRED for rate limiting)
[server.client_identification]
enabled = true
client_id.jwt_claim = "sub"
group_id.jwt_claim = "subscription_tier"
[server.client_identification.validation]
group_values = ["free", "pro", "enterprise"]

# OpenAI provider with rate limiting
[llm.providers.openai]
type = "openai"
api_key = "{{ env.OPENAI_API_KEY }}"

# Provider-level defaults
[llm.providers.openai.rate_limits.per_user]
input_token_limit = 100000
interval = "60s"

[llm.providers.openai.rate_limits.per_user.groups]
free = { input_token_limit = 10000, interval = "60s" }
pro = { input_token_limit = 100000, interval = "60s" }

# GPT-4 specific limits (more restrictive)
[llm.providers.openai.models.gpt-4]
[llm.providers.openai.models.gpt-4.rate_limits.per_user]
input_token_limit = 50000
interval = "60s"

[llm.providers.openai.models.gpt-4.rate_limits.per_user.groups]
free = { input_token_limit = 5000, interval = "60s" }
pro = { input_token_limit = 50000, interval = "60s" }

# GPT-3.5 uses provider defaults
[llm.providers.openai.models.gpt-3-5-turbo]
```

##### How Token Counting Works

1. **Input Tokens Only**: Rate limiting is based solely on input tokens counted from the request's messages and system prompts
2. **No Output Buffering**: Output tokens and `max_tokens` parameter are NOT considered in rate limit calculations
3. **Pre-check**: Input tokens are checked against rate limits before processing
4. **Token Accumulation**: Uses a sliding window algorithm to track usage over time

Note: The rate limiting is designed to be predictable and based only on what the client sends, not on variable output sizes.

##### Rate Limit Response

When rate limited, the server returns a 429 status code. No Retry-After headers are sent to maintain consistency with downstream LLM provider behavior.

##### Error Responses

When rate limits are exceeded:

```json
{
  "error": {
    "message": "Rate limit exceeded: Token rate limit exceeded. Please try again later.",
    "type": "rate_limit_error",
    "code": 429
  }
}
```

##### Important Notes

- **Per-User Limits**: All limits are per individual user/client ID
- **No Shared Pools**: Currently, there are no shared/global token pools
- **Streaming Support**: Token counting works with both regular and streaming responses
- **Provider Agnostic**: Works consistently across all LLM providers
- **Validation**: Configuration is validated at startup; invalid group names will cause errors

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

### LLM Provider Configuration

Nexus provides a unified interface for multiple LLM providers, allowing you to route chat completions through various services with a consistent API.

#### Enabling LLM Routing

```toml
[llm]
enabled = true  # Enable LLM functionality (default: true)
path = "/llm"   # LLM endpoint path (default: "/llm")
```

#### Supported Providers

Nexus currently supports four major LLM providers with full tool calling capabilities:

1. **OpenAI** (including OpenAI-compatible APIs) - Full tool calling and parallel execution support
2. **Anthropic** (Claude models) - Tool calling with function definitions and tool choice controls
3. **Google** (Gemini models) - Function calling with parameter schemas and tool selection
4. **AWS Bedrock** (Multiple model families via AWS) - Tool calling support across all supported model families

#### Provider Configuration

Configure one or more LLM providers in your `nexus.toml`:

##### OpenAI Provider

```toml
[llm.providers.openai]
type = "openai"
api_key = "{{ env.OPENAI_API_KEY }}"
# Optional: Use a custom base URL (for Azure OpenAI, proxies, or compatible APIs)
base_url = "https://api.openai.com/v1"  # Default

# Model Configuration (REQUIRED - at least one model must be configured)
[llm.providers.openai.models.gpt-4]
# Optional: Rename the model for your users
# rename = "smart-model"  # Users will see "openai/smart-model"

[llm.providers.openai.models.gpt-3-5-turbo]
# Models without rename use their original ID
```

##### Anthropic Provider

```toml
[llm.providers.anthropic]
type = "anthropic"
api_key = "{{ env.ANTHROPIC_API_KEY }}"
# Optional: Use a custom base URL
base_url = "https://api.anthropic.com/v1"  # Default

# Model Configuration (REQUIRED - at least one model must be configured)
[llm.providers.anthropic.models.claude-3-opus-20240229]

[llm.providers.anthropic.models.claude-3-5-sonnet-20241022]
```

##### Google Provider

```toml
[llm.providers.google]
type = "google"
api_key = "{{ env.GOOGLE_API_KEY }}"
# Optional: Use a custom base URL
base_url = "https://generativelanguage.googleapis.com/v1beta"  # Default

# Model Configuration (REQUIRED - at least one model must be configured)
# Note: Model IDs with dots must be quoted in TOML
[llm.providers.google.models."gemini-1.5-flash"]

[llm.providers.google.models.gemini-pro]
```

##### AWS Bedrock Provider

```toml
[llm.providers.bedrock]
type = "bedrock"
# Optional: AWS profile to use (defaults to environment settings)
profile = "{{ env.AWS_PROFILE }}"
# Optional: AWS region (defaults to environment or us-east-1)
region = "us-west-2"

# Model Configuration (REQUIRED - at least one model must be configured)
# Bedrock uses model IDs with dots, so they must be quoted
[llm.providers.bedrock.models."anthropic.claude-3-5-sonnet-20241022-v2:0"]

[llm.providers.bedrock.models."anthropic.claude-3-opus-20240229-v1:0"]

[llm.providers.bedrock.models."amazon.nova-micro-v1:0"]

[llm.providers.bedrock.models."meta.llama3-8b-instruct-v1:0"]

[llm.providers.bedrock.models."ai21.jamba-1.5-mini-v1:0"]

# Rename models for simpler access
[llm.providers.bedrock.models.claude-haiku]
rename = "anthropic.claude-3-5-haiku-20241022-v1:0"  # Users will access as "bedrock/claude-haiku"

[llm.providers.bedrock.models.jamba-mini]
rename = "ai21.jamba-1.5-mini-v1:0"  # Users will access as "bedrock/jamba-mini"
```

AWS Bedrock provides access to multiple foundation models through a single API. Key features:
- **Unified Access**: Use models from Anthropic, Amazon, Meta, Cohere, and more through one interface
- **AWS Integration**: Leverages AWS credentials and IAM for authentication
- **Regional Availability**: Models may vary by AWS region
- **Native Streaming**: Full streaming support for all compatible models

**Authentication**: Bedrock uses standard AWS credential chain:
1. Environment variables (`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`)
2. AWS profile (specified in config or via `AWS_PROFILE` environment variable)
3. IAM role (when running on EC2/ECS/Lambda)
4. AWS SSO credentials

**Supported Model Families**:
- **AI21 Jamba**: Jamba 1.5 Mini and Large models with 256K context window
- **Anthropic Claude**: All Claude 3 models (Opus, Sonnet, Haiku) and Claude Instant
- **Amazon Nova**: Nova Micro, Lite, Pro models
- **Amazon Titan**: Titan Text and Embeddings models
- **Meta Llama**: Llama 2 and Llama 3 models
- **Cohere Command**: Command and Command R models
- **DeepSeek**: DeepSeek R1 reasoning models
- **Mistral**: Mistral 7B and Mixtral models

**Model ID Format**: Bedrock model IDs follow the pattern `provider.model-name-version:revision`, for example:
- `ai21.jamba-1.5-mini-v1:0`
- `anthropic.claude-3-5-sonnet-20241022-v2:0`
- `amazon.nova-micro-v1:0`
- `meta.llama3-8b-instruct-v1:0`

#### Model Configuration

Each LLM provider requires explicit model configuration. This ensures that only the models you want to expose are available through Nexus.

##### Basic Model Configuration

```toml
[llm.providers.openai]
type = "openai"
api_key = "{{ env.OPENAI_API_KEY }}"

# Each model you want to expose must be explicitly configured
[llm.providers.openai.models.gpt-4]
[llm.providers.openai.models.gpt-3-5-turbo]
```

##### Model Renaming

You can rename models to provide custom identifiers for your users:

```toml
[llm.providers.openai.models.gpt-4]
rename = "smart-model"  # Users will access this as "openai/smart-model"

[llm.providers.openai.models.gpt-3-5-turbo]
rename = "fast-model"   # Users will access this as "openai/fast-model"
```

This is useful for:
- Creating business-friendly model names
- Abstracting away provider-specific model names
- Providing consistent naming across different providers

##### TOML Syntax for Model IDs

Model IDs that contain dots must be quoted in TOML:

```toml
# Correct - dots in model IDs require quotes
[llm.providers.google.models."gemini-1.5-flash"]
[llm.providers.google.models."gemini-1.5-pro"]

# Also correct - no dots, no quotes needed
[llm.providers.google.models.gemini-pro]
```

#### Multiple Provider Configuration

You can configure multiple instances of the same provider type with different names:

```toml
# Primary OpenAI account
[llm.providers.openai_primary]
type = "openai"
api_key = "{{ env.OPENAI_PRIMARY_KEY }}"

[llm.providers.openai_primary.models.gpt-4]
[llm.providers.openai_primary.models.gpt-3-5-turbo]

# Secondary OpenAI account or Azure OpenAI
[llm.providers.openai_secondary]
type = "openai"
api_key = "{{ env.OPENAI_SECONDARY_KEY }}"
base_url = "https://my-azure-instance.openai.azure.com/v1"

[llm.providers.openai_secondary.models.gpt-4]
rename = "azure-gpt-4"  # Distinguish from primary account

# Anthropic
[llm.providers.claude]
type = "anthropic"
api_key = "{{ env.ANTHROPIC_API_KEY }}"

[llm.providers.claude.models.claude-3-opus-20240229]

# Google Gemini
[llm.providers.gemini]
type = "google"
api_key = "{{ env.GOOGLE_API_KEY }}"

[llm.providers.gemini.models."gemini-1.5-flash"]
```

#### Token Forwarding

Nexus supports token forwarding, allowing users to provide their own API keys at request time instead of using the configured keys. This feature is opt-in and disabled by default.

##### Configuring Token Forwarding

Enable token forwarding for any provider by setting `forward_token = true`:

```toml
[llm.providers.openai]
type = "openai"
api_key = "{{ env.OPENAI_API_KEY }}"  # Fallback key (optional with token forwarding)
forward_token = true  # Enable token forwarding for this provider

[llm.providers.openai.models.gpt-4]
[llm.providers.openai.models.gpt-3-5-turbo]

[llm.providers.anthropic]
type = "anthropic"
# No api_key required when token forwarding is enabled
forward_token = true

[llm.providers.anthropic.models.claude-3-5-sonnet-20241022]

[llm.providers.google]
type = "google"
api_key = "{{ env.GOOGLE_API_KEY }}"
forward_token = false  # Explicitly disabled (default)

[llm.providers.google.models."gemini-1.5-flash"]
```

##### Using Token Forwarding

When token forwarding is enabled for a provider, users can pass their own API key using the `X-Provider-API-Key` header:

```bash
# Using your own OpenAI key
curl -X POST http://localhost:8000/llm/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "X-Provider-API-Key: sk-your-openai-key" \
  -d '{
    "model": "openai/gpt-4",
    "messages": [{"role": "user", "content": "Hello"}]
  }'

# Using your own Anthropic key
curl -X POST http://localhost:8000/llm/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "X-Provider-API-Key: sk-ant-your-anthropic-key" \
  -d '{
    "model": "anthropic/claude-3-opus-20240229",
    "messages": [{"role": "user", "content": "Hello"}]
  }'
```

##### Token Forwarding Behavior

- **When token forwarding is enabled (`forward_token = true`)**:
  - User-provided keys (via header) take priority
  - Falls back to configured key if no header is provided
  - Returns 401 error if neither key is available

- **When token forwarding is disabled (`forward_token = false`, default)**:
  - Always uses the configured API key
  - Ignores the `X-Provider-API-Key` header
  - Returns 401 error if no configured key exists

##### Security Considerations

- **OAuth2 Integration**: When OAuth2 is enabled, users must still authenticate with Nexus even when using token forwarding
- **Key Validation**: API keys are validated by the provider's API
- **No Logging**: User-provided keys are never logged
- **HTTPS Recommended**: Always use HTTPS in production to protect API keys in transit

##### Provider Limitations
- **AWS Bedrock**: Token forwarding is **not supported** for Bedrock providers. Bedrock uses AWS IAM credentials and request signing, which cannot be provided via simple API key headers. You must configure AWS credentials at the provider level (via environment variables, AWS profile, or explicit credentials in configuration).

#### Header Transformation for LLM Providers

Nexus supports header transformation for LLM providers, allowing you to forward, insert, remove, or rename headers when making requests to LLM APIs.

##### Provider-Level Headers

Configure header rules at the provider level:

```toml
[llm.providers.openai]
type = "openai"
api_key = "{{ env.OPENAI_API_KEY }}"

# Forward headers from incoming requests
[[llm.providers.openai.headers]]
rule = "forward"
name = "X-Request-ID"

# Forward with a default value if not present
[[llm.providers.openai.headers]]
rule = "forward"
name = "X-Trace-ID"
default = "generated-trace-id"

# Forward and rename
[[llm.providers.openai.headers]]
rule = "forward"
name = "X-Custom-Header"
rename = "X-OpenAI-Custom"

# Insert static headers
[[llm.providers.openai.headers]]
rule = "insert"
name = "X-OpenAI-Beta"
value = "assistants=v2"

# Remove headers
[[llm.providers.openai.headers]]
rule = "remove"
name = "X-Internal-Secret"

# Pattern-based forwarding (regex)
[[llm.providers.openai.headers]]
rule = "forward"
pattern = "X-Debug-.*"

# Pattern-based removal
[[llm.providers.openai.headers]]
rule = "remove"
pattern = "X-Internal-.*"

# Rename and duplicate (keeps original and adds renamed copy)
[[llm.providers.openai.headers]]
rule = "rename_duplicate"
name = "X-User-ID"
rename = "X-OpenAI-User"
```

##### Model-Level Headers

Configure headers for specific models (overrides provider-level rules):

```toml
[llm.providers.openai.models.gpt-4]
# Model-specific headers override provider headers
[[llm.providers.openai.models.gpt-4.headers]]
rule = "insert"
name = "X-Model-Config"
value = "premium"

[[llm.providers.openai.models.gpt-4.headers]]
rule = "forward"
pattern = "X-Premium-.*"
```

##### Header Rule Types

1. **forward**: Pass headers from incoming requests to the LLM provider
   - `name`: Single header name to forward
   - `pattern`: Regex pattern to match multiple headers
   - `default`: Optional default value if header is missing
   - `rename`: Optional new name for the forwarded header

2. **insert**: Add static headers to requests
   - `name`: Header name
   - `value`: Static value (supports `{{ env.VAR }}` substitution)

3. **remove**: Remove headers before sending to provider
   - `name`: Single header name to remove
   - `pattern`: Regex pattern to match headers to remove

4. **rename_duplicate**: Forward header with both original and new name
   - `name`: Original header name
   - `rename`: New header name for the duplicate
   - `default`: Optional default if header is missing

##### Important Notes

- **AWS Bedrock**: Does not support custom headers due to SigV4 signing requirements
- **Priority**: Model-level rules override provider-level rules
- **Token Forwarding**: The `X-Provider-API-Key` header is handled separately for token forwarding
- **Pattern Matching**: Patterns are case-insensitive regex expressions

#### Using the LLM API

Once configured, you can interact with LLM providers through Nexus's unified API:

##### List Available Models

```bash
curl http://localhost:8000/llm/models
```

Response:
```json
{
  "object": "list",
  "data": [
    {
      "id": "openai_primary/gpt-4-turbo",
      "object": "model",
      "created": 1677651200,
      "owned_by": "openai"
    },
    {
      "id": "claude/claude-3-5-sonnet-20241022",
      "object": "model",
      "created": 1709164800,
      "owned_by": "anthropic"
    },
    {
      "id": "gemini/gemini-1.5-pro",
      "object": "model",
      "created": 1710000000,
      "owned_by": "google"
    }
  ]
}
```

##### Tool Calling (Function Calling)

Nexus supports advanced tool calling across all LLM providers, allowing models to invoke external functions with structured parameters. All providers use the standardized OpenAI tool calling format for consistency.

**Basic Tool Calling Example:**

```bash
curl -X POST http://localhost:8000/llm/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "anthropic/claude-3-5-sonnet-20241022",
    "messages": [
      {"role": "user", "content": "What'\''s the weather in San Francisco?"}
    ],
    "tools": [{
      "type": "function",
      "function": {
        "name": "get_weather",
        "description": "Get current weather for a location",
        "parameters": {
          "type": "object",
          "properties": {
            "location": {"type": "string", "description": "City and state"},
            "unit": {"type": "string", "enum": ["celsius", "fahrenheit"]}
          },
          "required": ["location"]
        }
      }
    }],
    "tool_choice": "auto"
  }'
```

**Tool Choice Options:**
- `"auto"` (default): Model decides whether to call tools
- `"none"`: Model won't call any tools
- `"required"`: Model must call at least one tool
- `{"type": "function", "function": {"name": "specific_function"}}`: Force a specific tool

**Parallel Tool Calls:**

When supported by the provider, models can call multiple tools simultaneously:

```json
{
  "model": "openai/gpt-4",
  "messages": [{"role": "user", "content": "Get weather for NYC and LA"}],
  "tools": [/* tool definitions */],
  "parallel_tool_calls": true
}
```

**Tool Conversation Flow:**

Tool calling creates a multi-turn conversation:

1. **User message** → asks a question requiring tool use
2. **Assistant message** → responds with tool calls (no content)
3. **Tool messages** → provide results from tool execution
4. **Assistant message** → synthesizes final response

```json
{
  "messages": [
    {"role": "user", "content": "What's the weather in Paris?"},
    {
      "role": "assistant",
      "tool_calls": [{
        "id": "call_123",
        "type": "function",
        "function": {"name": "get_weather", "arguments": "{\"location\": \"Paris\"}"}
      }]
    },
    {
      "role": "tool",
      "tool_call_id": "call_123",
      "content": "Weather in Paris: 22°C, sunny"
    },
    {
      "role": "assistant",
      "content": "The weather in Paris is currently 22°C and sunny!"
    }
  ]
}
```

**Provider-Specific Tool Support:**

All providers support the standardized format, but have different capabilities:

- **OpenAI**: Full support including parallel calls and streaming tool calls
- **Anthropic**: Tool calling with robust function definitions and tool choice controls
- **Google**: Function calling with JSON schema validation and tool selection
- **AWS Bedrock**: Tool calling support varies by model family (Claude, Nova, etc.)

**Streaming Tool Calls:**

Tool calls can be streamed just like regular responses:

```bash
curl -X POST http://localhost:8000/llm/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "openai/gpt-4",
    "messages": [{"role": "user", "content": "Search for Python tutorials"}],
    "tools": [/* tool definitions */],
    "stream": true
  }'
```

The stream will include tool call chunks as they're generated, followed by the tool execution results.

##### Chat Completions

Send a chat completion request using the OpenAI-compatible format:

```bash
curl -X POST http://localhost:8000/llm/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "openai_primary/gpt-4-turbo",
    "messages": [
      {"role": "system", "content": "You are a helpful assistant."},
      {"role": "user", "content": "Hello, how are you?"}
    ],
    "temperature": 0.7,
    "max_tokens": 150
  }'
```

The model name format is `<provider_name>/<model_id>`. Nexus automatically routes the request to the appropriate provider and transforms the request/response as needed.

##### Streaming Responses

Nexus supports streaming responses for all LLM providers using Server-Sent Events (SSE):

```bash
curl -X POST http://localhost:8000/llm/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "anthropic/claude-3-5-sonnet-20241022",
    "messages": [
      {"role": "user", "content": "Write a short poem"}
    ],
    "stream": true,
    "max_tokens": 100
  }'
```

When `stream: true` is set, the response will be streamed as Server-Sent Events with the following format:

```
data: {"id":"msg_123","object":"chat.completion.chunk","created":1234567890,"model":"anthropic/claude-3-5-sonnet-20241022","choices":[{"index":0,"delta":{"role":"assistant","content":"Here"}}]}

data: {"id":"msg_123","object":"chat.completion.chunk","created":1234567890,"model":"anthropic/claude-3-5-sonnet-20241022","choices":[{"index":0,"delta":{"content":" is"}}]}

data: {"id":"msg_123","object":"chat.completion.chunk","created":1234567890,"model":"anthropic/claude-3-5-sonnet-20241022","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":25,"total_tokens":35}}

data: [DONE]
```

Streaming is supported for all providers (OpenAI, Anthropic, Google) and provides:
- Real-time token delivery as they're generated
- Consistent chunk format across all providers
- Usage statistics in the final chunk
- Standard SSE format compatible with OpenAI SDKs

#### Provider-Specific Considerations

##### OpenAI
- Supports all standard OpenAI models (GPT-3.5, GPT-4, etc.)
- Compatible with Azure OpenAI endpoints
- **Tool Calling**: Full support including parallel tool calls and streaming tool calls
- **Function Definitions**: JSON schema support for parameters
- **Tool Choice**: Supports all tool choice modes including specific function forcing
- Supports streaming responses with Server-Sent Events (SSE)

##### Anthropic
- System messages are automatically extracted and placed in the `system` field
- Messages must alternate between user and assistant roles
- Requires explicit `max_tokens` parameter (defaults to 4096 if not specified)
- **Tool Calling**: Robust tool calling with function definitions and tool choice controls
- **Tool Use**: Supports tool_use blocks with structured parameter validation
- **Streaming Tools**: Tool calls can be streamed incrementally
- Supports all Claude models (Opus, Sonnet, Haiku)
- Supports streaming responses with Server-Sent Events (SSE)

##### Google
- Assistant role is automatically mapped to "model" role
- System messages are placed in the `systemInstruction` field
- **Function Calling**: Native function calling with JSON schema validation
- **Tool Selection**: Supports automatic tool selection and forced function calls
- **Parameter Validation**: Strict parameter schema enforcement
- Returns appropriate safety ratings when available
- Supports streaming responses with Server-Sent Events (SSE)

##### AWS Bedrock
- Uses the Bedrock conversation API for seamless integration
- We have tested and verified Anthropic Claude, Amazon Nova, Meta Llama, Cohere Command, Mistral, and DeepSeek models
- Other models may work, but we have not tested them. Please report any issues or successes.
- **Tool Calling**: Support varies by model family:
  - **Anthropic Claude**: Full tool calling support through Bedrock
  - **Amazon Nova**: Native tool calling capabilities
  - **Meta Llama**: Function calling has issues, avoid using
  - **Cohere Command**: Tool use support
  - **Mistral**: Tools work well with Bedrock
  - **DeepSeek**: Tools do not work with Bedrock
- Uses AWS SDK for authentication and request signing
- Supports all Bedrock features including streaming and model-specific parameters
- Regional endpoint selection based on configuration or AWS defaults
- Model availability varies by AWS region

#### Using Nexus with LLM SDKs

Nexus provides an OpenAI-compatible API, making it easy to use with existing LLM SDKs and libraries. Simply point the SDK to your Nexus instance instead of the provider's API.

##### OpenAI SDK (Python)

```python
from openai import OpenAI

# Point to your Nexus instance
client = OpenAI(
    base_url="http://localhost:8000/llm",
    api_key="your-service-token"  # Use a JWT token if OAuth2 is enabled, or any string if not
)

# Use any configured provider/model
response = client.chat.completions.create(
    model="anthropic/claude-3-5-sonnet-20241022",
    messages=[
        {"role": "user", "content": "Hello!"}
    ]
)

# Streaming works seamlessly
stream = client.chat.completions.create(
    model="openai/gpt-4-turbo",
    messages=[
        {"role": "user", "content": "Write a poem"}
    ],
    stream=True
)

for chunk in stream:
    if chunk.choices[0].delta.content:
        print(chunk.choices[0].delta.content, end="")
```

##### OpenAI SDK (Node.js/TypeScript)

```typescript
import OpenAI from 'openai';

// Configure to use Nexus
const openai = new OpenAI({
  baseURL: 'http://localhost:8000/llm',
  apiKey: 'your-service-token', // Use a JWT token if OAuth2 is enabled, or any string if not
});

// Use any provider through Nexus
const response = await openai.chat.completions.create({
  model: 'google/gemini-1.5-pro',
  messages: [
    { role: 'user', content: 'Explain quantum computing' }
  ],
});

// Streaming with any provider
const stream = await openai.chat.completions.create({
  model: 'anthropic/claude-3-opus-20240229',
  messages: [
    { role: 'user', content: 'Write a story' }
  ],
  stream: true,
});

for await (const chunk of stream) {
  process.stdout.write(chunk.choices[0]?.delta?.content || '');
}
```

##### LangChain Integration

```python
from langchain_openai import ChatOpenAI

# Use Nexus as the LLM provider
llm = ChatOpenAI(
    base_url="http://localhost:8000/llm",
    api_key="your-service-token",  # Use a JWT token if OAuth2 is enabled
    model="openai/gpt-4-turbo"
)

# Works with any configured provider
claude = ChatOpenAI(
    base_url="http://localhost:8000/llm",
    api_key="your-service-token",  # Use a JWT token if OAuth2 is enabled
    model="anthropic/claude-3-5-sonnet-20241022"
)
```

##### cURL with jq for Command Line

```bash
# Regular completion (with OAuth2 authentication if enabled)
curl -s http://localhost:8000/llm/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer your-jwt-token" \
  -d '{
    "model": "openai/gpt-4",
    "messages": [{"role": "user", "content": "Hello"}]
  }' | jq -r '.choices[0].message.content'

# Streaming with SSE parsing
curl -s http://localhost:8000/llm/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer your-jwt-token" \
  -d '{
    "model": "anthropic/claude-3-5-sonnet-20241022",
    "messages": [{"role": "user", "content": "Write a haiku"}],
    "stream": true
  }' | grep "^data: " | sed 's/^data: //' | jq -r 'select(.choices != null) | .choices[0].delta.content // empty'
```

##### Authentication with OAuth2

When OAuth2 is enabled in your Nexus configuration, you must provide a valid JWT token:

```python
# With OAuth2 enabled
client = OpenAI(
    base_url="http://localhost:8000/llm",
    api_key="eyJhbGciOiJSUzI1NiIs..."  # Your JWT token
)
```

Without OAuth2, the `api_key` field is still required by most SDKs but can be any non-empty string:

```python
# Without OAuth2
client = OpenAI(
    base_url="http://localhost:8000/llm",
    api_key="dummy"  # Any non-empty string works
)
```

#### Error Handling

Nexus provides consistent error responses across all providers:

- **400 Bad Request**: Invalid request format or parameters
- **401 Unauthorized**: Missing or invalid API key
- **429 Too Many Requests**: Rate limit exceeded
- **500 Internal Server Error**: Provider API error or network issues

Example error response:
```json
{
  "error": {
    "message": "Invalid model format: expected 'provider/model', got 'invalid-format'",
    "type": "invalid_request_error",
    "code": 400
  }
}
```

### Telemetry

Nexus supports OpenTelemetry metrics export for monitoring system performance and usage. Metrics are collected via middleware and have zero overhead when disabled.

#### Metrics Configuration

```toml
[telemetry]
service_name = "nexus-production"  # Optional, defaults to "nexus"

# Resource attributes for all telemetry (optional)
[telemetry.resource_attributes]
environment = "production"
region = "us-east-1"

# OTLP exporter for metrics
[telemetry.exporters.otlp]
enabled = true                       # Default: false
endpoint = "http://localhost:4317"   # Default: http://localhost:4317
protocol = "grpc"                    # Default: grpc (options: grpc, http)
timeout = "60s"                      # Default: 60s

# Batch export configuration (all optional with defaults)
[telemetry.exporters.otlp.batch_export]
scheduled_delay = "5s"               # Default: 5s
max_queue_size = 2048                # Default: 2048
max_export_batch_size = 512          # Default: 512
max_concurrent_exports = 1           # Default: 1
```

#### Available Metrics

All histograms also function as counters (count field tracks number of observations).

**HTTP Server Metrics:**
- `http.server.request.duration` (histogram)
  - Attributes: `http.route`, `http.request.method`, `http.response.status_code`

**MCP Operation Metrics:**
- `mcp.tool.call.duration` (histogram)
  - Attributes: `tool_name`, `tool_type` (builtin/downstream), `status` (success/error), `client.id`, `client.group`
  - Additional for search: `keyword_count`, `result_count`
  - Additional for execute: `server_name` (for downstream tools)
  - Additional for errors: `error.type`:
    - `parse_error` - Invalid JSON (-32700)
    - `invalid_request` - Not a valid request (-32600)
    - `method_not_found` - Method/tool does not exist (-32601)
    - `invalid_params` - Invalid method parameters (-32602)
    - `internal_error` - Internal server error (-32603)
    - `rate_limit_exceeded` - Rate limit hit (-32000)
    - `server_error` - Other server errors (-32001 to -32099)
    - `unknown` - Any other error code

- `mcp.tools.list.duration` (histogram)
  - Attributes: `method` (list_tools), `status`, `client.id`, `client.group`

- `mcp.prompt.request.duration` (histogram)
  - Attributes: `method` (list_prompts/get_prompt), `status`, `client.id`, `client.group`
  - Additional for errors: `error.type` (same values as above)

- `mcp.resource.request.duration` (histogram)
  - Attributes: `method` (list_resources/read_resource), `status`, `client.id`, `client.group`
  - Additional for errors: `error.type` (same values as above)

**LLM Operation Metrics:**
- `gen_ai.client.operation.duration` (histogram)
  - Tracks the total duration of LLM chat completion operations
  - Attributes:
    - `gen_ai.system` (always "nexus.llm")
    - `gen_ai.operation.name` (always "chat.completions")
    - `gen_ai.request.model` (e.g., "openai/gpt-4")
    - `gen_ai.response.finish_reason` (stop/length/tool_calls/content_filter)
    - `client.id` (from x-client-id header)
    - `client.group` (from x-client-group header)
    - `error.type` (for failed requests):
      - `invalid_request` - Malformed request
      - `authentication_failed` - Invalid API key
      - `insufficient_quota` - Quota exceeded
      - `model_not_found` - Unknown model
      - `rate_limit_exceeded` - Provider or token rate limit hit
      - `streaming_not_supported` - Streaming unavailable for model
      - `invalid_model_format` - Incorrect model name format
      - `provider_not_found` - Unknown provider
      - `internal_error` - Server error
      - `provider_api_error` - Upstream provider error
      - `connection_error` - Network failure

- `gen_ai.client.time_to_first_token` (histogram)
  - Tracks time until first token in streaming responses
  - Attributes: `gen_ai.system`, `gen_ai.operation.name`, `gen_ai.request.model`, `client.id`, `client.group`

- `gen_ai.client.input.token.usage` (counter)
  - Cumulative input token consumption
  - Attributes: `gen_ai.system`, `gen_ai.request.model`, `client.id`, `client.group`

- `gen_ai.client.output.token.usage` (counter)
  - Cumulative output token consumption
  - Attributes: `gen_ai.system`, `gen_ai.request.model`, `client.id`, `client.group`

- `gen_ai.client.total.token.usage` (counter)
  - Cumulative total token consumption (input + output)
  - Attributes: `gen_ai.system`, `gen_ai.request.model`, `client.id`, `client.group`

**Redis Storage Metrics (when rate limiting with Redis is enabled):**
- `redis.command.duration` (histogram)
  - Tracks Redis command execution times
  - Attributes:
    - `operation` - The Redis operation type:
      - `check_and_consume` - HTTP rate limit checking
      - `check_and_consume_tokens` - Token-based rate limit checking
    - `status` (success/error)
    - `tokens` (for token operations) - Number of tokens checked

- `redis.pool.connections.in_use` (gauge)
  - Current number of connections in use from the pool
  - No additional attributes

- `redis.pool.connections.available` (gauge)
  - Current number of available connections in the pool
  - No additional attributes

### Distributed Tracing

Nexus supports distributed tracing using OpenTelemetry, providing detailed insights into request flows across all components. Traces help you understand latency, identify bottlenecks, and debug issues in production.

#### Tracing Configuration

```toml
[telemetry.tracing]
enabled = true                    # Enable/disable tracing (default: true)
sampling = 0.15                   # Sample 15% of requests (0.0 to 1.0)

# Collection limits (per span)
[telemetry.tracing.collect]
max_events_per_span = 128
max_attributes_per_span = 128
max_links_per_span = 128
max_attributes_per_event = 128
max_attributes_per_link = 128

# Trace context propagation formats
[telemetry.tracing.propagation]
trace_context = true              # W3C Trace Context
aws_xray = false                  # AWS X-Ray format

# Optional: Override global OTLP exporter for traces
[telemetry.tracing.exporters.otlp]
enabled = true
endpoint = "http://localhost:4317"
protocol = "grpc"                 # or "http"
timeout = "60s"
```

#### Trace Context Propagation

Nexus supports multiple trace context propagation formats for incoming requests:

- **W3C Trace Context**: Standard `traceparent` and `tracestate` headers (enabled by default)
- **AWS X-Ray**: `X-Amzn-Trace-Id` header for AWS services integration

When enabled, Nexus will:
1. Extract trace context from incoming HTTP requests
2. Continue the trace with the parent context or start a new trace
3. Create spans for all internal operations

**Note**: Nexus is a terminal node for traces - it does not propagate trace context to downstream MCP servers or LLM providers.

#### Span Hierarchy

Nexus creates a hierarchical span structure for each request:

```
HTTP Request (root span)
├── Redis Rate Limit Check (redis:check_and_consume:global/ip) - HTTP-level rate limits
├── MCP Operation
│   ├── Redis Rate Limit Check (redis:check_and_consume:server/tool) - MCP-specific rate limits
│   ├── Tool Search (index:search)
│   └── Tool Execution
│       └── Downstream Operations (downstream:execute, downstream:call_tool)
└── LLM Operation (llm:chat_completion or llm:chat_completion_stream)
    └── Token Rate Limit Check (redis:check_and_consume_tokens) - Token-based rate limits
```

#### Span Attributes

Each span includes semantic attributes following OpenTelemetry conventions:

**HTTP Spans:**
- `http.request.method` - HTTP method (GET, POST, etc.)
- `http.route` - Matched route pattern
- `http.response.status_code` - Response status code
- `url.scheme` - URL scheme (http/https)
- `server.address` - Host header value
- `url.full` - Full request URL
- `client.id` - Client identifier (if configured)
- `client.group` - Client group (if configured)
- `error` - Set to "true" if request failed

**MCP Spans:**
- `mcp.method` - MCP method name (tools/list, tools/call, etc.)
- `mcp.tool.name` - Tool being called
- `mcp.tool.type` - Tool type (builtin/downstream)
- `mcp.transport` - Transport type (stdio/http)
- `mcp.auth_forwarded` - Whether auth was forwarded
- `client.id` - Client identifier (if configured)
- `client.group` - Client group (if configured)
- `mcp.search.keywords` - Search keywords (for search tool)
- `mcp.search.keyword_count` - Number of search keywords
- `mcp.execute.target_tool` - Tool being executed
- `mcp.execute.target_server` - Server hosting the tool
- `mcp.error.code` - Error code if operation failed

**LLM Spans:**
- `gen_ai.request.model` - Model identifier
- `gen_ai.request.max_tokens` - Max tokens requested
- `gen_ai.request.temperature` - Temperature parameter
- `gen_ai.request.has_tools` - Whether tools were provided
- `gen_ai.request.tool_count` - Number of tools provided
- `gen_ai.response.model` - Model used for response
- `gen_ai.response.finish_reason` - Completion reason
- `gen_ai.usage.input_tokens` - Input token count
- `gen_ai.usage.output_tokens` - Output token count
- `gen_ai.usage.total_tokens` - Total token count
- `llm.stream` - Whether streaming was used
- `llm.auth_forwarded` - Whether auth was forwarded
- `client.id` - Client identifier (if configured)
- `client.group` - Client group (if configured)
- `error.type` - Error type if operation failed

**Redis Spans:**
- `redis.operation` - Operation type (check_and_consume or check_and_consume_tokens)
- `rate_limit.scope` - Scope (global/ip/server/tool/token)
- `rate_limit.limit` - Request/token limit
- `rate_limit.interval_ms` - Time window in milliseconds
- `rate_limit.tokens` - Number of tokens (for token operations)
- `rate_limit.allowed` - Whether request was allowed
- `rate_limit.retry_after_ms` - Retry delay if rate limited
- `redis.pool.size` - Connection pool size
- `redis.pool.available` - Available connections
- `redis.pool.in_use` - Connections in use
- `client.address_hash` - Hashed IP for privacy (per-IP limits)
- `llm.provider` - Provider name (token limits)
- `llm.model` - Model name (token limits)
- `mcp.server` - Server name (per-server limits)
- `mcp.tool` - Tool name (per-tool limits)

#### Sampling

Nexus uses fixed-rate sampling to randomly sample a percentage of traces:

```toml
[telemetry.tracing]
sampling = 0.15  # Sample 15% of requests (0.0 to 1.0)
```

When a parent trace context is provided in request headers (via W3C Trace Context or AWS X-Ray), Nexus will continue that trace.

#### Performance Considerations

- **Zero Overhead When Disabled**: When `telemetry.tracing.enabled = false`, no spans are created
- **Efficient Sampling**: Unsampled traces have minimal overhead
- **Batched Export**: Traces are batched before export to reduce network overhead
- **Configurable Limits**: Adjust collection limits to balance detail vs. resource usage

#### Integration with APM Tools

Nexus traces can be exported to any OpenTelemetry-compatible APM tool:

- **Jaeger**: Full trace visualization and analysis
- **Zipkin**: Distributed tracing system
- **AWS X-Ray**: Native AWS integration with X-Ray propagation
- **Datadog**: APM and distributed tracing
- **New Relic**: Application performance monitoring
- **Grafana Tempo**: Cloud-native tracing backend

Example Jaeger configuration:
```toml
[telemetry.tracing.exporters.otlp]
enabled = true
endpoint = "http://jaeger:4317"
protocol = "grpc"
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

### MCP Tool Aggregation

Nexus provides two main tools to AI assistants:

1. **`search`**: A context-aware tool search that uses fuzzy matching to find relevant tools across all connected MCP servers
2. **`execute`**: Executes a specific tool with the provided parameters

When an AI assistant connects to Nexus, it can:
1. Search for tools using natural language queries
2. Discover tool names, descriptions, and required parameters
3. Execute tools from any connected MCP server

All tools from downstream servers are namespaced with their server name (e.g., `github__search_code`, `filesystem__read_file`).

### LLM Provider Routing

Nexus acts as a unified gateway for multiple LLM providers:

1. **Model Discovery**: Lists all available models from configured providers with consistent naming
2. **Request Routing**: Automatically routes requests to the correct provider based on model name
3. **Format Translation**: Converts between OpenAI's API format and provider-specific formats
4. **Response Normalization**: Returns consistent response format regardless of provider

Models are namespaced with their provider name (e.g., `openai/gpt-4`, `anthropic/claude-3-opus-20240229`).

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
