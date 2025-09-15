use indoc::indoc;
use integration_tests::TestServer;
use integration_tests::llms::OpenAIMock;
use serde_json::json;

#[tokio::test]
async fn openai_tool_calling_basic() {
    let mock = OpenAIMock::new("openai")
        .with_models(vec!["gpt-4".to_string()])
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
    let llm = server.llm_client("/llm");

    let request = json!({
        "model": "openai/gpt-4",
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

    let response = llm.completions(request).await;

    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[timestamp]",
        ".choices[0].message.tool_calls[0].id" => "[call_id]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[timestamp]",
      "model": "openai/gpt-4",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "tool_calls": [
              {
                "id": "[call_id]",
                "type": "function",
                "function": {
                  "name": "get_weather",
                  "arguments": "{\"location\": \"San Francisco\", \"unit\": \"celsius\"}"
                }
              }
            ]
          },
          "finish_reason": "tool_calls"
        }
      ],
      "usage": {
        "prompt_tokens": 10,
        "completion_tokens": 15,
        "total_tokens": 25
      }
    }
    "#);
}

#[tokio::test]
async fn openai_tool_calling_with_parallel_tools() {
    let mock = OpenAIMock::new("openai")
        .with_models(vec!["gpt-4".to_string()])
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
    let llm = server.llm_client("/llm");

    let request = json!({
        "model": "openai/gpt-4",
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

    let response = llm.completions(request).await;

    // Verify parallel tool calls are returned
    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[timestamp]",
        ".choices[0].message.tool_calls[0].id" => "[call_id_1]",
        ".choices[0].message.tool_calls[1].id" => "[call_id_2]"
    }, @r###"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[timestamp]",
      "model": "openai/gpt-4",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "tool_calls": [
              {
                "id": "[call_id_1]",
                "type": "function",
                "function": {
                  "name": "get_weather",
                  "arguments": "{\"location\": \"New York City\"}"
                }
              },
              {
                "id": "[call_id_2]",
                "type": "function",
                "function": {
                  "name": "get_weather",
                  "arguments": "{\"location\": \"Los Angeles\"}"
                }
              }
            ]
          },
          "finish_reason": "tool_calls"
        }
      ],
      "usage": {
        "prompt_tokens": 10,
        "completion_tokens": 15,
        "total_tokens": 25
      }
    }
    "###);
}

#[tokio::test]
async fn openai_specific_tool_choice() {
    let mock = OpenAIMock::new("openai")
        .with_models(vec!["gpt-4".to_string()])
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
    let llm = server.llm_client("/llm");

    let request = json!({
        "model": "openai/gpt-4",
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

    let response = llm.completions(request).await;

    // Verify that the specific tool was called
    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[timestamp]",
        ".choices[0].message.tool_calls[0].id" => "[call_id]",
        ".choices[0].message.tool_calls[0].function.arguments" => "[arguments]"
    }, @r###"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[timestamp]",
      "model": "openai/gpt-4",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "tool_calls": [
              {
                "id": "[call_id]",
                "type": "function",
                "function": {
                  "name": "calculator",
                  "arguments": "[arguments]"
                }
              }
            ]
          },
          "finish_reason": "tool_calls"
        }
      ],
      "usage": {
        "prompt_tokens": 10,
        "completion_tokens": 15,
        "total_tokens": 25
      }
    }
    "###);
}

#[tokio::test]
async fn openai_tool_message_handling() {
    let mock = OpenAIMock::new("openai")
        .with_models(vec!["gpt-4".to_string()])
        .with_response("72°F", "The weather in San Francisco is 72°F and sunny.");

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
    let llm = server.llm_client("/llm");

    // Test handling of tool response messages
    let request = json!({
        "model": "openai/gpt-4",
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
                "content": "72°F and sunny",
                "tool_call_id": "call_abc123"
            }
        ]
    });

    let response = llm.completions(request).await;

    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[timestamp]"
    }, @r###"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[timestamp]",
      "model": "openai/gpt-4",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "The weather in San Francisco is 72°F and sunny."
          },
          "finish_reason": "stop"
        }
      ],
      "usage": {
        "prompt_tokens": 10,
        "completion_tokens": 15,
        "total_tokens": 25
      }
    }
    "###);
}

#[tokio::test]
async fn openai_no_tools_regular_response() {
    let mock = OpenAIMock::new("openai")
        .with_models(vec!["gpt-4".to_string()])
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
    let llm = server.llm_client("/llm");

    // Regular request without tools should work normally
    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{
            "role": "user",
            "content": "Hello"
        }]
    });

    let response = llm.completions(request).await;

    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[timestamp]"
    }, @r###"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[timestamp]",
      "model": "openai/gpt-4",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "Hi there! How can I help you?"
          },
          "finish_reason": "stop"
        }
      ],
      "usage": {
        "prompt_tokens": 10,
        "completion_tokens": 15,
        "total_tokens": 25
      }
    }
    "###);
}

#[tokio::test]
async fn openai_tool_calling_streaming() {
    let mock = OpenAIMock::new("openai")
        .with_models(vec!["gpt-4".to_string()])
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
    let llm = server.llm_client("/llm");

    let request = json!({
        "model": "openai/gpt-4",
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
    let chunks = llm.stream_completions(request.clone()).await;

    // Should have multiple chunks for streaming
    assert!(chunks.len() >= 2, "Expected at least 2 chunks, got {}", chunks.len());

    // Check first chunk structure (usually contains role)
    insta::assert_json_snapshot!(chunks[0], {
        ".id" => "[id]",
        ".created" => "[timestamp]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion.chunk",
      "created": "[timestamp]",
      "model": "openai/gpt-4",
      "choices": [
        {
          "index": 0,
          "delta": {
            "role": "assistant"
          }
        }
      ]
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
      "id": "[id]",
      "object": "chat.completion.chunk",
      "created": "[timestamp]",
      "model": "openai/gpt-4",
      "choices": [
        {
          "index": 0,
          "delta": {
            "tool_calls": [
              {
                "index": 0,
                "id": "[call_id]",
                "type": "function",
                "function": {
                  "name": "get_weather",
                  "arguments": "[arguments]"
                }
              }
            ]
          }
        }
      ]
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
        }, @r###"
        {
          "id": "[id]",
          "object": "chat.completion.chunk",
          "created": "[timestamp]",
          "model": "openai/gpt-4",
          "choices": [
            {
              "index": 0,
              "delta": {},
              "finish_reason": "tool_calls"
            }
          ],
          "usage": "[usage]"
        }
        "###);
}
