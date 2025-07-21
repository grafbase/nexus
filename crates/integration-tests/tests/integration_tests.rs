mod cors;
mod sse;
mod streamable_http;
mod tools;

use indoc::indoc;
use integration_tests::TestServer;

#[tokio::test]
async fn mcp_server_info() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let server = TestServer::builder().build(config).await;
    let mcp_client = server.mcp_client("/mcp").await;
    let server_info = mcp_client.get_server_info();

    insta::assert_json_snapshot!(&server_info.protocol_version, @r#""2025-03-26""#);

    mcp_client.disconnect().await;
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

    let server = TestServer::builder().build(config).await;

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

    let server = TestServer::builder().build(config).await;

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

    let server = TestServer::builder().build(config).await;

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

    let server = TestServer::builder().build(config).await;

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

    let server = TestServer::builder().build(config).await;

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

#[tokio::test]
async fn no_tools_by_default() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let server = TestServer::builder().build(config).await;
    let mcp_client = server.mcp_client("/mcp").await;

    let tools_result = mcp_client.list_tools().await;

    // Should have no tools when no services are configured
    insta::assert_json_snapshot!(&tools_result, @r#"
    {
      "tools": [
        {
          "name": "search",
          "description": "Search for relevant tools. A list of matching tools with their\nscore is returned with a map of input fields and their types.\n\nUsing this information, you can call the execute tool with the\nname of the tool you want to execute, and defining the input\nparameters.\n",
          "inputSchema": {
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "title": "SearchParameters",
            "type": "object",
            "properties": {
              "keywords": {
                "description": "A list of keywords to search with.",
                "type": "array",
                "items": {
                  "type": "string"
                }
              }
            },
            "required": [
              "keywords"
            ]
          },
          "annotations": {
            "readOnlyHint": true
          }
        },
        {
          "name": "execute",
          "description": "Executes a tool with the given parameters. Before using, you must call the\nsearch function to retrieve the tools you need for your task. If you do not\nknow how to call this tool, call search first.\n\nThe tool name and parameters are specified in the request body. The tool name\nmust be a string, and the parameters must be a map of strings to JSON values.\n",
          "inputSchema": {
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "title": "ExecuteParameters",
            "description": "Parameters for executing a tool. You must call search if you have trouble finding the right arguments here.",
            "type": "object",
            "properties": {
              "name": {
                "description": "The exact name of the tool to execute. This must match the tool name returned by the search function. For example: 'calculator__adder', 'web_search__search', or 'file_reader__read'.",
                "type": "string"
              },
              "arguments": {
                "description": "The arguments to pass to the tool, as a JSON object. Each tool expects specific arguments - use the search function to discover what arguments each tool requires. For example: {\"query\": \"weather in NYC\"} or {\"x\": 5, \"y\": 10}.",
                "type": "object",
                "additionalProperties": true
              }
            },
            "required": [
              "name",
              "arguments"
            ]
          },
          "annotations": {
            "destructiveHint": true,
            "openWorldHint": true
          }
        }
      ]
    }
    "#);

    mcp_client.disconnect().await;
}
