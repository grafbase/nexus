use integration_tests::TestServer;

use crate::oauth2::RequestBuilderExt;
use indoc::indoc;

#[tokio::test]
async fn invalid_base64_in_jwt() {
    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    // Create JWT with valid structure but invalid base64 in payload
    let invalid_b64_token = "Bearer eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9.invalid-base64-payload!@#.signature";

    let response = server
        .client
        .request(reqwest::Method::GET, "/mcp")
        .header("Authorization", invalid_b64_token)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 401);

    let response_body = response.text().await.unwrap();
    let error_response: serde_json::Value = serde_json::from_str(&response_body).unwrap();

    insta::assert_json_snapshot!(error_response, @r#"
    {
      "error": "invalid_token",
      "error_description": "invalid token"
    }
    "#);
}

#[tokio::test]
async fn invalid_json_in_jwt_payload() {
    use base64::{Engine as _, engine::general_purpose};

    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    // Create JWT with valid base64 but malformed JSON in payload
    let valid_header = general_purpose::URL_SAFE_NO_PAD.encode(r#"{"alg":"RS256","typ":"JWT"}"#);
    let invalid_json_payload = general_purpose::URL_SAFE_NO_PAD.encode(r#"{"iss":"incomplete json"#);
    let signature = general_purpose::URL_SAFE_NO_PAD.encode("fake-signature");

    let malformed_json_token = format!("Bearer {valid_header}.{invalid_json_payload}.{signature}");

    let response = server
        .client
        .request(reqwest::Method::GET, "/mcp")
        .header("Authorization", &malformed_json_token)
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

#[tokio::test]
async fn lowercase_bearer() {
    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    // Test with lowercase "bearer" - RFC 7235: authentication scheme is case-insensitive
    let response = server
        .client
        .request(reqwest::Method::GET, "/mcp")
        .header("authorization", "bearer sometoken123") // lowercase header name
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 401);

    let response_body = response.text().await.unwrap();
    let error_response: serde_json::Value = serde_json::from_str(&response_body).unwrap();

    insta::assert_json_snapshot!(error_response, @r#"
    {
      "error": "invalid_token",
      "error_description": "invalid token"
    }
    "#);
}

#[tokio::test]
async fn very_long_token() {
    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    // Test extremely long JWT token that might cause parsing issues
    let very_long_token_part = "a".repeat(100000);
    let very_long_token = format!("Bearer {very_long_token_part}");

    let response = server
        .client
        .request(reqwest::Method::GET, "/mcp")
        .header("Authorization", &very_long_token)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 401);

    let response_body = response.text().await.unwrap();
    let error_response: serde_json::Value = serde_json::from_str(&response_body).unwrap();

    insta::assert_json_snapshot!(error_response, @r#"
    {
      "error": "invalid_token",
      "error_description": "invalid token"
    }
    "#);
}

#[tokio::test]
async fn jwt_with_empty_parts() {
    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    // Test JWT with valid structure but empty parts
    let empty_parts_token = "Bearer ..";

    let response = server
        .client
        .request(reqwest::Method::GET, "/mcp")
        .header("Authorization", empty_parts_token)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 401);

    let response_body = response.text().await.unwrap();
    let error_response: serde_json::Value = serde_json::from_str(&response_body).unwrap();

    insta::assert_json_snapshot!(error_response, @r#"
    {
      "error": "invalid_token",
      "error_description": "invalid token"
    }
    "#);
}

#[tokio::test]
async fn jwt_with_null_claims() {
    use base64::{Engine as _, engine::general_purpose};

    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    // Create JWT with valid structure but null required claims
    let header = r#"{"alg":"RS256","typ":"JWT"}"#;
    let claims_with_nulls = r#"{"iss":null,"aud":null,"sub":null,"exp":null}"#;

    let header_b64 = general_purpose::URL_SAFE_NO_PAD.encode(header);
    let payload_b64 = general_purpose::URL_SAFE_NO_PAD.encode(claims_with_nulls);
    let signature_b64 = general_purpose::URL_SAFE_NO_PAD.encode("fake-signature");

    let null_claims_token = format!("Bearer {header_b64}.{payload_b64}.{signature_b64}");
    let response = server
        .client
        .request(reqwest::Method::GET, "/mcp")
        .header("Authorization", &null_claims_token)
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

#[tokio::test]
async fn jwt_with_invalid_claim_types() {
    use base64::{Engine as _, engine::general_purpose};

    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    // Create JWT with wrong data types for claims
    let header = r#"{"alg":"RS256","typ":"JWT"}"#;
    let invalid_claims = r#"{"iss":123,"aud":true,"sub":[],"exp":"not-a-number"}"#;

    let header_b64 = general_purpose::URL_SAFE_NO_PAD.encode(header);
    let payload_b64 = general_purpose::URL_SAFE_NO_PAD.encode(invalid_claims);
    let signature_b64 = general_purpose::URL_SAFE_NO_PAD.encode("fake-signature");

    let invalid_types_token = format!("Bearer {header_b64}.{payload_b64}.{signature_b64}");

    let response = server
        .client
        .request(reqwest::Method::GET, "/mcp")
        .header("Authorization", &invalid_types_token)
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

#[tokio::test]
async fn jwt_with_future_issued_time() {
    use base64::{Engine as _, engine::general_purpose};

    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    // Create JWT with iat (issued at) in the future
    let header = r#"{"alg":"RS256","typ":"JWT"}"#;
    let future_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 3600; // 1 hour in the future

    let future_iat_claims = format!(
        r#"{{"iss":"http://127.0.0.1:4444","aud":"http://127.0.0.1:8080","sub":"test-user","exp":{},"iat":{}}}"#,
        future_time + 7200,
        future_time
    );

    let header_b64 = general_purpose::URL_SAFE_NO_PAD.encode(header);
    let payload_b64 = general_purpose::URL_SAFE_NO_PAD.encode(future_iat_claims);
    let signature_b64 = general_purpose::URL_SAFE_NO_PAD.encode("fake-signature");

    let future_iat_token = format!("Bearer {header_b64}.{payload_b64}.{signature_b64}");
    let response = server
        .client
        .request(reqwest::Method::GET, "/mcp")
        .header("Authorization", &future_iat_token)
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

#[tokio::test]
async fn jwt_with_unicode_characters() {
    use base64::{Engine as _, engine::general_purpose};

    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    // Create JWT with Unicode characters in claims
    let header = r#"{"alg":"RS256","typ":"JWT"}"#;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let unicode_claims = format!(
        r#"{{"iss":"http://127.0.0.1:4444","aud":"http://127.0.0.1:8080","sub":"用户-тест-🚀","exp":{},"iat":{}}}"#,
        now + 3600,
        now
    );

    let header_b64 = general_purpose::URL_SAFE_NO_PAD.encode(header);
    let payload_b64 = general_purpose::URL_SAFE_NO_PAD.encode(unicode_claims);
    let signature_b64 = general_purpose::URL_SAFE_NO_PAD.encode("fake-signature");

    let unicode_token = format!("Bearer {header_b64}.{payload_b64}.{signature_b64}");

    let response = server
        .client
        .request(reqwest::Method::GET, "/mcp")
        .header("Authorization", &unicode_token)
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

#[tokio::test]
async fn authorization_header_with_extra_spaces() {
    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    // Test Authorization header with extra spaces
    let response = server
        .client
        .request(reqwest::Method::GET, "/mcp")
        .header("Authorization", "  Bearer   sometoken123  ")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 401);

    let response_body = response.text().await.unwrap();
    let error_response: serde_json::Value = serde_json::from_str(&response_body).unwrap();

    insta::assert_json_snapshot!(error_response, @r#"
    {
      "error": "invalid_token",
      "error_description": "invalid token"
    }
    "#);
}

#[tokio::test]
async fn bearer_with_multiple_spaces() {
    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    // Test Bearer with multiple spaces before token
    let response = server
        .client
        .request(reqwest::Method::GET, "/mcp")
        .header("Authorization", "Bearer    sometoken123")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 401);

    let response_body = response.text().await.unwrap();
    let error_response: serde_json::Value = serde_json::from_str(&response_body).unwrap();

    insta::assert_json_snapshot!(error_response, @r#"
    {
      "error": "invalid_token",
      "error_description": "invalid token"
    }
    "#);
}

#[tokio::test]
async fn malformed_jwks_response() {
    // Use a config that points to a non-existent JWKS server
    // This will cause JWKS fetch failures
    let config = indoc! {r#"
        [server.oauth]
        url = "http://127.0.0.1:9999/.well-known/jwks.json"
        poll_interval = "5m"

        [server.oauth.protected_resource]
        resource = "http://127.0.0.1:8080"
        authorization_servers = ["http://127.0.0.1:9999"]
    "#};

    let server = TestServer::builder().build(config).await;

    let unsigned_token = super::create_test_jwt_unsigned();

    let response = server
        .client
        .request(reqwest::Method::GET, "/mcp")
        .authorization(&unsigned_token)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 500);

    let response_body = response.text().await.unwrap();
    let error_response: serde_json::Value = serde_json::from_str(&response_body).unwrap();

    insta::assert_json_snapshot!(error_response, @r#"
    {
      "error": "internal_server_error",
      "error_description": "An internal error occurred"
    }
    "#);
}

#[tokio::test]
async fn oauth_jwks_network_failure() {
    // Test network failure by using a non-responsive port
    let config = indoc! {r#"
        [server.oauth]
        url = "http://127.0.0.1:65535/.well-known/jwks.json"
        poll_interval = "5m"

        [server.oauth.protected_resource]
        resource = "http://127.0.0.1:8080"
        authorization_servers = ["http://127.0.0.1:65535"]
    "#};

    let server = TestServer::builder().build(config).await;

    let unsigned_token = super::create_test_jwt_unsigned();
    let response = server
        .client
        .request(reqwest::Method::GET, "/mcp")
        .authorization(&unsigned_token)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 500);

    let response_body = response.text().await.unwrap();
    let error_response: serde_json::Value = serde_json::from_str(&response_body).unwrap();

    insta::assert_json_snapshot!(error_response, @r#"
    {
      "error": "internal_server_error",
      "error_description": "An internal error occurred"
    }
    "#);
}

#[tokio::test]
async fn jwt_with_missing_required_claims() {
    use base64::{Engine as _, engine::general_purpose};

    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    // Create JWT missing required claims (no exp, iss, aud)
    let header = r#"{"alg":"RS256","typ":"JWT"}"#;
    let minimal_claims = r#"{"sub":"test-user"}"#;

    let header_b64 = general_purpose::URL_SAFE_NO_PAD.encode(header);
    let payload_b64 = general_purpose::URL_SAFE_NO_PAD.encode(minimal_claims);
    let signature_b64 = general_purpose::URL_SAFE_NO_PAD.encode("fake-signature");

    let minimal_token = format!("Bearer {header_b64}.{payload_b64}.{signature_b64}");
    let response = server
        .client
        .request(reqwest::Method::GET, "/mcp")
        .header("Authorization", &minimal_token)
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

#[tokio::test]
async fn jwt_with_wrong_audience() {
    use base64::{Engine as _, engine::general_purpose};

    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    // Create JWT with wrong audience
    let header = r#"{"alg":"RS256","typ":"JWT"}"#;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let wrong_aud_claims = format!(
        r#"{{"iss":"http://127.0.0.1:4444","aud":"http://wrong-audience.com","sub":"test-user","exp":{},"iat":{}}}"#,
        now + 3600,
        now
    );

    let header_b64 = general_purpose::URL_SAFE_NO_PAD.encode(header);
    let payload_b64 = general_purpose::URL_SAFE_NO_PAD.encode(wrong_aud_claims);
    let signature_b64 = general_purpose::URL_SAFE_NO_PAD.encode("fake-signature");

    let wrong_aud_token = format!("Bearer {header_b64}.{payload_b64}.{signature_b64}");

    let response = server
        .client
        .request(reqwest::Method::GET, "/mcp")
        .header("Authorization", &wrong_aud_token)
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

#[tokio::test]
async fn jwt_with_nbf_future() {
    use base64::{Engine as _, engine::general_purpose};

    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    // Create JWT with nbf (not before) in the future
    let header = r#"{"alg":"RS256","typ":"JWT"}"#;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let future_time = now + 3600; // 1 hour in the future

    let future_nbf_claims = format!(
        r#"{{"iss":"http://127.0.0.1:4444","aud":"http://127.0.0.1:8080","sub":"test-user","exp":{},"iat":{},"nbf":{}}}"#,
        now + 7200,  // exp 2 hours from now
        now,         // iat now
        future_time  // nbf 1 hour from now
    );

    let header_b64 = general_purpose::URL_SAFE_NO_PAD.encode(header);
    let payload_b64 = general_purpose::URL_SAFE_NO_PAD.encode(future_nbf_claims);
    let signature_b64 = general_purpose::URL_SAFE_NO_PAD.encode("fake-signature");

    let future_nbf_token = format!("Bearer {header_b64}.{payload_b64}.{signature_b64}");

    let response = server
        .client
        .request(reqwest::Method::GET, "/mcp")
        .header("Authorization", &future_nbf_token)
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

#[tokio::test]
async fn bearer_case_variations() {
    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    // Test various case combinations - RFC 7235: authentication scheme is case-insensitive
    let test_cases = vec![
        ("BEARER token123", "invalid token"),
        ("bearer token123", "invalid token"),
        ("BeArEr token123", "invalid token"),
        ("Bearer token123", "invalid token"),
        ("bEaReR token123", "invalid token"),
    ];

    for (auth_header, expected_description) in test_cases {
        let response = server
            .client
            .request(reqwest::Method::GET, "/mcp")
            .header("Authorization", auth_header)
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), 401);
        let response_body = response.text().await.unwrap();
        let error_response: serde_json::Value = serde_json::from_str(&response_body).unwrap();

        // Verify the specific error description
        assert_eq!(
            error_response["error_description"].as_str().unwrap(),
            expected_description,
            "Failed for auth header: {auth_header}"
        );
    }
}

#[tokio::test]
async fn case_insensitive_bearer_with_valid_token() {
    // Test that case-insensitive Bearer works with actual valid tokens (RFC 7235 compliance)
    let (server, access_token) = super::setup_hydra_test().await.unwrap();

    // Test various case combinations with a valid token
    let test_cases = vec![
        format!("Bearer {}", access_token),
        format!("bearer {}", access_token),
        format!("BEARER {}", access_token),
        format!("BeArEr {}", access_token),
        format!("bEaReR {}", access_token),
    ];

    for auth_header in test_cases {
        let response = server
            .client
            .request(reqwest::Method::POST, "/mcp")
            .header("Authorization", &auth_header)
            .mcp_json(r#"{"jsonrpc": "2.0", "method": "ping", "id": 1}"#)
            .send()
            .await
            .unwrap();

        // All case variations should work with valid tokens
        let status = response.status();
        #[allow(clippy::panic)]
        if status != 200 {
            let error_body = response.text().await.unwrap();
            panic!("Failed for auth header: {auth_header}, status: {status}, body: {error_body}");
        }

        let response_text = response.text().await.unwrap();

        #[allow(clippy::panic)]
        if response_text.is_empty() {
            panic!("Empty response for auth header: {auth_header}");
        }

        // Handle SSE format: strip "data: " prefix if present
        let json_text = if let Some(stripped) = response_text.strip_prefix("data: ") {
            stripped
        } else {
            &response_text
        };

        #[allow(clippy::panic)]
        let response_body: serde_json::Value = serde_json::from_str(json_text).unwrap_or_else(|e| {
            panic!("Failed to parse JSON for auth header: {auth_header}, error: {e}, body: {response_text}")
        });

        assert_eq!(response_body["id"], 1);
        assert!(
            response_body["result"].is_object(),
            "Expected result object for auth header: {auth_header}"
        );
    }
}
