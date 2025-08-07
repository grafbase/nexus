use indoc::indoc;
use integration_tests::{TestServer, llms::OpenAIMock};
use serde_json::json;

#[tokio::test]
async fn invalid_model_format_returns_400() {
    let mut builder = TestServer::builder();
    builder.spawn_llm(OpenAIMock::new("openai")).await;

    let server = builder.build("").await;
    let llm = server.llm_client("/llm");

    // Model without provider prefix
    let request = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "Test"}]
    });

    let response = llm.completions_error(request).await;
    assert_eq!(response.status(), 400);

    let body: serde_json::Value = response.json().await.unwrap();
    insta::assert_json_snapshot!(body, @r#"
    {
      "error": {
        "message": "Invalid model format: expected 'provider/model', got 'gpt-4'",
        "type": "invalid_request_error",
        "code": 400
      }
    }
    "#);
}

#[tokio::test]
async fn provider_not_found_returns_404() {
    let mut builder = TestServer::builder();
    builder.spawn_llm(OpenAIMock::new("openai")).await;

    let server = builder.build("").await;
    let llm = server.llm_client("/llm");

    // Non-existent provider
    let request = json!({
        "model": "nonexistent/model",
        "messages": [{"role": "user", "content": "Test"}]
    });

    let response = llm.completions_error(request).await;
    assert_eq!(response.status(), 404);

    let body: serde_json::Value = response.json().await.unwrap();
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
    let llm = server.llm_client("/llm");

    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Test"}]
    });

    let response = llm.completions_error(request).await;
    assert_eq!(response.status(), 401);

    let body: serde_json::Value = response.json().await.unwrap();
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
    let llm = server.llm_client("/llm");

    let request = json!({
        "model": "openai/gpt-5",
        "messages": [{"role": "user", "content": "Test"}]
    });

    let response = llm.completions_error(request).await;
    assert_eq!(response.status(), 404);

    let body: serde_json::Value = response.json().await.unwrap();
    insta::assert_json_snapshot!(body, @r#"
    {
      "error": {
        "message": "Model 'The model 'gpt-5' does not exist' not found",
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
    let llm = server.llm_client("/llm");

    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Test"}]
    });

    let response = llm.completions_error(request).await;
    assert_eq!(response.status(), 429);

    let body: serde_json::Value = response.json().await.unwrap();
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
    let llm = server.llm_client("/llm");

    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Test"}]
    });

    let response = llm.completions_error(request).await;
    assert_eq!(response.status(), 403);

    let body: serde_json::Value = response.json().await.unwrap();
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
    let llm = server.llm_client("/llm");

    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Test"}],
        "stream": true
    });

    let response = llm.completions_error(request).await;
    // OpenAI supports streaming now, but the mock doesn't implement it
    // So we get an error when trying to connect to the streaming endpoint
    assert!(response.status() == 400 || response.status() == 502);

    let body: serde_json::Value = response.json().await.unwrap();

    // The error message depends on whether we fail at mock level or stream parsing
    if body["error"]["code"] == 400 {
        assert_eq!(body["error"]["type"], "invalid_request_error");
        assert!(
            body["error"]["message"]
                .as_str()
                .unwrap()
                .contains("Streaming is not yet supported")
        );
    } else {
        assert_eq!(body["error"]["code"], 502);
        assert_eq!(body["error"]["type"], "api_error");
    }
}

#[tokio::test]
async fn list_models_with_auth_error_returns_empty_list() {
    // Create a mock that returns 401 for list models
    // Note: The server aggregates models from multiple providers,
    // so if one fails with auth error, it still returns 200 with models from other providers
    // In this case with only one provider that fails, it returns an empty list
    let mock = OpenAIMock::new("openai").with_auth_error("Invalid API key");

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let server = builder.build("").await;

    let response = server.client.get("/llm/v1/models").await;
    assert_eq!(response.status(), 200); // Still returns 200 even if providers fail

    let body: serde_json::Value = response.json().await.unwrap();
    insta::assert_json_snapshot!(body, @r#"
    {
      "object": "list",
      "data": []
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
    let llm = server.llm_client("/llm");

    let request = json!({
        "model": "openai/gpt-4",
        "messages": []  // Empty messages array
    });

    let response = llm.completions_error(request).await;
    assert_eq!(response.status(), 400);

    let body: serde_json::Value = response.json().await.unwrap();
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
    let llm = server.llm_client("/llm");

    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Test"}]
    });

    let response = llm.completions_error(request).await;
    assert_eq!(response.status(), 500);

    let body: serde_json::Value = response.json().await.unwrap();
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
    "#};

    let server = builder.build(config).await;
    let llm = server.llm_client("/llm");

    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Hi"}],
        "stream": true
    });

    let response = llm.completions_error(request).await;

    // Streaming errors should return HTTP 500 (Internal Server Error from provider)
    assert_eq!(response.status(), 500);

    let body: serde_json::Value = response.json().await.unwrap();
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
    let llm = server.llm_client("/llm");

    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Test"}]
    });

    let response = llm.completions_error(request).await;
    assert_eq!(response.status(), 502);

    let body: serde_json::Value = response.json().await.unwrap();
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
