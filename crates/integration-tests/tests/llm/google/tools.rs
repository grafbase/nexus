use indoc::indoc;
use integration_tests::TestServer;
use integration_tests::llms::GoogleMock;
use serde_json::json;

#[tokio::test]
async fn google_tool_calling_basic() {
    let mock = GoogleMock::new("google")
        .with_models(vec!["gemini-1.5-flash".to_string()])
        .with_tool_call("get_weather", r#"{"location": "San Francisco", "unit": "celsius"}"#);

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
        "model": "google/gemini-1.5-flash",
        "messages": [{
            "role": "user",
            "content": "What's the weather in San Francisco?"
        }],
        "tools": [{
            "type": "function",
            "function": {
                "name": "get_weather",
                "description": "Get the current weather in a given location",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "location": {
                            "type": "string",
                            "description": "The city and state, e.g. San Francisco, CA"
                        },
                        "unit": {
                            "type": "string",
                            "enum": ["celsius", "fahrenheit"]
                        }
                    },
                    "required": ["location"]
                }
            }
        }],
        "tool_choice": "auto"
    });

    let response = server.openai_completions(request).send().await;

    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[timestamp]",
        ".choices[0].message.tool_calls[0].id" => "[call_id]"
    }, @r#"
    {
      "choices": [
        {
          "finish_reason": "tool_calls",
          "index": 0,
          "message": {
            "role": "assistant",
            "tool_calls": [
              {
                "function": {
                  "arguments": "{\"location\":\"San Francisco\",\"unit\":\"celsius\"}",
                  "name": "get_weather"
                },
                "id": "[call_id]",
                "type": "function"
              }
            ]
          }
        }
      ],
      "created": "[timestamp]",
      "id": "[id]",
      "model": "google/gemini-1.5-flash",
      "object": "chat.completion",
      "usage": {
        "completion_tokens": 15,
        "prompt_tokens": 10,
        "total_tokens": 25
      }
    }
    "#);
}

#[tokio::test]
async fn google_tool_calling_with_parallel_tools() {
    let mock = GoogleMock::new("google")
        .with_models(vec!["gemini-1.5-flash".to_string()])
        .with_parallel_tool_calls(vec![
            ("get_weather", r#"{"location": "New York City"}"#),
            ("get_weather", r#"{"location": "Los Angeles"}"#),
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
        "model": "google/gemini-1.5-flash",
        "messages": [{
            "role": "user",
            "content": "Get weather for both NYC and LA"
        }],
        "tools": [{
            "type": "function",
            "function": {
                "name": "get_weather",
                "description": "Get the current weather in a given location",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "location": {
                            "type": "string"
                        }
                    },
                    "required": ["location"]
                }
            }
        }],
        "tool_choice": "auto",
        "parallel_tool_calls": true
    });

    let response = server.openai_completions(request).send().await;

    // Google mock returns multiple tool calls for parallel calls
    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[timestamp]",
        ".choices[0].message.tool_calls[0].id" => "[call_id_1]",
        ".choices[0].message.tool_calls[1].id" => "[call_id_2]"
    }, @r#"
    {
      "choices": [
        {
          "finish_reason": "tool_calls",
          "index": 0,
          "message": {
            "role": "assistant",
            "tool_calls": [
              {
                "function": {
                  "arguments": "{\"location\":\"New York City\"}",
                  "name": "get_weather"
                },
                "id": "[call_id_1]",
                "type": "function"
              },
              {
                "function": {
                  "arguments": "{\"location\":\"Los Angeles\"}",
                  "name": "get_weather"
                },
                "id": "[call_id_2]",
                "type": "function"
              }
            ]
          }
        }
      ],
      "created": "[timestamp]",
      "id": "[id]",
      "model": "google/gemini-1.5-flash",
      "object": "chat.completion",
      "usage": {
        "completion_tokens": 15,
        "prompt_tokens": 10,
        "total_tokens": 25
      }
    }
    "#);
}

#[tokio::test]
async fn google_tool_message_handling() {
    let mock = GoogleMock::new("google")
        .with_models(vec!["gemini-1.5-flash".to_string()])
        .with_response("22°C", "The weather in San Francisco is 22°C and sunny.");

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

    // Test handling of tool response messages (converted to Google's functionResponse format)
    let request = json!({
        "model": "google/gemini-1.5-flash",
        "messages": [
            {
                "role": "user",
                "content": "What's the weather in San Francisco?"
            },
            {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_abc123",
                    "type": "function",
                    "function": {
                        "name": "get_weather",
                        "arguments": "{\"location\": \"San Francisco\"}"
                    }
                }]
            },
            {
                "role": "tool",
                "content": "22°C and sunny",
                "tool_call_id": "call_abc123"
            }
        ]
    });

    let response = server.openai_completions(request).send().await;

    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[timestamp]"
    }, @r#"
    {
      "choices": [
        {
          "finish_reason": "stop",
          "index": 0,
          "message": {
            "content": "The weather in San Francisco is 22°C and sunny.",
            "role": "assistant"
          }
        }
      ],
      "created": "[timestamp]",
      "id": "[id]",
      "model": "google/gemini-1.5-flash",
      "object": "chat.completion",
      "usage": {
        "completion_tokens": 15,
        "prompt_tokens": 10,
        "total_tokens": 25
      }
    }
    "#);
}

#[tokio::test]
async fn google_no_tools_regular_response() {
    let mock = GoogleMock::new("google")
        .with_models(vec!["gemini-1.5-flash".to_string()])
        .with_response("Hello", "Hi there! How can I help you?");

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

    // Regular request without tools should work normally
    let request = json!({
        "model": "google/gemini-1.5-flash",
        "messages": [{
            "role": "user",
            "content": "Hello"
        }]
    });

    let response = server.openai_completions(request).send().await;

    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[timestamp]"
    }, @r#"
    {
      "choices": [
        {
          "finish_reason": "stop",
          "index": 0,
          "message": {
            "content": "Hi there! How can I help you?",
            "role": "assistant"
          }
        }
      ],
      "created": "[timestamp]",
      "id": "[id]",
      "model": "google/gemini-1.5-flash",
      "object": "chat.completion",
      "usage": {
        "completion_tokens": 15,
        "prompt_tokens": 10,
        "total_tokens": 25
      }
    }
    "#);
}

#[tokio::test]
async fn google_tool_with_additional_properties_stripped() {
    // This test ensures that additionalProperties is stripped from tool parameters
    // since Google's API doesn't support this JSON Schema feature
    let mock = GoogleMock::new("google")
        .with_models(vec!["gemini-1.5-flash".to_string()])
        .with_tool_call(
            "execute",
            r#"{"name": "search", "arguments": {"keywords": ["github", "user"]}}"#,
        );

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

    // This mimics the MCP execute tool which has additionalProperties: true
    let request = json!({
        "model": "google/gemini-1.5-flash",
        "messages": [{
            "role": "user",
            "content": "Execute the search tool"
        }],
        "tools": [{
            "type": "function",
            "function": {
                "name": "execute",
                "description": "Executes a tool with the given parameters",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "The name of the tool to execute"
                        },
                        "arguments": {
                            "type": "object",
                            "description": "The arguments to pass to the tool",
                            "additionalProperties": true
                        }
                    },
                    "required": ["name", "arguments"],
                    "additionalProperties": false
                }
            }
        }],
        "tool_choice": "auto"
    });

    // This should succeed - Google API should not receive additionalProperties
    let response = server.openai_completions(request).send().await;

    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[timestamp]",
        ".choices[0].message.tool_calls[0].id" => "[call_id]"
    }, @r#"
    {
      "choices": [
        {
          "finish_reason": "tool_calls",
          "index": 0,
          "message": {
            "role": "assistant",
            "tool_calls": [
              {
                "function": {
                  "arguments": "{\"arguments\":{\"keywords\":[\"github\",\"user\"]},\"name\":\"search\"}",
                  "name": "execute"
                },
                "id": "[call_id]",
                "type": "function"
              }
            ]
          }
        }
      ],
      "created": "[timestamp]",
      "id": "[id]",
      "model": "google/gemini-1.5-flash",
      "object": "chat.completion",
      "usage": {
        "completion_tokens": 15,
        "prompt_tokens": 10,
        "total_tokens": 25
      }
    }
    "#);
}

#[tokio::test]
async fn google_tool_calling_streaming() {
    let mock = GoogleMock::new("google")
        .with_models(vec!["gemini-1.5-flash".to_string()])
        .with_streaming()
        .with_streaming_tool_call("get_weather", r#"{"location": "Tokyo", "unit": "celsius"}"#);

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
        "model": "google/gemini-1.5-flash",
        "messages": [{
            "role": "user",
            "content": "What's the weather in Tokyo?"
        }],
        "tools": [{
            "type": "function",
            "function": {
                "name": "get_weather",
                "description": "Get the current weather in a given location",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "location": {
                            "type": "string",
                            "description": "The city and state, e.g. San Francisco, CA"
                        },
                        "unit": {
                            "type": "string",
                            "enum": ["celsius", "fahrenheit"]
                        }
                    },
                    "required": ["location"]
                }
            }
        }],
        "tool_choice": "auto",
        "stream": true
    });

    // Test streaming tool calls
    let chunks = server.openai_completions_stream(request.clone()).send().await;

    // Should have multiple chunks for streaming
    let chunk_count = chunks.len();
    assert!(chunks.len() >= 2, "Expected at least 2 chunks, got {chunk_count}");

    // Check tool call chunks
    let tool_chunks: Vec<_> = chunks
        .iter()
        .filter(|c| c["choices"][0]["delta"]["tool_calls"].is_array())
        .collect();

    assert!(!tool_chunks.is_empty(), "Expected tool call chunks");

    let tool_chunk = tool_chunks[0];

    insta::assert_json_snapshot!(tool_chunk, {
        ".id" => "[id]",
        ".created" => "[timestamp]",
        ".choices[0].delta.tool_calls[0].id" => "[call_id]",
        ".choices[0].delta.tool_calls[0].function.arguments" => "[arguments]"
    }, @r#"
    {
      "choices": [
        {
          "delta": {
            "role": "assistant",
            "tool_calls": [
              {
                "function": {
                  "arguments": "[arguments]",
                  "name": "get_weather"
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
      "model": "google/gemini-1.5-flash",
      "object": "chat.completion.chunk"
    }
    "#);

    // Check final chunk with finish_reason
    let final_chunk = chunks
        .iter()
        .find(|c| c["choices"][0]["finish_reason"].is_string())
        .expect("Expected to find chunk with finish_reason");

    insta::assert_json_snapshot!(final_chunk, {
            ".id" => "[id]",
            ".created" => "[timestamp]",
            ".usage" => "[usage]",
            ".choices[0].delta.tool_calls[0].id" => "[call_id]"
        }, @r#"
    {
      "choices": [
        {
          "delta": {
            "role": "assistant",
            "tool_calls": [
              {
                "function": {
                  "arguments": "{}",
                  "name": "get_weather"
                },
                "id": "[call_id]",
                "index": 0,
                "type": "function"
              }
            ]
          },
          "finish_reason": "tool_calls",
          "index": 0
        }
      ],
      "created": "[timestamp]",
      "id": "[id]",
      "model": "google/gemini-1.5-flash",
      "object": "chat.completion.chunk",
      "usage": "[usage]"
    }
    "#);
}

#[tokio::test]
async fn google_tool_calling_with_thought_signature_anthropic_protocol() {
    // Test that Google's thoughtSignature field is properly handled when using Anthropic protocol
    // This simulates Claude Code calling through Nexus to Google
    let mock = GoogleMock::new("google")
        .with_models(vec!["gemini-2.5-flash".to_string()])
        .with_tool_call(
            "Bash",
            r#"{"command": "ls nexus/", "description": "List files in the nexus/ directory"}"#,
        );

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let config = indoc! {r#"
        [llm.protocols.anthropic]
        enabled = true
        path = "/anthropic"
    "#};

    let server = builder.build(config).await;

    // Send request in Anthropic format (as Claude Code would)
    let request = json!({
        "model": "google/gemini-2.5-flash",
        "messages": [{
            "role": "user",
            "content": "List the files in the nexus directory"
        }],
        "max_tokens": 1000,
        "tools": [{
            "name": "Bash",
            "description": "Execute a bash command",
            "input_schema": {
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The bash command to execute"
                    },
                    "description": {
                        "type": "string",
                        "description": "Description of what the command does"
                    }
                },
                "required": ["command"]
            }
        }]
    });

    let response = server.anthropic_completions(request).send().await;

    // Response should be in Anthropic format with tool_use content blocks
    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".content[0].id" => "[tool_id]",
        ".usage" => "[usage]"
    }, @r#"
    {
      "content": [
        {
          "id": "[tool_id]",
          "input": {
            "command": "ls nexus/",
            "description": "List files in the nexus/ directory"
          },
          "name": "Bash",
          "type": "tool_use"
        }
      ],
      "id": "[id]",
      "model": "google/gemini-2.5-flash",
      "role": "assistant",
      "stop_reason": null,
      "stop_sequence": null,
      "type": "message",
      "usage": "[usage]"
    }
    "#);
}

#[tokio::test]
async fn google_handles_claude_code_tool_result_format() {
    // Test that tool results from Claude Code (string format) are properly handled
    let mock = GoogleMock::new("google")
        .with_models(vec!["gemini-2.5-pro".to_string()])
        .with_response("result", "Command executed successfully");

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let config = indoc! {r#"
        [llm.protocols.anthropic]
        enabled = true
        path = "/anthropic"
    "#};

    let server = builder.build(config).await;

    // Send request with tool result in Claude Code format (string content)
    let request = json!({
        "model": "google/gemini-2.5-pro",
        "messages": [
            {
                "role": "user",
                "content": "Run ls command"
            },
            {
                "role": "assistant",
                "content": [
                    {
                        "type": "tool_use",
                        "id": "call_123",
                        "name": "Bash",
                        "input": {
                            "command": "ls"
                        }
                    }
                ]
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "call_123",
                        "content": "Cargo.toml\nsrc"  // String format as Claude Code sends
                    }
                ]
            }
        ],
        "max_tokens": 1000
    });

    let response = server.anthropic_completions(request).send().await;

    // Should get a valid response back
    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".content[0].text" => "[response_text]",
        ".usage" => "[usage]"
    }, @r#"
    {
      "content": [
        {
          "text": "[response_text]",
          "type": "text"
        }
      ],
      "id": "[id]",
      "model": "google/gemini-2.5-pro",
      "role": "assistant",
      "stop_reason": null,
      "stop_sequence": null,
      "type": "message",
      "usage": "[usage]"
    }
    "#);
}
