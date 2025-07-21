use indoc::indoc;
use integration_tests::TestServer;
use reqwest::{Method, StatusCode};

#[tokio::test]
async fn disabled_by_default() {
    let config = indoc! {r#"
        [server.health]
        enabled = true

        [mcp]
        enabled = true
    "#};

    let server = TestServer::builder().build(config).await;

    let response = server.client.request(Method::GET, "/health").send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = server
        .client
        .request(Method::OPTIONS, "/mcp")
        .header("Origin", "https://example.com")
        .header("Access-Control-Request-Method", "POST")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn enabled_blocks_requests_without_header() {
    let config = indoc! {r#"
        [server.csrf]
        enabled = true

        [server.health]
        enabled = true

        [mcp]
        enabled = true
    "#};

    let server = TestServer::builder().build(config).await;

    let response = server.client.request(Method::GET, "/health").send().await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = server.client.request(Method::POST, "/health").send().await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn enabled_allows_requests_with_header() {
    let config = indoc! {r#"
        [server.csrf]
        enabled = true

        [server.health]
        enabled = true

        [mcp]
        enabled = true
    "#};

    let server = TestServer::builder().build(config).await;

    let response = server
        .client
        .request(Method::GET, "/health")
        .header("X-Nexus-CSRF-Protection", "1")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let response = server
        .client
        .request(Method::POST, "/health")
        .header("X-Nexus-CSRF-Protection", "1")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn enabled_allows_options_requests() {
    let config = indoc! {r#"
        [server.csrf]
        enabled = true

        [mcp]
        enabled = true
    "#};

    let server = TestServer::builder().build(config).await;

    let response = server
        .client
        .request(Method::OPTIONS, "/mcp")
        .header("Origin", "https://example.com")
        .header("Access-Control-Request-Method", "POST")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn custom_header_name() {
    let config = indoc! {r#"
        [server.csrf]
        enabled = true
        header_name = "X-Custom-CSRF-Token"

        [server.health]
        enabled = true

        [mcp]
        enabled = true
    "#};

    let server = TestServer::builder().build(config).await;

    let response = server
        .client
        .request(Method::GET, "/health")
        .header("X-Nexus-CSRF-Protection", "1")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = server
        .client
        .request(Method::GET, "/health")
        .header("X-Custom-CSRF-Token", "1")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn header_value_doesnt_matter() {
    let config = indoc! {r#"
        [server.csrf]
        enabled = true

        [server.health]
        enabled = true

        [mcp]
        enabled = true
    "#};

    let server = TestServer::builder().build(config).await;

    let response = server
        .client
        .request(Method::GET, "/health")
        .header("X-Nexus-CSRF-Protection", "any-value")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let response = server
        .client
        .request(Method::GET, "/health")
        .header("X-Nexus-CSRF-Protection", "")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn works_with_cors() {
    let config = indoc! {r#"
        [server.cors]
        allow_origins = ["https://example.com"]
        allow_methods = ["GET", "POST"]
        allow_headers = ["content-type", "x-nexus-csrf-protection"]

        [server.csrf]
        enabled = true

        [server.health]
        enabled = true

        [mcp]
        enabled = true
    "#};

    let server = TestServer::builder().build(config).await;

    let response = server
        .client
        .request(Method::OPTIONS, "/mcp")
        .header("Origin", "https://example.com")
        .header("Access-Control-Request-Method", "POST")
        .header("Access-Control-Request-Headers", "content-type,x-nexus-csrf-protection")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    assert_eq!(
        response.headers().get("access-control-allow-origin").unwrap(),
        "https://example.com"
    );

    let response = server
        .client
        .request(Method::GET, "/health")
        .header("Origin", "https://example.com")
        .header("X-Nexus-CSRF-Protection", "1")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let response = server
        .client
        .request(Method::GET, "/health")
        .header("Origin", "https://example.com")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn applies_to_all_endpoints() {
    let config = indoc! {r#"
        [server.csrf]
        enabled = true

        [server.health]
        enabled = true
        path = "/custom-health"

        [mcp]
        enabled = true
    "#};

    let server = TestServer::builder().build(config).await;

    let response = server
        .client
        .request(Method::GET, "/custom-health")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = server
        .client
        .request(Method::GET, "/custom-health")
        .header("X-Nexus-CSRF-Protection", "1")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let response = server.client.request(Method::GET, "/").send().await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = server.client.request(Method::GET, "/unknown").send().await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = server
        .client
        .request(Method::GET, "/unknown")
        .header("X-Nexus-CSRF-Protection", "1")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn different_http_methods() {
    let config = indoc! {r#"
        [server.csrf]
        enabled = true

        [server.health]
        enabled = true

        [mcp]
        enabled = true
    "#};

    let server = TestServer::builder().build(config).await;

    for method in &[Method::GET, Method::POST, Method::PUT, Method::DELETE, Method::PATCH] {
        let response = server.client.request(method.clone(), "/health").send().await.unwrap();

        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "Method {method} should be blocked without CSRF header",
        );

        let response = server
            .client
            .request(method.clone(), "/health")
            .header("X-Nexus-CSRF-Protection", "1")
            .send()
            .await
            .unwrap();

        assert_ne!(
            response.status(),
            StatusCode::FORBIDDEN,
            "Method {method} should not be blocked with CSRF header",
        );
    }
}

#[tokio::test]
async fn case_sensitive_header() {
    let config = indoc! {r#"
        [server.csrf]
        enabled = true
        header_name = "X-CSRF-Token"

        [server.health]
        enabled = true
    "#};

    let server = TestServer::builder().build(config).await;

    for header_variant in &["X-CSRF-Token", "x-csrf-token", "X-Csrf-Token", "X-CSRF-TOKEN"] {
        let response = server
            .client
            .request(Method::GET, "/health")
            .header(*header_variant, "1")
            .send()
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Header variant {header_variant} should work",
        );
    }
}

#[tokio::test]
async fn blocks_mcp_protocol() {
    let config = indoc! {r#"
        [server.csrf]
        enabled = true

        [mcp]
        enabled = true
    "#};

    let server = TestServer::builder().build(config).await;

    let response = server
        .client
        .request(Method::GET, "/mcp")
        .header("Accept", "text/event-stream")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = server
        .client
        .request(Method::GET, "/mcp")
        .header("Accept", "text/event-stream")
        .header("X-Nexus-CSRF-Protection", "1")
        .send()
        .await
        .unwrap();

    assert_ne!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn disabled_allows_mcp_protocol() {
    let config = indoc! {r#"
        [server.csrf]
        enabled = false

        [mcp]
        enabled = true
    "#};

    let server = TestServer::builder().build(config).await;

    let mcp_client = server.mcp_client("/mcp").await;

    let server_info = mcp_client.get_server_info();
    assert_eq!(server_info.protocol_version.to_string(), "2025-03-26");

    let tools = mcp_client.list_tools().await;
    let _ = tools.tools.len();

    mcp_client.disconnect().await;
}
