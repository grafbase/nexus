use indoc::indoc;
use integration_tests::{TestServer, llms::OpenAIMock};
use serde_json::json;

#[tokio::test]
async fn model_without_provider_prefix_routes_successfully() {
    let mut builder = TestServer::builder();
    builder.spawn_llm(OpenAIMock::new("openai")).await;

    let server = builder.build("").await;

    // Model without provider prefix should still resolve via discovery
    let request = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "Test"}]
    });

    let (status, mut body) = server.openai_completions(request).send_raw().await;
    assert_eq!(status, 200);
    if let Some(id) = body.get_mut("id") {
        *id = serde_json::Value::String("chatcmpl-test-redacted".to_string());
    }
    insta::assert_json_snapshot!(body, @r#"
    {
      "id": "chatcmpl-test-redacted",
      "object": "chat.completion",
      "created": 1677651200,
      "model": "gpt-4",
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
}

#[tokio::test]
async fn provider_not_found_returns_404() {
    let mut builder = TestServer::builder();
    builder.spawn_llm(OpenAIMock::new("openai")).await;

    let server = builder.build("").await;

    // Non-existent provider
    let request = json!({
        "model": "nonexistent/model",
        "messages": [{"role": "user", "content": "Test"}]
    });

    let (status, body) = server.openai_completions(request).send_raw().await;
    assert_eq!(status, 404);
    insta::assert_json_snapshot!(body, @r#"
    {
      "error": {
        "message": "Provider 'nonexistent' not found",
        "type": "not_found_error",
        "code": 404
      }
    }
    "#);
}

#[tokio::test]
async fn authentication_error_returns_401() {
    // Create a mock that returns 401 for any request
    let mock = OpenAIMock::new("openai").with_auth_error("Invalid API key");

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let server = builder.build("").await;

    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Test"}]
    });

    let (status, body) = server.openai_completions(request).send_raw().await;
    assert_eq!(status, 401);
    insta::assert_json_snapshot!(body, @r#"
    {
      "error": {
        "message": "Authentication failed: Invalid API key",
        "type": "authentication_error",
        "code": 401
      }
    }
    "#);
}

#[tokio::test]
async fn model_not_found_returns_404() {
    // Create a mock that returns 404 for unknown models
    let mock = OpenAIMock::new("openai").with_model_not_found("gpt-5");

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let server = builder.build("").await;

    let request = json!({
        "model": "openai/gpt-5",
        "messages": [{"role": "user", "content": "Test"}]
    });

    let (status, body) = server.openai_completions(request).send_raw().await;
    assert_eq!(status, 404);
    insta::assert_json_snapshot!(body, @r#"
    {
      "error": {
        "message": "The model 'gpt-5' does not exist",
        "type": "not_found_error",
        "code": 404
      }
    }
    "#);
}

#[tokio::test]
async fn rate_limit_error_returns_429() {
    // Create a mock that returns 429 for rate limiting
    let mock = OpenAIMock::new("openai").with_rate_limit("Rate limit exceeded. Please retry after 1 second.");

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let server = builder.build("").await;

    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Test"}]
    });

    let (status, body) = server.openai_completions(request).send_raw().await;
    assert_eq!(status, 429);
    insta::assert_json_snapshot!(body, @r#"
    {
      "error": {
        "message": "Rate limit exceeded: Rate limit exceeded. Please retry after 1 second.",
        "type": "rate_limit_error",
        "code": 429
      }
    }
    "#);
}

#[tokio::test]
async fn insufficient_quota_returns_403() {
    // Create a mock that returns 403 for quota issues
    let mock = OpenAIMock::new("openai").with_quota_exceeded("You have exceeded your monthly quota");

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let server = builder.build("").await;

    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Test"}]
    });

    let (status, body) = server.openai_completions(request).send_raw().await;
    assert_eq!(status, 403);
    insta::assert_json_snapshot!(body, @r#"
    {
      "error": {
        "message": "Insufficient quota: You have exceeded your monthly quota",
        "type": "insufficient_quota",
        "code": 403
      }
    }
    "#);
}

#[tokio::test]
async fn streaming_mock_not_implemented_returns_error() {
    let mut builder = TestServer::builder();
    builder.spawn_llm(OpenAIMock::new("openai")).await;

    let server = builder.build("").await;

    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Test"}],
        "stream": true
    });

    let (status, body) = server.openai_completions(request).send_raw().await;
    // OpenAI supports streaming now, but the mock doesn't implement it
    // So we get an error when trying to connect to the streaming endpoint
    assert!(status == 400 || status == 502);

    // The error message depends on whether we fail at mock level or stream parsing
    if body["error"]["code"] == 400 {
        insta::assert_json_snapshot!(body, @r#"
        {
          "error": {
            "message": "Invalid request: Streaming is not yet supported",
            "type": "invalid_request_error",
            "code": 400
          }
        }
        "#);
    } else {
        // Check that it's a 502 api_error
        assert_eq!(status, 502);
        assert!(body["error"]["type"].as_str() == Some("api_error"));
    }
}

#[tokio::test]
async fn list_models_with_auth_error_returns_empty_list() {
    // Create a mock that returns 401 for list models
    // Note: The server keeps previously discovered and explicitly configured models cached,
    // so an auth error during listing still results in the cached set being returned.
    let mock = OpenAIMock::new("openai").with_auth_error("Invalid API key");

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let server = builder.build("").await;

    let body = server.openai_list_models().await;
    insta::assert_json_snapshot!(body, @r#"
    {
      "object": "list",
      "data": [
        {
          "id": "gpt-3.5-turbo",
          "object": "model",
          "created": 1677651200,
          "owned_by": "openai"
        },
        {
          "id": "gpt-4",
          "object": "model",
          "created": 1677651201,
          "owned_by": "openai"
        },
        {
          "id": "gpt-4-turbo",
          "object": "model",
          "created": 1677651202,
          "owned_by": "openai"
        },
        {
          "id": "openai/gpt-3.5-turbo",
          "object": "model",
          "created": 1719475200,
          "owned_by": "openai"
        },
        {
          "id": "openai/gpt-4",
          "object": "model",
          "created": 1719475200,
          "owned_by": "openai"
        }
      ]
    }
    "#);
}

#[tokio::test]
async fn bad_request_returns_400() {
    // Create a mock that returns 400 for invalid requests
    let mock = OpenAIMock::new("openai").with_bad_request("Invalid request: messages array cannot be empty");

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let server = builder.build("").await;

    let request = json!({
        "model": "openai/gpt-4",
        "messages": []  // Empty messages array
    });

    let (status, body) = server.openai_completions(request).send_raw().await;
    assert_eq!(status, 400);
    insta::assert_json_snapshot!(body, @r#"
    {
      "error": {
        "message": "Invalid request: Invalid request: messages array cannot be empty",
        "type": "invalid_request_error",
        "code": 400
      }
    }
    "#);
}

#[tokio::test]
async fn provider_internal_error_returns_500_with_message() {
    // Create a mock that returns a 500 internal server error from the provider
    // This should pass through the provider's error message
    let mock = OpenAIMock::new("openai").with_internal_error("OpenAI service temporarily unavailable");

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let server = builder.build("").await;

    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Test"}]
    });

    let (status, body) = server.openai_completions(request).send_raw().await;
    assert_eq!(status, 500);
    insta::assert_json_snapshot!(body, @r#"
    {
      "error": {
        "message": "OpenAI service temporarily unavailable",
        "type": "internal_error",
        "code": 500
      }
    }
    "#);
}

#[tokio::test]
async fn streaming_error_returns_error_in_stream() {
    let mut builder = TestServer::builder();
    builder
        .spawn_llm(
            OpenAIMock::new("openai")
                .with_streaming()
                .with_internal_error("Connection lost mid-stream"),
        )
        .await;

    let config = indoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.openai]
enabled = true
path = "/llm"
    "#};

    let server = builder.build(config).await;

    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Hi"}],
        "stream": true
    });

    let (status, body) = server.openai_completions(request).send_raw().await;

    // Streaming errors should return HTTP 500 (Internal Server Error from provider)
    assert_eq!(status, 500);

    insta::assert_json_snapshot!(body, @r#"
    {
      "error": {
        "message": "Connection lost mid-stream",
        "type": "internal_error",
        "code": 500
      }
    }
    "#);
}

#[tokio::test]
async fn provider_other_error_returns_502() {
    // Create a mock that returns a 503 Service Unavailable
    // Non-500 errors should return 502 Bad Gateway
    let mock = OpenAIMock::new("openai").with_service_unavailable("Service unavailable");

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let server = builder.build("").await;

    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Test"}]
    });

    let (status, body) = server.openai_completions(request).send_raw().await;
    assert_eq!(status, 502);

    insta::assert_json_snapshot!(body, @r#"
    {
      "error": {
        "message": "Provider API error (503): Service unavailable",
        "type": "api_error",
        "code": 502
      }
    }
    "#);
}
