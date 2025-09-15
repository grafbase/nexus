//! Integration tests for token-based rate limiting in LLM endpoints.

mod edge_cases;
mod group_fallback;
mod redis;

use indoc::indoc;
use integration_tests::{TestServer, llms::OpenAIMock};
use serde_json::json;

/// Test that requests without client identification are allowed when rate limiting is not enforced.
#[tokio::test]
async fn no_client_id_allows_requests() {
    let openai = OpenAIMock::new("openai").with_models(vec!["gpt-4".to_string()]);

    let mut builder = TestServer::builder();
    builder.spawn_llm(openai).await;

    let config = indoc! {r#"
        # Rate limits without client identification should not be configured
        # This test verifies requests work without rate limiting
    "#};

    let server = builder.build(config).await;

    // Request should succeed without client identification
    let request = json!({
        "model": "openai/gpt-4",
        "messages": [
            {
                "role": "user",
                "content": "Hello"
            }
        ]
    });
    let response = server.openai_completions(request).send().await;

    insta::assert_json_snapshot!(response["choices"][0]["message"], @r#"
    {
      "role": "assistant",
      "content": "Hello! I'm a test LLM assistant. How can I help you today?"
    }
    "#);
}

/// Test basic token rate limiting with client identification and accumulation.
#[tokio::test]
async fn token_rate_limit_enforced_with_client_id() {
    let openai = OpenAIMock::new("openai").with_models(vec!["gpt-4".to_string()]);

    let mut builder = TestServer::builder();
    builder.spawn_llm(openai).await;

    let config = indoc! {r#"
        [llm.providers.openai.rate_limits.per_user]
        input_token_limit = 100
        interval = "60s"

        [server.client_identification]
        enabled = true
        client_id = { http_header = "X-Client-Id" }
    "#};

    let server = builder.build(config).await;

    // Each request: ~8 input tokens (output tokens not counted anymore)
    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Hello"}],
        "max_tokens": 30
    });

    // Make requests up to the limit (100 tokens / 8 tokens per request = 12 requests)
    for i in 1..=12 {
        let (status, _body) = server
            .openai_completions(request.clone())
            .header("X-Client-Id", "test-client")
            .send_raw()
            .await;
        assert_eq!(status, 200, "Request {} ({} total tokens) should succeed", i, i * 8);
    }

    // 13th request: 104 tokens (should fail - exceeds 100 limit)
    let (status, body) = server
        .openai_completions(request.clone())
        .header("X-Client-Id", "test-client")
        .send_raw()
        .await;
    assert_eq!(status, 429, "13th request (104 total tokens) should be rate limited");

    // Verify error response format
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Rate limit exceeded")
    );
}

/// Test the 4-level hierarchy for rate limit resolution with accumulation.
#[tokio::test]
async fn rate_limit_hierarchy_resolution() {
    let openai = OpenAIMock::new("openai").with_models(vec!["gpt-4".to_string(), "gpt-3.5-turbo".to_string()]);

    let mut builder = TestServer::builder();
    builder.spawn_llm(openai).await;

    let config = indoc! {r#"
        # Level 4: Provider default
        [llm.providers.openai.rate_limits.per_user]
        input_token_limit = 100
        interval = "60s"

        # Level 3: Provider + Group
        [llm.providers.openai.rate_limits.per_user.groups.pro]
        input_token_limit = 200
        interval = "60s"

        # Level 2: Model default
        [llm.providers.openai.models."gpt-4".rate_limits.per_user]
        input_token_limit = 300
        interval = "60s"

        # Level 1: Model + Group (highest priority)
        [llm.providers.openai.models."gpt-4".rate_limits.per_user.groups.pro]
        input_token_limit = 400
        interval = "60s"

        [server.client_identification]
        enabled = true
        client_id = { http_header = "X-Client-Id" }
        group_id = { http_header = "X-Group" }

        [server.client_identification.validation]
        group_values = ["pro"]  # Must define allowed_groups when using group_id
    "#};

    let server = builder.build(config).await;

    // Test Level 1: Model + Group (gpt-4 + pro group) - limit 400
    // Each request: ~8 input tokens (max_tokens not counted)
    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Test"}],
        "max_tokens": 150
    });

    // Make requests up to the limit (400 tokens / 8 tokens per request = 50 requests)
    for i in 1..=50 {
        let (status, _body) = server
            .openai_completions(request.clone())
            .header("X-Client-Id", "client1")
            .header("X-Group", "pro")
            .send_raw()
            .await;
        assert_eq!(
            status,
            200,
            "Level 1: Request {} ({} total tokens) should succeed",
            i,
            i * 8
        );
    }

    // 51st request: 408 tokens (exceeds 400 limit)
    let (status, _body) = server
        .openai_completions(request.clone())
        .header("X-Client-Id", "client1")
        .header("X-Group", "pro")
        .send_raw()
        .await;
    assert_eq!(
        status, 429,
        "Level 1: 51st request (408 total tokens) should be rate limited"
    );

    // Test Level 2: Model default (gpt-4 without group) - limit 300
    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Test"}],
        "max_tokens": 140
    });

    // Make requests up to the limit (300 tokens / 8 tokens per request = 37 requests)
    for i in 1..=37 {
        let (status, _body) = server
            .openai_completions(request.clone())
            .header("X-Client-Id", "client2")
            .send_raw()
            .await;
        assert_eq!(
            status,
            200,
            "Level 2: Request {} ({} total tokens) should succeed",
            i,
            i * 8
        );
    }

    // 38th request: 304 tokens (exceeds 300 limit)
    let (status, _body) = server
        .openai_completions(request.clone())
        .header("X-Client-Id", "client2")
        .send_raw()
        .await;
    assert_eq!(status, 429, "Level 2: 38th request should be rate limited");

    // Test Level 3: Provider + Group (gpt-3.5-turbo + pro group) - limit 200
    let request = json!({
        "model": "openai/gpt-3.5-turbo",
        "messages": [{"role": "user", "content": "Test"}],
        "max_tokens": 90
    });

    // Make requests up to the limit (200 tokens / 8 tokens per request = 25 requests)
    for i in 1..=25 {
        let (status, _body) = server
            .openai_completions(request.clone())
            .header("X-Client-Id", "client3")
            .header("X-Group", "pro")
            .send_raw()
            .await;
        assert_eq!(
            status,
            200,
            "Level 3: Request {} ({} total tokens) should succeed",
            i,
            i * 8
        );
    }

    // 26th request: 208 tokens (exceeds 200 limit)
    let (status, _body) = server
        .openai_completions(request.clone())
        .header("X-Client-Id", "client3")
        .header("X-Group", "pro")
        .send_raw()
        .await;
    assert_eq!(status, 429, "Level 3: 26th request should be rate limited");

    // Test Level 4: Provider default (gpt-3.5-turbo without group) - limit 100
    let request = json!({
        "model": "openai/gpt-3.5-turbo",
        "messages": [{"role": "user", "content": "Test"}],
        "max_tokens": 40
    });

    // Make requests up to the limit (100 tokens / 8 tokens per request = 12 requests)
    for i in 1..=12 {
        let (status, _body) = server
            .openai_completions(request.clone())
            .header("X-Client-Id", "client4")
            .send_raw()
            .await;
        assert_eq!(
            status,
            200,
            "Level 4: Request {} ({} total tokens) should succeed",
            i,
            i * 8
        );
    }

    // 13th request: 104 tokens (exceeds 100 limit)
    let (status, _body) = server
        .openai_completions(request.clone())
        .header("X-Client-Id", "client4")
        .send_raw()
        .await;
    assert_eq!(status, 429, "Level 4: 13th request should be rate limited");
}

/// Test that different clients have independent rate limits.
#[tokio::test]
async fn independent_client_rate_limits() {
    let openai = OpenAIMock::new("openai").with_models(vec!["gpt-4".to_string()]);

    let mut builder = TestServer::builder();
    builder.spawn_llm(openai).await;

    let config = indoc! {r#"
        [llm.providers.openai.rate_limits.per_user]
        input_token_limit = 100
        interval = "60s"

        [server.client_identification]
        enabled = true
        client_id = { http_header = "X-Client-Id" }

    "#};

    let server = builder.build(config).await;

    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Hello"}],
        "max_tokens": 80
    });

    // Client 1 uses most of their limit (12 requests * 8 tokens = 96 tokens)
    for i in 1..=12 {
        let (status, _body) = server
            .openai_completions(request.clone())
            .header("X-Client-Id", "client1")
            .send_raw()
            .await;
        assert_eq!(status, 200, "Client 1 request {} should succeed", i);
    }

    // Client 1 should be rate limited on 13th request (104 tokens > 100 limit)
    let (status, _body) = server
        .openai_completions(request.clone())
        .header("X-Client-Id", "client1")
        .send_raw()
        .await;
    assert_eq!(status, 429);

    // But Client 2 should still be able to make requests
    let (status, _body) = server
        .openai_completions(request.clone())
        .header("X-Client-Id", "client2")
        .send_raw()
        .await;
    assert_eq!(status, 200);
}

/// Test that group-based rate limits are enforced correctly.
#[tokio::test]
async fn group_based_rate_limits() {
    let openai = OpenAIMock::new("openai").with_models(vec!["gpt-4".to_string()]);

    let mut builder = TestServer::builder();
    builder.spawn_llm(openai).await;

    let config = indoc! {r#"
        [llm.providers.openai.rate_limits.per_user]
        input_token_limit = 500
        interval = "60s"

        [llm.providers.openai.rate_limits.per_user.groups.free]
        input_token_limit = 100
        interval = "60s"

        [llm.providers.openai.rate_limits.per_user.groups.pro]
        input_token_limit = 1000
        interval = "60s"

        [server.client_identification]
        enabled = true
        client_id = { http_header = "X-Client-Id" }
        group_id = { http_header = "X-Group" }

        [server.client_identification.validation]
        group_values = ["free", "pro"]  # Must define allowed_groups when using group_id
    "#};

    let server = builder.build(config).await;

    // Free tier client with small limit (100 tokens)
    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Hello"}],
        "max_tokens": 80
    });

    // Make 12 requests (96 tokens total) - should all succeed
    for i in 1..=12 {
        let (status, _body) = server
            .openai_completions(request.clone())
            .header("X-Client-Id", "free-client")
            .header("X-Group", "free")
            .send_raw()
            .await;
        assert_eq!(status, 200, "Free tier request {} should succeed", i);
    }

    // 13th request should be rate limited (104 tokens > 100 limit)
    let (status, _body) = server
        .openai_completions(request.clone())
        .header("X-Client-Id", "free-client")
        .header("X-Group", "free")
        .send_raw()
        .await;
    assert_eq!(status, 429);

    // Pro tier client with larger limit (1000 tokens)
    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Hello"}],
        "max_tokens": 900
    });

    // Make many requests to use most of the limit (125 requests * 8 tokens = 1000 tokens)
    for i in 1..=125 {
        let (status, _body) = server
            .openai_completions(request.clone())
            .header("X-Client-Id", "pro-client")
            .header("X-Group", "pro")
            .send_raw()
            .await;
        assert_eq!(status, 200, "Pro tier request {} should succeed", i);
    }

    // 126th request should be rate limited (1008 tokens > 1000 limit)
    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Hello"}],
        "max_tokens": 50
    });

    let (status, _body) = server
        .openai_completions(request.clone())
        .header("X-Client-Id", "pro-client")
        .header("X-Group", "pro")
        .send_raw()
        .await;
    assert_eq!(status, 429);
}

/// Test that requests without required client identification are rejected.
#[tokio::test]
async fn missing_client_id_rejected_when_required() {
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
        "messages": [{"role": "user", "content": "Hello"}]
    });

    // Request without X-Client-Id header should be rejected
    let (status, body) = server.openai_completions(request.clone()).send_raw().await;

    assert_eq!(status, 400);
    assert_eq!(body["error"], "missing_client_id");
    assert_eq!(body["error_description"], "Client identification is required");
}

/// Test that invalid group names are rejected with 400 Bad Request.
#[tokio::test]
async fn invalid_group_rejected() {
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
        group_values = ["gold", "silver", "bronze"]
    "#};

    let server = builder.build(config).await;

    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Test"}]
    });

    // Request with invalid group should be rejected
    let (status, body) = server
        .openai_completions(request.clone())
        .header("X-Client-Id", "test-client")
        .header("X-Group", "platinum") // Not in group_values
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

/// Test clear token accumulation behavior.
#[tokio::test]
async fn clear_token_accumulation() {
    let openai = OpenAIMock::new("openai").with_models(vec!["gpt-4".to_string()]);

    let mut builder = TestServer::builder();
    builder.spawn_llm(openai).await;

    let config = indoc! {r#"
        [llm.providers.openai.rate_limits.per_user]
        input_token_limit = 100
        interval = "60s"

        [server.client_identification]
        enabled = true
        client_id = { http_header = "X-Client-Id" }

    "#};

    let server = builder.build(config).await;

    // Test accumulation: each request is ~6 input tokens (max_tokens not counted)
    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Hi"}],  // ~6 input tokens
        "max_tokens": 24
    });

    // Make 12 requests that should stay under the 100 token limit
    for i in 1..=12 {
        let (status, _body) = server
            .openai_completions(request.clone())
            .header("X-Client-Id", "accumulation-test")
            .send_raw()
            .await;
        assert_eq!(status, 200, "Request {} should succeed", i);
    }

    // 13th request: should exceed the limit
    let (status, _body) = server
        .openai_completions(request.clone())
        .header("X-Client-Id", "accumulation-test")
        .send_raw()
        .await;
    assert_eq!(status, 429, "Request 13 should be rate limited");
}

/// Test exact boundary conditions for rate limits.
#[tokio::test]
async fn exact_boundary_conditions() {
    let openai = OpenAIMock::new("openai").with_models(vec!["gpt-4".to_string()]);

    let mut builder = TestServer::builder();
    builder.spawn_llm(openai).await;

    let config = indoc! {r#"
        [llm.providers.openai.rate_limits.per_user]
        input_token_limit = 100
        interval = "60s"

        [server.client_identification]
        enabled = true
        client_id = { http_header = "X-Client-Id" }

    "#};

    let server = builder.build(config).await;

    // Test 1: Make requests up to the limit
    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Test"}],  // ~8 input tokens
        "max_tokens": 92
    });

    // Make 12 requests that should succeed
    for i in 1..=12 {
        let (status, _body) = server
            .openai_completions(request.clone())
            .header("X-Client-Id", "boundary-test-1")
            .send_raw()
            .await;
        assert_eq!(status, 200, "Request {} should succeed", i);
    }

    // 13th request should be rate limited
    let (status, _body) = server
        .openai_completions(request.clone())
        .header("X-Client-Id", "boundary-test-1")
        .send_raw()
        .await;
    assert_eq!(status, 429, "Request exceeding limit should be rate limited");

    // Test 2: Different client should be independent
    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Test"}],
        "max_tokens": 93
    });

    // Make requests with different client - should succeed
    for i in 1..=12 {
        let (status, _body) = server
            .openai_completions(request.clone())
            .header("X-Client-Id", "boundary-test-2") // Different client
            .send_raw()
            .await;
        assert_eq!(status, 200, "Request {} should succeed", i);
    }

    // 13th request should fail
    let (status, _body) = server
        .openai_completions(request.clone())
        .header("X-Client-Id", "boundary-test-2")
        .send_raw()
        .await;
    assert_eq!(status, 429, "13th request should be rate limited");

    // Test 3: Just under limit should succeed
    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Test"}],  // ~8 input tokens
        "max_tokens": 91
    });

    // Make 12 requests (96 tokens) - should all succeed
    for i in 1..=12 {
        let (status, _body) = server
            .openai_completions(request.clone())
            .header("X-Client-Id", "boundary-test-3")
            .send_raw()
            .await;
        assert_eq!(status, 200, "Request {} should succeed", i);
    }

    // 13th request should fail (104 tokens > 100 limit)
    let (status, _body) = server
        .openai_completions(request.clone())
        .header("X-Client-Id", "boundary-test-3")
        .send_raw()
        .await;
    assert_eq!(status, 429, "13th request should be rate limited");
}

/// Test that rate limits reset after the interval expires.
#[tokio::test]
async fn rate_limit_reset_after_interval() {
    let openai = OpenAIMock::new("openai").with_models(vec!["gpt-4".to_string()]);

    let mut builder = TestServer::builder();
    builder.spawn_llm(openai).await;

    let config = indoc! {r#"
        [llm.providers.openai.rate_limits.per_user]
        input_token_limit = 50
        interval = "2s"  # Short interval for testing

        [server.client_identification]
        enabled = true
        client_id = { http_header = "X-Client-Id" }

    "#};

    let server = builder.build(config).await;

    // Request that uses up the limit (50 tokens / 8 tokens per request = 6 requests)
    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Test"}],  // ~8 input tokens
        "max_tokens": 42
    });

    // Make 6 requests (48 tokens) - should all succeed
    for i in 1..=6 {
        let (status, _body) = server
            .openai_completions(request.clone())
            .header("X-Client-Id", "reset-test")
            .send_raw()
            .await;
        assert_eq!(status, 200, "Request {} should succeed", i);
    }

    // 7th request should fail (56 tokens > 50 limit)
    let (status, _body) = server
        .openai_completions(request.clone())
        .header("X-Client-Id", "reset-test")
        .send_raw()
        .await;
    assert_eq!(status, 429, "7th request should fail (limit exhausted)");

    // Wait for interval to expire (2 seconds + buffer)
    tokio::time::sleep(tokio::time::Duration::from_millis(2500)).await;

    // After reset, should be able to make requests again
    let (status, _body) = server
        .openai_completions(request.clone())
        .header("X-Client-Id", "reset-test")
        .send_raw()
        .await;
    assert_eq!(status, 200, "Request after interval reset should succeed");
}

/// Test that missing group_id when group_values is configured uses default rate limits.
#[tokio::test]
async fn missing_group_when_group_values_configured() {
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
        group_values = ["gold", "silver", "bronze"]
    "#};

    let server = builder.build(config).await;

    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Test"}]
    });

    // Request without group header when group_values is configured
    // Should succeed and use default rate limits
    let (status, body) = server
        .openai_completions(request.clone())
        .header("X-Client-Id", "test-client")
        // Missing X-Group header - this is OK, will use default rate limit
        .send_raw()
        .await;

    // Should succeed since missing group is allowed (uses default rate limit)
    assert_eq!(
        status, 200,
        "Request without group should succeed with default rate limit"
    );

    // Verify the response is valid
    let body = body;
    assert!(body["choices"].is_array());
}

/// Test that token rate limiting allows full burst capacity consumption.
#[tokio::test]
async fn token_burst_capacity_allows_full_limit() {
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

    // Make requests that consume exactly 48 tokens (6 requests * 8 tokens)
    // This is just under the 50 token limit
    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Test"}],
        "max_tokens": 10
    });

    // Make 6 requests (48 tokens total)
    for i in 1..=6 {
        let (status, _body) = server
            .openai_completions(request.clone())
            .header("X-Client-Id", "burst-test")
            .send_raw()
            .await;
        assert_eq!(status, 200, "Request {} should succeed (within burst capacity)", i);
    }

    // 7th request should fail (56 tokens > 50 limit)
    let (status, _body) = server
        .openai_completions(request.clone())
        .header("X-Client-Id", "burst-test")
        .send_raw()
        .await;
    assert_eq!(status, 429, "7th request should be rate limited (exceeds limit)");
}

/// Test concurrent token requests with in-memory storage.
#[tokio::test]
async fn concurrent_token_requests_memory() {
    use std::sync::Arc;

    let openai = OpenAIMock::new("openai").with_models(vec!["gpt-4".to_string()]);

    let mut builder = TestServer::builder();
    builder.spawn_llm(openai).await;

    let config = indoc! {r#"
        [llm.providers.openai.rate_limits.per_user]
        input_token_limit = 100
        interval = "60s"

        [server.client_identification]
        enabled = true
        client_id = { http_header = "X-Client-Id" }
    "#};

    let server = Arc::new(builder.build(config).await);

    // Launch 20 concurrent requests, each consuming ~8 tokens
    let mut handles = vec![];

    for _ in 0..20 {
        let server_clone = Arc::clone(&server);
        let handle = tokio::spawn(async move {
            let request = json!({
                "model": "openai/gpt-4",
                "messages": [{"role": "user", "content": "Test"}],
                "max_tokens": 10
            });

            server_clone
                .client
                .request(reqwest::Method::POST, "/llm/openai/v1/chat/completions")
                .json(&request)
                .header("X-Client-Id", "concurrent-test")
                .send()
                .await
                .unwrap()
                .status()
                .as_u16()
        });
        handles.push(handle);
    }

    // Collect results
    let mut success_count = 0;
    let mut rate_limited_count = 0;

    for handle in handles {
        #[allow(clippy::panic)]
        match handle.await.unwrap() {
            200 => success_count += 1,
            429 => rate_limited_count += 1,
            status => panic!("Unexpected status code: {status}"),
        }
    }

    // With 100 token limit and ~8 tokens per request, should allow ~12 requests
    assert!(
        (11..=13).contains(&success_count),
        "Expected 11-13 successful requests, got {success_count}"
    );
    assert_eq!(
        success_count + rate_limited_count,
        20,
        "All requests should either succeed or be rate limited"
    );
}

/// Test that zero token requests are handled properly.
#[tokio::test]
async fn zero_token_request_handling() {
    let openai = OpenAIMock::new("openai").with_models(vec!["gpt-4".to_string()]);

    let mut builder = TestServer::builder();
    builder.spawn_llm(openai).await;

    let config = indoc! {r#"
        [llm.providers.openai.rate_limits.per_user]
        input_token_limit = 100
        interval = "60s"

        [server.client_identification]
        enabled = true
        client_id = { http_header = "X-Client-Id" }
    "#};

    let server = builder.build(config).await;

    // Empty message (should still count as minimum tokens for request overhead)
    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": ""}],
        "max_tokens": 10
    });

    // Even empty requests have some token overhead (role, structure, etc.)
    // Should still succeed but consume minimal tokens
    let (status, _body) = server
        .openai_completions(request.clone())
        .header("X-Client-Id", "zero-test")
        .send_raw()
        .await;
    assert_eq!(status, 200, "Empty content request should still succeed");
}
