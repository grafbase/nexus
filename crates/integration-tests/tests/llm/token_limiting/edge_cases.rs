//! Edge case tests for token-based rate limiting.

use indoc::indoc;
use integration_tests::{TestServer, llms::OpenAIMock};
use serde_json::json;

/// Test edge cases: empty string client_id is accepted.
#[tokio::test]
async fn empty_client_id_accepted() {
    let openai = OpenAIMock::new("openai").with_models(vec!["gpt-4".to_string()]);

    let mut builder = TestServer::builder();
    builder.spawn_llm(openai).await;

    let config = indoc! {r#"
        [llm.providers.openai.rate_limits.per_user]
        input_token_limit = 1000
        interval = "60s"

        [server.client_identification]
        enabled = true
        client_id = { http_header = "X-Client-Id" }
    "#};

    let server = builder.build(config).await;

    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Test"}],
        "max_tokens": 10
    });

    // Empty string client ID is treated as a valid (though strange) client ID
    let (status, _body) = server
        .openai_completions(request.clone())
        .header("X-Client-Id", "")
        .send_raw()
        .await;

    // Empty string is a valid client ID, just unusual
    assert_eq!(status, 200, "Empty string client ID should be accepted");
}

/// Test edge cases: whitespace-only client_id.
#[tokio::test]
async fn whitespace_client_id_accepted() {
    let openai = OpenAIMock::new("openai").with_models(vec!["gpt-4".to_string()]);

    let mut builder = TestServer::builder();
    builder.spawn_llm(openai).await;

    let config = indoc! {r#"
        [llm.providers.openai.rate_limits.per_user]
        input_token_limit = 1000
        interval = "60s"

        [server.client_identification]
        enabled = true
        client_id = { http_header = "X-Client-Id" }
    "#};

    let server = builder.build(config).await;

    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Test"}],
        "max_tokens": 10
    });

    // Whitespace-only client ID (spaces only, as tabs/newlines are invalid in headers)
    let (status, _body) = server
        .openai_completions(request.clone())
        .header("X-Client-Id", "     ")
        .send_raw()
        .await;

    // Whitespace-only is also treated as a valid client ID
    assert_eq!(status, 200, "Whitespace client ID should be accepted");
}

/// Test edge cases: very long client_id and group_id.
#[tokio::test]
async fn very_long_identifiers() {
    let openai = OpenAIMock::new("openai").with_models(vec!["gpt-4".to_string()]);

    let mut builder = TestServer::builder();
    builder.spawn_llm(openai).await;

    let config = indoc! {r#"
        [llm.providers.openai.rate_limits.per_user]
        input_token_limit = 1000
        interval = "60s"

        [llm.providers.openai.rate_limits.per_user.groups.enterprise]
        input_token_limit = 5000
        interval = "60s"

        [server.client_identification]
        enabled = true
        client_id = { http_header = "X-Client-Id" }
        group_id = { http_header = "X-Group" }
        
        [server.client_identification.validation]
        group_values = ["enterprise"]
    "#};

    let server = builder.build(config).await;

    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Test"}],
        "max_tokens": 100
    });

    // Very long identifiers (256 characters)
    let long_client_id = "a".repeat(256);

    let (status, _body) = server
        .openai_completions(request.clone())
        .header("X-Client-Id", &long_client_id)
        .header("X-Group", "enterprise")
        .send_raw()
        .await;

    // Should work with long identifiers
    assert_eq!(status, 200, "Long client ID should be accepted");
}

/// Test edge cases: special characters in identifiers.
#[tokio::test]
async fn special_characters_in_identifiers() {
    let openai = OpenAIMock::new("openai").with_models(vec!["gpt-4".to_string()]);

    let mut builder = TestServer::builder();
    builder.spawn_llm(openai).await;

    let config = indoc! {r#"
        [llm.providers.openai.rate_limits.per_user]
        input_token_limit = 1000
        interval = "60s"

        [server.client_identification]
        enabled = true
        client_id = { http_header = "X-Client-Id" }
    "#};

    let server = builder.build(config).await;

    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Test"}],
        "max_tokens": 100
    });

    // Special characters in client ID
    let special_client_id = "user@example.com:123-456_789/test";

    let (status, _body) = server
        .openai_completions(request.clone())
        .header("X-Client-Id", special_client_id)
        .send_raw()
        .await;

    // Should work with special characters
    assert_eq!(status, 200, "Special characters in client ID should be accepted");
}

/// Test error response for rate limit exceeded.
#[tokio::test]
async fn rate_limit_exceeded_response_format() {
    let openai = OpenAIMock::new("openai").with_models(vec!["gpt-4".to_string()]);

    let mut builder = TestServer::builder();
    builder.spawn_llm(openai).await;

    let config = indoc! {r#"
        [llm.providers.openai.rate_limits.per_user]
        input_token_limit = 50
        interval = "60s"

        [server.client_identification]
        enabled = true
        client_id = { http_header = "X-Client-Id" }
    "#};

    let server = builder.build(config).await;

    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Test"}],
        "max_tokens": 60
    });

    // Make 6 requests first to use up most of the limit (48 tokens)
    for i in 1..=6 {
        let (status, _body) = server
            .openai_completions(request.clone())
            .header("X-Client-Id", "rate-limit-test")
            .send_raw()
            .await;
        assert_eq!(status, 200, "Request {} should succeed", i);
    }

    // 7th request should exceed the limit (56 tokens > 50)
    let (status, body) = server
        .openai_completions(request.clone())
        .header("X-Client-Id", "rate-limit-test")
        .send_raw()
        .await;

    assert_eq!(status, 429);

    // Check the error response body (no headers available in send_raw)
    insta::assert_json_snapshot!(body, @r#"
    {
      "error": {
        "message": "Rate limit exceeded: Token rate limit exceeded. Please try again later.",
        "type": "rate_limit_error",
        "code": 429
      }
    }
    "#);
}

/// Test error response for missing required client identification.
#[tokio::test]
async fn missing_client_id_error_format() {
    let openai = OpenAIMock::new("openai").with_models(vec!["gpt-4".to_string()]);

    let mut builder = TestServer::builder();
    builder.spawn_llm(openai).await;

    let config = indoc! {r#"
        [llm.providers.openai.rate_limits.per_user]
        input_token_limit = 1000
        interval = "60s"

        [server.client_identification]
        enabled = true
        client_id = { http_header = "X-Client-Id" }
    "#};

    let server = builder.build(config).await;

    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Test"}]
    });

    let (status, body) = server
        .openai_completions(request.clone())
        // Missing X-Client-Id header
        .send_raw()
        .await;

    assert_eq!(status, 400);
    insta::assert_json_snapshot!(body, @r#"
    {
      "error": "missing_client_id",
      "error_description": "Client identification is required"
    }
    "#);
}

/// Test error response for unauthorized group.
#[tokio::test]
async fn unauthorized_group_error_format() {
    let openai = OpenAIMock::new("openai").with_models(vec!["gpt-4".to_string()]);

    let mut builder = TestServer::builder();
    builder.spawn_llm(openai).await;

    let config = indoc! {r#"
        [llm.providers.openai.rate_limits.per_user]
        input_token_limit = 1000
        interval = "60s"

        [server.client_identification]
        enabled = true
        client_id = { http_header = "X-Client-Id" }
        group_id = { http_header = "X-Group" }
        
        [server.client_identification.validation]
        group_values = ["basic", "premium"]
    "#};

    let server = builder.build(config).await;

    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Test"}]
    });

    let (status, body) = server
        .openai_completions(request.clone())
        .header("X-Client-Id", "test-client")
        .header("X-Group", "enterprise") // Not in group_values
        .send_raw()
        .await;

    assert_eq!(status, 400);
    insta::assert_json_snapshot!(body, @r#"
    {
      "error": "invalid_group",
      "error_description": "The specified group is not valid"
    }
    "#);
}
