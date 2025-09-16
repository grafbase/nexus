use indoc::indoc;
use integration_tests::{TestServer, llms::AnthropicMock};
use serde_json::json;

/// Test that we don't create duplicate tool_use IDs when converting messages
/// This was causing "tool_use ids must be unique" errors with Claude Code
#[tokio::test]
async fn no_duplicate_tool_ids_in_conversion() {
    let mut builder = TestServer::builder();

    // Set up mock Anthropic server
    builder
        .spawn_llm(AnthropicMock::new("anthropic").with_models(vec!["claude-3-5-haiku-latest".to_string()]))
        .await;

    // Configure with Anthropic protocol enabled
    let config = indoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.anthropic]
        enabled = true
        path = "/llm/anthropic"
    "#};

    let server = builder.build(config).await;

    // Simulate Claude Code's message format with tool uses in assistant message
    // This is what happens after Claude Code calls tools
    let request = json!({
        "model": "anthropic/claude-3-5-haiku-latest",
        "messages": [
            {
                "role": "user",
                "content": "can you describe me what's in the crates/config directory"
            },
            {
                "role": "assistant",
                "content": [
                    {
                        "type": "text",
                        "text": "I'll use the Glob and Bash tools to explore the crates/config directory."
                    },
                    {
                        "type": "tool_use",
                        "id": "toolu_01abc123",
                        "name": "Glob",
                        "input": {"pattern": "crates/config/*"}
                    },
                    {
                        "type": "tool_use",
                        "id": "toolu_01def456",
                        "name": "Bash",
                        "input": {"command": "ls crates/config"}
                    }
                ]
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_01abc123",
                        "content": [{"type": "text", "text": "Cargo.toml\nCLAUDE.md\nexamples\nsrc"}]
                    },
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_01def456",
                        "content": [{"type": "text", "text": "Cargo.toml\nCLAUDE.md\nexamples\nsrc"}]
                    }
                ]
            }
        ],
        "max_tokens": 1024,
        "stream": false
    });

    // This should not fail with "tool_use ids must be unique"
    let response = server.anthropic_completions(request).send().await;

    // Verify we got a valid response
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
          "text": "Test response to: can you describe me what's in the crates/config directory"
        }
      ],
      "model": "[model]",
      "stop_reason": null,
      "stop_sequence": null,
      "usage": "[usage]"
    }
    "###);
}

/// Test multiple tool calls in a single message to ensure IDs remain unique
#[tokio::test]
async fn multiple_tool_calls_remain_unique() {
    let mut builder = TestServer::builder();

    builder
        .spawn_llm(AnthropicMock::new("anthropic").with_models(vec!["claude-3-5-haiku-latest".to_string()]))
        .await;

    let config = indoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.anthropic]
        enabled = true
        path = "/llm/anthropic"
    "#};

    let server = builder.build(config).await;

    // Test with multiple unique tool calls
    let request = json!({
        "model": "anthropic/claude-3-5-haiku-latest",
        "messages": [
            {
                "role": "user",
                "content": "test multiple tools"
            },
            {
                "role": "assistant",
                "content": [
                    {
                        "type": "text",
                        "text": "I'll use multiple tools."
                    },
                    {
                        "type": "tool_use",
                        "id": "tool_1",
                        "name": "first_tool",
                        "input": {"param": "value1"}
                    },
                    {
                        "type": "tool_use",
                        "id": "tool_2",
                        "name": "second_tool",
                        "input": {"param": "value2"}
                    },
                    {
                        "type": "tool_use",
                        "id": "tool_3",
                        "name": "third_tool",
                        "input": {"param": "value3"}
                    }
                ]
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "tool_1",
                        "content": [{"type": "text", "text": "First result"}]
                    },
                    {
                        "type": "tool_result",
                        "tool_use_id": "tool_2",
                        "content": [{"type": "text", "text": "Second result"}]
                    },
                    {
                        "type": "tool_result",
                        "tool_use_id": "tool_3",
                        "content": [{"type": "text", "text": "Third result"}]
                    }
                ]
            }
        ],
        "max_tokens": 100,
        "stream": false
    });

    // Should not fail with duplicate IDs even with multiple tools
    let response = server.anthropic_completions(request).send().await;

    // Verify we got a valid response
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
          "text": "Test response to: test multiple tools"
        }
      ],
      "model": "[model]",
      "stop_reason": null,
      "stop_sequence": null,
      "usage": "[usage]"
    }
    "###);
}
