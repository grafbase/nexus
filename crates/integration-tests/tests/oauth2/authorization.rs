use integration_tests::TestServer;

use crate::oauth2::OAuthProtectedResourceMetadata;

#[tokio::test]
async fn health_endpoint_bypasses_auth() {
    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    // Health endpoint should bypass OAuth authentication
    // Health endpoints are typically used for container health checks and monitoring
    // and should remain accessible without authentication
    let response = server.client.get("/health").await;
    assert_eq!(response.status(), 200);

    // Verify response body contains health status
    let health_response: serde_json::Value = response.json().await.unwrap();
    assert_eq!(health_response["status"], "healthy");
}

#[tokio::test]
async fn metadata_endpoint_bypasses_auth() {
    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    // OAuth metadata endpoint should work without authentication
    // This is correct behavior per OAuth2 spec - metadata must be publicly accessible
    let response = server.client.get("/.well-known/oauth-protected-resource").await;
    assert_eq!(response.status(), 200);

    let metadata: OAuthProtectedResourceMetadata = response.json().await.unwrap();
    assert_eq!(metadata.resource, "http://127.0.0.1:8080/");
}

#[tokio::test]
async fn different_http_methods_require_auth() {
    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    let methods = vec![
        reqwest::Method::GET,
        reqwest::Method::POST,
        reqwest::Method::PUT,
        reqwest::Method::DELETE,
        reqwest::Method::PATCH,
    ];

    for method in methods {
        let response = server.client.request(method.clone(), "/mcp").send().await.unwrap();

        assert_eq!(
            response.status(),
            401,
            "Method {method:?} should require authentication"
        );

        // Verify WWW-Authenticate header
        let www_auth = response.headers().get("www-authenticate");
        assert!(
            www_auth.is_some(),
            "Method {method:?} should return WWW-Authenticate header"
        );
    }
}

#[tokio::test]
async fn layer_application_order() {
    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    // Test various endpoints to understand OAuth layer behavior

    // MCP endpoint should require auth
    let mcp_response = server.client.get("/mcp").await;
    assert_eq!(mcp_response.status(), 401);

    // Metadata endpoint should be public
    let metadata_response = server.client.get("/.well-known/oauth-protected-resource").await;
    assert_eq!(metadata_response.status(), 200);

    // Health endpoint should bypass auth
    let health_response = server.client.get("/health").await;
    assert_eq!(health_response.status(), 200);

    let www_auth = mcp_response.headers().get("www-authenticate");
    assert!(www_auth.is_some());

    let auth_header = www_auth.unwrap().to_str().unwrap();
    assert!(auth_header.starts_with("Bearer "));
    assert!(auth_header.contains("resource_metadata="));
}
