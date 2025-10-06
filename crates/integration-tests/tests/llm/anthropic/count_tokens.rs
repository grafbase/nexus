use indoc::indoc;
use integration_tests::{
    TestServer,
    llms::{AnthropicMock, OpenAIMock},
};
use serde_json::json;

#[tokio::test]
async fn count_tokens_returns_success() {
    let mut builder = TestServer::builder();
    let mock = AnthropicMock::new("anthropic");
    let header_recorder = mock.header_recorder();
    builder.spawn_llm(mock).await;

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
        "model": "anthropic/claude-3-sonnet-20240229",
        "messages": [
            {
                "role": "user",
                "content": "Hello"
            }
        ],
        "max_tokens": 256
    });

    let body = server.count_tokens(request.clone()).send().await;

    insta::assert_json_snapshot!(body, @r#"
    {
      "cache_creation_input_tokens": 0,
      "cache_read_input_tokens": 0,
      "input_tokens": 8,
      "type": "message_count_tokens_result"
    }
    "#);

    let headers = header_recorder.all_headers();
    assert!(
        headers
            .iter()
            .any(|(name, value)| name == "x-api-key" && value == "test-key")
    );
    assert!(
        headers
            .iter()
            .any(|(name, value)| name == "anthropic-version" && value == "2023-06-01")
    );

    // Ensure we can still access raw response helpers for error cases
    let (status, body) = server.count_tokens(request).send_raw().await;

    assert_eq!(status, 200);
    insta::assert_json_snapshot!(body, @r#"
    {
      "cache_creation_input_tokens": 0,
      "cache_read_input_tokens": 0,
      "input_tokens": 8,
      "type": "message_count_tokens_result"
    }
    "#);
}

#[tokio::test]
async fn count_tokens_rejects_non_anthropic_provider() {
    let mut builder = TestServer::builder();
    builder.spawn_llm(AnthropicMock::new("anthropic")).await;
    builder.spawn_llm(OpenAIMock::new("openai")).await;

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
        "model": "openai/gpt-4",
        "messages": [
            {
                "role": "user",
                "content": "Hello"
            }
        ],
        "max_tokens": 256
    });

    let (status, body) = server.count_tokens(request).send_raw().await;

    assert_eq!(status, 500);
    insta::assert_json_snapshot!(body, @r#"
    {
      "error": {
        "message": "Provider 'openai' does not implement token counting",
        "type": "internal_error"
      },
      "type": "error"
    }
    "#);
}
