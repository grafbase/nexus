use integration_tests::{TestServer, llms::AnthropicMock};
use serde_json::json;
use indoc::indoc;

/// Test that we can handle Claude Code's message format where content can be either
/// a string or an array of content blocks
#[tokio::test]
async fn claude_code_mixed_content_formats() {
    let mut builder = TestServer::builder();

    // Set up mock Anthropic server with the specific model we're testing
    builder
        .spawn_llm(
            AnthropicMock::new("anthropic")
                .with_models(vec!["claude-3-5-haiku-latest".to_string()])
        )
        .await;

    // Configure with Anthropic protocol enabled
    let config = indoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.anthropic]
        enabled = true
        path = "/llm/anthropic"

        [llm.protocols.openai]
        enabled = true
        path = "/llm/openai"
    "#};

    let server = builder.build(config).await;

    // Simulate Claude Code's request format with mixed content types
    let request = json!({
        "model": "anthropic/claude-3-5-haiku-latest",
        "messages": [
            {
                "role": "user",
                "content": "hey, what's up"  // String format
            },
            {
                "role": "assistant",
                "content": [  // Array format
                    {
                        "type": "text",
                        "text": "Hi there! I'm Claude Code, ready to help you with software engineering tasks."
                    }
                ]
            },
            {
                "role": "user",
                "content": "awesome, so what tools do I have available?"  // String format again
            }
        ],
        "max_tokens": 1024,
        "stream": false
    });

    // Send request using the Anthropic completions helper - this should parse without errors
    let response = server.anthropic_completions(request).send().await;

    // If we got here without panic, the deserialization worked correctly
    // The mock will return a response
    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".model" => "[model]",
        ".usage" => "[usage]"
    });
}

/// Test that we handle conversations with alternating message content formats
#[tokio::test]
async fn claude_code_conversation_flow() {
    let mut builder = TestServer::builder();

    // Set up mock Anthropic server with the specific model we're testing
    builder
        .spawn_llm(
            AnthropicMock::new("anthropic")
                .with_models(vec!["claude-3-5-haiku-latest".to_string()])
        )
        .await;

    // Configure with Anthropic protocol enabled
    let config = indoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.anthropic]
        enabled = true
        path = "/llm/anthropic"

        [llm.protocols.openai]
        enabled = true
        path = "/llm/openai"
    "#};

    let server = builder.build(config).await;

    let request = json!({
        "model": "anthropic/claude-3-5-haiku-latest",
        "messages": [
            {
                "role": "user",
                "content": "what's the story, morning glory?"  // String
            },
            {
                "role": "assistant",
                "content": [  // Array
                    {"type": "text", "text": "Just a reference to the Oasis song!"}
                ]
            },
            {
                "role": "user",
                "content": [  // Array
                    {"type": "text", "text": "Nice! Can you help me with coding?"}
                ]
            },
            {
                "role": "assistant",
                "content": "Absolutely! What would you like help with?"  // String
            }
        ],
        "max_tokens": 1024,
        "stream": false
    });

    // This should handle both string and array content formats without error
    let response = server.anthropic_completions(request).send().await;

    // Verify we got a valid response
    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".model" => "[model]",
        ".usage" => "[usage]"
    });
}