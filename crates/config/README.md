# Config Crate

This crate provides the configuration layer for Nexus, handling all aspects of loading, parsing, and validating configuration from TOML files.

## Overview

The config crate is responsible for:
- Parsing `nexus.toml` configuration files
- Environment variable substitution
- Type-safe configuration structures
- Validation of configuration values
- Default values for optional settings

## Environment Variable Substitution

The config loader supports environment variable expansion using the format: `{{ env.VAR_NAME }}`

Example:

```toml
[mcp.servers.github]
url = "https://api.github.com/mcp"

[mcp.servers.github.auth]
token = "{{ env.GITHUB_TOKEN }}"
```

## Usage

### Loading Configuration

```rust
use config::Config;

let config = Config::load("path/to/nexus.toml")?;
```

### Configuration File Format

```toml
[server]
listen_address = "0.0.0.0:3000"

[server.health]
enabled = true
path = "/health"
listen = "0.0.0.0:3001"  # Optional separate health port

[server.tls]
cert_path = "/path/to/cert.pem"
key_path = "/path/to/key.pem"

[server.cors]
allow_origins = ["https://example.com"]
allow_methods = ["GET", "POST"]
allow_headers = ["Content-Type", "Authorization"]
allow_credentials = true
max_age = 3600

[server.csrf]
enabled = true
skip_paths = ["/health", "/metrics"]

[server.oauth]
url = "https://your-oauth-provider.com/.well-known/jwks.json"
poll_interval = "5m"
expected_issuer = "https://your-oauth-provider.com"
expected_audience = "your-service-audience"

[server.oauth.protected_resource]
resource = "https://your-nexus-instance.com"
authorization_servers = ["https://your-oauth-provider.com"]

[mcp]
enabled = true
path = "/mcp"

# Stdio server
[mcp.servers.filesystem]
cmd = ["npx", "-y", "@modelcontextprotocol/server-filesystem", "/path/to/files"]

# SSE server
[mcp.servers.my_sse_server]
protocol = "sse"
url = "http://localhost:8080/sse"
message_url = "http://localhost:8080/messages"

# HTTP server with auth
[mcp.servers.api_server]
protocol = "streamable-http"
url = "https://api.example.com/mcp"
[mcp.servers.api_server.auth]
token = "{{ env.API_TOKEN }}"

# TLS configuration for downstream servers
[mcp.servers.secure_server.tls]
verify_certs = true
accept_invalid_hostnames = false
root_ca_cert_path = "/path/to/ca.pem"
```

## Validation

The config crate performs several validations:
- Required fields must be present
- URLs must be valid
- Socket addresses must be parseable
- File paths (for TLS certs) are validated if provided
- Unknown fields are rejected (`deny_unknown_fields`)

## Error Handling

Configuration errors are returned as `anyhow::Error` with descriptive messages:
- Missing required fields
- Invalid values (e.g., malformed URLs)
- Environment variables not found
- File I/O errors

## Testing

The crate includes comprehensive tests for:
- Default values
- Environment variable substitution
- All server types (stdio, SSE, HTTP)
- TLS configuration
- CORS and CSRF settings
- Error cases

Run tests with:
```bash
cargo test -p config
```

## OAuth2 Configuration

The config crate provides comprehensive OAuth2 authentication configuration:

### Basic OAuth2 Setup

```toml
[server.oauth]
url = "https://provider.com/.well-known/jwks.json"
poll_interval = "5m"
expected_issuer = "https://provider.com"
expected_audience = "your-service"

[server.oauth.protected_resource]
resource = "https://your-service.com"
authorization_servers = ["https://provider.com"]
```

### Configuration Fields

#### `[server.oauth]`

- **`url`** (required): JWKs endpoint URL for JWT token validation
- **`poll_interval`** (optional): How often to refresh JWKs (e.g., "5m", "1h", "30s")
- **`expected_issuer`** (optional): Expected `iss` claim in JWT tokens
- **`expected_audience`** (optional): Expected `aud` claim in JWT tokens

#### `[server.oauth.protected_resource]`

- **`resource`** (required): URL identifying this protected resource
- **`authorization_servers`** (required): Array of authorization server URLs

### Environment Variable Support

OAuth2 configuration supports environment variable substitution:

```toml
[server.oauth]
url = "{{ env.OAUTH_JWKS_URL }}"
expected_issuer = "{{ env.OAUTH_ISSUER }}"
expected_audience = "{{ env.OAUTH_AUDIENCE }}"

[server.oauth.protected_resource]
resource = "{{ env.SERVICE_URL }}"
authorization_servers = ["{{ env.OAUTH_ISSUER }}"]
```

### Validation Rules

The config loader validates OAuth2 settings:

- **URL Format**: All URLs must be valid and well-formed
- **Poll Interval**: Must be a valid duration string when provided
- **Required Fields**: `url`, `resource`, and `authorization_servers` are mandatory
- **Array Validation**: `authorization_servers` must be a non-empty array

### Examples

#### Minimal Configuration
```toml
[server.oauth]
url = "https://auth.example.com/.well-known/jwks.json"

[server.oauth.protected_resource]
resource = "https://api.example.com"
authorization_servers = ["https://auth.example.com"]
```

#### Production Configuration
```toml
[server.oauth]
url = "https://oauth-provider.com/.well-known/jwks.json"
poll_interval = "10m"
expected_issuer = "https://oauth-provider.com"
expected_audience = "nexus-api"

[server.oauth.protected_resource]
resource = "https://nexus.production.com"
authorization_servers = [
    "https://oauth-provider.com",
    "https://backup-oauth.com"
]
```

#### Development Configuration
```toml
[server.oauth]
url = "http://localhost:4444/.well-known/jwks.json"
poll_interval = "1m"
expected_issuer = "http://localhost:4444"

[server.oauth.protected_resource]
resource = "http://localhost:8080"
authorization_servers = ["http://localhost:4444"]
```

## Design Decisions

1. **BTreeMap for servers**: Provides consistent ordering for server names
2. **Box<HttpConfig>**: Reduces enum size for better performance
3. **SecretString for tokens**: Prevents accidental token exposure in logs
4. **Serde's deny_unknown_fields**: Catches typos in configuration files
5. **Environment variable substitution**: Allows secure credential management
6. **Optional OAuth2 config**: OAuth2 is optional - when not configured, no authentication is required
7. **URL validation**: All OAuth2 URLs are validated at config load time
8. **Duration parsing**: Poll intervals use human-readable duration strings
