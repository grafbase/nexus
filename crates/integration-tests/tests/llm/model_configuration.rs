//! Integration tests for LLM model configuration and rename functionality

use indoc::indoc;
use integration_tests::{
    TestServer,
    llms::{ModelConfig, OpenAIMock},
};
use serde_json::json;

#[tokio::test]
async fn model_rename_works() {
    let mock = OpenAIMock::new("openai")
        .with_models(vec!["gpt-3.5-turbo".to_string(), "gpt-4".to_string()])
        .with_model_configs(vec![
            ModelConfig::new("fast-model").with_rename("gpt-3.5-turbo"),
            ModelConfig::new("smart-model").with_rename("gpt-4"),
        ]);

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let server = builder.build("").await;

    // List models should show user-facing names
    let models = server.openai_list_models().await;
    insta::assert_json_snapshot!(models, {
        ".data[].created" => "[created]"
    }, @r#"
    {
      "object": "list",
      "data": [
        {
          "id": "openai/fast-model",
          "object": "model",
          "created": "[created]",
          "owned_by": "openai"
        },
        {
          "id": "openai/smart-model",
          "object": "model",
          "created": "[created]",
          "owned_by": "openai"
        }
      ]
    }
    "#);

    // Chat completion with renamed model should work
    let request = json!({
        "model": "openai/fast-model",
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let response = server.openai_completions(request).send().await;
    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[created]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[created]",
      "model": "openai/fast-model",
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

    // The mock should have received the actual model name
    // This is verified by the mock's implementation
}

#[tokio::test]
async fn unconfigured_model_returns_404() {
    let mock = OpenAIMock::new("openai")
        .with_models(vec!["gpt-4".to_string(), "gpt-3.5-turbo".to_string()])
        .with_model_configs(vec![
            ModelConfig::new("gpt-4"), // Only gpt-4 is configured
        ]);

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let server = builder.build("").await;

    // List models should only show configured model
    let models = server.openai_list_models().await;
    insta::assert_json_snapshot!(models, {
        ".data[].created" => "[created]"
    }, @r#"
    {
      "object": "list",
      "data": [
        {
          "id": "openai/gpt-4",
          "object": "model",
          "created": "[created]",
          "owned_by": "openai"
        }
      ]
    }
    "#);

    // Configured model should work
    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Hello"}]
    });
    let response = server.openai_completions(request).send().await;
    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[created]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[created]",
      "model": "openai/gpt-4",
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

    // Unconfigured model should return 404
    let request = json!({
        "model": "openai/gpt-3.5-turbo",
        "messages": [{"role": "user", "content": "Hello"}]
    });
    let (status, body) = server.openai_completions(request).send_raw().await;

    assert_eq!(status, 404);
    insta::assert_json_snapshot!(body, @r#"
    {
      "error": {
        "message": "Model 'Model 'gpt-3.5-turbo' is not configured' not found",
        "type": "not_found_error",
        "code": 404
      }
    }
    "#);
}

#[tokio::test]
async fn multiple_providers_with_different_models() {
    let mock = OpenAIMock::new("openai")
        .with_models(vec!["gpt-4".to_string(), "gpt-3.5-turbo".to_string()])
        .with_model_configs(vec![ModelConfig::new("gpt-4"), ModelConfig::new("gpt-3-5-turbo")]);

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    use integration_tests::llms::AnthropicMock;
    builder
        .spawn_llm(AnthropicMock::new("anthropic").with_models(vec!["claude-3-opus-20240229".to_string()]))
        .await;

    use integration_tests::llms::GoogleMock;
    builder
        .spawn_llm(GoogleMock::new("google").with_models(vec!["gemini-pro".to_string()]))
        .await;

    let server = builder.build("").await;

    // List models should show all configured models
    let models = server.openai_list_models().await;

    // Snapshot the model list
    insta::assert_json_snapshot!(models, {
        ".data[].created" => "[created]"
    }, @r#"
    {
      "object": "list",
      "data": [
        {
          "id": "anthropic/claude-3-opus-20240229",
          "object": "model",
          "created": "[created]",
          "owned_by": "anthropic"
        },
        {
          "id": "google/gemini-pro",
          "object": "model",
          "created": "[created]",
          "owned_by": "google"
        },
        {
          "id": "openai/gpt-3-5-turbo",
          "object": "model",
          "created": "[created]",
          "owned_by": "openai"
        },
        {
          "id": "openai/gpt-4",
          "object": "model",
          "created": "[created]",
          "owned_by": "openai"
        }
      ]
    }
    "#);

    // Each provider should only accept its configured models
    let openai_request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Hello"}]
    });
    let openai_response = server.openai_completions(openai_request).send().await;
    insta::assert_json_snapshot!(openai_response, {
        ".id" => "[id]",
        ".created" => "[created]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[created]",
      "model": "openai/gpt-4",
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

    let anthropic_request = json!({
        "model": "anthropic/claude-3-opus-20240229",
        "messages": [{"role": "user", "content": "Hello"}]
    });
    let anthropic_response = server.openai_completions(anthropic_request).send().await;
    insta::assert_json_snapshot!(anthropic_response, {
        ".id" => "[id]",
        ".created" => "[created]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[created]",
      "model": "anthropic/claude-3-opus-20240229",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "Test response to: Hello"
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

    let google_request = json!({
        "model": "google/gemini-pro",
        "messages": [{"role": "user", "content": "Hello"}]
    });
    let google_response = server.openai_completions(google_request).send().await;
    insta::assert_json_snapshot!(google_response, {
        ".id" => "[id]",
        ".created" => "[created]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[created]",
      "model": "google/gemini-pro",
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

#[tokio::test]
async fn provider_with_no_models_returns_error() {
    // This test verifies that configuration validation enforces Phase 3 rules

    use config::Config;

    let config_str = indoc! {r#"
        [llm.providers.openai]
        type = "openai"
        api_key = "test-key"
        # No models configured - should fail to parse
    "#};

    // Parsing should fail because neither models nor model_pattern is configured
    let result: Result<Config, _> = toml::from_str(config_str);

    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    insta::assert_snapshot!(error_msg, @r"
    TOML parse error at line 1, column 1
      |
    1 | [llm.providers.openai]
      | ^^^^^^^^^^^^^^^^^^^^^^
    model_pattern or at least one model must be configured
    ");
}

#[tokio::test]
async fn renamed_model_in_streaming() {
    let mut builder = TestServer::builder();
    builder
        .spawn_llm(
            OpenAIMock::new("openai")
                .with_streaming()
                .with_models(vec!["gpt-3.5-turbo".to_string()])
                .with_model_configs(vec![ModelConfig::new("fast").with_rename("gpt-3.5-turbo")]),
        )
        .await;

    let server = builder.build("").await;

    let request = json!({
        "model": "openai/fast",
        "messages": [{"role": "user", "content": "Hello"}],
        "stream": true
    });

    let chunks = server.openai_completions_stream(request).send().await;

    // Snapshot first chunk to verify model name
    insta::assert_json_snapshot!(chunks.first().unwrap(), {
        ".id" => "[id]",
        ".created" => "[created]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion.chunk",
      "created": "[created]",
      "model": "openai/fast",
      "choices": [
        {
          "index": 0,
          "delta": {
            "role": "assistant",
            "content": "Why don't scientists trust atoms? "
          }
        }
      ]
    }
    "#);
}
