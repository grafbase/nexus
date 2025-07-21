use indoc::indoc;
use integration_tests::TestServer;
use reqwest::Method;

#[tokio::test]
async fn default_behavior() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let server = TestServer::builder().build(config).await;

    let response = server
        .client
        .request(Method::OPTIONS, "/mcp")
        .header("Origin", "https://example.com")
        .header("Access-Control-Request-Method", "POST")
        .header("Access-Control-Request-Headers", "content-type")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let headers = response.headers();
    assert!(headers.contains_key("access-control-allow-methods"));
}

#[tokio::test]
async fn allow_origins_any() {
    let config = indoc! {r#"
        [server.cors]
        allow_origins = "*"

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

    assert_eq!(response.status(), 200);

    let headers = response.headers();
    assert_eq!(headers.get("access-control-allow-origin").unwrap(), "*");
}

#[tokio::test]
async fn allow_origins_explicit() {
    let config = indoc! {r#"
        [server.cors]
        allow_origins = ["https://allowed.com", "https://also-allowed.com"]

        [mcp]
        enabled = true
    "#};

    let server = TestServer::builder().build(config).await;

    let response = server
        .client
        .request(Method::OPTIONS, "/mcp")
        .header("Origin", "https://allowed.com")
        .header("Access-Control-Request-Method", "POST")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let headers = response.headers();
    assert_eq!(
        headers.get("access-control-allow-origin").unwrap(),
        "https://allowed.com"
    );

    let response = server
        .client
        .request(Method::OPTIONS, "/mcp")
        .header("Origin", "https://not-allowed.com")
        .header("Access-Control-Request-Method", "POST")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let headers = response.headers();
    assert!(headers.get("access-control-allow-origin").is_none());
}

#[tokio::test]
async fn allow_methods_explicit() {
    let config = indoc! {r#"
        [server.cors]
        allow_methods = ["GET", "POST"]

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

    assert_eq!(response.status(), 200);

    let headers = response.headers();
    let allowed_methods = headers.get("access-control-allow-methods").unwrap().to_str().unwrap();
    assert!(allowed_methods.contains("POST"));
    assert!(allowed_methods.contains("GET"));
    assert!(allowed_methods.contains("OPTIONS"));
}

#[tokio::test]
async fn allow_credentials() {
    let config = indoc! {r#"
        [server.cors]
        allow_credentials = true
        allow_origins = ["https://trusted.com"]

        [mcp]
        enabled = true
    "#};

    let server = TestServer::builder().build(config).await;

    let response = server
        .client
        .request(Method::OPTIONS, "/mcp")
        .header("Origin", "https://trusted.com")
        .header("Access-Control-Request-Method", "POST")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let headers = response.headers();
    assert_eq!(headers.get("access-control-allow-credentials").unwrap(), "true");
}

#[tokio::test]
async fn max_age() {
    let config = indoc! {r#"
        [server.cors]
        max_age = "1h"

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

    assert_eq!(response.status(), 200);

    let headers = response.headers();
    assert_eq!(headers.get("access-control-max-age").unwrap(), "3600");
}

#[tokio::test]
async fn allow_headers() {
    let config = indoc! {r#"
        [server.cors]
        allow_headers = ["content-type", "authorization", "x-custom-header"]

        [mcp]
        enabled = true
    "#};

    let server = TestServer::builder().build(config).await;

    let response = server
        .client
        .request(Method::OPTIONS, "/mcp")
        .header("Origin", "https://example.com")
        .header("Access-Control-Request-Method", "POST")
        .header("Access-Control-Request-Headers", "content-type,authorization")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let headers = response.headers();
    let allowed_headers = headers.get("access-control-allow-headers").unwrap().to_str().unwrap();

    assert!(allowed_headers.contains("content-type"));
    assert!(allowed_headers.contains("authorization"));
    assert!(allowed_headers.contains("x-custom-header"));
}

#[tokio::test]
async fn expose_headers() {
    let config = indoc! {r#"
        [server.cors]
        expose_headers = ["x-custom-response", "x-rate-limit"]

        [mcp]
        enabled = true
    "#};

    let server = TestServer::builder().build(config).await;

    let response = server.client.get("/health").await;
    assert_eq!(response.status(), 200);

    if let Some(exposed_headers) = response.headers().get("access-control-expose-headers") {
        let exposed_headers = exposed_headers.to_str().unwrap();
        assert!(exposed_headers.contains("x-custom-response"));
        assert!(exposed_headers.contains("x-rate-limit"));
    }
}

#[tokio::test]
async fn private_network() {
    let config = indoc! {r#"
        [server.cors]
        allow_private_network = true

        [mcp]
        enabled = true
    "#};

    let server = TestServer::builder().build(config).await;

    let response = server
        .client
        .request(Method::OPTIONS, "/mcp")
        .header("Origin", "https://example.com")
        .header("Access-Control-Request-Method", "POST")
        .header("Access-Control-Request-Private-Network", "true")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let headers = response.headers();
    assert_eq!(headers.get("access-control-allow-private-network").unwrap(), "true");
}

#[tokio::test]
async fn comprehensive_config() {
    let config = indoc! {r#"
        [server.cors]
        allow_credentials = true
        allow_origins = ["https://app.example.com"]
        allow_methods = ["GET", "POST", "PUT", "DELETE"]
        allow_headers = ["content-type", "authorization", "x-api-key"]
        expose_headers = ["x-request-id", "x-rate-limit-remaining"]
        max_age = "2h"
        allow_private_network = true

        [mcp]
        enabled = true
    "#};

    let server = TestServer::builder().build(config).await;

    let response = server
        .client
        .request(Method::OPTIONS, "/mcp")
        .header("Origin", "https://app.example.com")
        .header("Access-Control-Request-Method", "PUT")
        .header("Access-Control-Request-Headers", "content-type,authorization")
        .header("Access-Control-Request-Private-Network", "true")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let headers = response.headers();
    assert_eq!(
        headers.get("access-control-allow-origin").unwrap(),
        "https://app.example.com"
    );
    assert_eq!(headers.get("access-control-allow-credentials").unwrap(), "true");
    assert_eq!(headers.get("access-control-max-age").unwrap(), "7200");
    assert_eq!(headers.get("access-control-allow-private-network").unwrap(), "true");

    let allowed_methods = headers.get("access-control-allow-methods").unwrap().to_str().unwrap();
    assert!(allowed_methods.contains("GET"));
    assert!(allowed_methods.contains("POST"));
    assert!(allowed_methods.contains("PUT"));
    assert!(allowed_methods.contains("DELETE"));
    assert!(allowed_methods.contains("OPTIONS"));

    let allowed_headers = headers.get("access-control-allow-headers").unwrap().to_str().unwrap();
    assert!(allowed_headers.contains("content-type"));
    assert!(allowed_headers.contains("authorization"));
    assert!(allowed_headers.contains("x-api-key"));
}

#[tokio::test]
async fn actual_cross_origin_request() {
    let config = indoc! {r#"
        [server.cors]
        allow_origins = ["https://example.com"]
        allow_methods = ["GET", "POST"]

        [mcp]
        enabled = true
    "#};

    let server = TestServer::builder().build(config).await;

    let response = server
        .client
        .request(Method::GET, "/health")
        .header("Origin", "https://example.com")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let headers = response.headers();

    assert_eq!(
        headers.get("access-control-allow-origin").unwrap(),
        "https://example.com"
    );
}
