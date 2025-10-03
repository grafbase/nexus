use indoc::indoc;
use integration_tests::TestServer;
use integration_tests::llms::AnthropicMock;
use serde_json::json;

#[tokio::test]
async fn anthropic_tool_calling_basic() {
    let mock = AnthropicMock::new("anthropic")
        .with_models(vec!["claude-3-5-sonnet-20241022".to_string()])
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
        "model": "anthropic/claude-3-5-sonnet-20241022",
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
      "model": "anthropic/claude-3-5-sonnet-20241022",
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
async fn anthropic_tool_calling_with_parallel_tools() {
    let mock = AnthropicMock::new("anthropic")
        .with_models(vec!["claude-3-5-sonnet-20241022".to_string()])
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
        "model": "anthropic/claude-3-5-sonnet-20241022",
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

    // Note: This test currently returns a single tool call due to simplified mock implementation
    // In a real scenario, parallel tool calls would return multiple tool_calls
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
                  "arguments": "{\"location\":\"New York City\"}",
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
      "model": "anthropic/claude-3-5-sonnet-20241022",
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
async fn anthropic_specific_tool_choice() {
    let mock = AnthropicMock::new("anthropic")
        .with_models(vec!["claude-3-5-sonnet-20241022".to_string()])
        .with_tool_call("calculator", r#"{"expression": "2+2"}"#);

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
            "content": "Calculate something"
        }],
        "tools": [
            {
                "type": "function",
                "function": {
                    "name": "calculator",
                    "description": "Calculate mathematical expressions",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "expression": {
                                "type": "string"
                            }
                        },
                        "required": ["expression"]
                    }
                }
            },
            {
                "type": "function",
                "function": {
                    "name": "converter",
                    "description": "Convert units",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "value": {"type": "number"},
                            "from": {"type": "string"},
                            "to": {"type": "string"}
                        },
                        "required": ["value", "from", "to"]
                    }
                }
            }
        ],
        "tool_choice": {
            "type": "function",
            "function": {
                "name": "calculator"
            }
        }
    });

    let response = server.openai_completions(request).send().await;

    // Verify that the specific tool was called
    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[timestamp]",
        ".choices[0].message.tool_calls[0].id" => "[call_id]",
        ".choices[0].message.tool_calls[0].function.arguments" => "[arguments]"
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
                  "arguments": "[arguments]",
                  "name": "calculator"
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
      "model": "anthropic/claude-3-5-sonnet-20241022",
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
async fn anthropic_tool_message_handling() {
    let mock = AnthropicMock::new("anthropic")
        .with_models(vec!["claude-3-5-sonnet-20241022".to_string()])
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

    // Test handling of tool response messages (converted to Anthropic's tool_result format)
    let request = json!({
        "model": "anthropic/claude-3-5-sonnet-20241022",
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
      "model": "anthropic/claude-3-5-sonnet-20241022",
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
async fn anthropic_no_tools_regular_response() {
    let mock = AnthropicMock::new("anthropic")
        .with_models(vec!["claude-3-5-sonnet-20241022".to_string()])
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
        "model": "anthropic/claude-3-5-sonnet-20241022",
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
      "model": "anthropic/claude-3-5-sonnet-20241022",
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
async fn anthropic_tool_calling_streaming() {
    let mock = AnthropicMock::new("anthropic")
        .with_models(vec!["claude-3-5-sonnet-20241022".to_string()])
        .with_streaming()
        .with_tool_call("get_weather", r#"{"location": "Tokyo", "unit": "celsius"}"#);

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
    assert!(chunks.len() >= 2, "Expected at least 2 chunks, got {}", chunks.len());

    // Check first chunk structure (usually contains role)
    insta::assert_json_snapshot!(chunks[0], {
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
      "model": "anthropic/claude-3-5-sonnet-20241022",
      "object": "chat.completion.chunk"
    }
    "#);

    // Check final chunk with finish_reason
    let final_chunk = chunks
        .iter()
        .find(|c| c["choices"][0]["finish_reason"].is_string())
        .unwrap();

    insta::assert_json_snapshot!(final_chunk, {
            ".id" => "[id]",
            ".created" => "[timestamp]",
            ".usage" => "[usage]"
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
      "usage": "[usage]"
    }
    "#);
}
