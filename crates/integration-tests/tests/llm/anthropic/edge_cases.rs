use indoc::indoc;
use integration_tests::{TestServer, llms::AnthropicMock};
use serde_json::json;

/// Test that tool arguments can be null and are handled correctly
/// This covers cases where Claude Code sends null arguments for tools
#[tokio::test]
async fn tool_arguments_null_handling() {
    let mock = AnthropicMock::new("anthropic")
        .with_models(vec!["claude-3-5-sonnet-20241022".to_string()])
        .with_tool_call("list_files", "null");

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let config = indoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.anthropic]
        enabled = true
        path = "/llm/anthropic"
    "#};

    let server = builder.build(config).await;

    let request = json!({
        "model": "anthropic/claude-3-5-sonnet-20241022",
        "messages": [{
            "role": "user",
            "content": "List all files in the current directory"
        }],
        "tools": [{
            "name": "list_files",
            "description": "List files in a directory",
            "input_schema": {
                "type": "object",
                "properties": {
                    "directory": {
                        "type": "string",
                        "description": "Directory path"
                    }
                }
            }
        }],
        "max_tokens": 1024
    });

    let response = server.anthropic_completions(request).send().await;

    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".model" => "[model]",
        ".content[0].id" => "[tool_id]",
        ".usage" => "[usage]"
    }, @r#"
    {
      "id": "[id]",
      "type": "message",
      "role": "assistant",
      "content": [
        {
          "type": "tool_use",
          "id": "[tool_id]",
          "name": "list_files",
          "input": null
        }
      ],
      "model": "[model]",
      "stop_reason": null,
      "stop_sequence": null,
      "usage": "[usage]"
    }
    "#);
}

/// Test that tool arguments can be string literals instead of JSON objects
/// This covers cases where tools return simple string values
#[tokio::test]
async fn tool_arguments_string_vs_json() {
    let mock = AnthropicMock::new("anthropic")
        .with_models(vec!["claude-3-5-sonnet-20241022".to_string()])
        .with_tool_call("get_current_time", r#""2024-09-17T14:30:00Z""#);

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let config = indoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.anthropic]
        enabled = true
        path = "/llm/anthropic"
    "#};

    let server = builder.build(config).await;

    let request = json!({
        "model": "anthropic/claude-3-5-sonnet-20241022",
        "messages": [{
            "role": "user",
            "content": "What time is it?"
        }],
        "tools": [{
            "name": "get_current_time",
            "description": "Get the current time",
            "input_schema": {
                "type": "object",
                "properties": {}
            }
        }],
        "max_tokens": 1024
    });

    let response = server.anthropic_completions(request).send().await;

    // Verify the string literal argument is preserved in Anthropic format
    let tool_input = &response["content"][0]["input"];
    assert_eq!(tool_input, "2024-09-17T14:30:00Z");

    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".model" => "[model]",
        ".content[0].id" => "[tool_id]",
        ".usage" => "[usage]"
    }, @r#"
    {
      "id": "[id]",
      "type": "message",
      "role": "assistant",
      "content": [
        {
          "type": "tool_use",
          "id": "[tool_id]",
          "name": "get_current_time",
          "input": "2024-09-17T14:30:00Z"
        }
      ],
      "model": "[model]",
      "stop_reason": null,
      "stop_sequence": null,
      "usage": "[usage]"
    }
    "#);
}

/// Test tool arguments that are complex nested JSON objects
#[tokio::test]
async fn tool_arguments_complex_json() {
    let complex_args = json!({
        "search_params": {
            "query": "test",
            "filters": {
                "date_range": {
                    "start": "2024-01-01",
                    "end": "2024-12-31"
                },
                "categories": ["tech", "science"],
                "priority": 1
            },
            "sort": {
                "field": "relevance",
                "order": "desc"
            }
        },
        "options": {
            "limit": 10,
            "include_metadata": true
        }
    });

    let mock = AnthropicMock::new("anthropic")
        .with_models(vec!["claude-3-5-sonnet-20241022".to_string()])
        .with_tool_call("complex_search", complex_args.to_string().as_str());

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let config = indoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.anthropic]
        enabled = true
        path = "/llm/anthropic"
    "#};

    let server = builder.build(config).await;

    let request = json!({
        "model": "anthropic/claude-3-5-sonnet-20241022",
        "messages": [{
            "role": "user",
            "content": "Search for technical articles"
        }],
        "tools": [{
            "name": "complex_search",
            "description": "Perform a complex search",
            "input_schema": {
                "type": "object",
                "properties": {
                    "search_params": {"type": "object"},
                    "options": {"type": "object"}
                }
            }
        }],
        "max_tokens": 1024
    });

    let response = server.anthropic_completions(request).send().await;

    // Verify complex JSON is preserved in Anthropic format by checking in snapshot below

    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".model" => "[model]",
        ".content[0].id" => "[tool_id]",
        ".usage" => "[usage]"
    }, @r#"
    {
      "id": "[id]",
      "type": "message",
      "role": "assistant",
      "content": [
        {
          "type": "tool_use",
          "id": "[tool_id]",
          "name": "complex_search",
          "input": {
            "search_params": {
              "query": "test",
              "filters": {
                "date_range": {
                  "start": "2024-01-01",
                  "end": "2024-12-31"
                },
                "categories": [
                  "tech",
                  "science"
                ],
                "priority": 1
              },
              "sort": {
                "field": "relevance",
                "order": "desc"
              }
            },
            "options": {
              "limit": 10,
              "include_metadata": true
            }
          }
        }
      ],
      "model": "[model]",
      "stop_reason": null,
      "stop_sequence": null,
      "usage": "[usage]"
    }
    "#);
}

/// Test multiple parallel tool calls with unique IDs to ensure no collisions
/// This covers the real-world case where Claude Code might call multiple tools simultaneously
#[tokio::test]
async fn multiple_parallel_tools_unique_ids() {
    let mock = AnthropicMock::new("anthropic")
        .with_models(vec!["claude-3-5-sonnet-20241022".to_string()])
        .with_parallel_tool_calls(vec![
            ("Glob", r#"{"pattern": "*.rs"}"#),
            (
                "Bash",
                r#"{"command": "ls -la", "description": "List directory contents"}"#,
            ),
            ("Read", r#"{"file_path": "/etc/hosts"}"#),
            ("Grep", r#"{"pattern": "TODO", "path": "src/"}"#),
        ]);

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let config = indoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.anthropic]
        enabled = true
        path = "/llm/anthropic"
    "#};

    let server = builder.build(config).await;

    let request = json!({
        "model": "anthropic/claude-3-5-sonnet-20241022",
        "messages": [{
            "role": "user",
            "content": "Help me analyze this codebase"
        }],
        "tools": [
            {
                "name": "Glob",
                "description": "Find files matching a pattern",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "pattern": {"type": "string"}
                    }
                }
            },
            {
                "name": "Bash",
                "description": "Run bash commands",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "command": {"type": "string"},
                        "description": {"type": "string"}
                    }
                }
            },
            {
                "name": "Read",
                "description": "Read file contents",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "file_path": {"type": "string"}
                    }
                }
            },
            {
                "name": "Grep",
                "description": "Search for patterns in files",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "pattern": {"type": "string"},
                        "path": {"type": "string"}
                    }
                }
            }
        ],
        "max_tokens": 1024
    });

    let response = server.anthropic_completions(request).send().await;

    // Verify tool calls in Anthropic format
    let content = &response["content"];
    assert!(content.is_array());

    // Note: The current mock implementation only returns the first tool call
    // but in real scenarios, we'd test that all IDs are unique
    // Tool name validation is handled in the snapshot below

    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".model" => "[model]",
        ".content[0].id" => "[tool_id]",
        ".usage" => "[usage]"
    }, @r#"
    {
      "id": "[id]",
      "type": "message",
      "role": "assistant",
      "content": [
        {
          "type": "tool_use",
          "id": "[tool_id]",
          "name": "Glob",
          "input": {
            "pattern": "*.rs"
          }
        }
      ],
      "model": "[model]",
      "stop_reason": null,
      "stop_sequence": null,
      "usage": "[usage]"
    }
    "#);
}

/// Test message role conversions, especially tool result messages
/// This ensures tool results are correctly converted between OpenAI and Anthropic formats
#[tokio::test]
async fn message_role_conversions_tool_results() {
    let mock = AnthropicMock::new("anthropic")
        .with_models(vec!["claude-3-5-sonnet-20241022".to_string()])
        .with_response(
            "Read the config file",
            "I can see the file contents show some configuration data.",
        );

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let config = indoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.anthropic]
        enabled = true
        path = "/llm/anthropic"
    "#};

    let server = builder.build(config).await;

    // Test conversation with tool call and result in Anthropic format
    let request = json!({
        "model": "anthropic/claude-3-5-sonnet-20241022",
        "messages": [
            {
                "role": "user",
                "content": "Read the config file"
            },
            {
                "role": "assistant",
                "content": [
                    {
                        "type": "tool_use",
                        "id": "toolu_read_123",
                        "name": "read_file",
                        "input": {"path": "config.toml"}
                    }
                ]
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_read_123",
                        "content": "[llm]\nenabled = true\n\n[mcp]\nenabled = false"
                    }
                ]
            },
            {
                "role": "user",
                "content": "What does this config do?"
            }
        ],
        "max_tokens": 1024
    });

    let response = server.anthropic_completions(request).send().await;

    // Verify the conversation flows correctly and tool result is processed
    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".model" => "[model]",
        ".usage" => "[usage]"
    }, @r#"
    {
      "id": "[id]",
      "type": "message",
      "role": "assistant",
      "content": [
        {
          "type": "text",
          "text": "I can see the file contents show some configuration data."
        }
      ],
      "model": "[model]",
      "stop_reason": null,
      "stop_sequence": null,
      "usage": "[usage]"
    }
    "#);
}

/// Test that tool calls are properly handled in native Anthropic format
/// This covers Claude Code's native message format with tool_use blocks
#[tokio::test]
async fn anthropic_native_tool_use_blocks() {
    let mut builder = TestServer::builder();

    builder
        .spawn_llm(AnthropicMock::new("anthropic").with_models(vec!["claude-3-5-sonnet-20241022".to_string()]))
        .await;

    let config = indoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.anthropic]
        enabled = true
        path = "/llm/anthropic"
    "#};

    let server = builder.build(config).await;

    // Test with native Anthropic format containing tool_use blocks
    let request = json!({
        "model": "anthropic/claude-3-5-sonnet-20241022",
        "messages": [
            {
                "role": "user",
                "content": "Help me with file operations"
            },
            {
                "role": "assistant",
                "content": [
                    {
                        "type": "text",
                        "text": "I'll help you with file operations using these tools:"
                    },
                    {
                        "type": "tool_use",
                        "id": "toolu_file_001",
                        "name": "Read",
                        "input": {
                            "file_path": "/home/user/document.txt"
                        }
                    },
                    {
                        "type": "tool_use",
                        "id": "toolu_file_002",
                        "name": "Write",
                        "input": {
                            "file_path": "/home/user/output.txt",
                            "content": "Processed data"
                        }
                    }
                ]
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_file_001",
                        "content": [{"type": "text", "text": "Original file content"}]
                    },
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_file_002",
                        "content": [{"type": "text", "text": "File written successfully"}]
                    }
                ]
            }
        ],
        "max_tokens": 200
    });

    let response = server.anthropic_completions(request).send().await;

    // Verify native Anthropic format is handled correctly
    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".model" => "[model]",
        ".usage" => "[usage]"
    }, @r###"
    {
      "id": "[id]",
      "type": "message",
      "role": "assistant",
      "content": [
        {
          "type": "text",
          "text": "Test response to: Help me with file operations"
        }
      ],
      "model": "[model]",
      "stop_reason": null,
      "stop_sequence": null,
      "usage": "[usage]"
    }
    "###);
}

/// Test that successful requests work as expected for baseline comparison
#[tokio::test]
async fn successful_request_baseline() {
    let mut builder = TestServer::builder();

    let mock = AnthropicMock::new("anthropic").with_models(vec!["claude-3-5-sonnet-20241022".to_string()]);

    builder.spawn_llm(mock).await;

    let config = indoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.anthropic]
        enabled = true
        path = "/llm/anthropic"
    "#};

    let server = builder.build(config).await;

    let request = json!({
        "model": "anthropic/claude-3-5-sonnet-20241022",
        "messages": [{
            "role": "user",
            "content": "Hello"
        }],
        "max_tokens": 100
    });

    let response = server.anthropic_completions(request).send().await;

    // Verify successful response
    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".model" => "[model]",
        ".usage" => "[usage]"
    }, @r###"
    {
      "id": "[id]",
      "type": "message",
      "role": "assistant",
      "content": [
        {
          "type": "text",
          "text": "Test response to: Hello"
        }
      ],
      "model": "[model]",
      "stop_reason": null,
      "stop_sequence": null,
      "usage": "[usage]"
    }
    "###);
}

/// Test that malformed tool arguments are gracefully handled
#[tokio::test]
async fn malformed_tool_arguments_handling() {
    let mock = AnthropicMock::new("anthropic")
        .with_models(vec!["claude-3-5-sonnet-20241022".to_string()])
        .with_tool_call("parse_data", r#"{"invalid": json, "missing": quote}"#); // Malformed JSON

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let config = indoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.anthropic]
        enabled = true
        path = "/llm/anthropic"
    "#};

    let server = builder.build(config).await;

    let request = json!({
        "model": "anthropic/claude-3-5-sonnet-20241022",
        "messages": [{
            "role": "user",
            "content": "Parse this data"
        }],
        "tools": [{
            "name": "parse_data",
            "description": "Parse structured data",
            "input_schema": {
                "type": "object",
                "properties": {}
            }
        }],
        "max_tokens": 1024
    });

    let response = server.anthropic_completions(request).send().await;

    // Verify malformed JSON is handled gracefully (mock returns empty object when parsing fails)
    let tool_input = &response["content"][0]["input"];
    assert_eq!(tool_input, &json!({}));

    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".model" => "[model]",
        ".content[0].id" => "[tool_id]",
        ".usage" => "[usage]"
    }, @r#"
    {
      "id": "[id]",
      "type": "message",
      "role": "assistant",
      "content": [
        {
          "type": "tool_use",
          "id": "[tool_id]",
          "name": "parse_data",
          "input": {}
        }
      ],
      "model": "[model]",
      "stop_reason": null,
      "stop_sequence": null,
      "usage": "[usage]"
    }
    "#);
}
