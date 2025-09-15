//! Integration tests for Redis-based rate limiting in LLM endpoints.

use indoc::formatdoc;
use integration_tests::{TestServer, llms::OpenAIMock};
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_prefix(test_name: &str) -> String {
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    format!("{test_name}_{timestamp}_")
}

#[tokio::test]
async fn rate_limiting_with_headers() {
    let openai = OpenAIMock::new("openai").with_models(vec!["gpt-4".to_string()]);

    let mut builder = TestServer::builder();
    builder.spawn_llm(openai).await;

    let prefix = unique_prefix("headers");

    let config = formatdoc! {r#"
        [server.rate_limits]
        enabled = true
        storage = {{ type = "redis", url = "redis://127.0.0.1:6379", key_prefix = "{prefix}" }}

        [llm.providers.openai.rate_limits.per_user]
        input_token_limit = 100
        interval = "60s"

        [server.client_identification]
        enabled = true
        client_id = {{ http_header = "X-Client-Id" }}
    "#};

    let server = builder.build(&config).await;

    // Each request: ~8 input tokens (max_tokens not counted)
    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Hello"}],
        "max_tokens": 30
    });

    // Make 12 requests (96 tokens) - should all succeed
    for i in 1..=12 {
        let (status, _body) = server
            .openai_completions(request.clone())
            .header("X-Client-Id", "redis-client-headers-test")
            .send_raw()
            .await;
        assert_eq!(status, 200, "Request {} should succeed", i);
    }

    // 13th request: 104 tokens total (should fail - exceeds 100 limit)
    let (status, body) = server
        .openai_completions(request.clone())
        .header("X-Client-Id", "redis-client-headers-test")
        .send_raw()
        .await;

    assert_eq!(status, 429, "13th request (104 total tokens) should be rate limited");

    // Verify error response format

    insta::assert_json_snapshot!(body, {
        ".error.message" => "[rate limit message]"
    }, @r#"
    {
      "error": {
        "message": "[rate limit message]",
        "type": "rate_limit_error",
        "code": 429
      }
    }
    "#);
}

/// Test Redis rate limiting with group-based limits using headers.
#[tokio::test]
async fn group_based_rate_limiting() {
    let openai = OpenAIMock::new("openai").with_models(vec!["gpt-4".to_string()]);

    let mut builder = TestServer::builder();
    builder.spawn_llm(openai).await;

    let prefix = unique_prefix("groups");

    let config = formatdoc! {r#"
        [server.rate_limits]
        enabled = true
        storage = {{ type = "redis", url = "redis://127.0.0.1:6379", key_prefix = "{prefix}" }}

        [llm.providers.openai.rate_limits.per_user]
        input_token_limit = 100
        interval = "60s"

        [llm.providers.openai.rate_limits.per_user.groups.premium]
        input_token_limit = 500
        interval = "60s"

        [llm.providers.openai.rate_limits.per_user.groups.basic]
        input_token_limit = 50
        interval = "60s"

        [server.client_identification]
        enabled = true
        client_id = {{ http_header = "X-Client-Id" }}
        group_id = {{ http_header = "X-Group" }}

        [server.client_identification.validation]
        group_values = ["premium", "basic"]  # Must define allowed_groups when using group_id
    "#};

    let server = builder.build(&config).await;

    // Basic tier client with small limit (50 tokens)
    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Test"}],
        "max_tokens": 40
    });

    // Make 6 requests (48 tokens) - should all succeed
    for i in 1..=6 {
        let (status, _body) = server
            .openai_completions(request.clone())
            .header("X-Client-Id", "basic-group-client")
            .header("X-Group", "basic")
            .send_raw()
            .await;
        assert_eq!(status, 200, "Basic tier request {} should succeed", i);
    }

    // 7th request should be rate limited (56 tokens > 50 limit)
    let (status, _body) = server
        .openai_completions(request.clone())
        .header("X-Client-Id", "basic-group-client")
        .header("X-Group", "basic")
        .send_raw()
        .await;
    assert_eq!(status, 429, "Basic tier 7th request should be rate limited");

    // Premium tier client with larger limit (500 tokens)
    let large_request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Test"}],
        "max_tokens": 200
    });

    // Make 62 requests (496 tokens) - should all succeed
    for i in 1..=62 {
        let (status, _body) = server
            .openai_completions(large_request.clone())
            .header("X-Client-Id", "premium-group-client")
            .header("X-Group", "premium")
            .send_raw()
            .await;
        assert_eq!(status, 200, "Premium tier request {} should succeed", i);
    }

    // 63rd request should be rate limited (504 tokens > 500 limit)
    let (status, _body) = server
        .openai_completions(large_request.clone())
        .header("X-Client-Id", "premium-group-client")
        .header("X-Group", "premium")
        .send_raw()
        .await;

    assert_eq!(status, 429, "Premium tier 63rd request should be rate limited");
}

/// Test that different clients have independent Redis rate limits.
#[tokio::test]
async fn independent_client_rate_limits() {
    let openai = OpenAIMock::new("openai").with_models(vec!["gpt-4".to_string()]);

    let mut builder = TestServer::builder();
    builder.spawn_llm(openai).await;

    let prefix = unique_prefix("independent");
    let config = format!(
        r#"
        [server.rate_limits]
        enabled = true
        storage = {{ type = "redis", url = "redis://127.0.0.1:6379", key_prefix = "{}" }}

        [llm.providers.openai.rate_limits.per_user]
        input_token_limit = 100
        interval = "60s"

        [server.client_identification]
        enabled = true
        client_id = {{ http_header = "X-Client-Id" }}

    "#,
        prefix
    );

    let server = builder.build(&config).await;

    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Hello"}],
        "max_tokens": 80
    });

    // Client 1 uses most of their limit (12 requests * 8 tokens = 96 tokens)
    for i in 1..=12 {
        let (status, _body) = server
            .openai_completions(request.clone())
            .header("X-Client-Id", "redis-independent-client-1")
            .send_raw()
            .await;
        assert_eq!(status, 200, "Client 1 request {} should succeed", i);
    }

    // Client 1 should be rate limited on 13th request (104 tokens > 100 limit)
    let (status, _body) = server
        .openai_completions(request.clone())
        .header("X-Client-Id", "redis-independent-client-1")
        .send_raw()
        .await;
    assert_eq!(status, 429, "Client 1 13th request should be rate limited");

    // But Client 2 should still be able to make requests (independent rate limit)
    let (status, _body) = server
        .openai_completions(request.clone())
        .header("X-Client-Id", "redis-independent-client-2")
        .send_raw()
        .await;
    assert_eq!(status, 200, "Client 2 should have independent rate limit");
}

/// Test Redis rate limiting persists across server restarts.
#[tokio::test]
async fn rate_limit_persistence() {
    let prefix = unique_prefix("persistence");
    let config = format!(
        r#"
        [server.rate_limits]
        enabled = true
        storage = {{ type = "redis", url = "redis://127.0.0.1:6379", key_prefix = "{}" }}

        [llm.providers.openai.rate_limits.per_user]
        input_token_limit = 100
        interval = "300s"  # 5 minute window for persistence test

        [server.client_identification]
        enabled = true
        client_id = {{ http_header = "X-Client-Id" }}

    "#,
        prefix
    );

    // First server instance
    {
        let openai = OpenAIMock::new("openai").with_models(vec!["gpt-4".to_string()]);

        let mut builder = TestServer::builder();
        builder.spawn_llm(openai).await;

        let server = builder.build(&config).await;

        let request = json!({
            "model": "openai/gpt-4",
            "messages": [{"role": "user", "content": "Test"}],
            "max_tokens": 80
        });

        // Make 12 requests (96 tokens) in first server instance
        for i in 1..=12 {
            let (status, _body) = server
                .openai_completions(request.clone())
                .header("X-Client-Id", "redis-persistent-client")
                .send_raw()
                .await;
            assert_eq!(status, 200, "First server request {} should succeed", i);
        }
    }

    // Second server instance (simulating restart)
    {
        let openai = OpenAIMock::new("openai").with_models(vec!["gpt-4".to_string()]);

        let mut builder = TestServer::builder();
        builder.spawn_llm(openai).await;

        let server = builder.build(&config).await;

        let request = json!({
            "model": "openai/gpt-4",
            "messages": [{"role": "user", "content": "Test"}],
            "max_tokens": 15
        });

        // Should be rate limited because Redis persisted the previous consumption (96 tokens used)
        // The next request would be 104 tokens total, exceeding the 100 limit
        let (status, _body) = server
            .openai_completions(request.clone())
            .header("X-Client-Id", "redis-persistent-client")
            .send_raw()
            .await;

        assert_eq!(status, 429, "Should be rate limited due to Redis persistence");
    }
}

/// Test concurrent requests with Redis rate limiting.
#[tokio::test]
async fn concurrent_requests() {
    let openai = OpenAIMock::new("openai").with_models(vec!["gpt-4".to_string()]);

    let mut builder = TestServer::builder();
    builder.spawn_llm(openai).await;

    let prefix = unique_prefix("concurrent");
    let config = format!(
        r#"
        [server.rate_limits]
        enabled = true
        storage = {{ type = "redis", url = "redis://127.0.0.1:6379", key_prefix = "{}" }}

        [llm.providers.openai.rate_limits.per_user]
        input_token_limit = 200
        interval = "60s"

        [server.client_identification]
        enabled = true
        client_id = {{ http_header = "X-Client-Id" }}

    "#,
        prefix
    );

    let server = builder.build(&config).await;

    // Prepare multiple requests that together exceed the limit
    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Test"}],
        "max_tokens": 45
    });

    // Send 26 concurrent requests (26 * 8 = 208 tokens > 200 limit)
    let mut futures = Vec::new();
    for _ in 0..26 {
        let future = server
            .openai_completions(request.clone())
            .header("X-Client-Id", "redis-concurrent-test")
            .send_raw();
        futures.push(future);
    }

    let responses = futures::future::join_all(futures).await;

    // Count successful vs rate-limited responses
    let success_count = responses.iter().filter(|(status, _)| *status == 200).count();
    let rate_limited_count = responses.iter().filter(|(status, _)| *status == 429).count();

    // At least one should be rate limited (since 26 * 8 = 208 > 200)
    assert!(rate_limited_count >= 1, "At least one request should be rate limited");

    assert!(
        success_count >= 25,
        "At least 25 requests should succeed (200 / 8 = 25)"
    );

    assert_eq!(
        success_count + rate_limited_count,
        26,
        "All requests should either succeed or be rate limited"
    );
}
