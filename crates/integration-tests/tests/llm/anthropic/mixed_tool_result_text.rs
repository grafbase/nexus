use indoc::indoc;
use integration_tests::{TestServer, llms::AnthropicMock};
use serde_json::json;

/// Test that mixed tool_result and text blocks don't create duplicate tool IDs
/// This reproduces the Claude Code issue where text blocks after tool_results
/// were incorrectly wrapped with the same tool_use_id
#[tokio::test]
async fn mixed_tool_result_and_text_no_duplicates() {
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

    // Simulate Claude Code's pattern: tool_result followed by additional text
    let request = json!({
        "model": "anthropic/claude-3-5-haiku-latest",
        "messages": [
            {
                "role": "user",
                "content": "Run some tools"
            },
            {
                "role": "assistant",
                "content": [
                    {
                        "type": "tool_use",
                        "id": "toolu_01DdefXGqa9DDwPkPqBVm1V3",
                        "name": "get_data",
                        "input": {"query": "test"}
                    }
                ]
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_01DdefXGqa9DDwPkPqBVm1V3",
                        "content": [{"type": "text", "text": "Tool execution result"}]
                    },
                    {
                        "type": "text",
                        "text": "Additional context about the tool result"
                    }
                ]
            }
        ],
        "max_tokens": 100,
        "stream": false
    });

    // This should NOT fail with duplicate tool_use_id error
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
          "text": "Test response to: Run some tools"
        }
      ],
      "model": "[model]",
      "stop_reason": null,
      "stop_sequence": null,
      "usage": "[usage]"
    }
    "###);
}

/// Test multiple tool results with different IDs remain separate
#[tokio::test]
async fn multiple_tool_results_different_ids() {
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

    let request = json!({
        "model": "anthropic/claude-3-5-haiku-latest",
        "messages": [
            {
                "role": "user",
                "content": "Run multiple tools"
            },
            {
                "role": "assistant",
                "content": [
                    {
                        "type": "tool_use",
                        "id": "toolu_01ABC",
                        "name": "tool1",
                        "input": {}
                    },
                    {
                        "type": "tool_use",
                        "id": "toolu_01XYZ",
                        "name": "tool2",
                        "input": {}
                    }
                ]
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_01ABC",
                        "content": [{"type": "text", "text": "Result 1"}]
                    },
                    {
                        "type": "text",
                        "text": "Intermediate text"
                    },
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_01XYZ",
                        "content": [{"type": "text", "text": "Result 2"}]
                    }
                ]
            }
        ],
        "max_tokens": 100,
        "stream": false
    });

    let response = server.anthropic_completions(request).send().await;

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
          "text": "Test response to: Run multiple tools"
        }
      ],
      "model": "[model]",
      "stop_reason": null,
      "stop_sequence": null,
      "usage": "[usage]"
    }
    "###);
}
