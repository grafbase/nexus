use indoc::indoc;
use integration_tests::{TestServer, TestService};
use serde_json::json;
use tokio::runtime::Handle;

use crate::tools::{AdderTool, CalculatorTool};

#[tokio::test]
async fn successful_token_auth() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let mut test_service =
        TestService::streamable_http("auth_service".to_string()).with_auth_token("valid_token_123".to_string());
    test_service.add_tool(AdderTool).await;

    let mut builder = TestServer::builder();
    builder.spawn_service(test_service).await;

    let server = builder.build(config).await;
    let mcp_client = server.mcp_client("/mcp").await;
    let tools_result = mcp_client.list_tools().await;

    insta::assert_json_snapshot!(tools_result, @r#"
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

    let result = mcp_client
        .execute("auth_service__adder", json!({ "a": 5, "b": 3 }))
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
async fn multiple_services_with_different_tokens() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let mut service_one =
        TestService::streamable_http("service_one".to_string()).with_auth_token("valid_token_123".to_string());
    service_one.add_tool(AdderTool).await;

    let mut service_two = TestService::sse("service_two".to_string()).with_auth_token("valid_token_123".to_string());
    service_two.add_tool(CalculatorTool).await;

    let mut builder = TestServer::builder();
    builder.spawn_service(service_one).await;
    builder.spawn_service(service_two).await;

    let server = builder.build(config).await;
    let mcp_client = server.mcp_client("/mcp").await;

    let add_result = mcp_client
        .execute("service_one__adder", json!({ "a": 1, "b": 2 }))
        .await;

    insta::assert_json_snapshot!(add_result, @r#"
    {
      "content": [
        {
          "type": "text",
          "text": "1 + 2 = 3"
        }
      ],
      "isError": false
    }
    "#);

    let calc_result = mcp_client
        .execute(
            "service_two__calculator",
            json!({
                "operation": "add",
                "x": 3,
                "y": 4
            }),
        )
        .await;

    insta::assert_json_snapshot!(calc_result, @r#"
    {
      "content": [
        {
          "type": "text",
          "text": "3 add 4 = 7"
        }
      ],
      "isError": false
    }
    "#);

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn startup_fails_with_invalid_downstream_auth() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let mut test_service = TestService::streamable_http("auth_service".to_string())
        .with_required_auth_token("correct_token".to_string())
        .with_auth_token("wrong_token".to_string());

    test_service.add_tool(AdderTool).await;

    let mut builder = TestServer::builder();
    builder.spawn_service(test_service).await;

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        Handle::current().block_on(async { builder.build(config).await })
    }));

    assert!(result.is_err());
}
