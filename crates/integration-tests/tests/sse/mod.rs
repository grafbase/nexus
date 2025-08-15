use indoc::indoc;
use integration_tests::{TestServer, TestService, get_test_cert_paths};
use serde_json::json;

use crate::tools::{AdderTool, FailingTool};

#[tokio::test]
async fn list_single_tool() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let mut test_service = TestService::sse("sse_math_service".to_string());
    test_service.add_tool(AdderTool);

    let mut builder = TestServer::builder();
    builder.spawn_service(test_service).await;
    let server = builder.build(config).await;

    let mcp_client = server.mcp_client("/mcp").await;

    // Should list the adder tool with proper naming
    let tools_result = mcp_client.list_tools().await;

    insta::assert_json_snapshot!(&tools_result, @r#"
    {
      "tools": [
        {
          "name": "search",
          "description": "Search for relevant tools",
          "inputSchema": {
            "type": "object",
            "properties": {
              "keywords": {
                "type": "array",
                "items": {
                  "type": "string"
                }
              }
            },
            "required": [
              "keywords"
            ]
          }
        },
        {
          "name": "execute",
          "description": "Executes a tool with the given parameters",
          "inputSchema": {
            "type": "object",
            "properties": {
              "name": {
                "type": "string"
              },
              "arguments": {
                "type": "object"
              }
            },
            "required": [
              "name",
              "arguments"
            ]
          }
        }
      ]
    }
    "#);

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn call_tool_success() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let mut test_service = TestService::sse("sse_math_service".to_string());
    test_service.add_tool(AdderTool);

    let mut builder = TestServer::builder();
    builder.spawn_service(test_service).await;
    let server = builder.build(config).await;

    let mcp_client = server.mcp_client("/mcp").await;

    // Call the adder tool with test values
    let result = mcp_client
        .execute("sse_math_service__adder", json!({ "a": 10, "b": 15 }))
        .await;

    insta::assert_json_snapshot!(result, @r#"
    {
      "content": [
        {
          "type": "text",
          "text": "10 + 15 = 25"
        }
      ],
      "isError": false
    }
    "#);

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn autodetected_call_tool_success() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let mut test_service = TestService::sse_autodetect("sse_math_service".to_string());
    test_service.add_tool(AdderTool);

    let mut builder = TestServer::builder();
    builder.spawn_service(test_service).await;
    let server = builder.build(config).await;

    let mcp_client = server.mcp_client("/mcp").await;

    // Call the adder tool with test values
    let result = mcp_client
        .execute("sse_math_service__adder", json!({ "a": 10, "b": 15 }))
        .await;

    insta::assert_json_snapshot!(result, @r#"
    {
      "content": [
        {
          "type": "text",
          "text": "10 + 15 = 25"
        }
      ],
      "isError": false
    }
    "#);

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn mixed_sse_and_streaming_services() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    // Create HTTP service
    let mut http_service = TestService::streamable_http("http_service".to_string());
    http_service.add_tool(AdderTool);

    // Create SSE service
    let mut sse_service = TestService::sse("sse_service".to_string());
    sse_service.add_tool(FailingTool);

    let mut builder = TestServer::builder();
    builder.spawn_service(http_service).await;
    builder.spawn_service(sse_service).await;
    let server = builder.build(config).await;

    let mcp_client = server.mcp_client("/mcp").await;

    // Should list tools from both services
    let tools_result = mcp_client.list_tools().await;

    insta::assert_json_snapshot!(&tools_result, @r#"
    {
      "tools": [
        {
          "name": "search",
          "description": "Search for relevant tools",
          "inputSchema": {
            "type": "object",
            "properties": {
              "keywords": {
                "type": "array",
                "items": {
                  "type": "string"
                }
              }
            },
            "required": [
              "keywords"
            ]
          }
        },
        {
          "name": "execute",
          "description": "Executes a tool with the given parameters",
          "inputSchema": {
            "type": "object",
            "properties": {
              "name": {
                "type": "string"
              },
              "arguments": {
                "type": "object"
              }
            },
            "required": [
              "name",
              "arguments"
            ]
          }
        }
      ]
    }
    "#);

    // Test calling tools from both services
    let add_result = mcp_client
        .execute("http_service__adder", json!({ "a": 3, "b": 4 }))
        .await;

    insta::assert_json_snapshot!(&add_result, @r#"
    {
      "content": [
        {
          "type": "text",
          "text": "3 + 4 = 7"
        }
      ],
      "isError": false
    }
    "#);

    let fail_error = mcp_client
        .execute_expect_error("sse_service__failing_tool", json!({}))
        .await;

    insta::assert_snapshot!(fail_error.to_string(), @r#"Mcp error: -32000: This tool always fails({"reason":"intentional_failure"})"#);

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn tls_downstream_service() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let (cert_path, key_path) = get_test_cert_paths();
    let mut test_service = TestService::sse("tls_sse_service".to_string()).with_tls(cert_path, key_path);
    test_service.add_tool(AdderTool);

    let mut builder = TestServer::builder();
    builder.spawn_service(test_service).await;
    let server = builder.build(config).await;

    let mcp_client = server.mcp_client("/mcp").await;

    // Verify the tool is listed correctly
    let tools_result = mcp_client.list_tools().await;
    insta::assert_json_snapshot!(&tools_result, @r#"
    {
      "tools": [
        {
          "name": "search",
          "description": "Search for relevant tools",
          "inputSchema": {
            "type": "object",
            "properties": {
              "keywords": {
                "type": "array",
                "items": {
                  "type": "string"
                }
              }
            },
            "required": [
              "keywords"
            ]
          }
        },
        {
          "name": "execute",
          "description": "Executes a tool with the given parameters",
          "inputSchema": {
            "type": "object",
            "properties": {
              "name": {
                "type": "string"
              },
              "arguments": {
                "type": "object"
              }
            },
            "required": [
              "name",
              "arguments"
            ]
          }
        }
      ]
    }
    "#);

    // Test calling the tool
    let result = mcp_client
        .execute("tls_sse_service__adder", json!({ "a": 5, "b": 7 }))
        .await;

    insta::assert_json_snapshot!(result, @r#"
    {
      "content": [
        {
          "type": "text",
          "text": "5 + 7 = 12"
        }
      ],
      "isError": false
    }
    "#);

    mcp_client.disconnect().await;
}
