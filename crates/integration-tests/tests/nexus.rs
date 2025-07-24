mod cors;
mod csrf;
mod oauth2;
mod sse;
mod streamable_http;
mod token_auth;
mod tools;

use indoc::indoc;
use integration_tests::TestServer;

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

#[tokio::test]
async fn server_info_with_downstream_servers() {
    use integration_tests::TestService;
    use tools::{AdderTool, CalculatorTool, FileSystemTool, TextProcessorTool};

    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    // Create multiple downstream servers with different tools
    let mut math_server = TestService::sse("math_server".to_string());
    math_server.add_tool(AdderTool).await;
    math_server.add_tool(CalculatorTool).await;

    let mut text_server = TestService::streamable_http("text_server".to_string());
    text_server.add_tool(TextProcessorTool).await;

    let mut fs_server = TestService::sse_autodetect("filesystem_server".to_string());
    fs_server.add_tool(FileSystemTool).await;

    // Build nexus server with multiple downstream servers
    let mut builder = TestServer::builder();
    builder.spawn_service(math_server).await;
    builder.spawn_service(text_server).await;
    builder.spawn_service(fs_server).await;
    let server = builder.build(config).await;

    let mcp_client = server.mcp_client("/mcp").await;

    // Test server info - this should show the aggregated nexus server information
    // including all downstream servers and their tools in the instructions
    let server_info = mcp_client.get_server_info();

    // Assert protocol version
    insta::assert_json_snapshot!(&server_info.protocol_version, @r#""2025-03-26""#);

    // Assert nexus server name shows all downstream servers
    insta::assert_snapshot!(&server_info.server_info.name, @"Tool Aggregator (filesystem_server, math_server, text_server)");

    // Assert instructions with proper formatting for readability
    insta::assert_snapshot!(&server_info.instructions.as_ref().unwrap(), @r###"
    This is an MCP server aggregator providing access to many tools through two main functions:
    `search` and `execute`.

    **Instructions:**
    1.  **Search for tools:** To find out what tools are available, use the `search` tool. Provide a
        clear description of your goal as the query. The search will return a list of relevant tools,
        including their exact names and required parameters.
    2.  **Execute a tool:** Once you have found a suitable tool using `search`, call the `execute` tool.
        You must provide the `name` of the tool and its `parameters` exactly as specified in the search results.

    Always use the `search` tool first to discover available tools. Do not guess tool names.

    **Available Servers and Tools:**

    **filesystem_server:**
    - `filesystem_server__filesystem`: Manages files and directories with operations like listing, creating, and deleting

    **math_server:**
    - `math_server__adder`: Adds two numbers together
    - `math_server__calculator`: Performs basic mathematical calculations including addition, subtraction, multiplication and division

    **text_server:**
    - `text_server__text_processor`: Processes text with various string manipulation operations like case conversion and reversal

    **Note:** When executing tools, use the full name format `server__tool` as shown above.
    "###);

    mcp_client.disconnect().await;
}
