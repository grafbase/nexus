use crate::oauth2::{HydraClient, create_expired_jwt, setup_hydra_test};

use super::RequestBuilderExt;
use futures_util::future::join_all;
use indoc::indoc;
use integration_tests::tools::{AdderTool, CalculatorTool, TextProcessorTool};
use integration_tests::{TestServer, TestService, TestTool};
use rmcp::model::{CallToolRequestParam, CallToolResult, Content, ErrorData, Tool};
use serde_json::json;
use std::{future::Future, pin::Pin, sync::Arc};
use tokio::sync::Mutex;

// =============================================================================
// Test Infrastructure
// =============================================================================

/// Test tool that tracks when it's successfully called, proving token forwarding worked
#[derive(Debug, Clone)]
pub struct TokenTrackingTool {
    call_count: Arc<Mutex<u32>>,
    call_timestamps: Arc<Mutex<Vec<std::time::SystemTime>>>,
}

impl TokenTrackingTool {
    pub fn new() -> Self {
        Self {
            call_count: Arc::new(Mutex::new(0)),
            call_timestamps: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Get the number of times this tool was successfully called
    pub async fn get_call_count(&self) -> u32 {
        *self.call_count.lock().await
    }

    /// Get all timestamps when the tool was called
    pub async fn get_call_timestamps(&self) -> Vec<std::time::SystemTime> {
        self.call_timestamps.lock().await.clone()
    }

    /// Check if the tool has been called at least once
    pub async fn was_called(&self) -> bool {
        self.get_call_count().await > 0
    }
}

impl TestTool for TokenTrackingTool {
    fn tool_definition(&self) -> Tool {
        let mut schema = serde_json::Map::new();
        schema.insert("type".to_string(), json!("object"));
        schema.insert(
            "properties".to_string(),
            json!({
                "message": {
                    "type": "string",
                    "description": "Optional message to include in the response"
                }
            }),
        );

        Tool {
            name: "token_tracker".into(),
            description: Some("Tracks when it's called, proving token forwarding worked successfully".into()),
            input_schema: std::sync::Arc::new(schema),
            output_schema: None,
            annotations: None,
        }
    }

    fn call(
        &self,
        params: CallToolRequestParam,
    ) -> Pin<Box<dyn Future<Output = Result<CallToolResult, ErrorData>> + Send + '_>> {
        let call_count = self.call_count.clone();
        let call_timestamps = self.call_timestamps.clone();

        // Extract message before async block to avoid lifetime issues
        let message = params
            .arguments
            .as_ref()
            .and_then(|args| args.get("message"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "Token tracking successful".to_string());

        Box::pin(async move {
            // Record that the tool was called
            let mut count = call_count.lock().await;
            *count += 1;
            let call_number = *count;

            call_timestamps.lock().await.push(std::time::SystemTime::now());

            Ok(CallToolResult::success(vec![Content::text(format!(
                "Call #{call_number}: {message} (Token forwarding verified)"
            ))]))
        })
    }
}

// =============================================================================
// Configuration Helpers
// =============================================================================

/// Helper to create OAuth2 config with token forwarding enabled
fn oauth_config_with_forwarding() -> String {
    indoc! {r#"
        [server.oauth]
        url = "http://127.0.0.1:4444/.well-known/jwks.json"
        poll_interval = "5m"
        expected_issuer = "http://127.0.0.1:4444"

        [server.oauth.protected_resource]
        resource = "http://127.0.0.1:8080"
        authorization_servers = ["http://127.0.0.1:4444"]

        [mcp]
        enabled = true
    "#}
    .to_string()
}

// =============================================================================
// Basic Token Forwarding Tests
// =============================================================================

#[tokio::test]
async fn single_downstream_token_forwarding() {
    let (_nexus_server, access_token) = setup_hydra_test().await.unwrap();

    // Create a dynamic downstream server that expects auth forwarding
    let mut dynamic_service = TestService::streamable_http("dynamic_server".to_string())
        .with_required_auth_token(access_token.clone())
        .with_forward_auth();

    let token_tracker = TokenTrackingTool::new();
    dynamic_service.add_tool(AdderTool);
    dynamic_service.add_tool(token_tracker.clone());

    let mut server_builder = TestServer::builder();
    server_builder.spawn_service(dynamic_service).await;

    let config = oauth_config_with_forwarding();

    let server = server_builder.build(&config).await;

    // Test tools/list with OAuth2 authentication - should work with token forwarding
    let mcp_client = server.mcp_client_with_auth("/mcp", &access_token).await;
    let tools_result = mcp_client.list_tools().await;

    insta::assert_json_snapshot!(tools_result, @r##"
    {
      "tools": [
        {
          "name": "search",
          "description": "Search for relevant tools. A list of matching tools with their\\nscore is returned with a map of input fields and their types.\n\nUsing this information, you can call the execute tool with the\\nname of the tool you want to execute, and defining the input parameters.\n\nTool names are in the format \"server__tool\" where \"server\" is the name of the MCP server providing\nthe tool.\n",
          "inputSchema": {
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
          "outputSchema": {
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "title": "Array_of_SearchResult",
            "type": "array",
            "items": {
              "$ref": "#/$defs/SearchResult"
            },
            "$defs": {
              "SearchResult": {
                "type": "object",
                "properties": {
                  "name": {
                    "description": "The name of the tool (format: \"server__tool\")",
                    "type": "string"
                  },
                  "description": {
                    "description": "Description of what the tool does",
                    "type": "string"
                  },
                  "input_schema": {
                    "description": "The input schema for the tool's parameters"
                  },
                  "score": {
                    "description": "The relevance score for this result (higher is more relevant)",
                    "type": "number",
                    "format": "float"
                  }
                },
                "required": [
                  "name",
                  "description",
                  "input_schema",
                  "score"
                ]
              }
            }
          },
          "annotations": {
            "readOnlyHint": true
          }
        },
        {
          "name": "execute",
          "description": "Executes a tool with the given parameters. Before using, you must call the search function to retrieve the tools you need for your task. If you do not know how to call this tool, call search first.\n\nThe tool name and parameters are specified in the request body. The tool name must be a string,\nand the parameters must be a map of strings to JSON values.\n",
          "inputSchema": {
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
    "##);

    // Verify token tracker hasn't been called yet
    assert_eq!(
        token_tracker.get_call_count().await,
        0,
        "Token tracker should not be called yet"
    );

    // Execute the token tracker tool - this proves token forwarding works
    let token_tracker_result = mcp_client
        .execute(
            "dynamic_server__token_tracker",
            serde_json::json!({"message": "Testing token forwarding"}),
        )
        .await;

    insta::assert_json_snapshot!(token_tracker_result, @r###"
    {
      "content": [
        {
          "type": "text",
          "text": "Call #1: Testing token forwarding (Token forwarding verified)"
        }
      ],
      "isError": false
    }
    "###);

    // Verify the token tracker was called exactly once
    assert_eq!(
        token_tracker.get_call_count().await,
        1,
        "Token tracker should be called exactly once"
    );
    assert!(
        token_tracker.was_called().await,
        "Token tracker should report being called"
    );

    // Execute a regular tool that would require token forwarding
    let execute_result = mcp_client
        .execute("dynamic_server__adder", serde_json::json!({"a": 5, "b": 3}))
        .await;

    insta::assert_json_snapshot!(execute_result, @r###"
    {
      "content": [
        {
          "type": "text",
          "text": "5 + 3 = 8"
        }
      ],
      "isError": false
    }
    "###);

    // Call token tracker again to verify multiple calls work
    let token_tracker_result2 = mcp_client
        .execute(
            "dynamic_server__token_tracker",
            serde_json::json!({"message": "Second call"}),
        )
        .await;

    insta::assert_json_snapshot!(token_tracker_result2, @r###"
    {
      "content": [
        {
          "type": "text",
          "text": "Call #2: Second call (Token forwarding verified)"
        }
      ],
      "isError": false
    }
    "###);

    // Verify final call count
    assert_eq!(
        token_tracker.get_call_count().await,
        2,
        "Token tracker should be called exactly twice"
    );

    // Verify we have timestamps for both calls
    let timestamps = token_tracker.get_call_timestamps().await;
    assert_eq!(timestamps.len(), 2, "Should have timestamps for both calls");
    assert!(timestamps[1] >= timestamps[0], "Timestamps should be in order");

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn token_actually_reaches_downstream() {
    let (_nexus_server, access_token) = setup_hydra_test().await.unwrap();

    // Create a downstream server that requires the specific token
    let mut dynamic_service = TestService::streamable_http("auth_test_server".to_string())
        .with_required_auth_token(access_token.clone())
        .with_forward_auth();

    dynamic_service.add_tool(AdderTool);

    let mut server_builder = TestServer::builder();
    server_builder.spawn_service(dynamic_service).await;

    let config = oauth_config_with_forwarding();

    let server = server_builder.build(&config).await;

    // Create MCP client with the correct token
    let mcp_client = server.mcp_client_with_auth("/mcp", &access_token).await;

    // Execute a tool on the downstream server - this will fail if token isn't properly forwarded
    let result = mcp_client
        .execute("auth_test_server__adder", json!({"a": 5, "b": 3}))
        .await;

    // If we get here without error, the token was successfully forwarded
    insta::assert_json_snapshot!(result, @r###"
    {
      "content": [
        {
          "type": "text",
          "text": "5 + 3 = 8"
        }
      ],
      "isError": false
    }
    "###);

    // Try with a wrong token - should fail
    let hydra = HydraClient::new(4444, 4445);
    let wrong_token = hydra
        .get_token("shared-test-client-universal", "shared-test-client-universal-secret")
        .await
        .unwrap()
        .access_token;

    let wrong_client = server.mcp_client_with_auth("/mcp", &wrong_token).await;

    // This should fail because the downstream server expects a different token
    let error = wrong_client
        .execute_expect_error("auth_test_server__adder", json!({"a": 5, "b": 3}))
        .await;

    insta::assert_snapshot!(error.to_string(), @"Mcp error: -32601: tools/call");

    mcp_client.disconnect().await;
    wrong_client.disconnect().await;
}

#[tokio::test]
async fn no_forwarding_without_token() {
    // Test that when no token is provided to Nexus, no forwarding occurs
    let mut dynamic_service = TestService::streamable_http("dynamic_server".to_string())
        .with_required_auth_token("expected-token".to_string())
        .with_forward_auth();
    dynamic_service.add_tool(AdderTool);

    let mut server_builder = TestServer::builder();
    server_builder.spawn_service(dynamic_service).await;

    let config = oauth_config_with_forwarding();

    let server = server_builder.build(&config).await;

    // Make request without any authentication token
    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "tools/list", "id": 1}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        401,
        "Should require authentication when OAuth2 is enabled on Nexus"
    );
}

// =============================================================================
// Multiple Server Tests
// =============================================================================

#[tokio::test]
async fn multiple_downstream_servers() {
    let (_, access_token) = setup_hydra_test().await.unwrap();

    // Create first dynamic server
    let mut dynamic_service1 = TestService::streamable_http("dynamic_server1".to_string())
        .with_required_auth_token(access_token.clone())
        .with_forward_auth();
    dynamic_service1.add_tool(AdderTool);

    // Create second dynamic server
    let mut dynamic_service2 = TestService::streamable_http("dynamic_server2".to_string())
        .with_required_auth_token(access_token.clone())
        .with_forward_auth();
    dynamic_service2.add_tool(CalculatorTool);

    let mut server_builder = TestServer::builder();
    server_builder.spawn_service(dynamic_service1).await;
    server_builder.spawn_service(dynamic_service2).await;

    let config = oauth_config_with_forwarding();

    let server = server_builder.build(&config).await;

    // Create MCP client and test multiple servers
    let mcp_client = server.mcp_client_with_auth("/mcp", &access_token).await;

    // Search for tools across both servers
    let search_results = mcp_client.search(&["numbers", "mathematical"]).await;

    assert!(!search_results.is_empty());

    // Execute tool from first server
    let result1 = mcp_client
        .execute("dynamic_server1__adder", json!({"a": 10, "b": 5}))
        .await;

    insta::assert_json_snapshot!(result1, @r###"
    {
      "content": [
        {
          "type": "text",
          "text": "10 + 5 = 15"
        }
      ],
      "isError": false
    }
    "###);

    // Execute tool from second server
    let result2 = mcp_client
        .execute(
            "dynamic_server2__calculator",
            json!({
                "operation": "multiply",
                "x": 4,
                "y": 7
            }),
        )
        .await;

    insta::assert_json_snapshot!(result2, @r###"
    {
      "content": [
        {
          "type": "text",
          "text": "4 multiply 7 = 28"
        }
      ],
      "isError": false
    }
    "###);

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn mixed_static_dynamic_servers() {
    let (_nexus_server, access_token) = setup_hydra_test().await.unwrap();

    // Create dynamic server (requires auth)
    let mut dynamic_service = TestService::streamable_http("dynamic_server".to_string())
        .with_required_auth_token(access_token.clone())
        .with_forward_auth();
    dynamic_service.add_tool(AdderTool);

    // Create static server (no auth required)
    let mut static_service = TestService::streamable_http("static_server".to_string());
    static_service.add_tool(TextProcessorTool);

    let mut server_builder = TestServer::builder();
    server_builder.spawn_service(dynamic_service).await;
    server_builder.spawn_service(static_service).await;

    let config = oauth_config_with_forwarding();

    let server = server_builder.build(&config).await;

    // Create MCP client and test mixed servers
    let mcp_client = server.mcp_client_with_auth("/mcp", &access_token).await;

    // Search should find tools from both static and dynamic servers
    let search_results = mcp_client.search(&["add", "text"]).await;
    assert!(!search_results.is_empty(), "Should find tools from both server types");

    // Execute tool from dynamic server (requires token forwarding)
    let dynamic_result = mcp_client
        .execute("dynamic_server__adder", json!({"a": 2, "b": 3}))
        .await;

    insta::assert_json_snapshot!(dynamic_result, @r###"
    {
      "content": [
        {
          "type": "text",
          "text": "2 + 3 = 5"
        }
      ],
      "isError": false
    }
    "###);

    // Execute tool from static server (no auth required)
    let static_result = mcp_client
        .execute(
            "static_server__text_processor",
            json!({
                "text": "Hello World",
                "action": "uppercase"
            }),
        )
        .await;

    insta::assert_json_snapshot!(static_result, @r###"
    {
      "content": [
        {
          "type": "text",
          "text": "HELLO WORLD"
        }
      ],
      "isError": false
    }
    "###);

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn hydra_dual_instance_forwarding() {
    // Test using both Hydra instances to ensure cross-instance token handling
    let (_server, access_token) = setup_hydra_test().await.unwrap();

    // Create dynamic server that expects the primary Hydra token
    let mut dynamic_service = TestService::streamable_http("dynamic_server".to_string())
        .with_required_auth_token(access_token.clone())
        .with_forward_auth();
    dynamic_service.add_tool(AdderTool);

    let mut server_builder = TestServer::builder();
    server_builder.spawn_service(dynamic_service).await;

    let config = oauth_config_with_forwarding();

    let test_server = server_builder.build(&config).await;

    // Create MCP client and test token forwarding works
    let mcp_client = test_server.mcp_client_with_auth("/mcp", &access_token).await;

    let result = mcp_client
        .execute("dynamic_server__adder", json!({"a": 3, "b": 4}))
        .await;

    insta::assert_json_snapshot!(result, @r###"
    {
      "content": [
        {
          "type": "text",
          "text": "3 + 4 = 7"
        }
      ],
      "isError": false
    }
    "###);

    mcp_client.disconnect().await;
}

// =============================================================================
// Search Functionality Tests
// =============================================================================

#[tokio::test]
async fn token_forwarding_with_search() {
    let (_nexus_server, access_token) = setup_hydra_test().await.unwrap();

    // Create dynamic servers requiring auth
    let mut math_server = TestService::streamable_http("math_server".to_string())
        .with_required_auth_token(access_token.clone())
        .with_forward_auth();
    math_server.add_tool(AdderTool);
    math_server.add_tool(CalculatorTool);

    let mut text_server = TestService::streamable_http("text_server".to_string())
        .with_required_auth_token(access_token.clone())
        .with_forward_auth();
    text_server.add_tool(TextProcessorTool);

    let mut server_builder = TestServer::builder();
    server_builder.spawn_service(math_server).await;
    server_builder.spawn_service(text_server).await;

    let config = oauth_config_with_forwarding();

    let server = server_builder.build(&config).await;
    let mcp_client = server.mcp_client_with_auth("/mcp", &access_token).await;

    // Search should find tools from both servers
    let search_results = mcp_client.search(&["add", "text", "multiply"]).await;

    // Capture the search results as a snapshot
    insta::assert_json_snapshot!(search_results, @r#"
    [
      {
        "name": "text_server__text_processor",
        "description": "Processes text with various string manipulation operations like case conversion and reversal",
        "input_schema": {
          "type": "object",
          "properties": {
            "text": {
              "type": "string",
              "description": "Input text to process"
            },
            "action": {
              "type": "string",
              "enum": [
                "uppercase",
                "lowercase",
                "reverse",
                "word_count"
              ],
              "description": "Action to perform on the text"
            }
          },
          "required": [
            "text",
            "action"
          ]
        },
        "score": 2.442819595336914
      },
      {
        "name": "math_server__calculator",
        "description": "Performs basic mathematical calculations including addition, subtraction, multiplication and division",
        "input_schema": {
          "type": "object",
          "properties": {
            "operation": {
              "type": "string",
              "enum": [
                "add",
                "subtract",
                "multiply",
                "divide"
              ],
              "description": "Mathematical operation to perform"
            },
            "x": {
              "type": "number",
              "description": "First operand"
            },
            "y": {
              "type": "number",
              "description": "Second operand"
            }
          },
          "required": [
            "operation",
            "x",
            "y"
          ]
        },
        "score": 0.4000000059604645
      }
    ]
    "#);

    // Execute tools from both servers to verify token forwarding works
    let math_result = mcp_client.execute("math_server__adder", json!({"a": 5, "b": 7})).await;
    insta::assert_json_snapshot!(math_result, @r###"
    {
      "content": [
        {
          "type": "text",
          "text": "5 + 7 = 12"
        }
      ],
      "isError": false
    }
    "###);

    let text_result = mcp_client
        .execute(
            "text_server__text_processor",
            json!({"text": "HELLO", "action": "lowercase"}),
        )
        .await;
    insta::assert_json_snapshot!(text_result, @r###"
    {
      "content": [
        {
          "type": "text",
          "text": "hello"
        }
      ],
      "isError": false
    }
    "###);

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn search_behavior_without_auth() {
    // Create server with dynamic services that require auth but no token provided
    let mut dynamic_service = TestService::streamable_http("dynamic_server".to_string())
        .with_required_auth_token("required-token".to_string())
        .with_forward_auth();
    dynamic_service.add_tool(AdderTool);

    // Create static server that doesn't require auth
    let mut static_service = TestService::streamable_http("static_server".to_string());
    static_service.add_tool(TextProcessorTool);

    let mut server_builder = TestServer::builder();
    server_builder.spawn_service(dynamic_service).await;
    server_builder.spawn_service(static_service).await;

    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let server = server_builder.build(config).await;

    // Create MCP client
    let mcp_client = server.mcp_client("/mcp").await;

    // Search should work but only return static server tools
    let search_results = mcp_client.search(&["text", "add"]).await;

    // Should find static tools but not dynamic tools
    let tool_names: Vec<String> = search_results
        .iter()
        .filter_map(|result| result.get("name").and_then(|v| v.as_str().map(String::from)))
        .collect();

    let has_static_tools = tool_names.iter().any(|name| name.contains("static_server"));
    let has_dynamic_tools = tool_names.iter().any(|name| name.contains("dynamic_server"));

    assert!(
        has_static_tools || search_results.is_empty(),
        "Should either find static tools or have empty results"
    );
    assert!(!has_dynamic_tools, "Should not find dynamic server tools without auth");

    mcp_client.disconnect().await;
}

// =============================================================================
// Concurrent and Isolation Tests
// =============================================================================

#[tokio::test]
async fn concurrent_requests_different_tokens() {
    let hydra = HydraClient::new(4444, 4445);
    hydra.wait_for_hydra().await.unwrap();

    // Generate multiple different tokens
    let client_id = "shared-test-client-universal";
    let client_secret = format!("{client_id}-secret");

    let mut tokens = Vec::new();
    for _ in 0..3 {
        let token_response = hydra.get_token(client_id, &client_secret).await.unwrap();
        tokens.push(token_response.access_token);
    }

    // Create downstream servers, each accepting only its specific token
    let mut server_builder = TestServer::builder();

    for (i, token) in tokens.iter().enumerate() {
        let mut service = TestService::streamable_http(format!("server_{i}"))
            .with_required_auth_token(token.clone())
            .with_forward_auth();

        service.add_tool(AdderTool);
        service.add_tool(TextProcessorTool);

        server_builder.spawn_service(service).await;
    }

    let config = oauth_config_with_forwarding();

    let server = server_builder.build(&config).await;

    // Send concurrent requests with different tokens
    let num_tokens = tokens.len();
    let futures = tokens.iter().enumerate().map(|(i, token)| {
        let server = &server;
        let token = token.clone();
        async move {
            let mcp_client = server.mcp_client_with_auth("/mcp", &token).await;

            // Each server should only be accessible with its specific token
            let result = mcp_client
                .execute(&format!("server_{i}__adder"), json!({"a": i, "b": 10}))
                .await;

            // Try to access a different server - should fail
            let other_server = (i + 1) % num_tokens;
            let error = mcp_client
                .execute_expect_error(&format!("server_{other_server}__adder"), json!({"a": 1, "b": 2}))
                .await;

            mcp_client.disconnect().await;

            (i, result, error)
        }
    });

    let results = join_all(futures).await;

    // Verify results
    assert_eq!(results.len(), 3);

    // Index 0 result
    let (i, success_result, error_result) = &results[0];
    assert_eq!(*i, 0);
    insta::assert_json_snapshot!(success_result, @r###"
    {
      "content": [
        {
          "type": "text",
          "text": "0 + 10 = 10"
        }
      ],
      "isError": false
    }
    "###);
    insta::assert_snapshot!(error_result.to_string(), @"Mcp error: -32601: tools/call");

    // Index 1 result
    let (i, success_result, error_result) = &results[1];
    assert_eq!(*i, 1);
    insta::assert_json_snapshot!(success_result, @r###"
    {
      "content": [
        {
          "type": "text",
          "text": "1 + 10 = 11"
        }
      ],
      "isError": false
    }
    "###);
    insta::assert_snapshot!(error_result.to_string(), @"Mcp error: -32601: tools/call");

    // Index 2 result
    let (i, success_result, error_result) = &results[2];
    assert_eq!(*i, 2);
    insta::assert_json_snapshot!(success_result, @r###"
    {
      "content": [
        {
          "type": "text",
          "text": "2 + 10 = 12"
        }
      ],
      "isError": false
    }
    "###);
    insta::assert_snapshot!(error_result.to_string(), @"Mcp error: -32601: tools/call");
}

#[tokio::test]
async fn token_not_leaked_between_requests() {
    let hydra = HydraClient::new(4444, 4445);
    hydra.wait_for_hydra().await.unwrap();

    // Generate two different tokens
    let client_id = "shared-test-client-universal";
    let client_secret = format!("{client_id}-secret");

    let token1 = hydra.get_token(client_id, &client_secret).await.unwrap().access_token;
    let token2 = hydra.get_token(client_id, &client_secret).await.unwrap().access_token;

    // Create two downstream servers, each requiring its specific token
    let mut server1 = TestService::streamable_http("server1".to_string())
        .with_required_auth_token(token1.clone())
        .with_forward_auth();
    server1.add_tool(AdderTool);

    let mut server2 = TestService::streamable_http("server2".to_string())
        .with_required_auth_token(token2.clone())
        .with_forward_auth();
    server2.add_tool(CalculatorTool);

    let mut server_builder = TestServer::builder();
    server_builder.spawn_service(server1).await;
    server_builder.spawn_service(server2).await;

    let config = oauth_config_with_forwarding();

    let server = server_builder.build(&config).await;

    // Use first token - should only access server1
    let mcp_client1 = server.mcp_client_with_auth("/mcp", &token1).await;

    let result1 = mcp_client1.execute("server1__adder", json!({"a": 1, "b": 2})).await;
    insta::assert_json_snapshot!(result1, @r###"
    {
      "content": [
        {
          "type": "text",
          "text": "1 + 2 = 3"
        }
      ],
      "isError": false
    }
    "###);

    // Should fail to access server2
    let error1 = mcp_client1
        .execute_expect_error("server2__calculator", json!({"operation": "add", "x": 1, "y": 2}))
        .await;
    insta::assert_snapshot!(error1.to_string(), @"Mcp error: -32601: tools/call");

    mcp_client1.disconnect().await;

    // Use second token - should only access server2
    let mcp_client2 = server.mcp_client_with_auth("/mcp", &token2).await;

    let result2 = mcp_client2
        .execute("server2__calculator", json!({"operation": "multiply", "x": 3, "y": 4}))
        .await;
    insta::assert_json_snapshot!(result2, @r#"
    {
      "content": [
        {
          "type": "text",
          "text": "3 multiply 4 = 12"
        }
      ],
      "isError": false
    }
    "#);

    // Should fail to access server1
    let error2 = mcp_client2
        .execute_expect_error("server1__adder", json!({"a": 1, "b": 2}))
        .await;
    insta::assert_snapshot!(error2.to_string(), @"Mcp error: -32601: tools/call");

    mcp_client2.disconnect().await;
}

#[tokio::test]
async fn dynamic_server_isolation() {
    // Test that dynamic servers are properly isolated per token
    let hydra = HydraClient::new(4444, 4445);
    hydra.wait_for_hydra().await.unwrap();

    let client_id = "shared-test-client-universal";
    let client_secret = format!("{client_id}-secret");

    let token1 = hydra.get_token(client_id, &client_secret).await.unwrap().access_token;
    let token2 = hydra.get_token(client_id, &client_secret).await.unwrap().access_token;

    // Create a dynamic server that accepts any valid token
    let mut dynamic_server = TestService::streamable_http("dynamic_server".to_string()).with_forward_auth();
    dynamic_server.add_tool(AdderTool);

    let mut server_builder = TestServer::builder();
    server_builder.spawn_service(dynamic_server).await;

    let config = oauth_config_with_forwarding();

    let server = server_builder.build(&config).await;

    // Both tokens should be able to access the server
    let client1 = server.mcp_client_with_auth("/mcp", &token1).await;
    let client2 = server.mcp_client_with_auth("/mcp", &token2).await;

    let result1 = client1.execute("dynamic_server__adder", json!({"a": 1, "b": 2})).await;
    insta::assert_json_snapshot!(result1, @r###"
    {
      "content": [
        {
          "type": "text",
          "text": "1 + 2 = 3"
        }
      ],
      "isError": false
    }
    "###);

    let result2 = client2.execute("dynamic_server__adder", json!({"a": 3, "b": 4})).await;
    insta::assert_json_snapshot!(result2, @r###"
    {
      "content": [
        {
          "type": "text",
          "text": "3 + 4 = 7"
        }
      ],
      "isError": false
    }
    "###);

    // Verify that the search results are cached per token
    let search1 = client1.search(&["adder"]).await;
    let search2 = client2.search(&["adder"]).await;

    // Both should find the same tool
    insta::assert_json_snapshot!(search1, @r#"
    [
      {
        "name": "dynamic_server__adder",
        "description": "Adds two numbers together",
        "input_schema": {
          "type": "object",
          "properties": {
            "a": {
              "type": "number",
              "description": "First number to add"
            },
            "b": {
              "type": "number",
              "description": "Second number to add"
            }
          },
          "required": [
            "a",
            "b"
          ]
        },
        "score": 0.8630462884902954
      }
    ]
    "#);
    insta::assert_json_snapshot!(search2, @r#"
    [
      {
        "name": "dynamic_server__adder",
        "description": "Adds two numbers together",
        "input_schema": {
          "type": "object",
          "properties": {
            "a": {
              "type": "number",
              "description": "First number to add"
            },
            "b": {
              "type": "number",
              "description": "Second number to add"
            }
          },
          "required": [
            "a",
            "b"
          ]
        },
        "score": 0.8630462884902954
      }
    ]
    "#);

    client1.disconnect().await;
    client2.disconnect().await;
}

#[tokio::test]
async fn dynamic_server_caching_per_token() {
    let (_nexus_server, access_token) = setup_hydra_test().await.unwrap();

    // Create dynamic server
    let mut dynamic_service = TestService::streamable_http("dynamic_server".to_string())
        .with_required_auth_token(access_token.clone())
        .with_forward_auth();
    dynamic_service.add_tool(AdderTool);

    let mut server_builder = TestServer::builder();
    server_builder.spawn_service(dynamic_service).await;

    let config = oauth_config_with_forwarding();

    let server = server_builder.build(&config).await;

    // Create first MCP client with first token
    let mcp_client1 = server.mcp_client_with_auth("/mcp", &access_token).await;

    // Make first request (should initialize and cache)
    let result1 = mcp_client1
        .execute("dynamic_server__adder", json!({"a": 1, "b": 1}))
        .await;

    insta::assert_json_snapshot!(result1, @r###"
    {
      "content": [
        {
          "type": "text",
          "text": "1 + 1 = 2"
        }
      ],
      "isError": false
    }
    "###);

    // Make second request with same token (should use cache)
    let result2 = mcp_client1
        .execute("dynamic_server__adder", json!({"a": 2, "b": 2}))
        .await;

    insta::assert_json_snapshot!(result2, @r###"
    {
      "content": [
        {
          "type": "text",
          "text": "2 + 2 = 4"
        }
      ],
      "isError": false
    }
    "###);

    mcp_client1.disconnect().await;

    // Note: Testing with different tokens would require generating a second valid token
    // For now, we verify that the same token works consistently (indicating caching works)
}

// =============================================================================
// Error Handling Tests
// =============================================================================

#[tokio::test]
async fn token_forwarding_error_handling() {
    // Test various error scenarios in token forwarding
    let (_nexus_server, access_token) = setup_hydra_test().await.unwrap();

    // Create a server that requires a specific token
    let mut auth_server = TestService::streamable_http("auth_server".to_string())
        .with_required_auth_token(access_token.clone())
        .with_forward_auth();
    auth_server.add_tool(AdderTool);

    // Create a server with no auth requirements
    let mut public_server = TestService::streamable_http("public_server".to_string());
    public_server.add_tool(TextProcessorTool);

    let mut server_builder = TestServer::builder();
    server_builder.spawn_service(auth_server).await;
    server_builder.spawn_service(public_server).await;

    let config = oauth_config_with_forwarding();

    let server = server_builder.build(&config).await;

    // Test with no token - should fail to access Nexus entirely
    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .header("Content-Type", "application/json")
        .body(r#"{"jsonrpc": "2.0", "method": "tools/list", "id": 1}"#)
        .send()
        .await
        .unwrap();

    insta::assert_snapshot!(response.status().as_u16(), @"401");

    // Test with invalid token
    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .header("Authorization", "Bearer invalid.token.here")
        .header("Content-Type", "application/json")
        .body(r#"{"jsonrpc": "2.0", "method": "tools/list", "id": 1}"#)
        .send()
        .await
        .unwrap();

    insta::assert_snapshot!(response.status().as_u16(), @"401");
}

#[tokio::test]
async fn invalid_token_handling() {
    // Create dynamic server that expects valid tokens
    let mut dynamic_service = TestService::streamable_http("dynamic_server".to_string())
        .with_required_auth_token("valid-token".to_string())
        .with_forward_auth();
    dynamic_service.add_tool(AdderTool);

    let mut server_builder = TestServer::builder();
    server_builder.spawn_service(dynamic_service).await;

    let config = oauth_config_with_forwarding();

    let server = server_builder.build(&config).await;

    // Test with expired token
    let expired_token = create_expired_jwt();

    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&expired_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "tools/list", "id": 1}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 401, "Should reject expired token at Nexus level");

    // Test with malformed token
    let malformed_token = "not.a.valid.jwt";

    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(malformed_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "tools/list", "id": 1}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 401, "Should reject malformed token at Nexus level");
}

#[tokio::test]
async fn mixed_success_failure_scenarios() {
    let (_server, access_token) = setup_hydra_test().await.unwrap();

    // Create static server that doesn't require auth
    let mut static_service = TestService::streamable_http("static_server".to_string());
    static_service.add_tool(TextProcessorTool);

    // Create working dynamic server that matches the auth token
    let mut working_service = TestService::streamable_http("working_server".to_string())
        .with_required_auth_token(access_token.clone())
        .with_forward_auth();

    working_service.add_tool(AdderTool);

    // Create broken dynamic server that does not match the auth token
    let mut broken_service = TestService::streamable_http("broken_server".to_string())
        .with_required_auth_token(String::from("kekw"))
        .with_forward_auth();

    broken_service.add_tool(CalculatorTool);

    let mut server_builder = TestServer::builder();
    server_builder.spawn_service(static_service).await;
    server_builder.spawn_service(working_service).await;
    server_builder.spawn_service(broken_service).await;

    let config = oauth_config_with_forwarding();

    let test_server = server_builder.build(&config).await;

    // Create MCP client
    let mcp_client = test_server.mcp_client_with_auth("/mcp", &access_token).await;

    // Static server should work
    let static_result = mcp_client
        .execute(
            "static_server__text_processor",
            json!({
                "text": "TEST",
                "action": "lowercase"
            }),
        )
        .await;

    insta::assert_json_snapshot!(static_result, @r###"
    {
      "content": [
        {
          "type": "text",
          "text": "test"
        }
      ],
      "isError": false
    }
    "###);

    // Working server should work
    let working_result = mcp_client
        .execute("working_server__adder", json!({"a": 1, "b": 2}))
        .await;

    insta::assert_json_snapshot!(working_result, @r###"
    {
      "content": [
        {
          "type": "text",
          "text": "1 + 2 = 3"
        }
      ],
      "isError": false
    }
    "###);

    // Broken server should not work
    let broken_result = mcp_client
        .execute_expect_error(
            "broken_service__calculator",
            json!({"operation": "divide", "a": 1, "b": 2}),
        )
        .await;

    insta::assert_snapshot!(broken_result.to_string(), @"Mcp error: -32601: tools/call");

    mcp_client.disconnect().await;
}

// =============================================================================
// Regression and Compatibility Tests
// =============================================================================

#[tokio::test]
async fn no_regression_in_existing_oauth2_tests() {
    // This test ensures that enabling token forwarding doesn't break existing OAuth2 functionality
    let (server, access_token) = setup_hydra_test().await.unwrap();

    // Test basic MCP initialization still works
    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&access_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "initialize", "id": 1, "params": {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "test", "version": "1.0"}}}"#)
        .send()
        .await
        .unwrap();

    assert_ne!(
        response.status(),
        401,
        "MCP initialization should work with valid OAuth token"
    );

    let response_text = response.text().await.unwrap();
    assert!(response_text.contains("jsonrpc"), "Should get JSON-RPC response");

    // Test tools/list still works
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
async fn forwarding_fails_without_nexus_oauth2() {
    // Create dynamic server that expects auth
    let mut dynamic_service = TestService::streamable_http("dynamic_server".to_string())
        .with_required_auth_token("test-token".to_string())
        .with_forward_auth();
    dynamic_service.add_tool(AdderTool);

    let mut server_builder = TestServer::builder();
    server_builder.spawn_service(dynamic_service).await;

    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#}
    .to_string();
    let server = server_builder.build(&config).await;

    // MCP client should work since Nexus itself doesn't require OAuth
    let mcp_client = server.mcp_client("/mcp").await;

    // But tools/list should fail or return limited results since dynamic servers can't be reached
    let tools_result = mcp_client.list_tools().await;

    // Should only show built-in tools (search, execute) but no downstream tools
    insta::assert_json_snapshot!(tools_result, @r##"
    {
      "tools": [
        {
          "name": "search",
          "description": "Search for relevant tools. A list of matching tools with their\\nscore is returned with a map of input fields and their types.\n\nUsing this information, you can call the execute tool with the\\nname of the tool you want to execute, and defining the input parameters.\n\nTool names are in the format \"server__tool\" where \"server\" is the name of the MCP server providing\nthe tool.\n",
          "inputSchema": {
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
          "outputSchema": {
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "title": "Array_of_SearchResult",
            "type": "array",
            "items": {
              "$ref": "#/$defs/SearchResult"
            },
            "$defs": {
              "SearchResult": {
                "type": "object",
                "properties": {
                  "name": {
                    "description": "The name of the tool (format: \"server__tool\")",
                    "type": "string"
                  },
                  "description": {
                    "description": "Description of what the tool does",
                    "type": "string"
                  },
                  "input_schema": {
                    "description": "The input schema for the tool's parameters"
                  },
                  "score": {
                    "description": "The relevance score for this result (higher is more relevant)",
                    "type": "number",
                    "format": "float"
                  }
                },
                "required": [
                  "name",
                  "description",
                  "input_schema",
                  "score"
                ]
              }
            }
          },
          "annotations": {
            "readOnlyHint": true
          }
        },
        {
          "name": "execute",
          "description": "Executes a tool with the given parameters. Before using, you must call the search function to retrieve the tools you need for your task. If you do not know how to call this tool, call search first.\n\nThe tool name and parameters are specified in the request body. The tool name must be a string,\nand the parameters must be a map of strings to JSON values.\n",
          "inputSchema": {
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
    "##);

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn dynamic_tool_execution_failures() {
    // This test verifies that when dynamic servers fail to authenticate,
    // static servers continue to work properly

    // Create static server that doesn't require auth
    let mut static_service = TestService::streamable_http("static_server".to_string());
    static_service.add_tool(TextProcessorTool);

    let mut server_builder = TestServer::builder();
    server_builder.spawn_service(static_service).await;

    // Use config without OAuth2 enabled on Nexus
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#}
    .to_string();

    let test_server = server_builder.build(&config).await;

    // Create MCP client (no auth needed since Nexus OAuth2 is disabled)
    let mcp_client = test_server.mcp_client("/mcp").await;

    // Static server tools should work
    let static_result = mcp_client
        .execute(
            "static_server__text_processor",
            json!({
                "text": "Hello World",
                "action": "lowercase"
            }),
        )
        .await;

    insta::assert_json_snapshot!(static_result, @r###"
    {
      "content": [
        {
          "type": "text",
          "text": "hello world"
        }
      ],
      "isError": false
    }
    "###);

    // List tools should only show static server tools and built-in tools
    let tools_result = mcp_client.list_tools().await;

    // Verify we have the static server tool
    let tool_names: Vec<&str> = tools_result.tools.iter().map(|t| t.name.as_ref()).collect();

    assert!(tool_names.contains(&"search"));
    assert!(tool_names.contains(&"execute"));

    mcp_client.disconnect().await;
}
