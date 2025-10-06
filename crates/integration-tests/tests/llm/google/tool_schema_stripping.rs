use integration_tests::{TestServer, llms::GoogleMock};
use serde_json::json;

/// Test that $schema field is stripped from tool parameters when sending to Google API
#[tokio::test]
async fn google_strips_schema_field_from_tools() {
    let mut builder = TestServer::builder();

    // Set up mock Google server
    builder
        .spawn_llm(GoogleMock::new("google").with_models(vec!["gemini-2.0-flash".to_string()]))
        .await;

    let server = builder.build("").await;

    // Request with tools that include $schema field (as Claude Code would send)
    let request = json!({
        "model": "google/gemini-2.0-flash",
        "messages": [
            {
                "role": "user",
                "content": "Hello, what tools do you have?"
            }
        ],
        "tools": [
            {
                "type": "function",
                "function": {
                    "name": "test_function",
                    "description": "A test function",
                    "parameters": {
                        "$schema": "http://json-schema.org/draft-07/schema#",
                        "type": "object",
                        "properties": {
                            "param1": {
                                "type": "string",
                                "description": "First parameter"
                            }
                        },
                        "required": ["param1"],
                        "additionalProperties": false
                    }
                }
            }
        ],
        "max_tokens": 100
    });

    // This should succeed without error - the $schema field should be stripped
    let response = server.openai_completions(request).send().await;

    // Verify we got a valid response
    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[created]",
        ".usage" => "[usage]"
    }, @r#"
    {
      "choices": [
        {
          "finish_reason": "stop",
          "index": 0,
          "message": {
            "content": "Hello! I'm Gemini, a test assistant. How can I help you today?",
            "role": "assistant"
          }
        }
      ],
      "created": "[created]",
      "id": "[id]",
      "model": "google/gemini-2.0-flash",
      "object": "chat.completion",
      "usage": "[usage]"
    }
    "#);
}

/// Test that nested $schema fields are also stripped
#[tokio::test]
async fn google_strips_nested_schema_fields() {
    let mut builder = TestServer::builder();

    builder
        .spawn_llm(GoogleMock::new("google").with_models(vec!["gemini-2.0-flash".to_string()]))
        .await;

    let server = builder.build("").await;

    let request = json!({
        "model": "google/gemini-2.0-flash",
        "messages": [
            {
                "role": "user",
                "content": "Test nested schemas"
            }
        ],
        "tools": [
            {
                "type": "function",
                "function": {
                    "name": "complex_function",
                    "description": "A function with nested schemas",
                    "parameters": {
                        "$schema": "http://json-schema.org/draft-07/schema#",
                        "type": "object",
                        "properties": {
                            "nested_object": {
                                "type": "object",
                                "properties": {
                                    "sub_field": {
                                        "type": "string"
                                    }
                                },
                                "additionalProperties": true
                            },
                            "array_field": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "item_field": {
                                            "type": "number"
                                        }
                                    },
                                    "additionalProperties": false
                                }
                            }
                        },
                        "additionalProperties": false
                    }
                }
            }
        ],
        "max_tokens": 100
    });

    // Should succeed - all $schema and additionalProperties fields should be stripped
    let response = server.openai_completions(request).send().await;

    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[created]",
        ".usage" => "[usage]"
    }, @r#"
    {
      "choices": [
        {
          "finish_reason": "stop",
          "index": 0,
          "message": {
            "content": "Test response to: Test nested schemas",
            "role": "assistant"
          }
        }
      ],
      "created": "[created]",
      "id": "[id]",
      "model": "google/gemini-2.0-flash",
      "object": "chat.completion",
      "usage": "[usage]"
    }
    "#);
}
