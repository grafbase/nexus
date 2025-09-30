//! Tests for model discovery with regex-based routing across all providers

use insta::assert_json_snapshot;
use integration_tests::TestServer;
use serde_json::json;

#[tokio::test]
async fn model_filter_routing_works_for_all_provider_types() {
    use integration_tests::llms::{AnthropicMock, GoogleMock, OpenAIMock};

    // Set up mocks for all three provider types with regex filtering
    let openai_mock = OpenAIMock::new("openai")
        .with_models(vec!["gpt-4o".to_string(), "gpt-3.5-turbo".to_string()])
        .with_model_filter("^gpt-.*")
        .with_response("test response", "OpenAI response");

    let anthropic_mock = AnthropicMock::new("anthropic")
        .with_models(vec!["claude-3-opus".to_string(), "claude-3-sonnet".to_string()])
        .with_model_filter("^claude-.*")
        .with_response("test response", "Anthropic response");

    let google_mock = GoogleMock::new("google")
        .with_models(vec!["gemini-pro".to_string(), "gemini-ultra".to_string()])
        .with_model_filter("^gemini-.*")
        .with_response("test response", "Google response");

    let mut builder = TestServer::builder();
    builder.spawn_llm(openai_mock).await;
    builder.spawn_llm(anthropic_mock).await;
    builder.spawn_llm(google_mock).await;
    let server = builder.build("").await;

    // Test OpenAI regex routing - should get successful response
    let request = json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "test"}]
    });
    let response = server.openai_completions(request).send().await;
    assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[created]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[created]",
      "model": "gpt-4o",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "This is a test response from the mock LLM server"
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

    // Test Anthropic regex routing
    let request = json!({
        "model": "claude-3-opus",
        "messages": [{"role": "user", "content": "test"}]
    });
    let response = server.openai_completions(request).send().await;
    assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[created]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[created]",
      "model": "claude-3-opus",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "Test response to: test"
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

    // Test Google regex routing
    let request = json!({
        "model": "gemini-pro",
        "messages": [{"role": "user", "content": "test"}]
    });
    let response = server.openai_completions(request).send().await;
    assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[created]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[created]",
      "model": "gemini-pro",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "Test response to: test"
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
async fn model_filter_routing_respects_provider_order() {
    use integration_tests::llms::OpenAIMock;

    let specific_mock = OpenAIMock::new("aaa_specific")
        .with_models(vec!["gpt-4o-mini".to_string()])
        .with_model_filter("^gpt-4o-mini$")
        .with_response("filter-hit", "specific provider response");

    let broad_mock = OpenAIMock::new("zzz_broad")
        .with_models(vec!["gpt-4o-mini".to_string(), "gpt-4o".to_string()])
        .with_model_filter("^gpt-.*")
        .with_response("filter-hit", "broad provider response");

    let mut builder = TestServer::builder();
    builder.spawn_llm(specific_mock).await;
    builder.spawn_llm(broad_mock).await;
    let server = builder.build("").await;

    let request = json!({
        "model": "gpt-4o-mini",
        "messages": [{"role": "user", "content": "filter-hit"}]
    });

    let response = server.openai_completions(request).send().await;

    assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[created]"
    }, @r###"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[created]",
      "model": "gpt-4o-mini",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "specific provider response"
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

    let request = json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "filter-hit"}]
    });

    let response = server.openai_completions(request).send().await;

    assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[created]"
    }, @r###"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[created]",
      "model": "gpt-4o",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "broad provider response"
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
async fn model_filter_routing_is_case_insensitive() {
    use integration_tests::llms::OpenAIMock;

    let mock = OpenAIMock::new("openai")
        .with_models(vec!["GPT-4O-MINI".to_string()])
        .with_model_filter("^gpt-4o.*")
        .with_response("Hello", "Case insensitive response");

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;
    let server = builder.build("").await;

    let request = json!({
        "model": "GPT-4O-MINI",
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let response = server.openai_completions(request).send().await;

    assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[created]"
    }, @r###"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[created]",
      "model": "GPT-4O-MINI",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "Case insensitive response"
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
async fn model_filter_routing_supports_streaming() {
    use integration_tests::llms::OpenAIMock;

    let mock = OpenAIMock::new("openai")
        .with_models(vec!["gpt-4o-mini".to_string()])
        .with_model_filter("^gpt-4o.*")
        .with_streaming()
        .with_streaming_chunks(vec!["Hello", " ", "model filter routing!"]);

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;
    let server = builder.build("").await;

    let request = json!({
        "model": "gpt-4o-mini",
        "messages": [{"role": "user", "content": "Hello"}],
        "stream": true
    });

    let chunks = server.openai_completions_stream(request).send().await;

    let assembled: String = chunks
        .iter()
        .filter_map(|chunk| {
            chunk
                .get("choices")
                .and_then(|choices| choices.get(0))
                .and_then(|choice| choice.get("delta"))
                .and_then(|delta| delta.get("content"))
                .and_then(|value| value.as_str())
        })
        .collect();

    insta::assert_snapshot!(assembled, @"Hello model filter routing!");
}

#[tokio::test]
async fn mixed_filter_and_explicit_models() {
    use integration_tests::llms::OpenAIMock;

    // Set up a mock with both filter-matched and explicit models
    let hybrid_mock = OpenAIMock::new("hybrid")
        .with_models(vec![
            "gpt-4o-mini".to_string(),
            "gpt-3.5-turbo".to_string(),
            "custom-model".to_string(),
            "dall-e-3".to_string(),
        ])
        .with_model_filter("^gpt-4.*")
        .with_model_configs(vec![
            integration_tests::llms::ModelConfig::new("gpt-3.5-turbo"),
            integration_tests::llms::ModelConfig::new("custom-model"),
        ])
        .with_response("test", "Mock response");

    let mut builder = TestServer::builder();
    builder.spawn_llm(hybrid_mock).await;
    let server = builder.build("").await;

    // Filter-matched model (gpt-4o-mini matches ^gpt-4.*)
    let request = json!({
        "model": "gpt-4o-mini",
        "messages": [{"role": "user", "content": "test"}]
    });
    let response = server.openai_completions(request).send().await;
    assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[created]"
    }, @r###"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[created]",
      "model": "gpt-4o-mini",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "Mock response"
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

    // Explicit model that doesn't match the filter
    let request = json!({
        "model": "hybrid/gpt-3.5-turbo",
        "messages": [{"role": "user", "content": "test"}]
    });
    let response = server.openai_completions(request).send().await;
    assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[created]"
    }, @r###"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[created]",
      "model": "hybrid/gpt-3.5-turbo",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "Mock response"
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

    // Custom explicit model
    let request = json!({
        "model": "hybrid/custom-model",
        "messages": [{"role": "user", "content": "test"}]
    });
    let response = server.openai_completions(request).send().await;
    assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[created]"
    }, @r###"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[created]",
      "model": "hybrid/custom-model",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "Mock response"
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

    // Model that doesn't match the filter or explicit config
    let request = json!({
        "model": "dall-e-3",
        "messages": [{"role": "user", "content": "test"}]
    });
    let (_status, body) = server.openai_completions(request).send_raw().await;
    assert_json_snapshot!(body, @r###"
    {
      "error": {
        "message": "Model 'dall-e-3' not found",
        "type": "not_found_error",
        "code": 404
      }
    }
    "###);
}
