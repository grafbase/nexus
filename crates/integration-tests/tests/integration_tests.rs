use indoc::indoc;
use integration_tests::*;

#[tokio::test]
async fn default_path() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let server = TestServer::start(config).await;

    let response = server.client.get("/mcp").await;
    assert_eq!(response.status(), 200);

    let body = response.text().await.unwrap();
    insta::assert_snapshot!(body, @"<h1>Hello, World!</h1>");
}

#[tokio::test]
async fn custom_path() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
        path = "/custom"
    "#};

    let server = TestServer::start(config).await;

    let response = server.client.get("/custom").await;
    assert_eq!(response.status(), 200);

    let body = response.text().await.unwrap();
    insta::assert_snapshot!(body, @"<h1>Hello, World!</h1>");
}

#[tokio::test]
async fn successful_tls_connection() {
    let config = indoc! {r#"
        [server]
        [server.tls]
        certificate = "certs/cert.pem"
        key = "certs/key.pem"

        [mcp]
        enabled = true
    "#};

    let server = TestServer::start(config).await;

    let response = server.client.get("/mcp").await;
    assert_eq!(response.status(), 200);

    let body = response.text().await.unwrap();
    insta::assert_snapshot!(body, @"<h1>Hello, World!</h1>");
}

#[tokio::test]
async fn health_endpoint_enabled() {
    let config = indoc! {r#"
        [server]
        [server.health]
        enabled = true

        [mcp]
        enabled = true
    "#};

    let server = TestServer::start(config).await;

    let response = server.client.get("/health").await;
    assert_eq!(response.status(), 200);

    let body = response.text().await.unwrap();
    insta::assert_snapshot!(body, @r#"{"status":"healthy"}"#);
}

#[tokio::test]
async fn health_endpoint_disabled() {
    let config = indoc! {r#"
        [server]
        [server.health]
        enabled = false

        [mcp]
        enabled = true
    "#};

    let server = TestServer::start(config).await;

    let response = server.client.get("/health").await;
    assert_eq!(response.status(), 404);
}

#[tokio::test]
async fn health_endpoint_custom_path() {
    let config = indoc! {r#"
        [server]
        [server.health]
        enabled = true
        path = "/status"

        [mcp]
        enabled = true
    "#};

    let server = TestServer::start(config).await;

    // Custom path should work
    let response = server.client.get("/status").await;
    assert_eq!(response.status(), 200);

    let body = response.text().await.unwrap();
    insta::assert_snapshot!(body, @r#"{"status":"healthy"}"#);

    // Default path should not work
    let response = server.client.get("/health").await;
    assert_eq!(response.status(), 404);
}

#[tokio::test]
async fn health_endpoint_with_tls() {
    let config = indoc! {r#"
        [server]
        [server.tls]
        certificate = "certs/cert.pem"
        key = "certs/key.pem"

        [server.health]
        enabled = true

        [mcp]
        enabled = true
    "#};

    let server = TestServer::start(config).await;

    let response = server.client.get("/health").await;
    assert_eq!(response.status(), 200);

    let body = response.text().await.unwrap();
    insta::assert_snapshot!(body, @r#"{"status":"healthy"}"#);
}

#[tokio::test]
async fn health_endpoint_enabled_by_default() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let server = TestServer::start(config).await;

    // Health endpoint should be enabled by default
    let response = server.client.get("/health").await;
    assert_eq!(response.status(), 200);

    let body = response.text().await.unwrap();
    insta::assert_snapshot!(body, @r#"{"status":"healthy"}"#);
}

#[tokio::test]
async fn health_endpoint_separate_listener() {
    let config = indoc! {r#"
        [server]
        [server.health]
        enabled = true
        listen = "127.0.0.1:0"

        [mcp]
        enabled = true
    "#};

    // For this test, we need to handle the separate health listener
    // The current TestServer doesn't support this, so we'll need to test it differently

    // Parse the configuration
    let config: config::Config = toml::from_str(config).unwrap();

    // Find available ports for both main server and health endpoint
    let main_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let main_addr = main_listener.local_addr().unwrap();

    let health_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let health_addr = health_listener.local_addr().unwrap();

    // Update config with the health listen address
    let mut config = config;
    config.server.health.listen = Some(health_addr);

    // Start the server
    let serve_config = server::ServeConfig {
        listen_address: main_addr,
        config,
    };

    drop(main_listener);
    drop(health_listener);

    let _handle = tokio::spawn(async move {
        let _ = server::serve(serve_config).await;
    });

    // Wait for servers to start
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let main_client = reqwest::Client::new();
    let health_client = reqwest::Client::new();

    // Test that health endpoint is NOT on the main server
    let response = main_client
        .get(format!("http://{main_addr}/health"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 404);

    // Test that health endpoint IS on the separate health listener
    let response = health_client
        .get(format!("http://{health_addr}/health"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    let body = response.text().await.unwrap();
    insta::assert_snapshot!(body, @r#"{"status":"healthy"}"#);
}
