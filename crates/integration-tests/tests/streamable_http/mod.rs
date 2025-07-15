use crate::tools::{AdderTool, FailingTool};
use indoc::indoc;
use integration_tests::{TestServer, TestService};
use serde_json::json;

#[tokio::test]
async fn service_with_single_tool() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let mut test_service = TestService::streamable_http("math_service".to_string());
    test_service.add_tool(AdderTool).await;

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
          "name": "math_service__adder",
          "description": "Adds two numbers together",
          "inputSchema": {
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

    let mut test_service = TestService::streamable_http("math_service".to_string());
    test_service.add_tool(AdderTool).await;

    let mut builder = TestServer::builder();
    builder.spawn_service(test_service).await;
    let server = builder.build(config).await;

    let mcp_client = server.mcp_client("/mcp").await;

    // Call the adder tool with test values
    let result = mcp_client
        .call_tool("math_service__adder", json!({ "a": 5, "b": 3 }))
        .await;

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
async fn call_tool_with_decimals() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let mut test_service = TestService::streamable_http("math_service".to_string());
    test_service.add_tool(AdderTool).await;

    let mut builder = TestServer::builder();
    builder.spawn_service(test_service).await;
    let server = builder.build(config).await;

    let mcp_client = server.mcp_client("/mcp").await;

    // Call the adder tool with decimal values
    let result = mcp_client
        .call_tool("math_service__adder", json!({ "a": 2.5, "b": 1.5 }))
        .await;

    insta::assert_json_snapshot!(result, @r#"
    {
      "content": [
        {
          "type": "text",
          "text": "2.5 + 1.5 = 4"
        }
      ],
      "isError": false
    }
    "#);

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn call_nonexistent_tool() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let mut test_service = TestService::streamable_http("math_service".to_string());
    test_service.add_tool(AdderTool).await;

    let mut builder = TestServer::builder();
    builder.spawn_service(test_service).await;
    let server = builder.build(config).await;

    let mcp_client = server.mcp_client("/mcp").await;

    // Try to call a tool that doesn't exist
    let error = mcp_client.call_tool_expect_error("nonexistent_tool", json!({})).await;

    insta::assert_snapshot!(error.to_string(), @"Mcp error: -32602: Unknown tool: nonexistent_tool");

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn call_tool_wrong_server_prefix() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let mut test_service = TestService::streamable_http("math_service".to_string());
    test_service.add_tool(AdderTool).await;

    let mut builder = TestServer::builder();
    builder.spawn_service(test_service).await;
    let server = builder.build(config).await;

    let mcp_client = server.mcp_client("/mcp").await;

    // Try to call the tool with wrong server prefix
    let error = mcp_client
        .call_tool_expect_error("wrong_service__adder", json!({ "a": 1, "b": 2 }))
        .await;

    insta::assert_snapshot!(error.to_string(), @"Mcp error: -32602: Unknown tool: wrong_service__adder");

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn call_tool_invalid_arguments() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let mut test_service = TestService::streamable_http("math_service".to_string());
    test_service.add_tool(AdderTool).await;

    let mut builder = TestServer::builder();
    builder.spawn_service(test_service).await;
    let server = builder.build(config).await;

    let mcp_client = server.mcp_client("/mcp").await;

    // Call the tool with missing argument
    let error = mcp_client
        .call_tool_expect_error("math_service__adder", json!({ "a": 5 }))
        .await;

    insta::assert_snapshot!(error.to_string(), @"Mcp error: -32603: Mcp error: -32602: Missing or invalid parameter 'b'");

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn call_tool_no_arguments() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let mut test_service = TestService::streamable_http("math_service".to_string());
    test_service.add_tool(AdderTool).await;

    let mut builder = TestServer::builder();
    builder.spawn_service(test_service).await;
    let server = builder.build(config).await;

    let mcp_client = server.mcp_client("/mcp").await;

    // Call the tool with no arguments
    let error = mcp_client
        .call_tool_expect_error("math_service__adder", json!({}))
        .await;

    insta::assert_snapshot!(error.to_string(), @"Mcp error: -32603: Mcp error: -32602: Missing or invalid parameter 'a'");

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn multiple_services_multiple_tools() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    // Create first service with adder tool
    let mut math_service = TestService::streamable_http("math_service".to_string());
    math_service.add_tool(AdderTool).await;

    // Create second service with failing tool
    let mut error_service = TestService::streamable_http("error_service".to_string());
    error_service.add_tool(FailingTool).await;

    let mut builder = TestServer::builder();
    builder.spawn_service(math_service).await;
    builder.spawn_service(error_service).await;

    let server = builder.build(config).await;

    let mcp_client = server.mcp_client("/mcp").await;

    // Should list tools from both services
    let tools_result = mcp_client.list_tools().await;

    insta::assert_json_snapshot!(&tools_result, @r#"
    {
      "tools": [
        {
          "name": "error_service__failing_tool",
          "description": "A tool that always fails for testing error handling",
          "inputSchema": {
            "type": "object",
            "properties": {}
          }
        },
        {
          "name": "math_service__adder",
          "description": "Adds two numbers together",
          "inputSchema": {
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
          }
        }
      ]
    }
    "#);

    // Test calling tools from both services
    let add_result = mcp_client
        .call_tool("math_service__adder", json!({ "a": 10, "b": 20 }))
        .await;

    insta::assert_json_snapshot!(&add_result, @r#"
    {
      "content": [
        {
          "type": "text",
          "text": "10 + 20 = 30"
        }
      ],
      "isError": false
    }
    "#);

    let fail_error = mcp_client
        .call_tool_expect_error("error_service__failing_tool", json!({}))
        .await;

    insta::assert_snapshot!(fail_error.to_string(), @"Mcp error: -32603: Mcp error: -32000: This tool always fails({\"reason\":\"intentional_failure\"})");

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn test_custom_mcp_path_with_tools() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
        path = "/custom-mcp"
    "#};

    let mut test_service = TestService::streamable_http("math_service".to_string());
    test_service.add_tool(AdderTool).await;

    let mut builder = TestServer::builder();
    builder.spawn_service(test_service).await;
    let server = builder.build(config).await;

    let mcp_client = server.mcp_client("/custom-mcp").await;

    // Should be able to connect and use tools on custom path
    let tools_result = mcp_client.list_tools().await;

    insta::assert_json_snapshot!(&tools_result, @r#"
    {
      "tools": [
        {
          "name": "math_service__adder",
          "description": "Adds two numbers together",
          "inputSchema": {
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
          }
        }
      ]
    }
    "#);

    let result = mcp_client
        .call_tool("math_service__adder", json!({ "a": 7, "b": 2 }))
        .await;

    insta::assert_json_snapshot!(&result, @r#"
    {
      "content": [
        {
          "type": "text",
          "text": "7 + 2 = 9"
        }
      ],
      "isError": false
    }
    "#);

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn tools_with_tls() {
    let config = indoc! {r#"
        [server]
        [server.tls]
        certificate = "certs/cert.pem"
        key = "certs/key.pem"

        [mcp]
        enabled = true
    "#};

    let mut test_service = TestService::streamable_http("math_service".to_string());
    test_service.add_tool(AdderTool).await;

    let mut builder = TestServer::builder();
    builder.spawn_service(test_service).await;
    let server = builder.build(config).await;

    let mcp_client = server.mcp_client("/mcp").await;

    // Should work over TLS
    let tools_result = mcp_client.list_tools().await;

    insta::assert_json_snapshot!(&tools_result, @r#"
    {
      "tools": [
        {
          "name": "math_service__adder",
          "description": "Adds two numbers together",
          "inputSchema": {
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
          }
        }
      ]
    }
    "#);

    // Test calling the tool over TLS
    let result = mcp_client
        .call_tool("math_service__adder", json!({ "a": 7, "b": 2 }))
        .await;

    insta::assert_json_snapshot!(result, @r#"
    {
      "content": [
        {
          "type": "text",
          "text": "7 + 2 = 9"
        }
      ],
      "isError": false
    }
    "#);

    mcp_client.disconnect().await;
}
