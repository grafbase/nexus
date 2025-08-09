use integration_tests::{
    TestServer,
    llms::{AnthropicMock, GoogleMock, OpenAIMock},
};
use serde_json::json;

mod anthropic;
mod error_handling;
mod forward_token;
mod google;
mod openai;

#[tokio::test]
async fn multiple_providers_work_together() {
    let openai_provider = OpenAIMock::new("openai");
    let anthropic_provider = AnthropicMock::new("anthropic");
    let google_provider = GoogleMock::new("google");

    let mut builder = TestServer::builder();
    builder.spawn_llm(openai_provider).await;
    builder.spawn_llm(anthropic_provider).await;
    builder.spawn_llm(google_provider).await;

    let server = builder.build("").await;
    let llm = server.llm_client("/llm");

    // Test listing models from both providers
    let models_body = llm.list_models().await;

    // Extract and normalize model IDs for snapshot
    let model_ids: Vec<String> = models_body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_string())
        .collect();

    insta::assert_debug_snapshot!(model_ids, @r#"
    [
        "anthropic/claude-3-5-sonnet-20241022",
        "anthropic/claude-3-5-haiku-20241022",
        "anthropic/claude-3-opus-20240229",
        "anthropic/claude-3-sonnet-20240229",
        "anthropic/claude-3-haiku-20240307",
        "google/gemini-1.5-flash",
        "google/gemini-1.5-pro",
        "google/gemini-pro",
        "openai/gpt-3.5-turbo",
        "openai/gpt-4",
        "openai/gpt-4-turbo",
    ]
    "#);

    // Test OpenAI completion
    let openai_request = json!({
        "model": "openai/gpt-3.5-turbo",
        "messages": [{"role": "user", "content": "Hello from OpenAI"}]
    });

    let openai_body = llm.completions(openai_request).await;

    insta::assert_json_snapshot!(openai_body, {
        ".id" => "chatcmpl-test-[uuid]"
    }, @r#"
    {
      "id": "chatcmpl-test-[uuid]",
      "object": "chat.completion",
      "created": 1677651200,
      "model": "openai/gpt-3.5-turbo",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "Hello! I'm a test LLM assistant. How can I help you today?"
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

    // Test Anthropic completion
    let anthropic_request = json!({
        "model": "anthropic/claude-3-5-sonnet-20241022",
        "messages": [{"role": "user", "content": "Hello from Anthropic"}]
    });

    let anthropic_body = llm.completions(anthropic_request).await;

    insta::assert_json_snapshot!(anthropic_body, {
        ".id" => "msg_[id]",
        ".created" => "[created]"
    }, @r#"
    {
      "id": "msg_[id]",
      "object": "chat.completion",
      "created": "[created]",
      "model": "anthropic/claude-3-5-sonnet-20241022",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "Test response to: Hello from Anthropic"
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

    // Test Google completion
    let google_request = json!({
        "model": "google/gemini-1.5-flash",
        "messages": [{"role": "user", "content": "Hello from Google"}]
    });

    let google_body = llm.completions(google_request).await;

    insta::assert_json_snapshot!(google_body, {
        ".id" => "[id]",
        ".created" => "[created]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[created]",
      "model": "google/gemini-1.5-flash",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "Hello! I'm Gemini, a test assistant. How can I help you today?"
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
