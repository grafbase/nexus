use crate::tools::{AdderTool, CalculatorTool, FailingTool, FileSystemTool, TextProcessorTool};
use indoc::indoc;
use integration_tests::{TestServer, TestService, get_test_cert_paths};
use serde_json::json;

#[tokio::test]
async fn list_tools() {
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

    let tools_result = mcp_client.list_tools().await;

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
            "type": "object",
            "properties": {
              "name": {
                "type": "string"
              },
              "arguments": {
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
async fn call_tool_success_autodetect() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let mut test_service = TestService::streamable_http_autodetect("math_service".to_string());
    test_service.add_tool(AdderTool).await;

    let mut builder = TestServer::builder();
    builder.spawn_service(test_service).await;
    let server = builder.build(config).await;

    let mcp_client = server.mcp_client("/mcp").await;

    let result = mcp_client
        .execute("math_service__adder", json!({ "a": 5, "b": 3 }))
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

    let result = mcp_client
        .execute("math_service__adder", json!({ "a": 2.5, "b": 1.5 }))
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
    let error = mcp_client.execute_expect_error("nonexistent_tool", json!({})).await;

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

    let error = mcp_client
        .execute_expect_error("mtah_service__adder", json!({ "a": 1, "b": 2 }))
        .await;

    insta::assert_snapshot!(error.to_string(), @"Mcp error: -32602: Unknown tool: mtah_service__adder. Did you mean: math_service__adder");

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

    let arguments = json!({
        "name": "math_service__adder",
        "arguments": {
            "a": 5,
        }
    });

    let error = mcp_client.execute_expect_error("execute", arguments).await;
    insta::assert_snapshot!(error.to_string(), @"Mcp error: -32602: Unknown tool: execute");

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

    let error = mcp_client.execute_expect_error("math_service__adder", json!({})).await;

    insta::assert_snapshot!(error.to_string(), @"Mcp error: -32602: Missing or invalid parameter 'a'");

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn multiple_services_multiple_tools() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let mut math_service = TestService::streamable_http("math_service".to_string());
    math_service.add_tool(AdderTool).await;

    let mut error_service = TestService::streamable_http("error_service".to_string());
    error_service.add_tool(FailingTool).await;

    let mut builder = TestServer::builder();
    builder.spawn_service(math_service).await;
    builder.spawn_service(error_service).await;

    let server = builder.build(config).await;

    let mcp_client = server.mcp_client("/mcp").await;

    let tools_result = mcp_client.list_tools().await;

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
            "type": "object",
            "properties": {
              "name": {
                "type": "string"
              },
              "arguments": {
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

    let add_result = mcp_client
        .execute("math_service__adder", json!({ "a": 10, "b": 20 }))
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
        .execute_expect_error("error_service__failing_tool", json!({}))
        .await;

    insta::assert_snapshot!(fail_error.to_string(), @r#"Mcp error: -32000: This tool always fails({"reason":"intentional_failure"})"#);

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

    let tools_result = mcp_client.list_tools().await;

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
            "type": "object",
            "properties": {
              "name": {
                "type": "string"
              },
              "arguments": {
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
        .execute("math_service__adder", json!({ "a": 7, "b": 2 }))
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
    let tools_result = mcp_client.list_tools().await;

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
            "type": "object",
            "properties": {
              "name": {
                "type": "string"
              },
              "arguments": {
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
        .execute("math_service__adder", json!({ "a": 7, "b": 2 }))
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

#[tokio::test]
async fn tls_downstream_service() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let (cert_path, key_path) = get_test_cert_paths();
    let mut test_service = TestService::streamable_http("tls_http_service".to_string()).with_tls(cert_path, key_path);
    test_service.add_tool(AdderTool).await;

    let mut builder = TestServer::builder();
    builder.spawn_service(test_service).await;
    let server = builder.build(config).await;

    let mcp_client = server.mcp_client("/mcp").await;

    let tools_result = mcp_client.list_tools().await;
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
            "type": "object",
            "properties": {
              "name": {
                "type": "string"
              },
              "arguments": {
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
        .execute("tls_http_service__adder", json!({ "a": 10, "b": 20 }))
        .await;

    insta::assert_json_snapshot!(result, @r#"
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

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn search_exact_matching() {
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

    let result = mcp_client.search(&["adder"]).await;

    insta::assert_json_snapshot!(result, @r#"
    [
      {
        "tool": {
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
        },
        "score": 0.8630463
      }
    ]
    "#);

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn search_fuzzy_matching() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let mut test_service = TestService::streamable_http("text_service".to_string());
    test_service.add_tool(TextProcessorTool).await;

    let mut builder = TestServer::builder();
    builder.spawn_service(test_service).await;
    let server = builder.build(config).await;

    let mcp_client = server.mcp_client("/mcp").await;

    let result = mcp_client.search(&["processer"]).await;

    insta::assert_json_snapshot!(result, @r#"
    [
      {
        "tool": {
          "name": "text_service__text_processor",
          "description": "Processes text with various string manipulation operations like case conversion and reversal",
          "inputSchema": {
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
          "annotations": {
            "title": "Text Processor"
          }
        },
        "score": 0.6
      }
    ]
    "#);

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn search_multiple_keywords() {
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

    let result = mcp_client.search(&["add", "numbers"]).await;

    insta::assert_json_snapshot!(result, @r#"
    [
      {
        "tool": {
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
        },
        "score": 0.6
      }
    ]
    "#);

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn search_two() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let mut test_service = TestService::streamable_http("math_service".to_string());
    test_service.add_tool(AdderTool).await;
    test_service.add_tool(FailingTool).await;

    let mut builder = TestServer::builder();
    builder.spawn_service(test_service).await;
    let server = builder.build(config).await;

    let mcp_client = server.mcp_client("/mcp").await;

    let result = mcp_client.search(&["adder", "failing"]).await;

    insta::assert_json_snapshot!(result, @r#"
    [
      {
        "tool": {
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
        },
        "score": 2.4077742
      },
      {
        "tool": {
          "name": "math_service__failing_tool",
          "description": "A tool that always fails for testing error handling",
          "inputSchema": {
            "type": "object",
            "properties": {}
          }
        },
        "score": 1.8299086
      }
    ]
    "#);

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn search_case_insensitive() {
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

    let result = mcp_client.search(&["ADDER"]).await;

    insta::assert_json_snapshot!(result, @r#"
    [
      {
        "tool": {
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
        },
        "score": 0.8630463
      }
    ]
    "#);

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn search_by_description() {
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

    let result = mcp_client.search(&["together"]).await;

    insta::assert_json_snapshot!(result, @r#"
    [
      {
        "tool": {
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
        },
        "score": 0.6
      }
    ]
    "#);

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn search_by_server_name() {
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

    let result = mcp_client.search(&["math"]).await;

    insta::assert_json_snapshot!(result, @r#"
    [
      {
        "tool": {
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
        },
        "score": 0.2301457
      }
    ]
    "#);

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn search_no_results() {
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

    let result = mcp_client.search(&["nonexistent"]).await;

    insta::assert_json_snapshot!(result, @"[]");

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn search_multiple_tools_ranking() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let mut math_service = TestService::streamable_http("math_service".to_string());
    math_service.add_tool(AdderTool).await;

    let mut error_service = TestService::streamable_http("error_service".to_string());
    error_service.add_tool(FailingTool).await;

    let mut builder = TestServer::builder();
    builder.spawn_service(math_service).await;
    builder.spawn_service(error_service).await;

    let server = builder.build(config).await;

    let mcp_client = server.mcp_client("/mcp").await;

    let result = mcp_client.search(&["tool"]).await;

    insta::assert_json_snapshot!(result, @r#"
    [
      {
        "tool": {
          "name": "error_service__failing_tool",
          "description": "A tool that always fails for testing error handling",
          "inputSchema": {
            "type": "object",
            "properties": {}
          }
        },
        "score": 1.8299086
      }
    ]
    "#);

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn search_parameter_fields() {
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

    let result = mcp_client.search(&["First"]).await;

    insta::assert_json_snapshot!(result, @r#"
    [
      {
        "tool": {
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
        },
        "score": 0.4
      }
    ]
    "#);

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn search_empty_query() {
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

    let result = mcp_client.search(&[]).await;

    insta::assert_json_snapshot!(result, @"[]");

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn search_whitespace_handling() {
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

    let result = mcp_client.search(&["  add  ", "numbers  "]).await;

    insta::assert_json_snapshot!(result, @r#"
    [
      {
        "tool": {
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
        },
        "score": 0.6
      }
    ]
    "#);

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn search_tool_annotations() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let mut test_service = TestService::streamable_http("math_service".to_string());
    test_service.add_tool(CalculatorTool).await;

    let mut builder = TestServer::builder();
    builder.spawn_service(test_service).await;
    let server = builder.build(config).await;

    let mcp_client = server.mcp_client("/mcp").await;

    let result = mcp_client.search(&["Scientific"]).await;

    insta::assert_json_snapshot!(result, @r#"
    [
      {
        "tool": {
          "name": "math_service__calculator",
          "description": "Performs basic mathematical calculations including addition, subtraction, multiplication and division",
          "inputSchema": {
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
          "annotations": {
            "title": "Scientific Calculator"
          }
        },
        "score": 0.57536423
      }
    ]
    "#);

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn search_relevance_scoring_with_different_tools() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let mut math_service = TestService::streamable_http("math_service".to_string());
    math_service.add_tool(CalculatorTool).await;
    math_service.add_tool(AdderTool).await;

    let mut text_service = TestService::streamable_http("text_service".to_string());
    text_service.add_tool(TextProcessorTool).await;

    let mut builder = TestServer::builder();
    builder.spawn_service(math_service).await;
    builder.spawn_service(text_service).await;

    let server = builder.build(config).await;

    let mcp_client = server.mcp_client("/mcp").await;

    let result = mcp_client.search(&["text"]).await;

    insta::assert_json_snapshot!(result, @r#"
    [
      {
        "tool": {
          "name": "text_service__text_processor",
          "description": "Processes text with various string manipulation operations like case conversion and reversal",
          "inputSchema": {
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
          "annotations": {
            "title": "Text Processor"
          }
        },
        "score": 2.4428196
      }
    ]
    "#);

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn search_partial_word_matching() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let mut test_service = TestService::streamable_http("text_service".to_string());
    test_service.add_tool(TextProcessorTool).await;

    let mut builder = TestServer::builder();
    builder.spawn_service(test_service).await;
    let server = builder.build(config).await;

    let mcp_client = server.mcp_client("/mcp").await;

    let result = mcp_client.search(&["process"]).await;

    insta::assert_json_snapshot!(result, @r#"
    [
      {
        "tool": {
          "name": "text_service__text_processor",
          "description": "Processes text with various string manipulation operations like case conversion and reversal",
          "inputSchema": {
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
          "annotations": {
            "title": "Text Processor"
          }
        },
        "score": 0.4
      }
    ]
    "#);

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn search_compound_words() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let mut test_service = TestService::streamable_http("file_service".to_string());
    test_service.add_tool(FileSystemTool).await;

    let mut builder = TestServer::builder();
    builder.spawn_service(test_service).await;
    let server = builder.build(config).await;

    let mcp_client = server.mcp_client("/mcp").await;

    let result = mcp_client.search(&["filesystem"]).await;

    insta::assert_json_snapshot!(result, @r#"
    [
      {
        "tool": {
          "name": "file_service__filesystem",
          "description": "Manages files and directories with operations like listing, creating, and deleting",
          "inputSchema": {
            "type": "object",
            "properties": {
              "path": {
                "type": "string",
                "description": "File or directory path"
              },
              "operation": {
                "type": "string",
                "enum": [
                  "list",
                  "create",
                  "delete",
                  "exists"
                ],
                "description": "Filesystem operation to perform"
              }
            },
            "required": [
              "path",
              "operation"
            ]
          },
          "annotations": {
            "title": "File System Manager"
          }
        },
        "score": 0.8630463
      }
    ]
    "#);

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn search_enum_values() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let mut test_service = TestService::streamable_http("math_service".to_string());
    test_service.add_tool(CalculatorTool).await;

    let mut builder = TestServer::builder();
    builder.spawn_service(test_service).await;
    let server = builder.build(config).await;

    let mcp_client = server.mcp_client("/mcp").await;
    let result = mcp_client.search(&["multiply"]).await;

    insta::assert_json_snapshot!(result, @r#"
    [
      {
        "tool": {
          "name": "math_service__calculator",
          "description": "Performs basic mathematical calculations including addition, subtraction, multiplication and division",
          "inputSchema": {
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
          "annotations": {
            "title": "Scientific Calculator"
          }
        },
        "score": 0.4
      }
    ]
    "#);

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn search_deduplication_test() {
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

    let result = mcp_client.search(&["add", "numbers"]).await;

    insta::assert_json_snapshot!(result, @r#"
    [
      {
        "tool": {
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
        },
        "score": 0.6
      }
    ]
    "#);

    mcp_client.disconnect().await;
}
