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
    "#};

    let server = builder.build(config).await;
    let llm = server.llm_client("/llm");

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
      "model": "google/gemini-1.5-flash",
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
                  "arguments": "{\"location\":\"San Francisco\",\"unit\":\"celsius\"}"
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
    "#};

    let server = builder.build(config).await;
    let llm = server.llm_client("/llm");

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

    let response = llm.completions(request).await;

    // Google mock returns multiple tool calls for parallel calls
    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[timestamp]",
        ".choices[0].message.tool_calls[0].id" => "[call_id_1]",
        ".choices[0].message.tool_calls[1].id" => "[call_id_2]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[timestamp]",
      "model": "google/gemini-1.5-flash",
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
                  "arguments": "{\"location\":\"New York City\"}"
                }
              },
              {
                "id": "[call_id_2]",
                "type": "function",
                "function": {
                  "name": "get_weather",
                  "arguments": "{\"location\":\"Los Angeles\"}"
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
async fn google_tool_message_handling() {
    let mock = GoogleMock::new("google")
        .with_models(vec!["gemini-1.5-flash".to_string()])
        .with_response("22°C", "The weather in San Francisco is 22°C and sunny.");

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let config = indoc! {r#"
        [llm]
        enabled = true
    "#};

    let server = builder.build(config).await;
    let llm = server.llm_client("/llm");

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

    let response = llm.completions(request).await;

    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[timestamp]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[timestamp]",
      "model": "google/gemini-1.5-flash",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "The weather in San Francisco is 22°C and sunny."
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
    "#};

    let server = builder.build(config).await;
    let llm = server.llm_client("/llm");

    // Regular request without tools should work normally
    let request = json!({
        "model": "google/gemini-1.5-flash",
        "messages": [{
            "role": "user",
            "content": "Hello"
        }]
    });

    let response = llm.completions(request).await;

    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[timestamp]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[timestamp]",
      "model": "google/gemini-1.5-flash",
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
    "#};

    let server = builder.build(config).await;
    let llm = server.llm_client("/llm");

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
      "model": "google/gemini-1.5-flash",
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
                  "name": "execute",
                  "arguments": "{\"name\":\"search\",\"arguments\":{\"keywords\":[\"github\",\"user\"]}}"
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
    "#};

    let server = builder.build(config).await;
    let llm = server.llm_client("/llm");

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
    let chunks = llm.stream_completions(request.clone()).await;

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
      "id": "[id]",
      "object": "chat.completion.chunk",
      "created": "[timestamp]",
      "model": "google/gemini-1.5-flash",
      "choices": [
        {
          "index": 0,
          "delta": {
            "role": "assistant",
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
        .expect("Expected to find chunk with finish_reason");

    insta::assert_json_snapshot!(final_chunk, {
            ".id" => "[id]",
            ".created" => "[timestamp]",
            ".usage" => "[usage]",
            ".choices[0].delta.tool_calls[0].id" => "[call_id]"
        }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion.chunk",
      "created": "[timestamp]",
      "model": "google/gemini-1.5-flash",
      "choices": [
        {
          "index": 0,
          "delta": {
            "role": "assistant",
            "tool_calls": [
              {
                "index": 0,
                "id": "[call_id]",
                "type": "function",
                "function": {
                  "name": "get_weather",
                  "arguments": "{}"
                }
              }
            ]
          },
          "finish_reason": "tool_calls"
        }
      ],
      "usage": "[usage]"
    }
    "#);
}
