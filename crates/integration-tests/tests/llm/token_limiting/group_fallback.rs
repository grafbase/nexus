//! Test to verify that users with undefined groups fall back to default limits.

use integration_tests::{TestServer, llms::OpenAIMock};
use serde_json::json;

/// Test that users with undefined groups fall back to default limits.
#[tokio::test]
async fn undefined_group_falls_back_to_default() {
    let openai = OpenAIMock::new("openai").with_models(vec!["gpt-4".to_string()]);

    let mut builder = TestServer::builder();
    builder.spawn_llm(openai).await;

    // Configuration with only "premium" group defined, but not "basic"
    let config = r#"
        [llm.providers.openai.rate_limits.per_user]
        input_token_limit = 100
        interval = "60s"

        [llm.providers.openai.rate_limits.per_user.groups.premium]
        input_token_limit = 500
        interval = "60s"

        [server.client_identification]
        enabled = true
        client_id = { http_header = "X-Client-Id" }
        group_id = { http_header = "X-Group" }
        
        [server.client_identification.validation]
        group_values = ["premium", "basic"]  # Must define group_values when using group_id
    "#;

    let server = builder.build(config).await;

    // Request with undefined group "basic" - should fall back to default (100 tokens)
    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Test"}],
        "max_tokens": 80
    });

    // Make 12 requests (96 tokens) with undefined group "basic"
    for i in 1..=12 {
        let (status, _body) = server
            .openai_completions(request.clone())
            .header("X-Client-Id", "test-client")
            .header("X-Group", "basic") // This group is NOT defined
            .send_raw()
            .await;
        assert_eq!(status, 200, "Request {} should succeed", i);
    }

    // 13th request - total would be 104 tokens (exceeds default 100 limit)
    let (status, body) = server
        .openai_completions(request.clone())
        .header("X-Client-Id", "test-client")
        .header("X-Group", "basic") // Still using undefined group
        .send_raw()
        .await;

    // This should be rate limited at 100 tokens (the default), not unlimited
    assert_eq!(
        status, 429,
        "Should be rate limited by default limit since 'basic' group is not defined"
    );

    // Verify the error message indicates default rate limit
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Rate limit exceeded"),
        "Should indicate rate limit exceeded"
    );
}

/// Test that users with a defined group do NOT fall back to default.
#[tokio::test]
async fn defined_group_uses_group_limit_not_default() {
    let openai = OpenAIMock::new("openai").with_models(vec!["gpt-4".to_string()]);

    let mut builder = TestServer::builder();
    builder.spawn_llm(openai).await;

    let config = r#"
        [llm.providers.openai.rate_limits.per_user]
        input_token_limit = 30
        interval = "60s"

        [llm.providers.openai.rate_limits.per_user.groups.premium]
        input_token_limit = 200
        interval = "60s"

        [server.client_identification]
        enabled = true
        client_id = { http_header = "X-Client-Id" }
        group_id = { http_header = "X-Group" }
        
        [server.client_identification.validation]
        group_values = ["premium"]  # Must define group_values when using group_id
    "#;

    let server = builder.build(config).await;

    // Request that would exceed default (30) but not group limit (200)
    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Test"}],
        "max_tokens": 40
    });

    // Make 25 requests with premium group (200 tokens total) - should all succeed
    for i in 1..=25 {
        let (status, _body) = server
            .openai_completions(request.clone())
            .header("X-Client-Id", "premium-client")
            .header("X-Group", "premium")
            .send_raw()
            .await;

        assert_eq!(
            status, 200,
            "Request {} should succeed - using group limit (200) not default (30)",
            i
        );
    }

    // 26th request would be 208 tokens total - exceeds group limit
    let (status, _body) = server
        .openai_completions(request.clone())
        .header("X-Client-Id", "premium-client")
        .header("X-Group", "premium")
        .send_raw()
        .await;

    assert_eq!(status, 429, "26th request should be rate limited at group limit (200)");
}
