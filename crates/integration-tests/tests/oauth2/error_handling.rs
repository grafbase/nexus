use integration_tests::TestServer;

use crate::oauth2::RequestBuilderExt;

#[tokio::test]
async fn comprehensive_error_descriptions() {
    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    // Test case 1: Missing Authorization header
    let response = server.client.get("/mcp").await;
    assert_eq!(response.status(), 401);

    let response_body = response.text().await.unwrap();
    let error_response: serde_json::Value = serde_json::from_str(&response_body).unwrap();

    insta::assert_json_snapshot!(error_response, @r#"
    {
      "error": "invalid_token",
      "error_description": "missing token"
    }
    "#);

    // Test case 2: Invalid Authorization header (non-Bearer)
    let response = server
        .client
        .request(reqwest::Method::GET, "/mcp")
        .header("Authorization", "Basic dXNlcjpwYXNz")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 401);

    let response_body = response.text().await.unwrap();
    let error_response: serde_json::Value = serde_json::from_str(&response_body).unwrap();

    insta::assert_json_snapshot!(error_response, @r#"
    {
      "error": "invalid_token",
      "error_description": "token must be prefixed with Bearer"
    }
    "#);

    // Test case 3: Bearer token with space but empty token value (HTTP trims trailing spaces)
    // Since HTTP headers trim trailing whitespace, "Bearer " becomes "Bearer"
    let response = server
        .client
        .request(reqwest::Method::GET, "/mcp")
        .header("Authorization", "Bearer ")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 401);

    let response_body = response.text().await.unwrap();
    let error_response: serde_json::Value = serde_json::from_str(&response_body).unwrap();

    insta::assert_json_snapshot!(error_response, @r#"
    {
      "error": "invalid_token",
      "error_description": "missing token"
    }
    "#);

    // Test case 4: Malformed JWT (not three parts)
    let response = server
        .client
        .request(reqwest::Method::GET, "/mcp")
        .header("Authorization", "Bearer invalid.jwt")
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

    // Test case 5: Completely invalid token
    let response = server
        .client
        .request(reqwest::Method::GET, "/mcp")
        .header("Authorization", "Bearer not-a-jwt-at-all")
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

    // Test case 6: Bearer without space (missing token scenario)
    let response = server
        .client
        .request(reqwest::Method::GET, "/mcp")
        .header("Authorization", "Bearer")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 401);

    let response_body = response.text().await.unwrap();
    let error_response: serde_json::Value = serde_json::from_str(&response_body).unwrap();

    insta::assert_json_snapshot!(error_response, @r#"
    {
      "error": "invalid_token",
      "error_description": "missing token"
    }
    "#);

    // Test case 7: Valid JWT structure but invalid signature (leads to Unauthorized)
    let unsigned_token = super::create_test_jwt_unsigned(Some("read write admin"));

    let response = server
        .client
        .request(reqwest::Method::GET, "/mcp")
        .authorization(&unsigned_token)
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

    // Test case 8: Expired token (leads to Unauthorized)
    let expired_token = super::create_expired_jwt();

    let response = server
        .client
        .request(reqwest::Method::GET, "/mcp")
        .authorization(&expired_token)
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

    // Test case 9: Bearer without token (simulating empty token scenario)
    let response = server
        .client
        .request(reqwest::Method::GET, "/mcp")
        .header("Authorization", "Bearer")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 401);

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
async fn internal_server_error_jwks_failure() {
    use indoc::indoc;

    // Create a config with an invalid JWKS URL that will cause network failure
    let config = indoc! {r#"
        [server.oauth]
        url = "http://127.0.0.1:9999/.well-known/jwks.json"
        poll_interval = "5m"

        [server.oauth.protected_resource]
        resource = "http://127.0.0.1:8080"
        authorization_servers = ["http://127.0.0.1:9999"]
        scopes_supported = ["read", "write", "admin"]

    "#};

    let server = TestServer::builder().build(config).await;

    // Create a valid JWT structure but the JWKS fetch will fail
    let unsigned_token = super::create_test_jwt_unsigned(Some("read write admin"));
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
