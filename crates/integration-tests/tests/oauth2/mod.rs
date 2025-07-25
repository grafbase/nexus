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
use uuid::Uuid;

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Vec<String>>,
}

/// Helper to create a basic test JWT (unsigned - for negative tests)
pub fn create_test_jwt_unsigned(scopes: Option<&str>) -> String {
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
        scope: scopes.map(|s| s.to_string()),
        scopes: None,
    };

    let header_b64 = general_purpose::URL_SAFE_NO_PAD.encode(header);
    let claims_b64 = general_purpose::URL_SAFE_NO_PAD.encode(serde_json::to_string(&claims).unwrap());

    format!("{header_b64}.{claims_b64}.") // No signature for unsigned
}

/// Helper to create a test JWT with custom audience (unsigned - for negative tests)
pub fn create_test_jwt_unsigned_with_audience(audience: &str, scopes: Option<&str>) -> String {
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
        scope: scopes.map(|s| s.to_string()),
        scopes: None,
    };

    let header_b64 = general_purpose::URL_SAFE_NO_PAD.encode(header);
    let claims_b64 = general_purpose::URL_SAFE_NO_PAD.encode(serde_json::to_string(&claims).unwrap());

    format!("{header_b64}.{claims_b64}.") // No signature for unsigned
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
        scope: Some("read write".to_string()),
        scopes: None,
    };

    let header_b64 = general_purpose::URL_SAFE_NO_PAD.encode(header);
    let claims_b64 = general_purpose::URL_SAFE_NO_PAD.encode(serde_json::to_string(&claims).unwrap());

    format!("{header_b64}.{claims_b64}.")
}

pub struct HydraClient {
    pub admin_url: String,
    pub public_url: String,
    pub client: reqwest::Client,
}

#[derive(Debug, serde::Serialize)]
pub struct CreateClientRequest {
    pub client_id: String,
    pub client_secret: String,
    pub grant_types: Vec<String>,
    pub scope: String,
    pub token_endpoint_auth_method: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct CreateClientResponse {
    pub client_id: String,
    pub client_secret: String,
}

#[derive(Debug, serde::Serialize)]
pub struct TokenRequest {
    pub grant_type: String,
    pub client_id: String,
    pub client_secret: String,
    pub scope: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
}

impl HydraClient {
    pub fn new(hydra_public_port: u16, hydra_admin_port: u16) -> Self {
        Self {
            admin_url: format!("http://127.0.0.1:{hydra_admin_port}"),
            public_url: format!("http://127.0.0.1:{hydra_public_port}"),
            client: reqwest::Client::new(),
        }
    }

    pub async fn wait_for_hydra(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut retries = 30;
        while retries > 0 {
            match self
                .client
                .get(format!("{}/.well-known/jwks.json", self.public_url))
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

    pub async fn create_client(
        &self,
        client_id: &str,
        scopes: &str,
    ) -> Result<CreateClientResponse, Box<dyn std::error::Error>> {
        self.create_client_with_audience(client_id, scopes, None).await
    }

    pub async fn create_client_with_audience(
        &self,
        client_id: &str,
        scopes: &str,
        audience: Option<&str>,
    ) -> Result<CreateClientResponse, Box<dyn std::error::Error>> {
        let mut request_body = serde_json::json!({
            "client_id": client_id,
            "client_secret": format!("{}-secret", client_id),
            "grant_types": ["client_credentials"],
            "scope": scopes,
            "token_endpoint_auth_method": "client_secret_basic",
            "access_token_strategy": "jwt"
        });

        // Add audience if specified
        if let Some(aud) = audience {
            request_body["audience"] = serde_json::json!([aud]);
        }

        let response = self
            .client
            .post(format!("{}/admin/clients", self.admin_url))
            .json(&request_body)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(format!("Failed to create client: {error_text}").into());
        }

        Ok(response.json().await?)
    }

    pub async fn get_token(
        &self,
        client_id: &str,
        client_secret: &str,
        scopes: &str,
    ) -> Result<TokenResponse, Box<dyn std::error::Error>> {
        self.get_token_with_audience(client_id, client_secret, scopes, None)
            .await
    }

    pub async fn get_token_with_audience(
        &self,
        client_id: &str,
        client_secret: &str,
        scopes: &str,
        audience: Option<&str>,
    ) -> Result<TokenResponse, Box<dyn std::error::Error>> {
        let mut body = format!("grant_type=client_credentials&scope={scopes}");
        if let Some(aud) = audience {
            // Simple URL encoding for audience parameter
            let encoded_aud = aud.replace(" ", "%20").replace("&", "%26").replace("=", "%3D");
            body.push_str(&format!("&audience={encoded_aud}"));
        }

        let response = self
            .client
            .post(format!("{}/oauth2/token", self.public_url))
            .basic_auth(client_id, Some(client_secret))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(format!("Failed to get token: {error_text}").into());
        }

        Ok(response.json().await?)
    }
}

/// Setup helper for Hydra-based tests using Hydra instance 1
pub async fn setup_hydra_test(
    client_id: &str,
    scopes: &str,
) -> Result<(TestServer, String), Box<dyn std::error::Error>> {
    setup_hydra_test_with_audience(client_id, scopes, None).await
}

/// Setup helper for Hydra-based tests using Hydra instance 1 with audience support
pub async fn setup_hydra_test_with_audience(
    client_id: &str,
    scopes: &str,
    audience: Option<&str>,
) -> Result<(TestServer, String), Box<dyn std::error::Error>> {
    let hydra = HydraClient::new(4444, 4445);

    // Wait for Hydra to be ready
    hydra.wait_for_hydra().await?;

    // Create OAuth client with unique ID to avoid conflicts
    let unique_client_id = format!("{client_id}-{}", Uuid::new_v4());
    let client_response = hydra
        .create_client_with_audience(&unique_client_id, scopes, audience)
        .await?;

    // Get access token
    let token_response = hydra
        .get_token_with_audience(
            &client_response.client_id,
            &client_response.client_secret,
            scopes,
            audience,
        )
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
pub async fn setup_cross_provider_test(
    client_id: &str,
    scopes: &str,
) -> Result<(TestServer, String), Box<dyn std::error::Error>> {
    // Get token from Hydra 2 (port 4454)
    let hydra2 = HydraClient::new(4454, 4455);

    // Wait for Hydra 2 to be ready
    hydra2.wait_for_hydra().await?;

    // Create OAuth client on Hydra 2 with unique ID to avoid conflicts
    let unique_client_id = format!("{}-hydra2-{}", client_id, chrono::Utc::now().timestamp());
    let client_response = hydra2.create_client(&unique_client_id, scopes).await?;

    // Get access token from Hydra 2
    let token_response = hydra2
        .get_token(&client_response.client_id, &client_response.client_secret, scopes)
        .await?;

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

pub fn oauth_config_without_scopes() -> &'static str {
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

pub fn oauth_config_complex_scopes() -> &'static str {
    indoc! {r#"
        [server.oauth]
        url = "http://127.0.0.1:4444/.well-known/jwks.json"
        poll_interval = "30s"
        expected_issuer = "http://127.0.0.1:4444"

        [server.oauth.protected_resource]
        resource = "https://api.example.com"
        authorization_servers = ["http://127.0.0.1:4444"]


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
