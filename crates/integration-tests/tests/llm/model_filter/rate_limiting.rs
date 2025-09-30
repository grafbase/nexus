//! Tests for rate limiting with filter-based model routing

use indoc::indoc;
use integration_tests::TestServer;
use serde_json::json;

#[tokio::test]
async fn rate_limiting_works_with_filter_routing() {
    use integration_tests::llms::OpenAIMock;

    // Set up a mock that will handle the requests
    let mock = OpenAIMock::new("openai")
        .with_models(vec!["gpt-4o-mini".to_string()])
        .with_model_filter("^gpt-4.*")
        .with_response("Hello", "Response");

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let config = indoc! {r#"
        [server.client_identification]
        enabled = true
        client_id.http_header = "x-client-id"

        [llm.providers.openai.rate_limits.per_user]
        input_token_limit = 100
        interval = "60s"
    "#};

    let server = builder.build(config).await;

    // First request with filter-matched model should succeed
    let request = json!({
        "model": "gpt-4o-mini",  // Matches filter ^gpt-4.*
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let response = server
        .openai_completions(request.clone())
        .header("x-client-id", "user1")
        .send()
        .await;

    // Should succeed
    insta::assert_json_snapshot!(response, {
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
            "content": "Response"
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

    // Repeated filter requests should eventually exhaust the provider-level bucket
    let mut filter_rate_limited_body = None;
    for _ in 0..20 {
        let (status, body) = server
            .openai_completions(request.clone())
            .header("x-client-id", "user1")
            .send_raw()
            .await;

        if status == 429 {
            filter_rate_limited_body = Some(body);
            break;
        }
    }

    let filter_rate_limited_body =
        filter_rate_limited_body.expect("expected filter-based routing to hit the provider token limit");

    insta::assert_json_snapshot!(filter_rate_limited_body, @r###"
    {
      "error": {
        "message": "Rate limit exceeded: Token rate limit exceeded. Please try again later.",
        "type": "rate_limit_error",
        "code": 429
      }
    }
    "###);

    // Legacy-prefixed requests must share the same bucket and remain rate limited
    let legacy_request = json!({
        "model": "openai/gpt-4o-mini",
        "messages": [{"role": "user", "content": "Short message"}]
    });

    let (status, legacy_body) = server
        .openai_completions(legacy_request)
        .header("x-client-id", "user1")
        .send_raw()
        .await;

    assert_eq!(status, 429);
    insta::assert_json_snapshot!(legacy_body, @r###"
    {
      "error": {
        "message": "Rate limit exceeded: Token rate limit exceeded. Please try again later.",
        "type": "rate_limit_error",
        "code": 429
      }
    }
    "###);

    // Different user should work
    let request = json!({
        "model": "gpt-4o-mini",
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let response = server
        .openai_completions(request)
        .header("x-client-id", "user2") // Different user
        .send()
        .await;

    // Should succeed for different user
    insta::assert_json_snapshot!(response, {
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
            "content": "Response"
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
async fn rate_limiting_with_model_specific_limits() {
    use integration_tests::llms::OpenAIMock;

    // Set up a mock with multiple models
    let mock = OpenAIMock::new("openai")
        .with_models(vec!["gpt-4o".to_string(), "gpt-3.5-turbo".to_string()])
        .with_model_filter("^gpt-.*")
        .with_response("Hello", "Response");

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let config = indoc! {r#"
        [server.client_identification]
        enabled = true
        client_id.http_header = "x-client-id"

        # Provider default rate limit (high)
        [llm.providers.openai.rate_limits.per_user]
        input_token_limit = 1000
        interval = "60s"

        # Specific model with stricter limit
        [llm.providers.openai.models."gpt-4o"]

        [llm.providers.openai.models."gpt-4o".rate_limits.per_user]
        input_token_limit = 50
        interval = "60s"
    "#};

    let server = builder.build(config).await;

    // Test that gpt-4o has stricter limit (50 tokens) even when accessed via filter
    let limited_request = json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let mut model_rate_limited_body = None;
    for _ in 0..20 {
        let (status, body) = server
            .openai_completions(limited_request.clone())
            .header("x-client-id", "user1")
            .send_raw()
            .await;

        if status == 429 {
            model_rate_limited_body = Some(body);
            break;
        }
    }

    let model_rate_limited_body =
        model_rate_limited_body.expect("expected model-specific token limit to trigger for gpt-4o");

    insta::assert_json_snapshot!(model_rate_limited_body, @r###"
    {
      "error": {
        "message": "Rate limit exceeded: Token rate limit exceeded. Please try again later.",
        "type": "rate_limit_error",
        "code": 429
      }
    }
    "###);

    // Test that other filter-matched models use provider default (1000 tokens)
    let small_content = "Hello, world!";
    let request = json!({
        "model": "gpt-3.5-turbo",  // Matches filter, uses provider default
        "messages": [{"role": "user", "content": small_content}]
    });

    let response = server
        .openai_completions(request)
        .header("x-client-id", "user2")
        .send()
        .await;

    // Should succeed with provider default limit
    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[created]"
    }, @r###"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[created]",
      "model": "gpt-3.5-turbo",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "Response"
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
