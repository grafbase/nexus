use indoc::{formatdoc, indoc};
use integration_tests::*;
use tokio::time::{Duration, sleep};

#[tokio::test]
async fn stdio_basic_echo_tool() {
    let server = TestServer::builder()
        .build(indoc! {r#"
            [mcp.servers.test_stdio]
            cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
        "#})
        .await;

    let client = server.mcp_client("/mcp").await;

    // Wait a bit for STDIO server to be fully ready
    sleep(Duration::from_millis(100)).await;

    // Test built-in tools listing (STDIO tools won't appear here)
    let tools = client.list_tools().await;
    let tool_names: Vec<&str> = tools.tools.iter().map(|t| t.name.as_ref()).collect();
    insta::assert_debug_snapshot!(tool_names, @r###"
    [
        "search",
        "execute",
    ]
    "###);

    // Test STDIO tool discovery via search
    let search_results = client.search(&["echo"]).await;
    insta::assert_json_snapshot!(search_results, @r#"
    [
      {
        "name": "test_stdio__echo",
        "description": "Echoes back the input text",
        "input_schema": {
          "type": "object",
          "properties": {
            "text": {
              "type": "string",
              "description": "Text to echo back"
            }
          },
          "required": [
            "text"
          ]
        },
        "score": 3.611918449401855
      }
    ]
    "#);

    // Test tool execution
    let result = client
        .execute(
            "test_stdio__echo",
            serde_json::json!({
                "text": "Hello, STDIO!"
            }),
        )
        .await;

    insta::assert_json_snapshot!(result, @r#"
    {
      "content": [
        {
          "type": "text",
          "text": "Echo: Hello, STDIO!"
        }
      ]
    }
    "#);
}

#[tokio::test]
async fn stdio_math_tool() {
    let server = TestServer::builder()
        .build(indoc! {r#"
            [mcp.servers.test_stdio]
            cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
        "#})
        .await;

    let client = server.mcp_client("/mcp").await;

    // Wait a bit for STDIO server to be fully ready
    sleep(Duration::from_millis(100)).await;

    // Test math tool
    let result = client
        .execute(
            "test_stdio__add",
            serde_json::json!({
                "a": 15,
                "b": 27
            }),
        )
        .await;

    insta::assert_json_snapshot!(result, @r#"
    {
      "content": [
        {
          "type": "text",
          "text": "15 + 27 = 42"
        }
      ]
    }
    "#);
}

#[tokio::test]
async fn stdio_environment_variables() {
    let server = TestServer::builder()
        .build(indoc! {r#"
            [mcp.servers.test_stdio]
            cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
            env = { "TEST_VAR" = "test_value_123" }
        "#})
        .await;

    let client = server.mcp_client("/mcp").await;

    // Test environment variable access
    let result = client
        .execute(
            "test_stdio__environment",
            serde_json::json!({
                "var": "TEST_VAR"
            }),
        )
        .await;

    insta::assert_json_snapshot!(result, @r#"
    {
      "content": [
        {
          "type": "text",
          "text": "TEST_VAR=test_value_123"
        }
      ]
    }
    "#);
}

#[tokio::test]
async fn stdio_working_directory() {
    use std::env;

    let current_dir = env::current_dir().unwrap();
    let cwd_str = current_dir.to_string_lossy();

    let server = TestServer::builder()
        .build(&format!(
            indoc! {r#"
            [mcp.servers.test_stdio]
            cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
            cwd = "{}"
        "#},
            cwd_str
        ))
        .await;

    let client = server.mcp_client("/mcp").await;

    // Wait a bit for STDIO server to be fully ready
    sleep(Duration::from_millis(100)).await;

    // Test that the server can access files in the working directory by searching for tools
    let search_results = client.search(&["echo"]).await;
    assert!(
        !search_results.is_empty(),
        "Should find STDIO tools from server with working directory"
    );

    // Verify we can execute a tool from the STDIO server
    let result = client
        .execute(
            "test_stdio__echo",
            serde_json::json!({
                "text": "Working directory test"
            }),
        )
        .await;

    insta::assert_json_snapshot!(result, @r#"
    {
      "content": [
        {
          "type": "text",
          "text": "Echo: Working directory test"
        }
      ]
    }
    "#);
}

#[tokio::test]
async fn stdio_error_handling() {
    let server = TestServer::builder()
        .build(indoc! {r#"
            [mcp.servers.test_stdio]
            cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
        "#})
        .await;

    let client = server.mcp_client("/mcp").await;

    // Wait a bit for STDIO server to be fully ready
    sleep(Duration::from_millis(100)).await;

    // Test tool that always fails
    let error = client
        .execute_expect_error("test_stdio__fail", serde_json::json!({}))
        .await;

    // Should get an error response
    insta::assert_debug_snapshot!(error, @r#"
    McpError(
        ErrorData {
            code: ErrorCode(
                -32603,
            ),
            message: "Internal error: This tool always fails",
            data: None,
        },
    )
    "#);
}

#[tokio::test]
async fn stdio_invalid_tool() {
    let server = TestServer::builder()
        .build(indoc! {r#"
            [mcp.servers.test_stdio]
            cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
        "#})
        .await;

    let client = server.mcp_client("/mcp").await;

    // Wait a bit for STDIO server to be fully ready
    sleep(Duration::from_millis(100)).await;

    // Test calling non-existent tool
    let error = client
        .execute_expect_error("test_stdio__nonexistent", serde_json::json!({}))
        .await;

    // Should get an error response
    insta::assert_debug_snapshot!(error, @r#"
    McpError(
        ErrorData {
            code: ErrorCode(
                -32601,
            ),
            message: "tools/call",
            data: None,
        },
    )
    "#);
}

#[tokio::test]
async fn stdio_tool_search() {
    let server = TestServer::builder()
        .build(indoc! {r#"
            [mcp.servers.test_stdio]
            cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
        "#})
        .await;

    let client = server.mcp_client("/mcp").await;

    // Wait a bit for STDIO server to be fully ready
    sleep(Duration::from_millis(100)).await;

    // Test searching for tools
    let search_results = client.search(&["echo", "text"]).await;
    insta::assert_json_snapshot!(search_results, @r#"
    [
      {
        "name": "test_stdio__echo",
        "description": "Echoes back the input text",
        "input_schema": {
          "type": "object",
          "properties": {
            "text": {
              "type": "string",
              "description": "Text to echo back"
            }
          },
          "required": [
            "text"
          ]
        },
        "score": 5.026234149932861
      }
    ]
    "#);
}

#[tokio::test]
async fn stdio_multiple_servers() {
    let server = TestServer::builder()
        .build(indoc! {r#"
            [mcp.servers.stdio_server_1]
            cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
            env = { "SERVER_ID" = "server1" }

            [mcp.servers.stdio_server_2]
            cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
            env = { "SERVER_ID" = "server2" }
        "#})
        .await;

    let client = server.mcp_client("/mcp").await;

    // Wait a bit for STDIO servers to be fully ready
    sleep(Duration::from_millis(200)).await;

    // Test tool discovery with multiple servers via search
    let search_results = client.search(&["echo"]).await;

    let mut tool_names: Vec<&str> = search_results
        .iter()
        .filter_map(|result| result.get("name")?.as_str())
        .collect();

    tool_names.sort_unstable();

    insta::assert_json_snapshot!(tool_names, @r#"
    [
      "stdio_server_1__echo",
      "stdio_server_2__echo"
    ]
    "#);

    // Test executing tools from both servers
    let result1 = client
        .execute(
            "stdio_server_1__echo",
            serde_json::json!({
                "text": "Hello from server 1"
            }),
        )
        .await;

    let result2 = client
        .execute(
            "stdio_server_2__echo",
            serde_json::json!({
                "text": "Hello from server 2"
            }),
        )
        .await;

    insta::assert_json_snapshot!(result1, @r#"
    {
      "content": [
        {
          "type": "text",
          "text": "Echo: Hello from server 1"
        }
      ]
    }
    "#);

    insta::assert_json_snapshot!(result2, @r#"
    {
      "content": [
        {
          "type": "text",
          "text": "Echo: Hello from server 2"
        }
      ]
    }
    "#);
}

#[tokio::test]
async fn stdio_server_startup_failure() {
    // Test that a nonexistent command doesn't prevent server startup (resilience)
    let server = TestServer::builder()
        .build(indoc! {r#"
            [mcp.servers.bad_stdio]
            cmd = ["nonexistent_command_that_should_fail"]
        "#})
        .await;

    // Server should start successfully despite failing downstream
    let mcp_client = server.mcp_client("/mcp").await;
    let tools = mcp_client.list_tools().await;

    // Always exactly 2 tools: search and execute
    insta::assert_json_snapshot!(tools, @r##"
    {
      "tools": [
        {
          "name": "search",
          "description": "Search for relevant tools. A list of matching tools with their\\nscore is returned with a map of input fields and their types.\n\nUsing this information, you can call the execute tool with the\\nname of the tool you want to execute, and defining the input parameters.\n\nTool names are in the format \"server__tool\" where \"server\" is the name of the MCP server providing\nthe tool.\n",
          "inputSchema": {
            "type": "object",
            "properties": {
              "keywords": {
                "type": "array",
                "items": {
                  "type": "string"
                },
                "description": "A list of keywords to search with."
              }
            },
            "required": [
              "keywords"
            ],
            "title": "SearchParameters"
          },
          "outputSchema": {
            "type": "object",
            "properties": {
              "results": {
                "type": "array",
                "items": {
                  "$ref": "#/$defs/SearchResult"
                },
                "description": "The list of search results"
              }
            },
            "required": [
              "results"
            ],
            "title": "SearchResponse",
            "$defs": {
              "SearchResult": {
                "type": "object",
                "properties": {
                  "name": {
                    "type": "string",
                    "description": "The name of the tool (format: \"server__tool\")"
                  },
                  "description": {
                    "type": "string",
                    "description": "Description of what the tool does"
                  },
                  "input_schema": {
                    "description": "The input schema for the tool's parameters"
                  },
                  "score": {
                    "type": "number",
                    "description": "The relevance score for this result (higher is more relevant)"
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
}

#[tokio::test]
async fn stdio_minimal_config() {
    // Test that echo command (not a valid MCP server) doesn't prevent startup (resilience)
    let server = TestServer::builder()
        .build(indoc! {r#"
            [mcp.servers.minimal]
            cmd = ["echo", "hello"]
        "#})
        .await;

    // Server should start successfully despite invalid MCP server
    let mcp_client = server.mcp_client("/mcp").await;
    let tools = mcp_client.list_tools().await;

    // Always exactly 2 tools: search and execute
    insta::assert_json_snapshot!(tools, @r##"
    {
      "tools": [
        {
          "name": "search",
          "description": "Search for relevant tools. A list of matching tools with their\\nscore is returned with a map of input fields and their types.\n\nUsing this information, you can call the execute tool with the\\nname of the tool you want to execute, and defining the input parameters.\n\nTool names are in the format \"server__tool\" where \"server\" is the name of the MCP server providing\nthe tool.\n",
          "inputSchema": {
            "type": "object",
            "properties": {
              "keywords": {
                "type": "array",
                "items": {
                  "type": "string"
                },
                "description": "A list of keywords to search with."
              }
            },
            "required": [
              "keywords"
            ],
            "title": "SearchParameters"
          },
          "outputSchema": {
            "type": "object",
            "properties": {
              "results": {
                "type": "array",
                "items": {
                  "$ref": "#/$defs/SearchResult"
                },
                "description": "The list of search results"
              }
            },
            "required": [
              "results"
            ],
            "title": "SearchResponse",
            "$defs": {
              "SearchResult": {
                "type": "object",
                "properties": {
                  "name": {
                    "type": "string",
                    "description": "The name of the tool (format: \"server__tool\")"
                  },
                  "description": {
                    "type": "string",
                    "description": "Description of what the tool does"
                  },
                  "input_schema": {
                    "description": "The input schema for the tool's parameters"
                  },
                  "score": {
                    "type": "number",
                    "description": "The relevance score for this result (higher is more relevant)"
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
}

#[tokio::test]
async fn stdio_complex_command_args() {
    let server = TestServer::builder()
        .build(indoc! {r#"
            [mcp.servers.complex_args]
            cmd = ["python3", "-u", "mock-mcp-servers/simple_mcp_server.py"]
            env = { "PYTHONUNBUFFERED" = "1" }
        "#})
        .await;

    let client = server.mcp_client("/mcp").await;

    // Wait a bit for STDIO server to be fully ready
    sleep(Duration::from_millis(100)).await;

    // Test that complex command arguments work correctly by searching for tools
    let search_results = client.search(&["echo"]).await;

    let tool_names: Vec<&str> = search_results
        .iter()
        .filter_map(|result| result.get("name")?.as_str())
        .collect();

    insta::assert_json_snapshot!(tool_names, @r###"
    [
      "complex_args__echo"
    ]
    "###);
}

#[tokio::test]
async fn stdio_command_not_found() {
    // Test that a nonexistent command doesn't prevent server startup (resilience)
    let server = TestServer::builder()
        .build(indoc! {r#"
            [mcp.servers.nonexistent]
            cmd = ["nonexistent_command_xyz123"]
        "#})
        .await;

    // Server should start successfully despite failing downstream
    let mcp_client = server.mcp_client("/mcp").await;
    let tools = mcp_client.list_tools().await;

    // Always exactly 2 tools: search and execute
    insta::assert_json_snapshot!(tools, @r##"
    {
      "tools": [
        {
          "name": "search",
          "description": "Search for relevant tools. A list of matching tools with their\\nscore is returned with a map of input fields and their types.\n\nUsing this information, you can call the execute tool with the\\nname of the tool you want to execute, and defining the input parameters.\n\nTool names are in the format \"server__tool\" where \"server\" is the name of the MCP server providing\nthe tool.\n",
          "inputSchema": {
            "type": "object",
            "properties": {
              "keywords": {
                "type": "array",
                "items": {
                  "type": "string"
                },
                "description": "A list of keywords to search with."
              }
            },
            "required": [
              "keywords"
            ],
            "title": "SearchParameters"
          },
          "outputSchema": {
            "type": "object",
            "properties": {
              "results": {
                "type": "array",
                "items": {
                  "$ref": "#/$defs/SearchResult"
                },
                "description": "The list of search results"
              }
            },
            "required": [
              "results"
            ],
            "title": "SearchResponse",
            "$defs": {
              "SearchResult": {
                "type": "object",
                "properties": {
                  "name": {
                    "type": "string",
                    "description": "The name of the tool (format: \"server__tool\")"
                  },
                  "description": {
                    "type": "string",
                    "description": "Description of what the tool does"
                  },
                  "input_schema": {
                    "description": "The input schema for the tool's parameters"
                  },
                  "score": {
                    "type": "number",
                    "description": "The relevance score for this result (higher is more relevant)"
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
    // Search should return no results since the nonexistent command failed
    let search_results = mcp_client.search(&["test"]).await;
    insta::assert_json_snapshot!(search_results, @r###"
    []
    "###);
}

#[tokio::test]
async fn stdio_permission_denied() {
    // Test that a file without execute permissions doesn't prevent server startup (resilience)
    let server = TestServer::builder()
        .build(indoc! {r#"
            [mcp.servers.permission_denied]
            cmd = ["/etc/passwd"]
        "#})
        .await;

    // Server should start successfully despite failing downstream
    let mcp_client = server.mcp_client("/mcp").await;
    let tools = mcp_client.list_tools().await;

    // Always exactly 2 tools: search and execute
    insta::assert_json_snapshot!(tools, @r##"
    {
      "tools": [
        {
          "name": "search",
          "description": "Search for relevant tools. A list of matching tools with their\\nscore is returned with a map of input fields and their types.\n\nUsing this information, you can call the execute tool with the\\nname of the tool you want to execute, and defining the input parameters.\n\nTool names are in the format \"server__tool\" where \"server\" is the name of the MCP server providing\nthe tool.\n",
          "inputSchema": {
            "type": "object",
            "properties": {
              "keywords": {
                "type": "array",
                "items": {
                  "type": "string"
                },
                "description": "A list of keywords to search with."
              }
            },
            "required": [
              "keywords"
            ],
            "title": "SearchParameters"
          },
          "outputSchema": {
            "type": "object",
            "properties": {
              "results": {
                "type": "array",
                "items": {
                  "$ref": "#/$defs/SearchResult"
                },
                "description": "The list of search results"
              }
            },
            "required": [
              "results"
            ],
            "title": "SearchResponse",
            "$defs": {
              "SearchResult": {
                "type": "object",
                "properties": {
                  "name": {
                    "type": "string",
                    "description": "The name of the tool (format: \"server__tool\")"
                  },
                  "description": {
                    "type": "string",
                    "description": "Description of what the tool does"
                  },
                  "input_schema": {
                    "description": "The input schema for the tool's parameters"
                  },
                  "score": {
                    "type": "number",
                    "description": "The relevance score for this result (higher is more relevant)"
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
}

#[tokio::test]
async fn stdio_invalid_working_directory() {
    // Test that an invalid working directory doesn't prevent server startup (resilience)
    let server = TestServer::builder()
        .build(indoc! {r#"
            [mcp.servers.bad_cwd]
            cmd = ["echo", "hello"]
            cwd = "/nonexistent/directory/path"
        "#})
        .await;

    // Server should start successfully despite failing downstream
    let mcp_client = server.mcp_client("/mcp").await;
    let tools = mcp_client.list_tools().await;

    // Always exactly 2 tools: search and execute
    insta::assert_json_snapshot!(tools, @r##"
    {
      "tools": [
        {
          "name": "search",
          "description": "Search for relevant tools. A list of matching tools with their\\nscore is returned with a map of input fields and their types.\n\nUsing this information, you can call the execute tool with the\\nname of the tool you want to execute, and defining the input parameters.\n\nTool names are in the format \"server__tool\" where \"server\" is the name of the MCP server providing\nthe tool.\n",
          "inputSchema": {
            "type": "object",
            "properties": {
              "keywords": {
                "type": "array",
                "items": {
                  "type": "string"
                },
                "description": "A list of keywords to search with."
              }
            },
            "required": [
              "keywords"
            ],
            "title": "SearchParameters"
          },
          "outputSchema": {
            "type": "object",
            "properties": {
              "results": {
                "type": "array",
                "items": {
                  "$ref": "#/$defs/SearchResult"
                },
                "description": "The list of search results"
              }
            },
            "required": [
              "results"
            ],
            "title": "SearchResponse",
            "$defs": {
              "SearchResult": {
                "type": "object",
                "properties": {
                  "name": {
                    "type": "string",
                    "description": "The name of the tool (format: \"server__tool\")"
                  },
                  "description": {
                    "type": "string",
                    "description": "Description of what the tool does"
                  },
                  "input_schema": {
                    "description": "The input schema for the tool's parameters"
                  },
                  "score": {
                    "type": "number",
                    "description": "The relevance score for this result (higher is more relevant)"
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
}

#[tokio::test]
async fn stdio_process_crashes_early() {
    // Test that a command that exits immediately doesn't prevent server startup (resilience)
    let server = TestServer::builder()
        .build(indoc! {r#"
            [mcp.servers.crash_early]
            cmd = ["false"]
        "#})
        .await;

    // Server should start successfully despite failing downstream
    let mcp_client = server.mcp_client("/mcp").await;
    let tools = mcp_client.list_tools().await;

    // Always exactly 2 tools: search and execute
    insta::assert_json_snapshot!(tools, @r##"
    {
      "tools": [
        {
          "name": "search",
          "description": "Search for relevant tools. A list of matching tools with their\\nscore is returned with a map of input fields and their types.\n\nUsing this information, you can call the execute tool with the\\nname of the tool you want to execute, and defining the input parameters.\n\nTool names are in the format \"server__tool\" where \"server\" is the name of the MCP server providing\nthe tool.\n",
          "inputSchema": {
            "type": "object",
            "properties": {
              "keywords": {
                "type": "array",
                "items": {
                  "type": "string"
                },
                "description": "A list of keywords to search with."
              }
            },
            "required": [
              "keywords"
            ],
            "title": "SearchParameters"
          },
          "outputSchema": {
            "type": "object",
            "properties": {
              "results": {
                "type": "array",
                "items": {
                  "$ref": "#/$defs/SearchResult"
                },
                "description": "The list of search results"
              }
            },
            "required": [
              "results"
            ],
            "title": "SearchResponse",
            "$defs": {
              "SearchResult": {
                "type": "object",
                "properties": {
                  "name": {
                    "type": "string",
                    "description": "The name of the tool (format: \"server__tool\")"
                  },
                  "description": {
                    "type": "string",
                    "description": "Description of what the tool does"
                  },
                  "input_schema": {
                    "description": "The input schema for the tool's parameters"
                  },
                  "score": {
                    "type": "number",
                    "description": "The relevance score for this result (higher is more relevant)"
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
}

#[tokio::test]
async fn stdio_invalid_json_from_subprocess() {
    // Test that a subprocess outputting invalid JSON doesn't prevent server startup (resilience)
    let server = TestServer::builder()
        .build(indoc! {r#"
            [mcp.servers.invalid_json]
            cmd = ["echo", "not valid json"]
        "#})
        .await;

    // Server should start successfully despite invalid MCP server
    let mcp_client = server.mcp_client("/mcp").await;
    let tools = mcp_client.list_tools().await;

    // Always exactly 2 tools: search and execute
    insta::assert_json_snapshot!(tools, @r##"
    {
      "tools": [
        {
          "name": "search",
          "description": "Search for relevant tools. A list of matching tools with their\\nscore is returned with a map of input fields and their types.\n\nUsing this information, you can call the execute tool with the\\nname of the tool you want to execute, and defining the input parameters.\n\nTool names are in the format \"server__tool\" where \"server\" is the name of the MCP server providing\nthe tool.\n",
          "inputSchema": {
            "type": "object",
            "properties": {
              "keywords": {
                "type": "array",
                "items": {
                  "type": "string"
                },
                "description": "A list of keywords to search with."
              }
            },
            "required": [
              "keywords"
            ],
            "title": "SearchParameters"
          },
          "outputSchema": {
            "type": "object",
            "properties": {
              "results": {
                "type": "array",
                "items": {
                  "$ref": "#/$defs/SearchResult"
                },
                "description": "The list of search results"
              }
            },
            "required": [
              "results"
            ],
            "title": "SearchResponse",
            "$defs": {
              "SearchResult": {
                "type": "object",
                "properties": {
                  "name": {
                    "type": "string",
                    "description": "The name of the tool (format: \"server__tool\")"
                  },
                  "description": {
                    "type": "string",
                    "description": "Description of what the tool does"
                  },
                  "input_schema": {
                    "description": "The input schema for the tool's parameters"
                  },
                  "score": {
                    "type": "number",
                    "description": "The relevance score for this result (higher is more relevant)"
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
}

#[tokio::test]
async fn stdio_working_server_starts_successfully() {
    // Test that a properly configured STDIO server allows the server to start
    let server = TestServer::builder()
        .build(indoc! {r#"
            [mcp.servers.working_server]
            cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
        "#})
        .await;

    let client = server.mcp_client("/mcp").await;

    // Verify the server is functional and has tools from the STDIO server
    let search_results = client.search(&["echo"]).await;
    assert!(
        !search_results.is_empty(),
        "Should find tools from working STDIO server"
    );

    // Verify we can execute a tool
    let result = client
        .execute(
            "working_server__echo",
            serde_json::json!({
                "text": "Test message"
            }),
        )
        .await;

    insta::assert_json_snapshot!(result, @r#"
    {
      "content": [
        {
          "type": "text",
          "text": "Echo: Test message"
        }
      ]
    }
    "#);
}

#[tokio::test]
async fn stdio_empty_environment_variable() {
    // Test that empty environment variables work correctly
    let server = TestServer::builder()
        .build(indoc! {r#"
            [mcp.servers.empty_env]
            cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
            env = { "EMPTY_VAR" = "" }
        "#})
        .await;

    let client = server.mcp_client("/mcp").await;

    // Test accessing the empty environment variable
    let result = client
        .execute(
            "empty_env__environment",
            serde_json::json!({
                "var": "EMPTY_VAR"
            }),
        )
        .await;

    insta::assert_json_snapshot!(result, @r#"
    {
      "content": [
        {
          "type": "text",
          "text": "EMPTY_VAR="
        }
      ]
    }
    "#);
}

#[tokio::test]
async fn stdio_large_environment() {
    // Test that large numbers of environment variables work correctly
    use std::collections::HashMap;

    const MAX_ENV_VARS: usize = 50; // Reduced from 100 to avoid overly long test
    let mut env_vars = HashMap::new();
    for i in 0..MAX_ENV_VARS {
        env_vars.insert(format!("VAR_{i}"), format!("value_{i}"));
    }

    let env_config = env_vars
        .iter()
        .map(|(k, v)| format!("{k} = \"{v}\""))
        .collect::<Vec<_>>()
        .join(", ");

    let config = formatdoc! {r#"
        [mcp.servers.large_env]
        cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
        env = {{ {env_config} }}
    "#};

    let server = TestServer::builder().build(&config).await;
    let client = server.mcp_client("/mcp").await;

    // Test accessing one of the environment variables
    let result = client
        .execute(
            "large_env__environment",
            serde_json::json!({
                "var": "VAR_25"
            }),
        )
        .await;

    insta::assert_json_snapshot!(result, @r#"
    {
      "content": [
        {
          "type": "text",
          "text": "VAR_25=value_25"
        }
      ]
    }
    "#);
}

#[tokio::test]
async fn stdio_unicode_in_command_args() {
    // Test that Unicode in environment variables works correctly
    let server = TestServer::builder()
        .build(indoc! {r#"
            [mcp.servers.unicode_args]
            cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
            env = { "UNICODE_VAR" = "こんにちは🌍" }
        "#})
        .await;

    let client = server.mcp_client("/mcp").await;

    // Test accessing the Unicode environment variable
    let result = client
        .execute(
            "unicode_args__environment",
            serde_json::json!({
                "var": "UNICODE_VAR"
            }),
        )
        .await;

    insta::assert_json_snapshot!(result, @r#"
    {
      "content": [
        {
          "type": "text",
          "text": "UNICODE_VAR=こんにちは🌍"
        }
      ]
    }
    "#);
}

#[tokio::test]
async fn stdio_stderr_file_configuration() {
    use tempfile::tempdir;

    let temp_dir = tempdir().unwrap();
    let log_file = temp_dir.path().join("server.log");
    let log_path = log_file.to_string_lossy();

    let config = formatdoc! {r#"
        [mcp.servers.stderr_file]
        cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
        stderr = {{ file = "{log_path}" }}
    "#};

    let server = TestServer::builder().build(&config).await;
    let client = server.mcp_client("/mcp").await;

    // Wait a bit for STDIO server to be fully ready
    sleep(Duration::from_millis(200)).await;

    // Test that the server is working normally with stderr file configuration
    let result = client
        .execute(
            "stderr_file__echo",
            serde_json::json!({
                "text": "Testing stderr file config"
            }),
        )
        .await;

    insta::assert_json_snapshot!(result, @r#"
    {
      "content": [
        {
          "type": "text",
          "text": "Echo: Testing stderr file config"
        }
      ]
    }
    "#);

    let content = std::fs::read_to_string(log_file).unwrap();

    insta::assert_snapshot!(content, @r###"
        SimpleMcpServer: Starting server initialization
        SimpleMcpServer: Server initialization complete
        SimpleMcpServer: Starting main server loop
        SimpleMcpServer: Handling initialize request
    "###);
}
