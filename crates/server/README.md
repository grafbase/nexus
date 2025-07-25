# Server Crate

This crate provides the HTTP server infrastructure for Nexus, including the web server setup, middleware, and request handling.

## Overview

The server crate is responsible for:
- Setting up and running the HTTP/HTTPS server
- Integrating MCP functionality into the web server
- Middleware configuration (CORS, CSRF protection)
- Health check endpoints
- TLS/SSL support
- Graceful shutdown handling
- OAuth2 JWT authentication and authorization
- OAuth2 protected resource metadata endpoint

## Architecture

### Core Components

```
┌─────────────────┐
│   Axum Router   │
├─────────────────┤
│   Middleware    │
│  - CORS Layer   │
│  - CSRF Layer   │
│  - JWT Auth     │
├─────────────────┤
│   Endpoints     │
│  - /mcp         │
│  - /health      │
└─────────────────┘
```

### Main Entry Point

The `serve` function is the primary entry point:

```rust
pub async fn serve(config: ServeConfig) -> anyhow::Result<()> {
    // 1. Create router
    // 2. Apply middleware
    // 3. Setup MCP routes
    // 4. Configure health endpoints
    // 5. Start server (HTTP or HTTPS)
}
```

## Key Features

### CORS Support

The server provides flexible CORS configuration:

```rust
// Permissive CORS (default)
let cors = CorsLayer::permissive();

// Or custom CORS from config
let cors = cors::generate(&cors_config);
```

CORS can be configured to:
- Allow specific origins
- Control allowed methods and headers
- Handle credentials
- Set max age for preflight caching

### CSRF Protection

CSRF protection is implemented for state-changing operations:
- Validates origin/referer headers
- Configurable skip paths (e.g., `/health`)
- Protects against cross-site request forgery

### Health Endpoints

Health checks support multiple configurations:
1. Same port as main server (default)
2. Separate port for health checks
3. Custom path configuration

```rust
// Health on main server
GET /health

// Or on separate port
GET :3001/health
```

### TLS/HTTPS Support

Full TLS support with:
- Certificate and key file loading
- Rustls for modern TLS implementation
- Support for both HTTP and HTTPS
- Configurable per endpoint

### OAuth2 Support

The server provides comprehensive OAuth2 authentication using JWT tokens:

#### JWT Token Validation

- **JWKs Integration**: Fetches and caches JSON Web Key Sets from OAuth2 providers
- **Token Verification**: Validates JWT signatures, expiration, and claims
- **Claim Validation**: Supports issuer (`iss`) and audience (`aud`) claim validation

#### Protected Resource Metadata

Exposes OAuth2 protected resource metadata at `/.well-known/oauth-protected-resource`:

```json
{
  "resource": "https://your-nexus-instance.com",
  "authorization_servers": ["https://your-oauth-provider.com"]
}
```

#### Authentication Flow

1. Client includes JWT token in `Authorization: Bearer <token>` header
2. Server validates token signature using cached JWKs
3. Server validates token expiration and claims (issuer, audience)
4. Request proceeds if validation succeeds, returns 401 if invalid

#### Bypass Rules

Certain endpoints bypass OAuth2 authentication:
- `/health` - For monitoring and health checks
- `/.well-known/oauth-protected-resource` - OAuth2 metadata (must be public per spec)

#### Configuration

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

### HTTPS Server

```rust
// Configure TLS in nexus.toml
[server.tls]
cert_path = "/path/to/cert.pem"
key_path = "/path/to/key.pem"
```

### Custom Health Check

```rust
// Separate health port in nexus.toml
[server.health]
enabled = true
path = "/health"
listen = "0.0.0.0:3001"
```

## Middleware Stack

The server applies middleware in a specific order:

1. **CORS** - Applied first to handle preflight requests (when enabled)
2. **CSRF** - Validates requests before routing  (when enabled)
3. **OAuth2 JWT** - Authenticates and authorizes requests (when enabled)
4. **Request routing** - Routes to appropriate handlers
5. **MCP handler** - Processes MCP protocol requests

## Integration with MCP

The server crate integrates with the MCP crate to:
- Mount MCP routes at the configured path
- Apply security middleware
- Handle protocol negotiation
- Manage connection lifecycle

```rust
if config.mcp.enabled {
    let mcp_router = mcp::router(&config.mcp).await?
        .layer(cors.clone());
    app = app.merge(mcp_router);
}
```

## Error Handling

The server provides comprehensive error handling:
- Startup errors (port binding, TLS setup)
- Runtime errors (connection handling)
- Graceful shutdown on signals
- Detailed error messages for debugging

## Performance Considerations

1. **Axum framework**: High-performance async web framework
2. **Tower middleware**: Efficient middleware composition
3. **Rustls**: Modern TLS with good performance
4. **Connection pooling**: For downstream MCP servers

## Testing

The server is primarily tested through integration tests:
- Full server startup/shutdown
- CORS behavior verification
- CSRF protection testing
- Health endpoint validation
- TLS connection testing

## Configuration Examples

### Minimal Server

```toml
[server]
listen_address = "127.0.0.1:3000"
```

### Production Server

```toml
[server]
listen_address = "0.0.0.0:443"

[server.tls]
cert_path = "/etc/nexus/cert.pem"
key_path = "/etc/nexus/key.pem"

[server.cors]
allow_origins = ["https://app.example.com"]
allow_methods = ["GET", "POST"]
allow_credentials = true

[server.csrf]
enabled = true
skip_paths = ["/health", "/mcp"]

[server.health]
enabled = true
path = "/health"
```

## Security Features

1. **CORS**: Prevents unauthorized cross-origin requests
2. **CSRF**: Protects against forged requests
3. **OAuth2 JWT**: Validates bearer tokens and claims
4. **TLS**: Encrypts all traffic when enabled
5. **Header validation**: Validates required headers
6. **Origin checking**: Verifies request origins

## Monitoring

The health endpoint provides:
- Basic liveness check
- Can be extended for readiness checks
- Suitable for Kubernetes/Docker health probes
- Separate port option for monitoring systems
- Bypasses OAuth2 authentication for monitoring access

## OAuth2 Implementation Details

### JWT Validation Process

The OAuth2 implementation follows these steps:

1. **Token Extraction**: Extracts Bearer token from Authorization header
2. **JWKs Retrieval**: Fetches or uses cached JSON Web Key Set
3. **Signature Validation**: Verifies JWT signature using appropriate key
4. **Claims Validation**: Validates standard and custom claims:
   - `exp` (expiration time) - must be in the future
   - `iss` (issuer) - must match expected issuer if configured
   - `aud` (audience) - must match expected audience if configured

### JWKs Caching

- **Automatic Refresh**: Polls JWKs endpoint at configured intervals
- **Error Handling**: Continues with cached keys if refresh fails
- **Key Rotation**: Supports multiple keys for seamless rotation
- **Algorithm Support**: Supports RS256, RS384, RS512, ES256, ES384, ES512

### Error Responses

The server returns appropriate OAuth2 error responses:

- **401 Unauthorized**: Missing or invalid token
- **403 Forbidden**: Valid token but insufficient permissions
- **WWW-Authenticate**: Includes realm and error description
