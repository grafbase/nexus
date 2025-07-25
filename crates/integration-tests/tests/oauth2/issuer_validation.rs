use integration_tests::TestServer;

use crate::oauth2::RequestBuilderExt;

#[tokio::test]
async fn no_issuer_audience_validation_when_not_configured() {
    // Test that when expected_issuer and expected_audience are not configured,
    // the server accepts any issuer/audience (only validates signature, expiry, scopes)
    let config = r#"
        [server.oauth]
        url = "http://127.0.0.1:4444/.well-known/jwks.json"
        poll_interval = "5m"
        # No expected_issuer or expected_audience configured

        [server.oauth.protected_resource]
        resource = "http://127.0.0.1:8080"
        authorization_servers = ["http://127.0.0.1:4444"]
        scopes_supported = ["read", "write", "admin"]

        [mcp]
        enabled = true
    "#;

    let server = TestServer::builder().build(config).await;

    // Use Hydra to get a valid signed token, even though issuer/audience might differ
    let (_hydra_server, access_token) = super::setup_hydra_test("no-validation-test", "read").await.unwrap();

    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&access_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "tools/list", "id": 1}"#)
        .send()
        .await
        .unwrap();

    // Should succeed because no issuer/audience validation is configured
    assert_eq!(
        response.status(),
        200,
        "Should accept token when issuer/audience validation is not configured"
    );
}

#[tokio::test]
async fn with_hydra_token() {
    // Test issuer validation using a real Hydra token
    let config = r#"
        [server.oauth]
        url = "http://127.0.0.1:4444/.well-known/jwks.json"
        poll_interval = "5m"
        expected_issuer = "http://127.0.0.1:4444"
        # No expected_audience - let's test issuer validation only

        [server.oauth.protected_resource]
        resource = "http://127.0.0.1:8080"
        authorization_servers = ["http://127.0.0.1:4444"]
        scopes_supported = ["read", "write", "admin"]

        [mcp]
        enabled = true
    "#;

    let server = TestServer::builder().build(config).await;

    // Get a real signed token from Hydra
    let (_, access_token) = super::setup_hydra_test("issuer-validation-test", "read").await.unwrap();

    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&access_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "tools/list", "id": 1}"#)
        .send()
        .await
        .unwrap();

    // This should work if the Hydra token has the expected issuer
    // (since Hydra is configured to use http://127.0.0.1:4444 as issuer)
    assert_eq!(response.status(), 200, "Valid Hydra token should be accepted");
}

#[tokio::test]
async fn wrong_issuer_validation() {
    // Test that tokens with wrong issuer are rejected
    let config = r#"
        [server.oauth]
        url = "http://127.0.0.1:4444/.well-known/jwks.json"
        poll_interval = "5m"
        expected_issuer = "https://wrong-issuer.example.com"
        # No expected_audience - focus on issuer validation

        [server.oauth.protected_resource]
        resource = "http://127.0.0.1:8080"
        authorization_servers = ["http://127.0.0.1:4444"]
        scopes_supported = ["read", "write", "admin"]

        [mcp]
        enabled = true
    "#;

    let server = TestServer::builder().build(config).await;

    // Get a real signed token from Hydra (which will have issuer http://127.0.0.1:4444)
    let (_, access_token) = super::setup_hydra_test("wrong-issuer-test", "read").await.unwrap();

    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&access_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "tools/list", "id": 1}"#)
        .send()
        .await
        .unwrap();

    // Should be rejected because Hydra token has wrong issuer
    assert_eq!(response.status(), 401, "Token with wrong issuer should be rejected");
}
