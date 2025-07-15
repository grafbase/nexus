use indoc::indoc;
use integration_tests::*;
use serde_json::json;

#[tokio::test]
async fn mcp_server_info() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let server = TestServer::start(config).await;
    let mcp_client = server.mcp_client("/mcp").await;

    let server_info = mcp_client.get_server_info();

    // Verify basic server info
    assert_eq!(server_info.protocol_version, rmcp::model::ProtocolVersion::V_2024_11_05);
    assert!(server_info.capabilities.tools.is_some());

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn mcp_list_tools() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let server = TestServer::start(config).await;
    let mcp_client = server.mcp_client("/mcp").await;

    let tools_result = mcp_client.list_tools().await;

    insta::assert_json_snapshot!(&tools_result, @r#"
    {
      "tools": [
        {
          "name": "adder",
          "description": "adds a and b together",
          "inputSchema": {
            "$schema": "http://json-schema.org/draft-07/schema#",
            "title": "Request",
            "type": "object",
            "required": [
              "a",
              "b"
            ],
            "properties": {
              "a": {
                "type": "integer",
                "format": "int32"
              },
              "b": {
                "type": "integer",
                "format": "int32"
              }
            }
          },
          "annotations": {
            "readOnlyHint": true,
            "destructiveHint": false,
            "idempotentHint": true,
            "openWorldHint": false
          }
        }
      ]
    }
    "#);

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn mcp_call_adder_tool() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let server = TestServer::start(config).await;
    let mcp_client = server.mcp_client("/mcp").await;

    // Call the adder tool with test values
    let result = mcp_client.call_tool("adder", json!({ "a": 5, "b": 3 })).await;

    insta::assert_json_snapshot!(result, @r#"
    {
      "content": [
        {
          "type": "text",
          "text": "5 + 3 = 8"
        }
      ],
      "isError": false
    }
    "#);

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn mcp_call_nonexistent_tool() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let server = TestServer::start(config).await;
    let mcp_client = server.mcp_client("/mcp").await;

    // Try to call a tool that doesn't exist
    let error = mcp_client.call_tool_expect_error("nonexistent", json!({})).await;

    insta::assert_snapshot!(error.to_string(), @"Mcp error: -32602: Unknown tool 'nonexistent'");

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn mcp_custom_path() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
        path = "/custom-mcp"
    "#};

    let server = TestServer::start(config).await;
    let mcp_client = server.mcp_client("/custom-mcp").await;

    // Should be able to connect and list tools on custom path
    let tools_result = mcp_client.list_tools().await;
    assert_eq!(tools_result.tools.len(), 1);
    assert_eq!(tools_result.tools[0].name, "adder");

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn mcp_with_tls() {
    let config = indoc! {r#"
        [server]
        [server.tls]
        certificate = "certs/cert.pem"
        key = "certs/key.pem"

        [mcp]
        enabled = true
    "#};

    let server = TestServer::start(config).await;
    let mcp_client = server.mcp_client("/mcp").await;

    // Should work over TLS
    let tools_result = mcp_client.list_tools().await;
    assert_eq!(tools_result.tools.len(), 1);

    // Test calling the tool over TLS
    let result = mcp_client.call_tool("adder", json!({ "a": 7, "b": 2 })).await;

    assert!(result.is_error != Some(true));
    // Check if the content contains the expected text
    let content_text = format!("{:?}", result.content[0]);
    assert!(content_text.contains("7 + 2 = 9"));

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn health_endpoint_still_works() {
    let config = indoc! {r#"
        [server]
        [server.health]
        enabled = true

        [mcp]
        enabled = true
    "#};

    let server = TestServer::start(config).await;

    // Health endpoint should still work alongside MCP
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
