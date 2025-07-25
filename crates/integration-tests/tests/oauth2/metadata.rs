use indoc::indoc;
use integration_tests::TestServer;

use super::OAuthProtectedResourceMetadata;

#[tokio::test]
async fn endpoint_basic() {
    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    let response = server.client.get("/.well-known/oauth-protected-resource").await;

    assert_eq!(response.status(), 200);
    assert_eq!(response.headers().get("content-type").unwrap(), "application/json");

    let metadata: OAuthProtectedResourceMetadata = response.json().await.unwrap();

    insta::assert_json_snapshot!(metadata, @r#"
    {
      "resource": "http://127.0.0.1:8080/",
      "authorization_servers": [
        "http://127.0.0.1:4444/"
      ]
    }
    "#);
}

#[tokio::test]
async fn multiple_auth_servers() {
    let config = super::oauth_config_multiple_auth_servers();
    let server = TestServer::builder().build(config).await;

    let response = server.client.get("/.well-known/oauth-protected-resource").await;
    assert_eq!(response.status(), 200);

    let metadata: OAuthProtectedResourceMetadata = response.json().await.unwrap();

    insta::assert_json_snapshot!(metadata, @r#"
    {
      "resource": "http://127.0.0.1:8080/",
      "authorization_servers": [
        "http://127.0.0.1:4444/",
        "http://127.0.0.1:4454/",
        "https://auth.example.com/"
      ]
    }
    "#);
}

#[tokio::test]
async fn without_scopes() {
    let config = super::oauth_config_without_scopes();
    let server = TestServer::builder().build(config).await;

    let response = server.client.get("/.well-known/oauth-protected-resource").await;
    assert_eq!(response.status(), 200);

    let metadata: OAuthProtectedResourceMetadata = response.json().await.unwrap();

    insta::assert_json_snapshot!(metadata, @r#"
    {
      "resource": "http://127.0.0.1:8080/",
      "authorization_servers": [
        "http://127.0.0.1:4444/"
      ]
    }
    "#);
}

#[tokio::test]
async fn complex_scopes() {
    let config = super::oauth_config_complex_scopes();
    let server = TestServer::builder().build(config).await;

    let response = server.client.get("/.well-known/oauth-protected-resource").await;
    assert_eq!(response.status(), 200);

    let metadata: OAuthProtectedResourceMetadata = response.json().await.unwrap();

    insta::assert_json_snapshot!(metadata, @r#"
    {
      "resource": "https://api.example.com/",
      "authorization_servers": [
        "http://127.0.0.1:4444/"
      ]
    }
    "#);
}

#[tokio::test]
async fn endpoint_not_found_without_oauth() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let server = TestServer::builder().build(config).await;

    let response = server.client.get("/.well-known/oauth-protected-resource").await;
    assert_eq!(response.status(), 404);
}

#[tokio::test]
async fn endpoint_public_access() {
    let config = indoc! {r#"
        [server.oauth]
        url = "http://127.0.0.1:4444/.well-known/jwks.json"

        [server.oauth.protected_resource]
        resource = "http://127.0.0.1:8080"
        authorization_servers = ["http://127.0.0.1:4444"]

        [server.cors]
        allow_origins = "*"
        allow_methods = ["GET", "POST", "OPTIONS"]
        allow_headers = ["Authorization", "Content-Type"]

        [mcp]
        enabled = true
    "#};

    let server = TestServer::builder().build(config).await;

    // Test that the OAuth metadata endpoint is publicly accessible
    // This endpoint should work without CORS restrictions as per OAuth2 spec
    let response = server
        .client
        .request(reqwest::Method::GET, "/.well-known/oauth-protected-resource")
        .header("Origin", "https://example.com")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    assert_eq!(response.headers().get("content-type").unwrap(), "application/json");

    let metadata: OAuthProtectedResourceMetadata = response.json().await.unwrap();
    assert_eq!(metadata.resource, "http://127.0.0.1:8080/");
    assert_eq!(metadata.authorization_servers.len(), 1);
}

#[tokio::test]
async fn endpoint_with_tls() {
    let config = indoc! {r#"
        [server]
        [server.tls]
        certificate = "test-certs/cert.pem"
        key = "test-certs/key.pem"

        [server.oauth]
        url = "https://127.0.0.1:4444/.well-known/jwks.json"

        [server.oauth.protected_resource]
        resource = "https://127.0.0.1:8080"
        authorization_servers = ["https://127.0.0.1:4444"]

        [mcp]
        enabled = true
    "#};

    let server = TestServer::builder().build(config).await;

    let response = server.client.get("/.well-known/oauth-protected-resource").await;
    assert_eq!(response.status(), 200);

    let metadata: OAuthProtectedResourceMetadata = response.json().await.unwrap();

    insta::assert_json_snapshot!(metadata, @r#"
    {
      "resource": "https://127.0.0.1:8080/",
      "authorization_servers": [
        "https://127.0.0.1:4444/"
      ]
    }
    "#);
}

#[tokio::test]
async fn endpoint_different_paths() {
    let config = indoc! {r#"
        [server.oauth]
        url = "http://127.0.0.1:4444/.well-known/jwks.json"

        [server.oauth.protected_resource]
        resource = "http://127.0.0.1:8080"
        authorization_servers = ["http://127.0.0.1:4444"]

        [mcp]
        enabled = true
        path = "/custom-mcp"
    "#};

    let server = TestServer::builder().build(config).await;

    // OAuth metadata endpoint should always be at the standard path
    let response = server.client.get("/.well-known/oauth-protected-resource").await;
    assert_eq!(response.status(), 200);

    let metadata: OAuthProtectedResourceMetadata = response.json().await.unwrap();
    assert_eq!(metadata.resource, "http://127.0.0.1:8080/");

    // Verify the custom MCP path works too (but skip MCP client test to avoid OAuth interference)
    let mcp_response = server.client.get("/custom-mcp").await;
    // Should get 401 because OAuth is enabled but no token provided
    assert_eq!(mcp_response.status(), 401);
}
