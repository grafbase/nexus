mod audience_validation;
mod authorization;
mod edge_cases;
mod error_handling;
mod issuer_validation;
mod jwks_caching;
mod mcp;
mod metadata;
mod token_validation;

use std::{net::SocketAddr, sync::Arc, time::Duration};

use indoc::indoc;
use integration_tests::TestServer;

/// Extension trait to add authorization helper methods to reqwest::RequestBuilder
pub trait RequestBuilderExt {
    /// Add Bearer token authorization header
    fn authorization(self, token: &str) -> Self;

    /// Add MCP-style JSON body and headers
    fn mcp_json(self, body: &str) -> Self;
}

impl RequestBuilderExt for reqwest::RequestBuilder {
    fn authorization(self, token: &str) -> Self {
        self.header("Authorization", format!("Bearer {token}"))
    }

    fn mcp_json(self, body: &str) -> Self {
        self.header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .body(body.to_string())
    }
}

/// Helper struct for creating test JWT tokens
#[derive(Debug, serde::Serialize)]
pub struct TestJwtClaims {
    pub iss: String,
    pub aud: String,
    pub sub: String,
    pub exp: u64,
    pub iat: u64,
}

/// Helper to create a basic test JWT (unsigned - for negative tests)
pub fn create_test_jwt_unsigned() -> String {
    use base64::{Engine as _, engine::general_purpose};

    let header = r#"{"alg":"none","typ":"JWT"}"#;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let claims = TestJwtClaims {
        iss: "http://127.0.0.1:4444".to_string(),
        aud: "test-audience".to_string(),
        sub: "test-user".to_string(),
        exp: now + 3600, // Valid for 1 hour
        iat: now,
    };

    let claims_json = serde_json::to_string(&claims).unwrap();

    let header_b64 = general_purpose::URL_SAFE_NO_PAD.encode(header);
    let claims_b64 = general_purpose::URL_SAFE_NO_PAD.encode(claims_json);

    format!("{header_b64}.{claims_b64}.")
}

/// Helper to create a basic test JWT with custom audience (unsigned - for negative tests)
pub fn create_test_jwt_unsigned_with_audience(audience: &str) -> String {
    use base64::{Engine as _, engine::general_purpose};
    use std::time::{SystemTime, UNIX_EPOCH};

    let header = r#"{"alg":"RS256","typ":"JWT"}"#;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();

    let claims = TestJwtClaims {
        iss: "http://127.0.0.1:4444".to_string(),
        aud: audience.to_string(),
        sub: "test-user".to_string(),
        exp: now + 3600, // Valid for 1 hour
        iat: now,
    };

    let claims_json = serde_json::to_string(&claims).unwrap();

    let header_b64 = general_purpose::URL_SAFE_NO_PAD.encode(header);
    let claims_b64 = general_purpose::URL_SAFE_NO_PAD.encode(claims_json);

    format!("{header_b64}.{claims_b64}.")
}

/// Helper to create an expired JWT
pub fn create_expired_jwt() -> String {
    use base64::{Engine as _, engine::general_purpose};

    let header = r#"{"alg":"none","typ":"JWT"}"#;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let claims = TestJwtClaims {
        iss: "http://127.0.0.1:4444".to_string(),
        aud: "test-audience".to_string(),
        sub: "test-user".to_string(),
        exp: now - 3600, // Expired 1 hour ago
        iat: now - 7200, // Issued 2 hours ago
    };

    let header_b64 = general_purpose::URL_SAFE_NO_PAD.encode(header);
    let claims_b64 = general_purpose::URL_SAFE_NO_PAD.encode(serde_json::to_string(&claims).unwrap());

    format!("{header_b64}.{claims_b64}.")
}

pub struct HydraClient {
    pub public_url: String,
    pub client: reqwest::Client,
}

#[derive(Debug, serde::Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
}

impl HydraClient {
    pub fn new(public_port: u16, _admin_port: u16) -> Self {
        Self {
            public_url: format!("http://127.0.0.1:{public_port}"),
            client: reqwest::Client::new(),
        }
    }

    pub async fn wait_for_hydra(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut retries = 30;
        while retries > 0 {
            match self
                .client
                .get(format!("{}/.well-known/jwks.json", self.public_url))
                .timeout(Duration::from_secs(5))
                .send()
                .await
            {
                Ok(response) if response.status().is_success() => return Ok(()),
                _ => {
                    tokio::time::sleep(Duration::from_millis(1000)).await;
                    retries -= 1;
                }
            }
        }
        Err("Hydra not ready after 30 seconds".into())
    }

    pub async fn get_token(
        &self,
        client_id: &str,
        client_secret: &str,
    ) -> Result<TokenResponse, Box<dyn std::error::Error>> {
        self.get_token_with_audience(client_id, client_secret, None).await
    }

    pub async fn get_token_with_audience(
        &self,
        client_id: &str,
        client_secret: &str,
        audience: Option<&str>,
    ) -> Result<TokenResponse, Box<dyn std::error::Error>> {
        let mut body = "grant_type=client_credentials".to_string();
        if let Some(aud) = audience {
            // Simple URL encoding for audience parameter
            let encoded_aud = aud.replace(" ", "%20").replace("&", "%26").replace("=", "%3D");
            body.push_str(&format!("&audience={encoded_aud}"));
        }

        // Retry logic for token requests to handle connection issues
        let mut retries = 3;
        let mut last_error = None;

        while retries > 0 {
            match self
                .client
                .post(format!("{}/oauth2/token", self.public_url))
                .basic_auth(client_id, Some(client_secret))
                .header("Content-Type", "application/x-www-form-urlencoded")
                .body(body.clone())
                .send()
                .await
            {
                Ok(response) => {
                    if !response.status().is_success() {
                        let error_text = response.text().await?;
                        return Err(format!("Failed to get token: {error_text}").into());
                    }
                    return Ok(response.json().await?);
                }
                Err(e) => {
                    last_error = Some(e);
                    retries -= 1;
                    if retries > 0 {
                        // Small delay before retry
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        }

        Err(format!("Failed to get token after 3 attempts: {last_error:?}").into())
    }
}

/// Setup helper for Hydra-based tests using Hydra instance 1
pub async fn setup_hydra_test() -> Result<(TestServer, String), Box<dyn std::error::Error>> {
    setup_hydra_test_with_audience(None).await
}

/// Setup helper for Hydra-based tests using Hydra instance 1 with audience support
pub async fn setup_hydra_test_with_audience(
    audience: Option<&str>,
) -> Result<(TestServer, String), Box<dyn std::error::Error>> {
    let hydra = HydraClient::new(4444, 4445);

    hydra.wait_for_hydra().await?;

    let client_id = "shared-test-client-universal";
    let client_secret = format!("{client_id}-secret");

    let token_response = hydra
        .get_token_with_audience(client_id, &client_secret, audience)
        .await?;

    let config = if let Some(aud) = audience {
        oauth_config_with_audience(aud)
    } else {
        oauth_config_basic().to_string()
    };

    let server = TestServer::builder().build(&config).await;

    Ok((server, token_response.access_token))
}

/// Setup helper for cross-provider testing: token from Hydra 2, Nexus configured for Hydra 1
pub async fn setup_cross_provider_test() -> Result<(TestServer, String), Box<dyn std::error::Error>> {
    // Get token from Hydra 2 (port 4454)
    let hydra2 = HydraClient::new(4454, 4455);

    // Wait for Hydra 2 to be ready
    hydra2.wait_for_hydra().await?;

    // Use universal client for Hydra 2
    let client_id = "shared-hydra2-client-universal";
    let client_secret = format!("{client_id}-secret");

    // Get access token from Hydra 2 using pre-created client
    let token_response = hydra2.get_token(client_id, &client_secret).await?;

    // Setup Nexus server with OAuth pointing to Hydra 1 (this should reject tokens from Hydra 2)
    let config = oauth_config_basic(); // This points to Hydra 1
    let server = TestServer::builder().build(config).await;

    Ok((server, token_response.access_token))
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct OAuthProtectedResourceMetadata {
    pub resource: String,
    pub authorization_servers: Vec<String>,
}

pub fn oauth_config_basic() -> &'static str {
    indoc! {r#"
        [server.oauth]
        url = "http://127.0.0.1:4444/.well-known/jwks.json"
        poll_interval = "5m"
        expected_issuer = "http://127.0.0.1:4444"

        [server.oauth.protected_resource]
        resource = "http://127.0.0.1:8080"
        authorization_servers = ["http://127.0.0.1:4444"]

        [mcp]
        enabled = true
    "#}
}

pub fn oauth_config_with_audience(audience: &str) -> String {
    format!(
        r#"
        [server.oauth]
        url = "http://127.0.0.1:4444/.well-known/jwks.json"
        poll_interval = "5m"
        expected_issuer = "http://127.0.0.1:4444"
        expected_audience = "{audience}"

        [server.oauth.protected_resource]
        resource = "http://127.0.0.1:8080"
        authorization_servers = ["http://127.0.0.1:4444"]

        [mcp]
        enabled = true
    "#
    )
}

pub fn oauth_config_multiple_auth_servers() -> &'static str {
    indoc! {r#"
        [server.oauth]
        url = "http://127.0.0.1:4444/.well-known/jwks.json"
        poll_interval = "5m"
        expected_issuer = "http://127.0.0.1:4444"

        [server.oauth.protected_resource]
        resource = "http://127.0.0.1:8080"
        authorization_servers = [
            "http://127.0.0.1:4444",
            "http://127.0.0.1:4454",
            "https://auth.example.com"
        ]

        [mcp]
        enabled = true
    "#}
}

/// Mock JWKS server for testing caching behavior
pub struct MockJwksServer {
    pub request_count: Arc<std::sync::atomic::AtomicU32>,
    pub server_addr: SocketAddr,
    _server_handle: tokio::task::JoinHandle<()>,
}

impl MockJwksServer {
    /// Start a mock JWKS server that tracks request count
    pub async fn start() -> Self {
        use axum::{Router, extract::State, response::Json, routing::get};
        use serde_json::json;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU32, Ordering};
        use tokio::net::TcpListener;

        let request_count = Arc::new(AtomicU32::new(0));
        let request_count_clone = request_count.clone();

        // Mock JWKS response (minimal valid JWKS)
        let jwks_response = json!({
            "keys": [
                {
                    "kty": "RSA",
                    "use": "sig",
                    "kid": "test-key-1",
                    "n": "0vx7agoebGcQSuuPiLJXZptN9nndrQmbXEps2aiAFbWhM78LhWx4cbbfAAtVT86zwu1RK7aPFFxuhDR1L6tSoc_BJECPebWKRXjBZCiFV4n3oknjhMstn64tZ_2W-5JsGY4Hc5n9yBXArwl93lqt7_RN5w6Cf0h4QyQ5v-65YGjQR0_FDW2QvzqY368QQMicAtaSqzs8KJZgnYb9c7d0zgdAZHzu6qMQvRL5hajrn1n91CbOpbIS",
                    "e": "AQAB",
                    "alg": "RS256"
                }
            ]
        });

        async fn handle_jwks(State(state): State<(Arc<AtomicU32>, serde_json::Value)>) -> Json<serde_json::Value> {
            let (request_count, jwks_response) = state;
            request_count.fetch_add(1, Ordering::SeqCst);
            Json(jwks_response)
        }

        let app = Router::new()
            .route("/.well-known/jwks.json", get(handle_jwks))
            .with_state((request_count_clone, jwks_response));

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_addr = listener.local_addr().unwrap();

        let server_handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        // Give the server a moment to start
        tokio::time::sleep(Duration::from_millis(100)).await;

        Self {
            request_count,
            server_addr,
            _server_handle: server_handle,
        }
    }

    /// Get the number of JWKS requests made
    pub fn get_request_count(&self) -> u32 {
        self.request_count.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Get the JWKS URL for this mock server
    pub fn jwks_url(&self) -> String {
        format!("http://127.0.0.1:{}/.well-known/jwks.json", self.server_addr.port())
    }
}

/// Create OAuth config with custom JWKS URL and poll interval
pub fn oauth_config_with_jwks_url(jwks_url: &str, poll_interval: &str) -> String {
    format!(
        r#"
        [server.oauth]
        url = "{jwks_url}"
        poll_interval = "{poll_interval}"

        [server.oauth.protected_resource]
        resource = "http://127.0.0.1:8080"
        authorization_servers = ["http://127.0.0.1:4444"]

        [mcp]
        enabled = true
        "#
    )
}

/// Create OAuth config with no poll interval (cache never expires)
pub fn oauth_config_no_poll_interval(jwks_url: &str) -> String {
    format!(
        r#"
        [server.oauth]
        url = "{jwks_url}"

        [server.oauth.protected_resource]
        resource = "http://127.0.0.1:8080"
        authorization_servers = ["http://127.0.0.1:4444"]

        [mcp]
        enabled = true
        "#
    )
}
