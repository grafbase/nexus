# Server Crate Guidelines

This crate implements the HTTP server layer for Nexus, handling routing, middleware, authentication, and request/response processing.

## Purpose

The server crate provides:
- HTTP server setup with Axum web framework
- OAuth2/JWT authentication middleware
- CORS policy enforcement
- CSRF protection
- Health check endpoints
- TLS/HTTPS support
- Well-known endpoint implementations
- Rate limiting middleware for global and per-IP limits

## Architecture Overview

### Core Components

```
server/
├── lib.rs           # Main server setup and routing
├── auth/            # OAuth2/JWT authentication
│   ├── claims.rs    # JWT claim validation
│   ├── error.rs     # Authentication error types
│   ├── jwks.rs      # JWKS fetching and caching
│   └── jwt.rs       # JWT validation logic
├── cors.rs          # CORS policy implementation
├── csrf.rs          # CSRF protection middleware
├── health.rs        # Health check endpoint
├── rate_limit.rs    # Rate limiting middleware
└── well_known.rs    # OAuth metadata endpoint
```

## Implementation Guidelines

### Server Setup

The main `serve` function orchestrates the server setup:

```rust
pub async fn serve(ServeConfig { listen_address, config }: ServeConfig) -> anyhow::Result<()> {
    // 1. Create base router
    let mut app = Router::new();

    // 2. Setup CORS first (applies to all routes)
    let cors = cors::generate(&config.server.cors);

    // 3. Initialize rate limit manager if enabled
    let rate_limit_manager = if config.server.rate_limits.enabled {
        Some(Arc::new(RateLimitManager::new(config.server.rate_limits.clone(), config.mcp.clone()).await?))
    } else {
        None
    };

    // 4. Create protected router for authenticated routes
    let mut protected_router = Router::new();

    // 5. Add MCP routes to protected router (with rate limiting)
    if config.mcp.enabled {
        let mcp_router = mcp::router(&config).await?.layer(cors.clone());
        protected_router = protected_router.merge(mcp_router);
    }

    // 6. Apply authentication to protected routes
    if let Some(oauth) = &config.server.oauth {
        protected_router = protected_router.layer(AuthLayer::new(oauth.clone()));
    }

    // 7. Apply rate limiting middleware (after auth, before routes)
    if let Some(manager) = rate_limit_manager {
        protected_router = protected_router.layer(RateLimitLayer::new(manager));
    }

    // 8. Merge protected routes into main app
    app = app.merge(protected_router);

    // 9. Add public endpoints (health, OAuth metadata)
    // 10. Apply CSRF protection if enabled
    // 11. Start server with or without TLS
}
```

### Middleware Order

**CRITICAL**: Middleware order matters in Axum (applied in reverse):

```rust
// Code order (bottom to top in the stack):
app
    .route("/api", handler)
    .layer(CorsLayer)      // Applied second
    .layer(AuthLayer)      // Applied first

// Request flow: AuthLayer -> CorsLayer -> handler
// Response flow: handler -> CorsLayer -> AuthLayer
```

### Authentication Layer

Implement OAuth2 Bearer token validation:

```rust
#[derive(Clone)]
pub struct AuthLayer(Arc<AuthLayerInner>);

impl AuthLayer {
    pub fn new(config: OauthConfig) -> Self {
        let jwt = JwtAuth::new(config);
        Self(Arc::new(AuthLayerInner { jwt }))
    }
}

// Key responsibilities:
// 1. Extract Bearer token from Authorization header
// 2. Validate JWT signature using JWKS
// 3. Check claims (issuer, audience, expiration)
// 4. Forward auth token to downstream services if needed
// 5. Return proper OAuth2 error responses
```

### CORS Configuration

Support flexible CORS policies:

```rust
pub fn generate(config: &CorsConfig) -> CorsLayer {
    let mut cors = CorsLayer::new()
        .allow_credentials(config.allow_credentials)
        .allow_private_network(config.allow_private_network);

    // Support exact origins and glob patterns
    match &config.allow_origins {
        AnyOrUrlArray::Any => cors.allow_origin(AllowOrigin::any()),
        AnyOrUrlArray::Explicit(origins) => {
            // Separate constants and globs
            // Use AllowOrigin::predicate for glob matching
        }
    }

    // Always include OPTIONS method for preflight
    if !methods.contains(&Method::OPTIONS) {
        methods.push(Method::OPTIONS);
    }
}
```

### Health Endpoint

Provide configurable health checks:

```rust
// Health endpoint can be:
// 1. On the main server (default)
// 2. On a separate port (for container orchestration)

if let Some(listen) = config.server.health.listen {
    // Spawn separate health server
    tokio::spawn(health::bind_health_endpoint(
        listen,
        config.server.tls.clone(),
        config.server.health,
    ));
} else {
    // Add to main router
    app.route(&config.server.health.path, get(health::health));
}
```

### TLS Support

Handle HTTPS with rustls:

```rust
match &config.server.tls {
    Some(tls_config) => {
        let rustls_config = RustlsConfig::from_pem_file(
            &tls_config.certificate,
            &tls_config.key
        ).await?;

        axum_server::bind_rustls(listen_address, rustls_config)
            .serve(app.into_make_service())
            .await?;
    }
    None => {
        // Plain HTTP
        axum_server::bind(listen_address)
            .serve(app.into_make_service())
            .await?;
    }
}
```

### Well-Known Endpoints

Implement OAuth2 Protected Resource metadata:

```rust
// GET /.well-known/oauth-protected-resource
#[derive(Serialize)]
struct OAuthProtectedResourceMetadata {
    resource: String,                    // The protected resource URL
    authorization_servers: Vec<String>,  // List of OAuth2 servers
}

// This endpoint must be public (no auth required)
// Allows clients to discover OAuth2 configuration
```

## Security Best Practices

### JWT Validation

1. **Always verify signature** using JWKS from the issuer
2. **Check standard claims**:
   - `exp`: Token not expired
   - `iss`: Matches expected issuer
   - `aud`: Matches expected audience (if configured)
3. **Cache JWKS** to avoid repeated fetches
4. **Handle clock skew** with reasonable tolerance (30 seconds)

### CSRF Protection

```rust
// Apply CSRF protection when enabled
if config.server.csrf.enabled {
    app = csrf::inject_layer(app, &config.server.csrf);
}

// CSRF considerations:
// - Check Origin/Referer headers
// - Use SameSite cookies
// - Validate state parameters
```

### Request Size Limits

Implement reasonable limits to prevent DoS:

```rust
// TODO: Add request size limits
// - Body size limits
// - Header size limits
// - Connection limits
```

### Rate Limiting

The server applies rate limiting at the HTTP middleware level:

```rust
// Rate limiting is applied after authentication but before route handlers
// This ensures we know the user identity for better rate limiting decisions

if config.server.rate_limits.enabled {
    // Global and per-IP limits are enforced at the HTTP layer
    protected_router = protected_router.layer(RateLimitLayer::new(manager));
}

// MCP-specific rate limits (per-server, per-tool) are handled in the MCP layer
```

Rate limiting responses follow RFC 6585:
- `429 Too Many Requests` status code
- `Retry-After` header with seconds until retry
- JSON error body with details

## Logging

Use structured logging with appropriate levels:

```rust
// Info: Server lifecycle events
log::info!("MCP endpoint available at: https://{}{}", address, path);

// Debug: Request processing details
log::debug!("Validating JWT for issuer: {}", issuer);

// Warn: Recoverable errors
log::warn!("JWKS fetch failed, using cached version");

// Error: Serious issues
log::error!("Failed to load TLS certificate: {}", error);
```

## Testing

### Test Naming
Don't prefix test functions with `test_`.

```rust
// Good: Clean and short test name
#[tokio::test]
async fn oauth_authentication_required() { ... }

// Bad: The name of the test is too verbose
#[tokio::test]
async fn test_oauth_authentication_required() { ... }
```

### Example Test
Use the integration-tests crate for testing server functionality:

```rust
#[tokio::test]
async fn oauth_authentication_required() {
    let config = indoc! {r#"
        [server.oauth]
        url = "http://localhost:4444/.well-known/jwks.json"
        expected_issuer = "http://localhost:4444"

        [mcp]
        enabled = true
    "#};

    let server = TestServer::builder().build(config).await;

    // Request without token should fail
    let response = server.client.get("/mcp").await;
    assert_eq!(response.status(), 401);

    // Check WWW-Authenticate header
    let www_auth = response.headers().get("www-authenticate").unwrap();
    assert!(www_auth.to_str().unwrap().starts_with("Bearer"));
}
```

### Snapshot Testing
Prefer insta snapshots over manual assertions:

```rust
// Good: Inline snapshot for response validation
insta::assert_json_snapshot!(response.json::<Value>().await?, @r###"
{
  "error": "unauthorized",
  "error_description": "Missing authorization header"
}
"###);

// Avoid: Manual assertions for complex data
assert_eq!(response["error"], "unauthorized");
assert_eq!(response["error_description"], "Missing authorization header");
```

## Performance Considerations

1. **Connection Pooling**: Reuse connections for JWKS fetches
2. **Caching**: Cache JWKS and validated tokens
3. **Async I/O**: Use Tokio for all I/O operations
4. **Zero-Copy**: Avoid unnecessary data copies in middleware

## Common Patterns

### Adding New Endpoints

```rust
// 1. Define handler function
async fn my_endpoint() -> impl IntoResponse {
    Json(json!({ "status": "ok" }))
}

// 2. Add route to appropriate router
app = app.route("/my-endpoint", get(my_endpoint));

// 3. Consider: Does it need auth? CORS? CSRF protection?
```

### Adding New Middleware

```rust
// Implement Tower Layer trait
impl<S> Layer<S> for MyLayer {
    type Service = MyService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        MyService { inner, config: self.0.clone() }
    }
}

// Remember: Middleware runs for EVERY request
// Keep it fast and avoid blocking operations
```

Remember: The server crate is the front door to Nexus - it must be secure, performant, and provide clear error messages while protecting against common web vulnerabilities.

## Keeping This Document Updated

**IMPORTANT**: Update this CLAUDE.md when server implementation changes:

1. **Middleware Changes**: Document new middleware or changes to existing ones
2. **Routing Updates**: Update when endpoint structure changes
3. **Auth Changes**: Document modifications to OAuth2/JWT handling
4. **Security Updates**: Add new security patterns or vulnerability mitigations
5. **Protocol Changes**: Document new protocols or transport methods

Update triggers:
- Adding new middleware layers
- Changing authentication/authorization logic
- Modifying CORS or CSRF policies
- Updating health check implementations
- Adding new well-known endpoints
- Changing TLS/HTTPS configuration
