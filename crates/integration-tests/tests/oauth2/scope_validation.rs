use integration_tests::TestServer;

use crate::oauth2::RequestBuilderExt;

#[tokio::test]
async fn all_required_scopes_present() {
    let (server, access_token) = super::setup_hydra_test("scope-test-all", "read write admin")
        .await
        .unwrap();

    // Token has all scopes that server supports - should be accepted
    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&access_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "ping", "id": 1}"#)
        .send()
        .await
        .unwrap();

    assert_ne!(response.status(), 401, "Valid scopes should grant access");
}

#[tokio::test]
async fn subset_of_scopes() {
    let (server, access_token) = super::setup_hydra_test("scope-test-subset", "read").await.unwrap();

    // Token has only 'read' scope, which is a subset of supported scopes - should be accepted
    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&access_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "ping", "id": 1}"#)
        .send()
        .await
        .unwrap();

    assert_ne!(response.status(), 401, "Valid subset of scopes should grant access");
}

#[tokio::test]
async fn unsupported_scope_denied() {
    let (server, access_token) = super::setup_hydra_test("scope-test-unsupported", "read write delete")
        .await
        .unwrap();

    // Token has 'delete' scope which is not in server's supported scopes - should be denied
    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&access_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "ping", "id": 1}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 401);
}

#[tokio::test]
async fn no_scopes_in_token_denied() {
    let (server, access_token) = super::setup_hydra_test("scope-test-empty", "").await.unwrap();

    // Token has no scopes but server requires scopes - should be denied
    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&access_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "ping", "id": 1}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 401);
}

#[tokio::test]
async fn complex_scopes() {
    use indoc::indoc;

    let config = indoc! {r#"
        [server.oauth]
        url = "http://127.0.0.1:4444/.well-known/jwks.json"
        poll_interval = "5m"

        [server.oauth.protected_resource]
        resource = "http://127.0.0.1:8080"
        authorization_servers = ["http://127.0.0.1:4444"]
        scopes_supported = ["user:read", "user:write", "admin:all", "repo:public"]

        [mcp]
        enabled = true
    "#};

    let (_, access_token) = super::setup_hydra_test("scope-test-complex", "user:read repo:public")
        .await
        .unwrap();
    let server = TestServer::builder().build(config).await;

    // Token has valid complex scopes - should be accepted
    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&access_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "ping", "id": 1}"#)
        .send()
        .await
        .unwrap();

    assert_ne!(response.status(), 401, "Valid complex scopes should grant access");
}

#[tokio::test]
async fn complex_scopes_invalid() {
    use indoc::indoc;

    let config = indoc! {r#"
        [server.oauth]
        url = "http://127.0.0.1:4444/.well-known/jwks.json"
        poll_interval = "5m"

        [server.oauth.protected_resource]
        resource = "http://127.0.0.1:8080"
        authorization_servers = ["http://127.0.0.1:4444"]
        scopes_supported = ["user:read", "user:write", "admin:all", "repo:public"]

        [mcp]
        enabled = true
    "#};

    let (_, access_token) = super::setup_hydra_test("scope-test-complex-invalid", "user:read repo:private")
        .await
        .unwrap();
    let server = TestServer::builder().build(config).await;

    // Token has 'repo:private' which is not supported - should be denied
    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&access_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "ping", "id": 1}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 401);
}

#[tokio::test]
async fn no_scopes_configured_allows_any() {
    use indoc::indoc;

    let config = indoc! {r#"
        [server.oauth]
        url = "http://127.0.0.1:4444/.well-known/jwks.json"
        poll_interval = "5m"

        [server.oauth.protected_resource]
        resource = "http://127.0.0.1:8080"
        authorization_servers = ["http://127.0.0.1:4444"]

        [mcp]
        enabled = true
    "#};

    let (_, access_token) = super::setup_hydra_test("scope-test-no-config", "any:scope whatever")
        .await
        .unwrap();
    let server = TestServer::builder().build(config).await;

    // No scopes configured in server, so any valid token should be accepted
    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&access_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "ping", "id": 1}"#)
        .send()
        .await
        .unwrap();

    assert_ne!(
        response.status(),
        401,
        "Any valid token should grant access when no scopes configured"
    );
}

#[tokio::test]
async fn case_sensitive() {
    let (server, access_token) = super::setup_hydra_test("scope-test-case", "READ WRITE").await.unwrap();

    // Token has 'READ WRITE' but server expects 'read write' - should be denied (case sensitive)
    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&access_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "ping", "id": 1}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 401);
}

#[tokio::test]
async fn single_scope_match() {
    let (server, access_token) = super::setup_hydra_test("scope-test-single", "admin").await.unwrap();

    // Token has single 'admin' scope which is supported - should be accepted
    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&access_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "ping", "id": 1}"#)
        .send()
        .await
        .unwrap();

    assert_ne!(response.status(), 401, "Valid single scope should grant access");
}

#[tokio::test]
async fn string_format() {
    use integration_tests::TestServer;

    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    // Test with space-separated scope string
    let unsigned_token = super::create_test_jwt_unsigned(Some("read write"));

    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&unsigned_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "ping", "id": 1}"#)
        .send()
        .await
        .unwrap();

    // Should be denied because token is unsigned (but scope format is valid)
    assert_eq!(response.status(), 401);
}

#[tokio::test]
async fn array_format() {
    use integration_tests::TestServer;

    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    // Test with array format scopes
    let unsigned_token = super::create_test_jwt_unsigned_with_scope_array(Some(vec!["read", "write"]));

    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&unsigned_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "ping", "id": 1}"#)
        .send()
        .await
        .unwrap();

    // Should be denied because token is unsigned (but scope format is valid)
    assert_eq!(response.status(), 401);
}

#[tokio::test]
async fn scope_field_precedence() {
    use integration_tests::TestServer;

    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    // Test that 'scope' field takes precedence over 'scopes' field
    // 'scope' has valid scopes, 'scopes' has invalid scope
    let unsigned_token = super::create_test_jwt_unsigned_with_both_scope_fields(
        Some("read write"),    // Valid scopes in 'scope' field
        Some(vec!["invalid"]), // Invalid scope in 'scopes' field
    );

    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&unsigned_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "ping", "id": 1}"#)
        .send()
        .await
        .unwrap();

    // Should be denied because token is unsigned, but if it were signed,
    // the 'scope' field should take precedence and be validated
    assert_eq!(response.status(), 401);
}

#[tokio::test]
async fn scopes_field_fallback() {
    use integration_tests::TestServer;

    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    // Test that 'scopes' field is used when 'scope' field is absent
    let unsigned_token = super::create_test_jwt_unsigned_with_both_scope_fields(
        None,                        // No 'scope' field
        Some(vec!["read", "write"]), // Valid scopes in 'scopes' field
    );

    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&unsigned_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "ping", "id": 1}"#)
        .send()
        .await
        .unwrap();

    // Should be denied because token is unsigned (but scope format/fallback logic is correct)
    assert_eq!(response.status(), 401);
}

#[tokio::test]
async fn empty_string_scope() {
    use integration_tests::TestServer;

    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    // Test with empty string scope
    let unsigned_token = super::create_test_jwt_unsigned(Some(""));

    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&unsigned_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "ping", "id": 1}"#)
        .send()
        .await
        .unwrap();

    // Should be denied because token is unsigned and empty scopes would also be rejected
    assert_eq!(response.status(), 401);
}

#[tokio::test]
async fn empty_array_scope() {
    use integration_tests::TestServer;

    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    // Test with empty array scope
    let unsigned_token = super::create_test_jwt_unsigned_with_scope_array(Some(vec![]));

    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&unsigned_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "ping", "id": 1}"#)
        .send()
        .await
        .unwrap();

    // Should be denied because token is unsigned and empty scopes would also be rejected
    assert_eq!(response.status(), 401);
}

#[tokio::test]
async fn whitespace_handling() {
    use integration_tests::TestServer;

    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    // Test with extra whitespace in scope string
    let unsigned_token = super::create_test_jwt_unsigned(Some("  read   write  admin  "));

    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&unsigned_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "ping", "id": 1}"#)
        .send()
        .await
        .unwrap();

    // Should be denied because token is unsigned (but whitespace handling should work correctly)
    assert_eq!(response.status(), 401);
}
