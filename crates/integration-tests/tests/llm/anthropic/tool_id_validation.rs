use indoc::indoc;
use integration_tests::{TestServer, llms::AnthropicMock};
use serde_json::json;

/// Test that duplicate tool_use IDs in content blocks correctly fail with 422 error.
/// This is the client's responsibility to fix, not ours.
#[tokio::test]
async fn duplicate_tool_ids_fail_as_expected() {
    let mut builder = TestServer::builder();

    // Create a mock that will fail if it receives duplicate tool IDs
    // The mock itself validates that the Anthropic API would accept the request
    let mock = AnthropicMock::new("anthropic").with_models(vec!["claude-3-5-haiku-latest".to_string()]);

    builder.spawn_llm(mock).await;

    let config = indoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.anthropic]
        enabled = true
        path = "/llm/anthropic"
    "#};

    let server = builder.build(config).await;

    // This simulates what Claude Code sends - assistant messages with tool_use blocks
    // that have duplicate IDs (like when it calls Glob and Bash together)
    let request = json!({
        "model": "anthropic/claude-3-5-haiku-latest",
        "messages": [
            {
                "role": "user",
                "content": "Help me find and run files"
            },
            {
                "role": "assistant",
                "content": [
                    {
                        "type": "text",
                        "text": "I'll search for files and run a command."
                    },
                    {
                        "type": "tool_use",
                        "id": "toolu_01XyzAbc123",  // Duplicate ID
                        "name": "Glob",
                        "input": {"pattern": "*.toml"}
                    },
                    {
                        "type": "tool_use",
                        "id": "toolu_01XyzAbc123",  // Same ID - THIS IS A CLIENT BUG
                        "name": "Bash",
                        "input": {"command": "ls -la"}
                    }
                ]
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_01XyzAbc123",
                        "content": [{"type": "text", "text": "Found: Cargo.toml"}]
                    },
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_01XyzAbc123",
                        "content": [{"type": "text", "text": "total 24\ndrwxr-xr-x..."}]
                    }
                ]
            }
        ],
        "max_tokens": 100,
        "stream": false
    });

    // This SHOULD fail - it's the client's responsibility to send unique IDs
    // Nexus will return 502 (Bad Gateway) when the upstream provider returns an error
    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{}/llm/anthropic/v1/messages", server.address))
        .header("x-api-key", "test-key")
        .header("anthropic-version", "2023-06-01")
        .header("Content-Type", "application/json")
        .json(&request)
        .send()
        .await
        .unwrap();

    // Nexus returns 502 when the provider returns an error
    assert_eq!(response.status(), 502);

    let body = response.json::<serde_json::Value>().await.unwrap();

    // Verify the error message using insta snapshot
    insta::assert_json_snapshot!(body, @r###"
    {
      "error": {
        "message": "Provider API error (422): {\"error\":{\"type\":\"invalid_request_error\",\"message\":\"messages: `tool_use` ids must be unique (duplicate id: toolu_01XyzAbc123)\"}}",
        "type": "api_error",
        "code": 502
      }
    }
    "###);
}

/// Test that unique tool_use IDs are preserved and work correctly
#[tokio::test]
async fn unique_tool_ids_work_correctly() {
    let mut builder = TestServer::builder();

    let mock = AnthropicMock::new("anthropic").with_models(vec!["claude-3-5-haiku-latest".to_string()]);

    builder.spawn_llm(mock).await;

    let config = indoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.anthropic]
        enabled = true
        path = "/llm/anthropic"
    "#};

    let server = builder.build(config).await;

    // Send a request with UNIQUE tool_use IDs - these should work fine
    let request = json!({
        "model": "anthropic/claude-3-5-haiku-latest",
        "messages": [
            {
                "role": "assistant",
                "content": [
                    {
                        "type": "tool_use",
                        "id": "toolu_unique_id_001",  // Unique ID
                        "name": "Glob",
                        "input": {"pattern": "*.rs"}
                    },
                    {
                        "type": "tool_use",
                        "id": "toolu_unique_id_002",  // Different unique ID
                        "name": "Bash",
                        "input": {"command": "ls"}
                    }
                ]
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_unique_id_001",  // Matches first tool_use
                        "content": [{"type": "text", "text": "Found files"}]
                    },
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_unique_id_002",  // Matches second tool_use
                        "content": [{"type": "text", "text": "Directory listing"}]
                    }
                ]
            }
        ],
        "max_tokens": 100,
        "stream": false
    });

    // This should succeed without any issues
    let response = server.anthropic_completions(request).send().await;

    // Verify successful response
    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".usage" => "[usage]"
    }, @r###"
    {
      "id": "[id]",
      "type": "message",
      "role": "assistant",
      "content": [
        {
          "type": "text",
          "text": "Test response to: "
        }
      ],
      "model": "anthropic/claude-3-5-haiku-latest",
      "stop_reason": null,
      "stop_sequence": null,
      "usage": "[usage]"
    }
    "###);
}

/// Test that the mock properly validates duplicate tool_use IDs like the real Anthropic API
#[tokio::test]
async fn mock_validates_duplicate_tool_ids() {
    // Test directly against the mock to verify it rejects duplicate IDs
    let mock = integration_tests::llms::TestAnthropicServer::spawn().await.unwrap();
    let client = reqwest::Client::new();

    let request = json!({
        "model": "claude-3-5-sonnet-20241022",
        "messages": [
            {
                "role": "assistant",
                "content": [
                    {
                        "type": "tool_use",
                        "id": "duplicate_id",  // First use
                        "name": "tool1",
                        "input": {"arg": "value1"}
                    },
                    {
                        "type": "tool_use",
                        "id": "duplicate_id",  // DUPLICATE - should cause 422
                        "name": "tool2",
                        "input": {"arg": "value2"}
                    }
                ]
            }
        ],
        "max_tokens": 100
    });

    let response = client
        .post(format!("{}/messages", mock.url()))
        .header("x-api-key", "test-key")
        .header("anthropic-version", "2023-06-01")
        .json(&request)
        .send()
        .await
        .unwrap();

    // The mock should reject this with 422, just like the real Anthropic API
    assert_eq!(response.status(), 422);

    let error_body = response.json::<serde_json::Value>().await.unwrap();

    // Verify error response using insta snapshot
    insta::assert_json_snapshot!(error_body, @r###"
    {
      "error": {
        "type": "invalid_request_error",
        "message": "messages: `tool_use` ids must be unique (duplicate id: duplicate_id)"
      }
    }
    "###);
}

/// Test that we don't accidentally duplicate tool_calls when converting
/// from Unified back to Anthropic format (checking our deduplication logic in input.rs)
#[tokio::test]
async fn no_duplicate_tool_calls_when_already_in_content() {
    let mut builder = TestServer::builder();

    let mock = AnthropicMock::new("anthropic").with_models(vec!["claude-3-opus-20240229".to_string()]);

    builder.spawn_llm(mock).await;

    let config = indoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.anthropic]
        enabled = true
        path = "/llm/anthropic"
    "#};

    let server = builder.build(config).await;

    // This tests the specific case where tool_use blocks already exist in content
    // and we must not add them again as tool_calls
    let request = json!({
        "model": "anthropic/claude-3-opus-20240229",
        "messages": [
            {
                "role": "user",
                "content": "Can you help me?"
            },
            {
                "role": "assistant",
                "content": [
                    {
                        "type": "text",
                        "text": "I'll use these tools to help."
                    },
                    {
                        "type": "tool_use",
                        "id": "tool_123",
                        "name": "calculator",
                        "input": {"expression": "2+2"}
                    },
                    {
                        "type": "tool_use",
                        "id": "tool_456",
                        "name": "converter",
                        "input": {"value": 100, "from": "USD", "to": "EUR"}
                    }
                ]
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "tool_123",
                        "content": [{"type": "text", "text": "4"}]
                    },
                    {
                        "type": "tool_result",
                        "tool_use_id": "tool_456",
                        "content": [{"type": "text", "text": "92.5 EUR"}]
                    }
                ]
            }
        ],
        "max_tokens": 50,
        "stream": false
    });

    let response = server.anthropic_completions(request).send().await;

    // Verify the request succeeds - if tool_calls were duplicated, we'd get an error
    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".usage" => "[usage]"
    }, @r###"
    {
      "id": "[id]",
      "type": "message",
      "role": "assistant",
      "content": [
        {
          "type": "text",
          "text": "Test response to: Can you help me?"
        }
      ],
      "model": "anthropic/claude-3-opus-20240229",
      "stop_reason": null,
      "stop_sequence": null,
      "usage": "[usage]"
    }
    "###);
}
