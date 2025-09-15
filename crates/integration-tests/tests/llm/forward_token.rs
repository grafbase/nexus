use indoc::indoc;
use integration_tests::{
    TestServer,
    llms::{AnthropicMock, GoogleMock, OpenAIMock, TestLlmProvider},
};
use serde_json::json;

const PROVIDER_API_KEY_HEADER: &str = "X-Provider-API-Key";

#[tokio::test]
async fn forward_token_disabled_uses_configured_key() {
    let mut builder = TestServer::builder();

    let mock = OpenAIMock::new("test_openai").with_response("Hello", "Hi from configured key!");
    builder.spawn_llm(mock).await;

    // spawn_llm generates the config with forward_token=false by default
    let config = "";

    let server = builder.build(config).await;

    // Make request WITHOUT token forwarding header
    let request = json!({
        "model": "test_openai/gpt-4",
        "messages": [
            {
                "role": "user",
                "content": "Hello"
            }
        ]
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
      "model": "test_openai/gpt-4",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "Hi from configured key!"
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
async fn forward_token_enabled_uses_provided_key() {
    // Start the mock server manually and configure with token forwarding
    let mock_server =
        Box::new(OpenAIMock::new("test_openai_forward_token").with_response("Hello", "Hi from token forwarding!"));
    let config_data = mock_server.spawn().await.unwrap();

    let config = indoc::formatdoc! {r#"
        [llm.providers.test_openai_forward_token]
        type = "openai"
        api_key = "fallback-key"
        forward_token = true
        base_url = "http://{}/v1"
        
        [llm.providers.test_openai_forward_token.models.gpt-4]
        
        [llm.providers.test_openai_forward_token.models.gpt-3-5-turbo]
    "#, config_data.address};

    let server = TestServer::builder().build(&config).await;

    // Make request WITH token forwarding header
    let request = json!({
        "model": "test_openai_forward_token/gpt-4",
        "messages": [{"role": "user", "content": "Hello"}],
    });

    let body = server
        .openai_completions(request)
        .header(PROVIDER_API_KEY_HEADER, "user-provided-key")
        .send()
        .await;

    insta::assert_json_snapshot!(body, {
        ".id" => "[id]",
        ".created" => "[created]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[created]",
      "model": "test_openai_forward_token/gpt-4",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "Hi from token forwarding!"
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
async fn forward_token_enabled_falls_back_to_configured_key() {
    // Start the mock server manually and configure with token forwarding
    let mock_server = Box::new(OpenAIMock::new("test_openai_fallback").with_response("Hello", "Hi from fallback!"));
    let config_data = mock_server.spawn().await.unwrap();

    let config = indoc::formatdoc! {r#"
        [llm.providers.test_openai_fallback]
        type = "openai"
        api_key = "fallback-key"
        forward_token = true
        base_url = "http://{}/v1"
        
        [llm.providers.test_openai_fallback.models.gpt-4]
        
        [llm.providers.test_openai_fallback.models.gpt-3-5-turbo]
    "#, config_data.address};

    let server = TestServer::builder().build(&config).await;

    // Make request WITHOUT token forwarding header (should use fallback)
    let request = json!({
        "model": "test_openai_fallback/gpt-4",
        "messages": [
            {
                "role": "user",
                "content": "Hello"
            }
        ]
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
      "model": "test_openai_fallback/gpt-4",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "Hi from fallback!"
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
async fn forward_token_enabled_no_keys_returns_error() {
    // For this test, we don't need a mock server since it should fail before reaching it
    let config = indoc! {r#"
        [llm.providers.test_openai_no_keys]
        type = "openai"
        # No api_key configured
        forward_token = true
        base_url = "http://127.0.0.1:1234/v1"  # Won't actually be used
        
        [llm.providers.test_openai_no_keys.models.gpt-4]
    "#};

    let server = TestServer::builder().build(config).await;

    // Make request WITHOUT token forwarding header and no configured key
    let request = json!({
        "model": "test_openai_no_keys/gpt-4",
        "messages": [{"role": "user", "content": "Hello"}],
    });

    let (status, body) = server.openai_completions(request).send_raw().await;

    assert_eq!(status, 401);

    insta::assert_json_snapshot!(body, @r#"
    {
      "error": {
        "message": "Authentication failed: Token forwarding is enabled but no API key was provided",
        "type": "authentication_error",
        "code": 401
      }
    }
    "#);
}

#[tokio::test]
async fn forward_token_works_with_anthropic() {
    // Start the mock server manually and configure with token forwarding
    let mock_server = Box::new(
        AnthropicMock::new("test_anthropic_forward_token")
            .with_response("Hello", "Hi from Anthropic token forwarding!"),
    );
    let config_data = mock_server.spawn().await.unwrap();

    let config = indoc::formatdoc! {r#"
        [llm.providers.test_anthropic_forward_token]
        type = "anthropic"
        forward_token = true
        api_key = "fallback-anthropic-key"
        base_url = "http://{}/v1"
        
        [llm.providers.test_anthropic_forward_token.models.claude-3-opus-20240229]
        
        [llm.providers.test_anthropic_forward_token.models.claude-3-5-sonnet-20241022]
    "#, config_data.address};

    let server = TestServer::builder().build(&config).await;

    let request = json!({
        "model": "test_anthropic_forward_token/claude-3-opus-20240229",
        "messages": [{"role": "user", "content": "Hello"}],
    });

    let body = server
        .openai_completions(request)
        .header(PROVIDER_API_KEY_HEADER, "anthropic-user-key")
        .send()
        .await;

    insta::assert_json_snapshot!(body, {
        ".id" => "[id]",
        ".created" => "[created]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[created]",
      "model": "test_anthropic_forward_token/claude-3-opus-20240229",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "Hi from Anthropic token forwarding!"
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
async fn forward_token_works_with_google() {
    // Start the mock server manually and configure with token forwarding
    let mock_server = Box::new(
        GoogleMock::new("test_google_forward_token").with_response("Hello", "Hi from Google token forwarding!"),
    );
    let config_data = mock_server.spawn().await.unwrap();

    let config = indoc::formatdoc! {r#"
        [llm.providers.test_google_forward_token]
        type = "google"
        forward_token = true
        api_key = "fallback-google-key"
        base_url = "http://{}/v1beta"
        
        [llm.providers.test_google_forward_token.models."gemini-1.5-flash"]
        
        [llm.providers.test_google_forward_token.models."gemini-pro"]
    "#, config_data.address};

    let server = TestServer::builder().build(&config).await;

    let request = json!({
        "model": "test_google_forward_token/gemini-pro",
        "messages": [{"role": "user", "content": "Hello"}],
    });

    let body = server
        .openai_completions(request)
        .header(PROVIDER_API_KEY_HEADER, "google-user-key")
        .send()
        .await;

    insta::assert_json_snapshot!(body, {
        ".id" => "[id]",
        ".created" => "[created]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[created]",
      "model": "test_google_forward_token/gemini-pro",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "Hi from Google token forwarding!"
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
async fn forward_token_disabled_ignores_header() {
    let mut builder = TestServer::builder();

    let mock = OpenAIMock::new("test_openai").with_response("Hello", "Using configured key");
    builder.spawn_llm(mock).await;

    // spawn_llm generates the config with forward_token=false by default
    let config = "";

    let server = builder.build(config).await;

    // Send token forwarding header even though token forwarding is disabled
    let request = json!({
        "model": "test_openai/gpt-4",
        "messages": [{"role": "user", "content": "Hello"}],
    });

    let body = server
        .openai_completions(request)
        .header(PROVIDER_API_KEY_HEADER, "should-be-ignored")
        .send()
        .await;

    insta::assert_json_snapshot!(body, {
        ".id" => "[id]",
        ".created" => "[created]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[created]",
      "model": "test_openai/gpt-4",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "Using configured key"
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
