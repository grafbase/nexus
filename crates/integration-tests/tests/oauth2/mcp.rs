use super::{HydraClient, RequestBuilderExt};
use integration_tests::TestServer;
use serde_json::json;

#[tokio::test]
async fn initialization_requires_oauth() {
    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "initialize", "id": 1, "params": {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "test", "version": "1.0"}}}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        401,
        "MCP initialization should require authentication"
    );
}

#[tokio::test]
async fn initialization_with_valid_oauth() {
    let (server, access_token) = super::setup_hydra_test().await.unwrap();

    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&access_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "initialize", "id": 1, "params": {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "test", "version": "1.0"}}}"#)
        .send()
        .await
        .unwrap();

    let status = response.status();
    assert_ne!(status, 401, "MCP initialization should work with valid OAuth token");

    let response_text = response.text().await.unwrap();
    assert!(response_text.contains("jsonrpc"), "Should get JSON-RPC response");
}

#[tokio::test]
async fn tools_list_with_oauth() {
    let (server, access_token) = super::setup_hydra_test().await.unwrap();

    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&access_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "tools/list", "id": 1}"#)
        .send()
        .await
        .unwrap();

    assert_ne!(response.status(), 401, "tools/list should work with valid OAuth token");

    let response_text = response.text().await.unwrap();
    assert!(response_text.contains("jsonrpc"), "Should get JSON-RPC response");
}

#[tokio::test]
async fn tools_list_denied_without_oauth() {
    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "tools/list", "id": 1}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 401, "tools/list should require authentication");
}

#[tokio::test]
async fn tools_call_with_oauth() {
    let (server, access_token) = super::setup_hydra_test().await.unwrap();

    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&access_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "tools/call", "id": 1, "params": {"name": "search", "arguments": {"keywords": ["test"]}}}"#)
        .send()
        .await
        .unwrap();

    assert_ne!(response.status(), 401, "tools/call should work with valid OAuth token");

    let response_text = response.text().await.unwrap();
    assert!(response_text.contains("jsonrpc"), "Should get JSON-RPC response");
}

#[tokio::test]
async fn tools_call_denied_without_oauth() {
    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "tools/call", "id": 1, "params": {"name": "search", "arguments": {"keywords": ["test"]}}}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 401, "tools/call should require authentication");
}

#[tokio::test]
async fn access_denied_expired_token() {
    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    let expired_token = super::create_expired_jwt();

    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&expired_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "tools/list", "id": 1}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 401, "Should deny access with expired token");
}

#[tokio::test]
async fn access_denied_malformed_token() {
    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    let malformed_token = "not.a.jwt";

    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(malformed_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "tools/list", "id": 1}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 401, "Should deny access with malformed token");
}

#[tokio::test]
async fn options_request_with_oauth() {
    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    let response = server
        .client
        .request(reqwest::Method::OPTIONS, "/mcp")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 401, "OPTIONS requires auth on MCP endpoint");

    let (server, access_token) = super::setup_hydra_test().await.unwrap();

    let auth_response = server
        .client
        .request(reqwest::Method::OPTIONS, "/mcp")
        .authorization(&access_token)
        .send()
        .await
        .unwrap();

    assert_ne!(auth_response.status(), 401, "OPTIONS should work with auth");
}

#[tokio::test]
async fn different_http_methods_oauth() {
    let (server, access_token) = super::setup_hydra_test().await.unwrap();

    let post_response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&access_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "tools/list", "id": 1}"#)
        .send()
        .await
        .unwrap();

    assert_ne!(post_response.status(), 401, "POST should work with auth");

    let get_response = server
        .client
        .request(reqwest::Method::GET, "/mcp")
        .authorization(&access_token)
        .send()
        .await
        .unwrap();

    assert_ne!(
        get_response.status(),
        401,
        "GET should not be unauthorized with valid token"
    );

    let put_response = server
        .client
        .request(reqwest::Method::PUT, "/mcp")
        .authorization(&access_token)
        .send()
        .await
        .unwrap();

    assert_ne!(
        put_response.status(),
        401,
        "PUT should not be unauthorized (auth works)"
    );
}

#[tokio::test]
async fn error_responses_require_oauth() {
    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .mcp_json(r#"{"invalid": "json-rpc"}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        401,
        "Even invalid requests should require auth first"
    );
}

#[tokio::test]
async fn concurrent_requests_with_oauth() {
    let (server, access_token) = super::setup_hydra_test().await.unwrap();

    let futures = (0..5).map(|i| {
        let server = &server;
        let access_token = &access_token;
        async move {
            server
                .client
                .request(reqwest::Method::POST, "/mcp")
                .authorization(access_token)
                .mcp_json(&format!(r#"{{"jsonrpc": "2.0", "method": "tools/list", "id": {i}}}"#))
                .send()
                .await
                .unwrap()
        }
    });

    let responses = futures_util::future::join_all(futures).await;

    for (i, response) in responses.into_iter().enumerate() {
        assert_ne!(response.status(), 401, "Request {i} should be authorized");
        assert_ne!(response.status(), 403, "Request {i} should not be forbidden");
    }
}

#[tokio::test]
async fn multiple_oauth_tokens() {
    let config = super::oauth_config_basic();
    let server = TestServer::builder().build(config).await;

    let hydra = HydraClient::new(4444, 4445);
    hydra.wait_for_hydra().await.unwrap();

    let mut tokens = Vec::new();

    // Use universal client to generate multiple tokens
    let client_id = "shared-test-client-universal";
    let client_secret = format!("{client_id}-secret");

    for _i in 0..3 {
        let token_response = hydra.get_token(client_id, &client_secret).await.unwrap();
        tokens.push(token_response.access_token);
    }

    for (i, token) in tokens.iter().enumerate() {
        let response = server
            .client
            .request(reqwest::Method::POST, "/mcp")
            .authorization(token)
            .mcp_json(&format!(r#"{{"jsonrpc": "2.0", "method": "tools/list", "id": {i}}}"#))
            .send()
            .await
            .unwrap();

        assert_ne!(response.status(), 401, "Token {i} should be authorized");
        assert_ne!(response.status(), 403, "Token {i} should not be forbidden");
    }
}

#[tokio::test]
async fn ping_method_with_oauth() {
    let (server, access_token) = super::setup_hydra_test().await.unwrap();

    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&access_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "ping", "id": 1}"#)
        .send()
        .await
        .unwrap();

    assert_ne!(response.status(), 401, "ping should work with valid OAuth token");

    let response_text = response.text().await.unwrap();
    assert!(response_text.contains("jsonrpc"), "Should get JSON-RPC response");
}

#[tokio::test]
async fn notifications_with_oauth() {
    let (server, access_token) = super::setup_hydra_test().await.unwrap();

    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&access_token)
        .mcp_json(
            r#"{"jsonrpc": "2.0", "method": "notifications/message", "params": {"level": "info", "text": "test"}}"#,
        )
        .send()
        .await
        .unwrap();

    assert_ne!(
        response.status(),
        401,
        "notifications should work with valid OAuth token"
    );
}

#[tokio::test]
async fn batch_requests_with_oauth() {
    let (server, access_token) = super::setup_hydra_test().await.unwrap();

    let batch_request = json!([
        {"jsonrpc": "2.0", "method": "tools/list", "id": 1},
        {"jsonrpc": "2.0", "method": "ping", "id": 2}
    ]);

    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&access_token)
        .mcp_json(&batch_request.to_string())
        .send()
        .await
        .unwrap();

    assert_ne!(
        response.status(),
        401,
        "batch requests should work with valid OAuth token"
    );

    // The important part for OAuth2 testing is that authentication worked
    // We don't need to validate the actual batch processing
}
