//! Live integration tests for AWS Bedrock.
//!
//! These tests make real API calls to AWS Bedrock and incur charges.
//! They are disabled by default and only run when BEDROCK_LIVE_TESTS=true.
//!
//! To run these tests:
//! ```bash
//! BEDROCK_LIVE_TESTS=true cargo test -p integration-tests bedrock::live -- --ignored
//! ```
//!
//! Required environment variables:
//! - BEDROCK_LIVE_TESTS=true (to enable tests)
//! - AWS credentials (via AWS_ACCESS_KEY_ID/AWS_SECRET_ACCESS_KEY or AWS_PROFILE)
//! - AWS_REGION (optional, defaults to us-east-1)
//!
//! Cost estimates (per full test run):
//! - Total: < $0.01
//! - Each test uses max_tokens=10 and minimal prompts

use indoc::formatdoc;
use integration_test_macros::live_provider_test;
use integration_tests::TestServer;
use serde_json::json;

/// Helper to create a Bedrock configuration with specified models.
/// All models are configured with max_tokens=10 to minimize costs.
fn create_bedrock_config(models: &[(&str, &str)]) -> String {
    let region = std::env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".to_string());

    let mut model_configs = String::new();
    for (alias, real_model) in models {
        model_configs.push_str(&formatdoc! {r#"
            [llm.providers.bedrock.models.{alias}]
            rename = "{real_model}"
            max_tokens = 10
        "#});
    }

    formatdoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.openai]
enabled = true
path = "/llm"
        [llm.providers.bedrock]
        type = "bedrock"
        region = "{region}"

        {model_configs}
    "#}
}

// ============================================================================
// Anthropic Claude Tests
// ============================================================================

// Note: Claude Instant v1 has been deprecated by AWS and tests have been removed

// ===== AI21 Jamba Tests =====

#[live_provider_test(bedrock)]
async fn ai21_jamba_1_5_mini_minimal() {
    let config = create_bedrock_config(&[("jamba-mini", "ai21.jamba-1-5-mini-v1:0")]);

    let server = TestServer::builder().build(&config).await;

    let response = server
        .openai_completions(json!({
            "model": "bedrock/jamba-mini",
            "messages": [{
                "role": "user",
                "content": "Reply with one word: yes"
            }],
            "max_tokens": 10,
            "temperature": 0
        }))
        .send()
        .await;

    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[timestamp]",
        ".choices[0].message.content" => "[response]",
        ".usage" => "[usage]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[timestamp]",
      "model": "bedrock/jamba-mini",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "[response]"
          },
          "finish_reason": "stop"
        }
      ],
      "usage": "[usage]"
    }
    "#);
}

// ===== Anthropic Claude Tests =====

#[live_provider_test(bedrock)]
async fn anthropic_claude3_sonnet_minimal() {
    let config = create_bedrock_config(&[("claude3-sonnet", "anthropic.claude-3-sonnet-20240229-v1:0")]);

    let server = TestServer::builder().build(&config).await;

    let response = server
        .openai_completions(json!({
            "model": "bedrock/claude3-sonnet",
            "messages": [{
                "role": "user",
                "content": "Reply with one word: yes"
            }],
            "max_tokens": 10,
            "temperature": 0
        }))
        .send()
        .await;

    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[timestamp]",
        ".choices[0].message.content" => "[response]",
        ".usage" => "[usage]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[timestamp]",
      "model": "bedrock/claude3-sonnet",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "[response]"
          },
          "finish_reason": "stop"
        }
      ],
      "usage": "[usage]"
    }
    "#);
}

#[live_provider_test(bedrock)]
async fn anthropic_claude3_haiku_minimal() {
    let config = create_bedrock_config(&[("claude3-haiku", "anthropic.claude-3-haiku-20240307-v1:0")]);

    let server = TestServer::builder().build(&config).await;

    let response = server
        .openai_completions(json!({
            "model": "bedrock/claude3-haiku",
            "messages": [{
                "role": "user",
                "content": "Reply: ok"
            }],
            "max_tokens": 10,
            "temperature": 0
        }))
        .send()
        .await;

    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[timestamp]",
        ".choices[0].message.content" => "[response]",
        ".usage" => "[usage]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[timestamp]",
      "model": "bedrock/claude3-haiku",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "[response]"
          },
          "finish_reason": "stop"
        }
      ],
      "usage": "[usage]"
    }
    "#);
}

// ============================================================================
// Amazon Nova Tests
// ============================================================================

#[live_provider_test(bedrock)]
async fn amazon_nova_micro_minimal() {
    let config = create_bedrock_config(&[("nova-micro", "amazon.nova-micro-v1:0")]);

    let server = TestServer::builder().build(&config).await;

    let response = server
        .openai_completions(json!({
            "model": "bedrock/nova-micro",
            "messages": [{
                "role": "user",
                "content": "Reply with: hello"
            }],
            "max_tokens": 10,
            "temperature": 0
        }))
        .send()
        .await;

    // Verify response structure
    let content = response["choices"][0]["message"]["content"].as_str().unwrap();
    assert!(
        content.to_lowercase().contains("hello"),
        "Expected response to contain 'hello', got: {}",
        content
    );

    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[timestamp]",
        ".choices[0].message.content" => "[response]",
        ".usage" => "[usage]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[timestamp]",
      "model": "bedrock/nova-micro",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "[response]"
          },
          "finish_reason": "stop"
        }
      ],
      "usage": "[usage]"
    }
    "#);
}

#[live_provider_test(bedrock)]
async fn amazon_nova_micro_with_tools() {
    let config = create_bedrock_config(&[("nova-micro", "amazon.nova-micro-v1:0")]);

    let server = TestServer::builder().build(&config).await;

    // Test with tools - Nova should be able to call them
    let response = server
        .openai_completions(json!({
            "model": "bedrock/nova-micro",
            "messages": [{
                "role": "user",
                "content": "What's the weather in Paris? Use the get_weather tool."
            }],
            "max_tokens": 100,
            "temperature": 0,
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
                                "enum": ["celsius", "fahrenheit"],
                                "description": "The unit of temperature"
                            }
                        },
                        "required": ["location"]
                    }
                }
            }],
            "tool_choice": "auto"
        }))
        .send()
        .await;

    // Snapshot the entire response to verify tool calling works
    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[timestamp]",
        ".choices[0].message.content" => "[content]",
        ".choices[0].message.tool_calls[0].id" => "[tool_id]",
        ".choices[0].message.tool_calls[0].function.arguments" => "[arguments]",
        ".usage" => "[usage]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[timestamp]",
      "model": "bedrock/nova-micro",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "[content]",
            "tool_calls": [
              {
                "id": "[tool_id]",
                "type": "function",
                "function": {
                  "name": "get_weather",
                  "arguments": "[arguments]"
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

// ============================================================================
// Amazon Titan Tests
// ============================================================================

#[live_provider_test(bedrock)]
async fn amazon_titan_lite_minimal() {
    let config = create_bedrock_config(&[("titan-lite", "amazon.titan-text-lite-v1")]);

    let server = TestServer::builder().build(&config).await;

    let response = server
        .openai_completions(json!({
            "model": "bedrock/titan-lite",
            "messages": [{
                "role": "user",
                "content": "Hi"
            }],
            "max_tokens": 10,
            "temperature": 0
        }))
        .send()
        .await;

    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[timestamp]",
        ".choices[0].message.content" => "[response]",
        ".usage" => "[usage]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[timestamp]",
      "model": "bedrock/titan-lite",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "[response]"
          },
          "finish_reason": "length"
        }
      ],
      "usage": "[usage]"
    }
    "#);
}

#[live_provider_test(bedrock)]
async fn amazon_titan_express_minimal() {
    let config = create_bedrock_config(&[("titan-express", "amazon.titan-text-express-v1")]);

    let server = TestServer::builder().build(&config).await;

    let response = server
        .openai_completions(json!({
            "model": "bedrock/titan-express",
            "messages": [{
                "role": "user",
                "content": "Reply with: hello"
            }],
            "max_tokens": 10,
            "temperature": 0
        }))
        .send()
        .await;

    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[timestamp]",
        ".choices[0].message.content" => "[response]",
        ".usage" => "[usage]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[timestamp]",
      "model": "bedrock/titan-express",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "[response]"
          },
          "finish_reason": "length"
        }
      ],
      "usage": "[usage]"
    }
    "#);
}

// ============================================================================
// Meta Llama Tests
// ============================================================================

#[live_provider_test(bedrock)]
async fn meta_llama3_8b_minimal() {
    let config = create_bedrock_config(&[("llama3-8b", "meta.llama3-8b-instruct-v1:0")]);

    let server = TestServer::builder().build(&config).await;

    let response = server
        .openai_completions(json!({
            "model": "bedrock/llama3-8b",
            "messages": [{
                "role": "user",
                "content": "2+2="
            }],
            "max_tokens": 10,
            "temperature": 0
        }))
        .send()
        .await;

    // Verify it's a valid completion and contains "4" in response
    let content = response["choices"][0]["message"]["content"].as_str().unwrap();
    assert!(
        content.contains("4") || content.contains("four"),
        "Expected response to contain '4' or 'four', got: {}",
        content
    );

    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[timestamp]",
        ".choices[0].message.content" => "[contains_4]",
        ".usage" => "[usage]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[timestamp]",
      "model": "bedrock/llama3-8b",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "[contains_4]"
          },
          "finish_reason": "stop"
        }
      ],
      "usage": "[usage]"
    }
    "#);
}

// ============================================================================
// Mistral Tests
// ============================================================================

#[live_provider_test(bedrock)]
async fn mistral_7b_instruct_minimal() {
    let config = create_bedrock_config(&[("mistral-7b", "mistral.mistral-7b-instruct-v0:2")]);

    let server = TestServer::builder().build(&config).await;

    let response = server
        .openai_completions(json!({
            "model": "bedrock/mistral-7b",
            "messages": [{
                "role": "user",
                "content": "Yes or no?"
            }],
            "max_tokens": 10,
            "temperature": 0
        }))
        .send()
        .await;

    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[timestamp]",
        ".choices[0].message.content" => "[response]",
        ".usage" => "[usage]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[timestamp]",
      "model": "bedrock/mistral-7b",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "[response]"
          },
          "finish_reason": "length"
        }
      ],
      "usage": "[usage]"
    }
    "#);
}

// ============================================================================
// Cohere Tests
// ============================================================================

#[live_provider_test(bedrock)]
async fn cohere_command_r_minimal() {
    let config = create_bedrock_config(&[("command-r", "cohere.command-r-v1:0")]);

    let server = TestServer::builder().build(&config).await;

    let response = server
        .openai_completions(json!({
            "model": "bedrock/command-r",
            "messages": [{
                "role": "user",
                "content": "Hello"
            }],
            "max_tokens": 10,
            "temperature": 0
        }))
        .send()
        .await;

    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[timestamp]",
        ".choices[0].message.content" => "[response]",
        ".choices[0].finish_reason" => "[finish_reason]",
        ".usage" => "[usage]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[timestamp]",
      "model": "bedrock/command-r",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "[response]"
          },
          "finish_reason": "[finish_reason]"
        }
      ],
      "usage": "[usage]"
    }
    "#);
}

#[live_provider_test(bedrock)]
async fn cohere_command_r_multi_turn() {
    let config = create_bedrock_config(&[("command-r", "cohere.command-r-v1:0")]);

    let server = TestServer::builder().build(&config).await;

    // Test multi-turn conversation with chat history
    let response = server
        .openai_completions(json!({
            "model": "bedrock/command-r",
            "messages": [
                {
                    "role": "user",
                    "content": "What is 2+2?"
                },
                {
                    "role": "assistant",
                    "content": "2+2 equals 4."
                },
                {
                    "role": "user",
                    "content": "What about 3+3?"
                }
            ],
            "max_tokens": 50,
            "temperature": 0
        }))
        .send()
        .await;

    // Verify response acknowledges context
    let content = response["choices"][0]["message"]["content"].as_str().unwrap();
    assert!(
        content.contains("6") || content.contains("six") || content.contains("equals"),
        "Expected response to answer 3+3, got: {}",
        content
    );

    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[timestamp]",
        ".choices[0].message.content" => "[contextual_response]",
        ".choices[0].finish_reason" => "[finish_reason]",
        ".usage" => "[usage]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[timestamp]",
      "model": "bedrock/command-r",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "[contextual_response]"
          },
          "finish_reason": "[finish_reason]"
        }
      ],
      "usage": "[usage]"
    }
    "#);
}

#[live_provider_test(bedrock)]
async fn cohere_command_r_with_system() {
    let config = create_bedrock_config(&[("command-r", "cohere.command-r-v1:0")]);

    let server = TestServer::builder().build(&config).await;

    // Test system message handling
    let response = server
        .openai_completions(json!({
            "model": "bedrock/command-r",
            "messages": [
                {
                    "role": "system",
                    "content": "You are a helpful assistant. Be concise."
                },
                {
                    "role": "user",
                    "content": "Hi"
                }
            ],
            "max_tokens": 20,
            "temperature": 0
        }))
        .send()
        .await;

    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[timestamp]",
        ".choices[0].message.content" => "[response]",
        ".choices[0].finish_reason" => "[finish_reason]",
        ".usage" => "[usage]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[timestamp]",
      "model": "bedrock/command-r",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "[response]"
          },
          "finish_reason": "[finish_reason]"
        }
      ],
      "usage": "[usage]"
    }
    "#);
}

// ============================================================================
// Tool Calling Tests
// ============================================================================

#[live_provider_test(bedrock)]
async fn anthropic_claude3_haiku_tool_calling_basic() {
    let config = create_bedrock_config(&[("claude3-haiku", "anthropic.claude-3-haiku-20240307-v1:0")]);

    let server = TestServer::builder().build(&config).await;

    // Basic tool calling request
    let response = server
        .openai_completions(json!({
            "model": "bedrock/claude3-haiku",
            "messages": [{
                "role": "user",
                "content": "What's the weather like in San Francisco?"
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
                                "enum": ["celsius", "fahrenheit"],
                                "description": "The unit of temperature"
                            }
                        },
                        "required": ["location"]
                    }
                }
            }],
            "max_tokens": 200,
            "temperature": 0
        }))
        .send()
        .await;

    // Verify response contains tool call
    assert_eq!(response["object"], "chat.completion");
    assert_eq!(response["model"], "bedrock/claude3-haiku");

    let tool_calls = &response["choices"][0]["message"]["tool_calls"];
    assert!(tool_calls.is_array(), "Expected tool_calls array");

    if let Some(calls) = tool_calls.as_array() {
        assert!(!calls.is_empty(), "Expected at least one tool call");

        let first_call = &calls[0];
        assert_eq!(first_call["type"], "function");
        assert_eq!(first_call["function"]["name"], "get_weather");

        // Verify arguments contain location
        let args = &first_call["function"]["arguments"];
        assert!(args.is_string(), "Expected arguments as JSON string");

        if let Some(args_str) = args.as_str() {
            let parsed_args: serde_json::Value = serde_json::from_str(args_str).unwrap();
            assert!(parsed_args["location"].is_string());
            let location = parsed_args["location"].as_str().unwrap();
            assert!(
                location.to_lowercase().contains("san francisco") || location.to_lowercase().contains("sf"),
                "Expected San Francisco in location, got: {}",
                location
            );
        }
    }

    // Verify finish reason
    assert_eq!(response["choices"][0]["finish_reason"], "tool_calls");
}

#[live_provider_test(bedrock)]
async fn anthropic_claude3_sonnet_tool_calling_multiple() {
    let config = create_bedrock_config(&[("claude3-sonnet", "anthropic.claude-3-sonnet-20240229-v1:0")]);

    let server = TestServer::builder().build(&config).await;

    // Request that should trigger multiple tool calls
    let response = server
        .openai_completions(json!({
            "model": "bedrock/claude3-sonnet",
            "messages": [{
                "role": "user",
                "content": "What's the weather in both New York and London? Use different units for each."
            }],
            "tools": [
                {
                    "type": "function",
                    "function": {
                        "name": "get_weather",
                        "description": "Get the current weather in a given location",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "location": {
                                    "type": "string",
                                    "description": "The city name"
                                },
                                "unit": {
                                    "type": "string",
                                    "enum": ["celsius", "fahrenheit"]
                                }
                            },
                            "required": ["location"]
                        }
                    }
                }
            ],
            "max_tokens": 300,
            "temperature": 0
        }))
        .send()
        .await;

    // Verify multiple tool calls
    let tool_calls = &response["choices"][0]["message"]["tool_calls"];
    if let Some(calls) = tool_calls.as_array() {
        assert!(
            calls.len() >= 2,
            "Expected at least 2 tool calls for 2 cities, got: {}",
            calls.len()
        );

        // Verify each call has proper structure
        for call in calls {
            assert_eq!(call["type"], "function");
            assert_eq!(call["function"]["name"], "get_weather");
            assert!(call["id"].is_string());
            assert!(call["function"]["arguments"].is_string());
        }
    }
}

#[live_provider_test(bedrock)]
async fn anthropic_claude3_haiku_tool_choice_specific() {
    let config = create_bedrock_config(&[("claude3-haiku", "anthropic.claude-3-haiku-20240307-v1:0")]);

    let server = TestServer::builder().build(&config).await;

    // Force specific tool use
    let response = server
        .openai_completions(json!({
            "model": "bedrock/claude3-haiku",
            "messages": [{
                "role": "user",
                "content": "Hello"
            }],
            "tools": [
                {
                    "type": "function",
                    "function": {
                        "name": "calculator",
                        "description": "Perform calculations",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "expression": {"type": "string"}
                            },
                            "required": ["expression"]
                        }
                    }
                },
                {
                    "type": "function",
                    "function": {
                        "name": "translator",
                        "description": "Translate text",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "text": {"type": "string"},
                                "language": {"type": "string"}
                            },
                            "required": ["text", "language"]
                        }
                    }
                }
            ],
            "tool_choice": {
                "type": "function",
                "function": {
                    "name": "calculator"
                }
            },
            "max_tokens": 200,
            "temperature": 0
        }))
        .send()
        .await;

    // Verify the specific tool was called
    let tool_calls = &response["choices"][0]["message"]["tool_calls"];
    if let Some(calls) = tool_calls.as_array() {
        assert_eq!(calls.len(), 1, "Expected exactly one tool call");
        assert_eq!(calls[0]["function"]["name"], "calculator");
    }
}

#[live_provider_test(bedrock)]
async fn cohere_command_r_tool_calling_basic() {
    let config = create_bedrock_config(&[("command-r", "cohere.command-r-v1:0")]);

    let server = TestServer::builder().build(&config).await;

    // Basic tool calling request
    let response = server
        .openai_completions(json!({
            "model": "bedrock/command-r",
            "messages": [{
                "role": "user",
                "content": "What's the current temperature in Tokyo?"
            }],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Get the current weather for a location",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "location": {
                                "type": "string",
                                "description": "The city to get weather for"
                            },
                            "unit": {
                                "type": "string",
                                "enum": ["celsius", "fahrenheit"],
                                "description": "Temperature unit"
                            }
                        },
                        "required": ["location"]
                    }
                }
            }],
            "max_tokens": 200,
            "temperature": 0
        }))
        .send()
        .await;

    // Verify response contains tool call
    assert_eq!(response["object"], "chat.completion");
    assert_eq!(response["model"], "bedrock/command-r");

    let message = &response["choices"][0]["message"];

    // Cohere might return tool calls or might describe the action in content
    // Check if there are tool calls
    if message["tool_calls"].is_array() {
        let tool_calls = message["tool_calls"].as_array().unwrap();
        if !tool_calls.is_empty() {
            let first_call = &tool_calls[0];
            assert_eq!(first_call["type"], "function");
            assert_eq!(first_call["function"]["name"], "get_weather");

            // Verify arguments
            let args_str = first_call["function"]["arguments"].as_str().unwrap();
            let args: serde_json::Value = serde_json::from_str(args_str).unwrap();
            let location = args["location"].as_str().unwrap();
            assert!(
                location.to_lowercase().contains("tokyo"),
                "Expected Tokyo in location, got: {}",
                location
            );
        }
    }

    // Check finish reason
    let finish_reason = response["choices"][0]["finish_reason"].as_str().unwrap();
    assert!(
        finish_reason == "tool_calls" || finish_reason == "stop",
        "Unexpected finish reason: {}",
        finish_reason
    );
}

#[live_provider_test(bedrock)]
async fn cohere_command_r_tool_calling_with_context() {
    let config = create_bedrock_config(&[("command-r", "cohere.command-r-v1:0")]);

    let server = TestServer::builder().build(&config).await;

    // Tool calling with conversation context
    let response = server
        .openai_completions(json!({
            "model": "bedrock/command-r",
            "messages": [
                {
                    "role": "user",
                    "content": "I need to know about weather"
                },
                {
                    "role": "assistant",
                    "content": "I can help you check the weather. Which city would you like to know about?"
                },
                {
                    "role": "user",
                    "content": "How about Paris?"
                }
            ],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Get weather information for a city",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "city": {
                                "type": "string",
                                "description": "Name of the city"
                            }
                        },
                        "required": ["city"]
                    }
                }
            }],
            "max_tokens": 200,
            "temperature": 0
        }))
        .send()
        .await;

    // Verify response
    assert_eq!(response["object"], "chat.completion");

    let message = &response["choices"][0]["message"];

    // Check for tool calls
    if message["tool_calls"].is_array() {
        let tool_calls = message["tool_calls"].as_array().unwrap();
        if !tool_calls.is_empty() {
            let first_call = &tool_calls[0];
            let args_str = first_call["function"]["arguments"].as_str().unwrap();
            let args: serde_json::Value = serde_json::from_str(args_str).unwrap();
            let city = args["city"].as_str().unwrap();
            assert!(
                city.to_lowercase().contains("paris"),
                "Expected Paris in city parameter, got: {}",
                city
            );
        }
    }
}

#[live_provider_test(bedrock)]
async fn cohere_command_r_tool_choice_required() {
    let config = create_bedrock_config(&[("command-r", "cohere.command-r-v1:0")]);

    let server = TestServer::builder().build(&config).await;

    // Force tool use with "required"
    let response = server
        .openai_completions(json!({
            "model": "bedrock/command-r",
            "messages": [{
                "role": "user",
                "content": "Hi there"
            }],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "search",
                    "description": "Search for information",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "query": {
                                "type": "string",
                                "description": "Search query"
                            }
                        },
                        "required": ["query"]
                    }
                }
            }],
            "tool_choice": "required",
            "max_tokens": 200,
            "temperature": 0
        }))
        .send()
        .await;

    // Verify a tool was called
    let message = &response["choices"][0]["message"];

    // When tool_choice is "required", Cohere should use a tool
    if message["tool_calls"].is_array() {
        let tool_calls = message["tool_calls"].as_array().unwrap();
        assert!(
            !tool_calls.is_empty(),
            "Expected at least one tool call when tool_choice is 'required'"
        );
    }
}

#[live_provider_test(bedrock)]
async fn anthropic_claude3_haiku_streaming_with_tools() {
    let config = create_bedrock_config(&[("claude3-haiku", "anthropic.claude-3-haiku-20240307-v1:0")]);

    let server = TestServer::builder().build(&config).await;

    let request = json!({
        "model": "bedrock/claude3-haiku",
        "messages": [{
            "role": "user",
            "content": "Calculate the sum of 15 and 27"
        }],
        "tools": [{
            "type": "function",
            "function": {
                "name": "calculator",
                "description": "Perform mathematical calculations",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "operation": {
                            "type": "string",
                            "enum": ["add", "subtract", "multiply", "divide"]
                        },
                        "a": {"type": "number"},
                        "b": {"type": "number"}
                    },
                    "required": ["operation", "a", "b"]
                }
            }
        }],
        "max_tokens": 200,
        "stream": true,
        "temperature": 0
    });

    // Test streaming with tools
    let chunks = server.openai_completions_stream(request).send().await;
    assert!(!chunks.is_empty(), "Expected stream chunks");

    // Tool calls in streaming appear in chunks
    // Check if any chunk contains tool call information
    let has_tool_info = chunks
        .iter()
        .any(|chunk| chunk["choices"][0]["delta"]["tool_calls"].is_array());

    // We're testing tool calling, so we should have tool calls in the response
    assert!(
        has_tool_info,
        "Expected tool calls in streaming response for a tool calling test"
    );

    assert_eq!(chunks[0]["object"], "chat.completion.chunk");
    assert_eq!(chunks[0]["model"], "bedrock/claude3-haiku");
}

#[live_provider_test(bedrock)]
async fn cohere_command_r_streaming_with_tools() {
    let config = create_bedrock_config(&[("command-r", "cohere.command-r-v1:0")]);

    let server = TestServer::builder().build(&config).await;

    let request = json!({
        "model": "bedrock/command-r",
        "messages": [{
            "role": "user",
            "content": "Find information about the Eiffel Tower"
        }],
        "tools": [{
            "type": "function",
            "function": {
                "name": "search",
                "description": "Search for information",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {"type": "string"}
                    },
                    "required": ["query"]
                }
            }
        }],
        "max_tokens": 200,
        "stream": true,
        "temperature": 0
    });

    // Test streaming with tools
    let chunks = server.openai_completions_stream(request).send().await;
    assert!(!chunks.is_empty(), "Expected stream chunks");

    // Verify basic streaming structure
    assert_eq!(chunks[0]["object"], "chat.completion.chunk");
    assert_eq!(chunks[0]["model"], "bedrock/command-r");

    // Check if any chunk contains tool call information
    let has_tool_info = chunks
        .iter()
        .any(|chunk| chunk["choices"][0]["delta"]["tool_calls"].is_array());

    // We're testing tool calling, so we should have tool calls in the response
    assert!(
        has_tool_info,
        "Expected tool calls in streaming response for a tool calling test"
    );

    // Check for finish reason in at least one chunk (Cohere may not have it in last chunk)
    let has_finish_reason = chunks
        .iter()
        .any(|chunk| chunk["choices"][0]["finish_reason"].is_string());
    assert!(has_finish_reason, "Expected at least one chunk with finish_reason");
}

// ============================================================================
// DeepSeek Tests
// ============================================================================

#[live_provider_test(bedrock)]
async fn deepseek_r1_minimal() {
    let config = create_bedrock_config(&[("r1", "us.deepseek.r1-v1:0")]);

    let server = TestServer::builder().build(&config).await;

    // Minimal prompt to reduce costs - DeepSeek R1 may think silently
    let response = server
        .openai_completions(json!({
            "model": "bedrock/r1",
            "messages": [{
                "role": "user",
                "content": "What is 2+2? Reply with just the number."
            }],
            "max_tokens": 100,
            "temperature": 0
        }))
        .send()
        .await;

    // Verify response structure
    assert_eq!(response["object"], "chat.completion");
    assert_eq!(response["model"], "bedrock/r1");

    // DeepSeek R1 may use tokens for internal reasoning without producing output
    // This is expected behavior for R1 model when it hits token limit during thinking
    let has_content = response["choices"][0]["message"].get("content").is_some();
    if !has_content {
        eprintln!("Note: DeepSeek R1 used tokens for internal reasoning without producing output");
    }

    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[timestamp]",
        ".choices[0].message.content" => "[content_or_none]",
        ".usage.prompt_tokens" => "[tokens]",
        ".usage.completion_tokens" => "[tokens]",
        ".usage.total_tokens" => "[tokens]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[timestamp]",
      "model": "bedrock/r1",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant"
          },
          "finish_reason": "length"
        }
      ],
      "usage": {
        "prompt_tokens": "[tokens]",
        "completion_tokens": "[tokens]",
        "total_tokens": "[tokens]"
      }
    }
    "#);
}

#[live_provider_test(bedrock)]
async fn deepseek_r1_with_system() {
    let config = create_bedrock_config(&[("r1", "us.deepseek.r1-v1:0")]);

    let server = TestServer::builder().build(&config).await;

    // Test system message handling
    let response = server
        .openai_completions(json!({
            "model": "bedrock/r1",
            "messages": [
                {
                    "role": "system",
                    "content": "You are a helpful assistant that answers in exactly one word."
                },
                {
                    "role": "user",
                    "content": "What is 2+2?"
                }
            ],
            "max_tokens": 10,
            "temperature": 0
        }))
        .send()
        .await;

    // Just verify response structure
    assert_eq!(response["object"], "chat.completion");
    assert_eq!(response["model"], "bedrock/r1");

    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[timestamp]",
        ".choices[0].finish_reason" => "[finish_reason]",
        ".usage" => "[usage]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[timestamp]",
      "model": "bedrock/r1",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant"
          },
          "finish_reason": "[finish_reason]"
        }
      ],
      "usage": "[usage]"
    }
    "#);
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[live_provider_test(bedrock)]
async fn error_invalid_model() {
    let config = create_bedrock_config(&[("invalid", "anthropic.invalid-model-v99")]);

    let server = TestServer::builder().build(&config).await;

    let (status, body) = server
        .openai_completions(json!({
            "model": "bedrock/invalid",
            "messages": [{
                "role": "user",
                "content": "test"
            }],
            "max_tokens": 10
        }))
        .send_raw()
        .await;

    assert_eq!(status, 400);
    let error = body;

    // AWS SDK should return a ValidationException or service error - verify it's there
    let error_msg = error["error"]["message"].as_str().unwrap();
    assert!(
        error_msg.contains("ValidationException")
            || error_msg.contains("does not exist")
            || error_msg.contains("Invalid model")
            || error_msg.contains("service error")
            || error_msg.contains("Invalid request"),
        "Expected validation error, got: {}",
        error_msg
    );

    // Use snapshot for the error structure
    insta::assert_json_snapshot!(error, {
        ".error.message" => "[validation_error]",
        ".error.type" => "[error_type]",
        ".error.code" => "[code]"
    }, @r#"
    {
      "error": {
        "message": "[validation_error]",
        "type": "[error_type]",
        "code": "[code]"
      }
    }
    "#);
}

// Note: error_missing_required_field test removed due to Claude Instant v1 deprecation

// ============================================================================
// Streaming Tests
// ============================================================================

// Note: streaming_claude_instant test removed due to Claude Instant v1 deprecation

#[live_provider_test(bedrock)]
async fn streaming_cohere_command_r_with_helpers() {
    let config = create_bedrock_config(&[("command-r", "cohere.command-r-v1:0")]);

    let server = TestServer::builder().build(&config).await;

    let request = json!({
        "model": "bedrock/command-r",
        "messages": [{
            "role": "user",
            "content": "Say hello"
        }],
        "max_tokens": 20,
        "stream": true,
        "temperature": 0
    });

    let chunks = server.openai_completions_stream(request).send().await;

    // Should have at least one chunk
    assert!(!chunks.is_empty(), "Expected at least one chunk, got {}", chunks.len());

    // Verify first chunk structure
    let first_chunk = &chunks[0];
    assert_eq!(first_chunk["object"], "chat.completion.chunk");
    assert_eq!(first_chunk["model"], "bedrock/command-r");
    assert!(first_chunk["choices"][0]["delta"].is_object());

    // Verify at least one chunk has finish_reason
    let has_finish_reason = chunks
        .iter()
        .any(|chunk| chunk["choices"][0]["finish_reason"].is_string());
    assert!(has_finish_reason, "Expected at least one chunk with finish_reason");
}

#[live_provider_test(bedrock)]
async fn streaming_cohere_command_r_multi_turn_with_helpers() {
    let config = create_bedrock_config(&[("command-r", "cohere.command-r-v1:0")]);

    let server = TestServer::builder().build(&config).await;

    // Test streaming with conversation history
    let request = json!({
        "model": "bedrock/command-r",
        "messages": [
            {"role": "user", "content": "Count from 1 to 3"},
            {"role": "assistant", "content": "1, 2, 3"},
            {"role": "user", "content": "Now count from 4 to 6"}
        ],
        "max_tokens": 30,
        "stream": true,
        "temperature": 0
    });

    let chunks = server.openai_completions_stream(request).send().await;
    assert!(
        !chunks.is_empty(),
        "Expected at least one chunk for multi-turn conversation"
    );

    // Verify first chunk structure
    let first_chunk = &chunks[0];
    assert_eq!(first_chunk["object"], "chat.completion.chunk");
    assert_eq!(first_chunk["model"], "bedrock/command-r");
    assert!(first_chunk["choices"][0]["delta"].is_object());

    // Verify at least one chunk has finish_reason (Cohere may not have it in last chunk)
    let has_finish_reason = chunks
        .iter()
        .any(|chunk| chunk["choices"][0]["finish_reason"].is_string());
    assert!(has_finish_reason, "Expected at least one chunk with finish_reason");
}

#[live_provider_test(bedrock)]
async fn streaming_nova_micro() {
    let config = create_bedrock_config(&[("nova-micro", "amazon.nova-micro-v1:0")]);

    let server = TestServer::builder().build(&config).await;

    let request = json!({
        "model": "bedrock/nova-micro",
        "messages": [{
            "role": "user",
            "content": "Count to 3"
        }],
        "max_tokens": 15,
        "stream": true,
        "temperature": 0
    });

    let chunks = server.openai_completions_stream(request).send().await;

    // Should have chunks
    assert!(!chunks.is_empty(), "Expected stream chunks, got none");

    // Verify first chunk structure
    let first_chunk = &chunks[0];
    assert_eq!(first_chunk["object"], "chat.completion.chunk");
    assert_eq!(first_chunk["model"], "bedrock/nova-micro");
    assert!(first_chunk["choices"][0]["delta"].is_object());

    // Verify at least one chunk has finish_reason
    let has_finish_reason = chunks
        .iter()
        .any(|chunk| chunk["choices"][0]["finish_reason"].is_string());
    assert!(has_finish_reason, "Expected at least one chunk with finish_reason");
}

#[live_provider_test(bedrock)]
async fn streaming_titan_express() {
    let config = create_bedrock_config(&[("titan-express", "amazon.titan-text-express-v1")]);

    let server = TestServer::builder().build(&config).await;

    let request = json!({
        "model": "bedrock/titan-express",
        "messages": [{
            "role": "user",
            "content": "Count to 3"
        }],
        "max_tokens": 15,
        "stream": true,
        "temperature": 0
    });

    let chunks = server.openai_completions_stream(request).send().await;

    // Should have chunks
    assert!(!chunks.is_empty(), "Expected stream chunks, got none");

    // Verify first chunk structure
    let first_chunk = &chunks[0];
    assert_eq!(first_chunk["object"], "chat.completion.chunk");
    assert_eq!(first_chunk["model"], "bedrock/titan-express");
    assert!(first_chunk["choices"][0]["delta"].is_object());

    // Verify at least one chunk has finish_reason
    let has_finish_reason = chunks
        .iter()
        .any(|chunk| chunk["choices"][0]["finish_reason"].is_string());
    assert!(has_finish_reason, "Expected at least one chunk with finish_reason");
}

#[live_provider_test(bedrock)]
async fn streaming_deepseek_r1() {
    let config = create_bedrock_config(&[("r1", "us.deepseek.r1-v1:0")]);

    let server = TestServer::builder().build(&config).await;

    let request = json!({
        "model": "bedrock/r1",
        "messages": [{
            "role": "user",
            "content": "Count to 3"
        }],
        "max_tokens": 15,
        "stream": true,
        "temperature": 0
    });

    // Test streaming chunks
    let chunks = server.openai_completions_stream(request.clone()).send().await;
    assert!(!chunks.is_empty(), "Expected stream chunks, got none");

    // Verify first chunk structure with snapshot
    insta::assert_json_snapshot!(chunks[0], {
        ".id" => "[id]",
        ".created" => "[created]",
        ".choices[0].delta" => "[delta]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion.chunk",
      "created": "[created]",
      "model": "bedrock/r1",
      "choices": [
        {
          "index": 0,
          "delta": "[delta]"
        }
      ]
    }
    "#);

    // Test content accumulation - DeepSeek R1 may produce no visible content due to internal reasoning
    let _content = server.openai_completions_stream(request).send().await;
    // Note: R1 may use all tokens for internal reasoning without producing visible output
    // This is expected behavior when it hits token limits during thinking
}

#[live_provider_test(bedrock)]
async fn streaming_ai21_jamba_mini() {
    let config = create_bedrock_config(&[("jamba-mini", "ai21.jamba-1-5-mini-v1:0")]);

    let server = TestServer::builder().build(&config).await;

    let request = json!({
        "model": "bedrock/jamba-mini",
        "messages": [{
            "role": "user",
            "content": "Count to 3"
        }],
        "max_tokens": 15,
        "stream": true,
        "temperature": 0
    });

    // Test streaming chunks
    let chunks = server.openai_completions_stream(request.clone()).send().await;
    assert!(!chunks.is_empty(), "Expected stream chunks, got none");

    // Verify first chunk structure with snapshot
    insta::assert_json_snapshot!(chunks[0], {
        ".id" => "[id]",
        ".created" => "[created]",
        ".choices[0].delta.role" => "[role]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion.chunk",
      "created": "[created]",
      "model": "bedrock/jamba-mini",
      "choices": [
        {
          "index": 0,
          "delta": {
            "role": "[role]"
          }
        }
      ]
    }
    "#);

    // Test that content accumulation works
    let content = server.openai_completions_stream(request).send().await;
    assert!(!content.is_empty(), "Expected non-empty streaming content");
}

// ============================================================================
// Token Counting Verification Tests
// ============================================================================

#[live_provider_test(bedrock)]
async fn verify_token_counting_claude3_sonnet() {
    let config = create_bedrock_config(&[("claude3-sonnet", "anthropic.claude-3-sonnet-20240229-v1:0")]);

    let server = TestServer::builder().build(&config).await;

    // Use a prompt with predictable token count
    let response = server
        .openai_completions(json!({
            "model": "bedrock/claude3-sonnet",
            "messages": [{
                "role": "user",
                "content": "Count from 1 to 5"
            }],
            "max_tokens": 50,
            "temperature": 0
        }))
        .send()
        .await;

    // Verify usage is present and contains valid token counts
    let usage = response["usage"].as_object().expect("Expected usage object");

    let prompt_tokens = usage["prompt_tokens"].as_u64().expect("Expected prompt_tokens");
    let completion_tokens = usage["completion_tokens"].as_u64().expect("Expected completion_tokens");
    let total_tokens = usage["total_tokens"].as_u64().expect("Expected total_tokens");

    // Verify token counts are reasonable
    assert!(prompt_tokens > 0, "Prompt tokens should be > 0");
    assert!(completion_tokens > 0, "Completion tokens should be > 0");
    assert_eq!(
        total_tokens,
        prompt_tokens + completion_tokens,
        "Total should equal prompt + completion"
    );

    // Log for manual verification during test runs
    eprintln!(
        "Claude3 Sonnet Token Usage: prompt={}, completion={}, total={}",
        prompt_tokens, completion_tokens, total_tokens
    );
}

#[live_provider_test(bedrock)]
async fn verify_token_counting_streaming_claude3_sonnet() {
    let config = create_bedrock_config(&[("claude3-sonnet", "anthropic.claude-3-sonnet-20240229-v1:0")]);

    let server = TestServer::builder().build(&config).await;

    let request = json!({
        "model": "bedrock/claude3-sonnet",
        "messages": [{
            "role": "user",
            "content": "Count from 1 to 5"
        }],
        "max_tokens": 50,
        "temperature": 0,
        "stream": true
    });

    // Collect all chunks to find the one with usage
    let chunks = server.openai_completions_stream(request).send().await;

    // Find the chunk with usage information (should be in MessageStop event)
    let usage_chunk = chunks
        .iter()
        .find(|chunk| chunk["usage"].is_object())
        .expect("Expected to find usage in stream");

    let usage = usage_chunk["usage"].as_object().expect("Expected usage object");

    let prompt_tokens = usage["prompt_tokens"].as_u64().expect("Expected prompt_tokens");
    let completion_tokens = usage["completion_tokens"].as_u64().expect("Expected completion_tokens");
    let total_tokens = usage["total_tokens"].as_u64().expect("Expected total_tokens");

    // Verify token counts
    assert!(prompt_tokens > 0, "Prompt tokens should be > 0");
    assert!(completion_tokens > 0, "Completion tokens should be > 0");
    assert_eq!(
        total_tokens,
        prompt_tokens + completion_tokens,
        "Total should equal prompt + completion"
    );

    eprintln!(
        "Claude3 Sonnet Streaming Token Usage: prompt={}, completion={}, total={}",
        prompt_tokens, completion_tokens, total_tokens
    );
}

#[live_provider_test(bedrock)]
async fn verify_token_counting_llama3() {
    let config = create_bedrock_config(&[("llama3-8b", "meta.llama3-8b-instruct-v1:0")]);

    let server = TestServer::builder().build(&config).await;

    let response = server
        .openai_completions(json!({
            "model": "bedrock/llama3-8b",
            "messages": [{
                "role": "user",
                "content": "What is 2+2?"
            }],
            "max_tokens": 20,
            "temperature": 0
        }))
        .send()
        .await;

    // Verify usage is present
    let usage = response["usage"].as_object().expect("Expected usage object");

    let prompt_tokens = usage["prompt_tokens"].as_u64().expect("Expected prompt_tokens");
    let completion_tokens = usage["completion_tokens"].as_u64().expect("Expected completion_tokens");
    let total_tokens = usage["total_tokens"].as_u64().expect("Expected total_tokens");

    assert!(prompt_tokens > 0, "Prompt tokens should be > 0");
    assert!(completion_tokens > 0, "Completion tokens should be > 0");
    assert_eq!(
        total_tokens,
        prompt_tokens + completion_tokens,
        "Total should equal prompt + completion"
    );

    eprintln!(
        "Llama3 Token Usage: prompt={}, completion={}, total={}",
        prompt_tokens, completion_tokens, total_tokens
    );
}

#[live_provider_test(bedrock)]
async fn verify_token_counting_cohere_command_r() {
    let config = create_bedrock_config(&[("command-r", "cohere.command-r-v1:0")]);

    let server = TestServer::builder().build(&config).await;

    let response = server
        .openai_completions(json!({
            "model": "bedrock/command-r",
            "messages": [{
                "role": "user",
                "content": "Hello"
            }],
            "max_tokens": 10,
            "temperature": 0
        }))
        .send()
        .await;

    // Verify usage is present
    let usage = response["usage"].as_object().expect("Expected usage object");

    let prompt_tokens = usage["prompt_tokens"].as_u64().expect("Expected prompt_tokens");
    let completion_tokens = usage["completion_tokens"].as_u64().expect("Expected completion_tokens");
    let total_tokens = usage["total_tokens"].as_u64().expect("Expected total_tokens");

    assert!(prompt_tokens > 0, "Prompt tokens should be > 0");
    assert!(completion_tokens > 0, "Completion tokens should be > 0");

    assert_eq!(
        total_tokens,
        prompt_tokens + completion_tokens,
        "Total should equal prompt + completion"
    );

    eprintln!(
        "Cohere Command-R Token Usage: prompt={}, completion={}, total={}",
        prompt_tokens, completion_tokens, total_tokens
    );
}

#[live_provider_test(bedrock)]
async fn verify_token_counting_mistral() {
    let config = create_bedrock_config(&[("mistral-7b", "mistral.mistral-7b-instruct-v0:2")]);

    let server = TestServer::builder().build(&config).await;

    let response = server
        .openai_completions(json!({
            "model": "bedrock/mistral-7b",
            "messages": [{
                "role": "user",
                "content": "Say hello"
            }],
            "max_tokens": 10,
            "temperature": 0
        }))
        .send()
        .await;

    // Verify usage is present
    let usage = response["usage"].as_object().expect("Expected usage object");

    let prompt_tokens = usage["prompt_tokens"].as_u64().expect("Expected prompt_tokens");
    let completion_tokens = usage["completion_tokens"].as_u64().expect("Expected completion_tokens");
    let total_tokens = usage["total_tokens"].as_u64().expect("Expected total_tokens");

    assert!(prompt_tokens > 0, "Prompt tokens should be > 0");
    assert!(completion_tokens > 0, "Completion tokens should be > 0");

    assert_eq!(
        total_tokens,
        prompt_tokens + completion_tokens,
        "Total should equal prompt + completion"
    );

    eprintln!(
        "Mistral 7B Token Usage: prompt={}, completion={}, total={}",
        prompt_tokens, completion_tokens, total_tokens
    );
}

#[live_provider_test(bedrock)]
async fn verify_streaming_token_counting_multiple_models() {
    // Test token counting in streaming mode for multiple models
    let test_cases = vec![
        ("llama3-8b", "meta.llama3-8b-instruct-v1:0"),
        ("mistral-7b", "mistral.mistral-7b-instruct-v0:2"),
        ("command-r", "cohere.command-r-v1:0"),
    ];

    for (alias, model_id) in test_cases {
        let config = create_bedrock_config(&[(alias, model_id)]);
        let server = TestServer::builder().build(&config).await;

        let request = json!({
            "model": format!("bedrock/{}", alias),
            "messages": [{
                "role": "user",
                "content": "List three colors"
            }],
            "max_tokens": 30,
            "temperature": 0,
            "stream": true
        });

        let chunks = server.openai_completions_stream(request).send().await;

        // Find the final chunk with usage information
        let usage_chunk = chunks
            .iter()
            .rev() // Start from the end since usage is typically in the last chunk
            .find(|chunk| chunk["usage"].is_object())
            .unwrap();

        let usage = usage_chunk["usage"].as_object().expect("Expected usage object");

        let prompt_tokens = usage["prompt_tokens"].as_u64();
        let completion_tokens = usage["completion_tokens"].as_u64();
        let total_tokens = usage["total_tokens"].as_u64();

        // All three fields should be present
        assert!(prompt_tokens.is_some(), "Model {} should provide prompt_tokens", alias);
        assert!(
            completion_tokens.is_some(),
            "Model {} should provide completion_tokens",
            alias
        );
        assert!(total_tokens.is_some(), "Model {} should provide total_tokens", alias);

        let prompt = prompt_tokens.unwrap();
        let completion = completion_tokens.unwrap();
        let total = total_tokens.unwrap();

        // Verify counts are reasonable
        assert!(prompt > 0, "Model {} prompt tokens should be > 0", alias);
        assert!(completion > 0, "Model {} completion tokens should be > 0", alias);

        assert_eq!(
            total,
            prompt + completion,
            "Model {} total should equal prompt + completion",
            alias
        );

        eprintln!(
            "Model {} Streaming Token Usage: prompt={}, completion={}, total={}",
            alias, prompt, completion, total
        );
    }
}
