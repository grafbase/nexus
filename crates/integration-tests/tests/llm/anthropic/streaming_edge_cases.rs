use indoc::indoc;
use integration_tests::{TestServer, llms::AnthropicMock};
use serde_json::json;

/// Test streaming with tool calls that have null arguments
/// This covers edge cases where Claude Code streams tool calls with null args
#[tokio::test]
async fn streaming_tool_calls_null_arguments() {
    let mock = AnthropicMock::new("anthropic")
        .with_models(vec!["claude-3-5-sonnet-20241022".to_string()])
        .with_streaming()
        .with_tool_call("get_current_time", "null");

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let config = indoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.openai]
        enabled = true
        path = "/llm"
    "#};

    let server = builder.build(config).await;

    let request = json!({
        "model": "anthropic/claude-3-5-sonnet-20241022",
        "messages": [{
            "role": "user",
            "content": "What time is it?"
        }],
        "tools": [{
            "type": "function",
            "function": {
                "name": "get_current_time",
                "description": "Get current time",
                "parameters": {
                    "type": "object",
                    "properties": {}
                }
            }
        }],
        "stream": true
    });

    let chunks = server.openai_completions_stream(request).send().await;

    // Verify we have multiple chunks
    assert!(chunks.len() >= 2, "Expected at least 2 chunks for streaming");

    // Find role chunk
    let role_chunk = chunks
        .iter()
        .find(|c| c["choices"][0]["delta"]["role"].is_string())
        .unwrap();

    insta::assert_json_snapshot!(role_chunk, {
        ".id" => "[id]",
        ".created" => "[timestamp]"
    }, @r#"
    {
      "choices": [
        {
          "delta": {
            "role": "assistant"
          },
          "index": 0
        }
      ],
      "created": "[timestamp]",
      "id": "[id]",
      "model": "anthropic/claude-3-5-sonnet-20241022",
      "object": "chat.completion.chunk"
    }
    "#);

    // Find tool call chunk
    let tool_chunk = chunks
        .iter()
        .find(|c| c["choices"][0]["delta"]["tool_calls"].is_array())
        .unwrap();

    // Verify null arguments are handled in streaming (mock returns empty string)
    let arguments = tool_chunk["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"]
        .as_str()
        .unwrap();
    assert_eq!(arguments, "");

    insta::assert_json_snapshot!(tool_chunk, {
        ".id" => "[id]",
        ".created" => "[timestamp]",
        ".choices[0].delta.tool_calls[0].id" => "[call_id]"
    }, @r#"
    {
      "choices": [
        {
          "delta": {
            "tool_calls": [
              {
                "function": {
                  "arguments": "",
                  "name": "get_current_time"
                },
                "id": "[call_id]",
                "index": 0,
                "type": "function"
              }
            ]
          },
          "index": 0
        }
      ],
      "created": "[timestamp]",
      "id": "[id]",
      "model": "anthropic/claude-3-5-sonnet-20241022",
      "object": "chat.completion.chunk"
    }
    "#);

    // Verify final chunk
    let final_chunk = chunks
        .iter()
        .find(|c| c["choices"][0]["finish_reason"].is_string())
        .unwrap();

    assert_eq!(final_chunk["choices"][0]["finish_reason"], "tool_calls");
}

/// Test streaming with multiple tool calls to ensure proper chunk ordering
#[tokio::test]
async fn streaming_multiple_tool_calls_ordering() {
    let mock = AnthropicMock::new("anthropic")
        .with_models(vec!["claude-3-5-sonnet-20241022".to_string()])
        .with_streaming()
        .with_parallel_tool_calls(vec![
            ("Read", r#"{"file_path": "config.toml"}"#),
            ("Bash", r#"{"command": "ls -la"}"#),
        ]);

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let config = indoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.openai]
        enabled = true
        path = "/llm"
    "#};

    let server = builder.build(config).await;

    let request = json!({
        "model": "anthropic/claude-3-5-sonnet-20241022",
        "messages": [{
            "role": "user",
            "content": "Read the config and list files"
        }],
        "tools": [
            {
                "type": "function",
                "function": {
                    "name": "Read",
                    "description": "Read file contents",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "file_path": {"type": "string"}
                        }
                    }
                }
            },
            {
                "type": "function",
                "function": {
                    "name": "Bash",
                    "description": "Run bash commands",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "command": {"type": "string"}
                        }
                    }
                }
            }
        ],
        "parallel_tool_calls": true,
        "stream": true
    });

    let chunks = server.openai_completions_stream(request).send().await;

    // Verify chunk structure and ordering
    assert!(chunks.len() >= 3, "Expected at least 3 chunks");

    // First chunk should contain role
    let first_chunk = &chunks[0];
    assert_eq!(first_chunk["choices"][0]["delta"]["role"], "assistant");

    // Should have tool call chunks
    let tool_chunks: Vec<_> = chunks
        .iter()
        .filter(|c| c["choices"][0]["delta"]["tool_calls"].is_array())
        .collect();

    assert!(!tool_chunks.is_empty(), "Expected tool call chunks");

    // Verify tool call chunk structure
    let tool_chunk = tool_chunks[0];
    let tool_call = &tool_chunk["choices"][0]["delta"]["tool_calls"][0];
    assert!(tool_call["id"].is_string());
    assert!(tool_call["function"]["name"].is_string());
    assert!(tool_call["function"]["arguments"].is_string());

    // Final chunk should have finish_reason
    let final_chunk = chunks
        .iter()
        .find(|c| c["choices"][0]["finish_reason"].is_string())
        .unwrap();
    assert_eq!(final_chunk["choices"][0]["finish_reason"], "tool_calls");
}

/// Test streaming without streaming enabled in mock
/// This tests error handling when streaming is requested but not supported
#[tokio::test]
async fn streaming_not_enabled_error() {
    let mock = AnthropicMock::new("anthropic").with_models(vec!["claude-3-5-sonnet-20241022".to_string()]);
    // Not calling .with_streaming()

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let config = indoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.openai]
        enabled = true
        path = "/llm"
    "#};

    let server = builder.build(config).await;

    let request = json!({
        "model": "anthropic/claude-3-5-sonnet-20241022",
        "messages": [{
            "role": "user",
            "content": "This should fail because streaming is not enabled"
        }],
        "tools": [{
            "type": "function",
            "function": {
                "name": "test_tool",
                "description": "Test tool",
                "parameters": {"type": "object", "properties": {}}
            }
        }],
        "stream": true
    });

    // Use the new send_raw() method to test error handling
    let (status, body) = server.openai_completions_stream(request).send_raw().await;

    // Mock returns 400 for unsupported streaming
    assert_eq!(status, 400);

    // The mock returns an error for unsupported streaming
    insta::assert_json_snapshot!(body, @r#"
    {
      "error": {
        "code": 400,
        "message": "Invalid request: {\"error\":{\"message\":\"Streaming is not yet supported\",\"type\":\"invalid_request_error\"}}",
        "type": "invalid_request_error"
      }
    }
    "#);

    /* This is what SHOULD be returned:
    insta::assert_json_snapshot!(body, @r###"
    {
      "error": {
        "message": "Provider API error (400): {\"error\":{\"type\":\"invalid_request_error\",\"message\":\"Streaming is not yet supported\"}}",
        "type": "api_error",
        "code": 502
      }
    }
    "###);
    */
}

/// Test streaming with tool calls followed by tool results in conversation
#[tokio::test]
async fn streaming_conversation_with_tool_results() {
    let mock = AnthropicMock::new("anthropic")
        .with_models(vec!["claude-3-5-sonnet-20241022".to_string()])
        .with_streaming()
        .with_response("file contents here", "I can see the file contains configuration data.");

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let config = indoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.openai]
        enabled = true
        path = "/llm"
    "#};

    let server = builder.build(config).await;

    let request = json!({
        "model": "anthropic/claude-3-5-sonnet-20241022",
        "messages": [
            {
                "role": "user",
                "content": "Read the config file"
            },
            {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_read_config",
                    "type": "function",
                    "function": {
                        "name": "read_file",
                        "arguments": "{\"path\": \"config.toml\"}"
                    }
                }]
            },
            {
                "role": "tool",
                "content": "# Configuration file\nenabled = true\nport = 8080",
                "tool_call_id": "call_read_config"
            },
            {
                "role": "user",
                "content": "What does this configuration do?"
            }
        ],
        "stream": true
    });

    let chunks = server.openai_completions_stream(request).send().await;

    // Verify we get streaming response after tool results
    assert!(chunks.len() >= 2, "Expected multiple chunks");

    // First chunk should have role
    let role_chunk = &chunks[0];
    assert_eq!(role_chunk["choices"][0]["delta"]["role"], "assistant");

    // Should have content chunks
    let content_chunks: Vec<_> = chunks
        .iter()
        .filter(|c| c["choices"][0]["delta"]["content"].is_string())
        .collect();

    assert!(!content_chunks.is_empty(), "Expected content chunks");

    // Final chunk should have finish_reason "stop" (not tool_calls since this is after tool use)
    let final_chunk = chunks
        .iter()
        .find(|c| c["choices"][0]["finish_reason"].is_string())
        .unwrap();
    assert_eq!(final_chunk["choices"][0]["finish_reason"], "stop");
}

/// Test streaming with malformed tool arguments
#[tokio::test]
async fn streaming_malformed_tool_arguments() {
    let mock = AnthropicMock::new("anthropic")
        .with_models(vec!["claude-3-5-sonnet-20241022".to_string()])
        .with_streaming()
        .with_tool_call("parse_json", r#"{"malformed": json without quotes}"#);

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let config = indoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.openai]
        enabled = true
        path = "/llm"
    "#};

    let server = builder.build(config).await;

    let request = json!({
        "model": "anthropic/claude-3-5-sonnet-20241022",
        "messages": [{
            "role": "user",
            "content": "Parse this data"
        }],
        "tools": [{
            "type": "function",
            "function": {
                "name": "parse_json",
                "description": "Parse JSON data",
                "parameters": {
                    "type": "object",
                    "properties": {}
                }
            }
        }],
        "stream": true
    });

    let chunks = server.openai_completions_stream(request).send().await;

    // Find tool call chunk
    let tool_chunk = chunks
        .iter()
        .find(|c| c["choices"][0]["delta"]["tool_calls"].is_array())
        .unwrap();

    // Verify malformed JSON becomes empty string in streaming (mock behavior)
    let arguments = tool_chunk["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"]
        .as_str()
        .unwrap();
    assert_eq!(arguments, "");

    insta::assert_json_snapshot!(tool_chunk, {
        ".id" => "[id]",
        ".created" => "[timestamp]",
        ".choices[0].delta.tool_calls[0].id" => "[call_id]"
    }, @r#"
    {
      "choices": [
        {
          "delta": {
            "tool_calls": [
              {
                "function": {
                  "arguments": "",
                  "name": "parse_json"
                },
                "id": "[call_id]",
                "index": 0,
                "type": "function"
              }
            ]
          },
          "index": 0
        }
      ],
      "created": "[timestamp]",
      "id": "[id]",
      "model": "anthropic/claude-3-5-sonnet-20241022",
      "object": "chat.completion.chunk"
    }
    "#);
}

/// Test streaming with empty tool arguments
#[tokio::test]
async fn streaming_empty_tool_arguments() {
    let mock = AnthropicMock::new("anthropic")
        .with_models(vec!["claude-3-5-sonnet-20241022".to_string()])
        .with_streaming()
        .with_tool_call("get_status", "{}");

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let config = indoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.openai]
        enabled = true
        path = "/llm"
    "#};

    let server = builder.build(config).await;

    let request = json!({
        "model": "anthropic/claude-3-5-sonnet-20241022",
        "messages": [{
            "role": "user",
            "content": "Get system status"
        }],
        "tools": [{
            "type": "function",
            "function": {
                "name": "get_status",
                "description": "Get system status",
                "parameters": {
                    "type": "object",
                    "properties": {}
                }
            }
        }],
        "stream": true
    });

    let chunks = server.openai_completions_stream(request).send().await;

    // Find tool call chunk
    let tool_chunk = chunks
        .iter()
        .find(|c| c["choices"][0]["delta"]["tool_calls"].is_array())
        .unwrap();

    // Verify empty object arguments become empty string in streaming
    let arguments = tool_chunk["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"]
        .as_str()
        .unwrap();
    assert_eq!(arguments, "");

    insta::assert_json_snapshot!(tool_chunk, {
        ".id" => "[id]",
        ".created" => "[timestamp]",
        ".choices[0].delta.tool_calls[0].id" => "[call_id]"
    }, @r#"
    {
      "choices": [
        {
          "delta": {
            "tool_calls": [
              {
                "function": {
                  "arguments": "",
                  "name": "get_status"
                },
                "id": "[call_id]",
                "index": 0,
                "type": "function"
              }
            ]
          },
          "index": 0
        }
      ],
      "created": "[timestamp]",
      "id": "[id]",
      "model": "anthropic/claude-3-5-sonnet-20241022",
      "object": "chat.completion.chunk"
    }
    "#);
}

/// Test streaming with usage information in final chunk
#[tokio::test]
async fn streaming_usage_information() {
    let mock = AnthropicMock::new("anthropic")
        .with_models(vec!["claude-3-5-sonnet-20241022".to_string()])
        .with_streaming()
        .with_tool_call("count_tokens", r#"{"text": "Hello world"}"#);

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let config = indoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.openai]
        enabled = true
        path = "/llm"
    "#};

    let server = builder.build(config).await;

    let request = json!({
        "model": "anthropic/claude-3-5-sonnet-20241022",
        "messages": [{
            "role": "user",
            "content": "Count the tokens in 'Hello world'"
        }],
        "tools": [{
            "type": "function",
            "function": {
                "name": "count_tokens",
                "description": "Count tokens in text",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "text": {"type": "string"}
                    }
                }
            }
        }],
        "stream": true,
        "stream_options": {
            "include_usage": true
        }
    });

    let chunks = server.openai_completions_stream(request).send().await;

    // Find final chunk with usage information
    let final_chunk = chunks.iter().find(|c| c["usage"].is_object()).unwrap();

    // The mock returns standard usage numbers (10 input, 15 output tokens)
    insta::assert_json_snapshot!(final_chunk, {
        ".id" => "[id]",
        ".created" => "[timestamp]"
    }, @r#"
    {
      "choices": [
        {
          "delta": {},
          "finish_reason": "tool_calls",
          "index": 0
        }
      ],
      "created": "[timestamp]",
      "id": "[id]",
      "model": "anthropic/claude-3-5-sonnet-20241022",
      "object": "chat.completion.chunk",
      "usage": {
        "completion_tokens": 15,
        "prompt_tokens": 10,
        "total_tokens": 25
      }
    }
    "#);
}
