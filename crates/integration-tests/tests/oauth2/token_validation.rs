use integration_tests::TestServer;

use crate::oauth2::{RequestBuilderExt, TestJwtClaims};

/// Helper to create a JWT with an unsupported algorithm
fn create_jwt_with_unsupported_algorithm() -> String {
    use base64::{Engine as _, engine::general_purpose};

    // Use an unsupported algorithm like "HS999"
    let header = r#"{"alg":"HS999","typ":"JWT"}"#;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let claims = TestJwtClaims {
        iss: "http://127.0.0.1:4444".to_string(),
        aud: "http://127.0.0.1:8080".to_string(),
        sub: "test-user".to_string(),
        exp: now + 3600,
        iat: now,
        scope: Some("read write admin".to_string()),
        scopes: None,
    };

    let header_b64 = general_purpose::URL_SAFE_NO_PAD.encode(header);
    let payload_b64 = general_purpose::URL_SAFE_NO_PAD.encode(serde_json::to_string(&claims).unwrap());

    // Create an invalid signature
    let signature_b64 = general_purpose::URL_SAFE_NO_PAD.encode("fake-signature");

    format!("{header_b64}.{payload_b64}.{signature_b64}")
}

#[tokio::test]
async fn missing_token_denied() {
    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    // Try to access MCP endpoint without Authorization header
    let response = server.client.get("/mcp").await;

    assert_eq!(response.status(), 401);

    // Verify WWW-Authenticate header is present with correct format
    let www_auth = response.headers().get("www-authenticate");
    assert!(www_auth.is_some());

    let auth_header = www_auth.unwrap().to_str().unwrap();
    assert!(auth_header.starts_with("Bearer "));
    assert!(auth_header.contains("resource_metadata="));
    assert!(auth_header.contains("/.well-known/oauth-protected-resource"));

    insta::assert_snapshot!(auth_header, @r#"Bearer resource_metadata="http://127.0.0.1:8080/.well-known/oauth-protected-resource""#);

    // Verify response body contains proper JSON error
    let response_body = response.text().await.unwrap();
    let error_response: serde_json::Value = serde_json::from_str(&response_body).unwrap();
    insta::assert_json_snapshot!(error_response, @r#"
    {
      "error": "invalid_token",
      "error_description": "missing token"
    }
    "#);
}

#[tokio::test]
async fn malformed_jwt_denied() {
    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    let test_cases = [
        "invalid-token",
        "not.a.jwt",
        "too.many.parts.here.invalid",
        "eyJhbGciOiJub25lIn0", // Only header, missing parts
        "",                    // Empty token
    ];

    for (i, invalid_token) in test_cases.iter().enumerate() {
        let response = server
            .client
            .request(reqwest::Method::GET, "/mcp")
            .authorization(invalid_token)
            .send()
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            401,
            "Test case {i}: Token '{invalid_token}' should be rejected"
        );

        // Verify WWW-Authenticate header is present
        let www_auth = response.headers().get("www-authenticate");
        assert!(www_auth.is_some(), "Test case {i}: Missing WWW-Authenticate header");

        // Verify response body contains proper JSON error
        let response_body = response.text().await.unwrap();
        let error_response: serde_json::Value = serde_json::from_str(&response_body).unwrap();

        assert_eq!(
            error_response["error"], "invalid_token",
            "Test case {i}: Wrong error type"
        );

        // Use insta snapshot for the specific error description
        insta::allow_duplicates! {
            match *invalid_token {
                "invalid-token" | "not.a.jwt" | "too.many.parts.here.invalid" => {
                    insta::assert_json_snapshot!(error_response, @r#"
                    {
                      "error": "invalid_token",
                      "error_description": "invalid token"
                    }
                    "#);
                }
                "eyJhbGciOiJub25lIn0" => {
                    insta::assert_json_snapshot!(error_response, @r#"
                    {
                      "error": "invalid_token",
                      "error_description": "invalid token"
                    }
                    "#);
                }
                "" => {
                    // Empty string becomes "Bearer " which gets trimmed to "Bearer"
                    insta::assert_json_snapshot!(error_response, @r#"
                    {
                      "error": "invalid_token",
                      "error_description": "missing token"
                    }
                    "#);
                }
                _ => {
                    unreachable!("Unexpected test case: {invalid_token}");
                }
            }
        }
    }
}

#[tokio::test]
async fn expired_jwt_denied() {
    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    let expired_token = super::create_expired_jwt();

    let response = server
        .client
        .request(reqwest::Method::GET, "/mcp")
        .authorization(&expired_token)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 401);

    let www_auth = response.headers().get("www-authenticate").unwrap();
    let auth_header = www_auth.to_str().unwrap();
    assert!(auth_header.contains("resource_metadata="));
}

#[tokio::test]
async fn invalid_jwt_signature_denied() {
    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    let unsigned_token = super::create_test_jwt_unsigned(Some("read write"));

    let response = server
        .client
        .request(reqwest::Method::GET, "/mcp")
        .authorization(&unsigned_token)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 401);

    let www_auth = response.headers().get("www-authenticate").unwrap();
    let auth_header = www_auth.to_str().unwrap();

    assert!(auth_header.starts_with("Bearer "));
    assert!(auth_header.contains("resource_metadata="));
    assert!(auth_header.contains("http://127.0.0.1:8080/.well-known/oauth-protected-resource"));
}

#[tokio::test]
async fn wrong_bearer_format_denied() {
    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    let test_cases = [
        "Basic dXNlcjpwYXNz", // Basic auth instead of Bearer
        "bearer token123",    // Lowercase 'bearer'
        "Bearer",             // Missing token
        "Bearer ",            // Empty token with space
        "Token abc123",       // Wrong auth type
    ];

    for (i, invalid_auth) in test_cases.iter().enumerate() {
        let response = server
            .client
            .request(reqwest::Method::GET, "/mcp")
            .header("Authorization", *invalid_auth)
            .send()
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            401,
            "Test case {i}: Auth header '{invalid_auth}' should be rejected"
        );
    }
}

#[tokio::test]
async fn multiple_authorization_headers_denied() {
    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    // Test with multiple Authorization headers (should be rejected)
    let response = server
        .client
        .request(reqwest::Method::GET, "/mcp")
        .header("Authorization", "Bearer token1")
        .header("Authorization", "Bearer token2")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 401);
}

#[tokio::test]
async fn cross_provider_token_rejected() {
    // This test verifies that tokens issued by one OAuth provider (Hydra 2)
    // are rejected when used with Nexus configured for a different provider (Hydra 1)
    // This is critical for security - prevents token reuse across different systems

    let (server, access_token) = super::setup_cross_provider_test("test-cross-provider", "read write")
        .await
        .unwrap();

    // Try to use token from Hydra 2 with Nexus configured for Hydra 1
    let response = server
        .client
        .request(reqwest::Method::GET, "/mcp")
        .authorization(&access_token)
        .send()
        .await
        .unwrap();

    // Should be rejected with 401 Unauthorized
    assert_eq!(response.status(), 401);

    // Verify WWW-Authenticate header is present
    let www_auth = response.headers().get("www-authenticate");
    assert!(www_auth.is_some());

    let auth_header = www_auth.unwrap().to_str().unwrap();
    assert!(auth_header.contains("resource_metadata="));

    // Also test that the health endpoint still bypasses auth even with cross-provider setup
    let health_response = server.client.get("/health").await;
    assert_eq!(health_response.status(), 200);
}

#[tokio::test]
async fn valid_jwt_access() {
    let (server, access_token) = super::setup_hydra_test("test-client", "read write").await.unwrap();

    // Test authenticated access to MCP endpoint
    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&access_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "ping", "id": 1}"#)
        .send()
        .await
        .unwrap();

    assert_ne!(response.status(), 401, "Valid JWT should grant access");
}

#[tokio::test]
async fn unsupported_algorithm_error() {
    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    // Create a JWT with an unsupported algorithm
    let token_with_unsupported_alg = create_jwt_with_unsupported_algorithm();
    let response = server
        .client
        .request(reqwest::Method::GET, "/mcp")
        .authorization(&token_with_unsupported_alg)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 401);
    let response_body = response.text().await.unwrap();
    let error_response: serde_json::Value = serde_json::from_str(&response_body).unwrap();
    insta::assert_json_snapshot!(error_response, @r#"
    {
      "error": "unauthorized"
    }
    "#);
}
