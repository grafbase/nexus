//! LLM header rules integration tests
//! Tests that verify headers are properly sent to LLM providers according to rules

use indoc::indoc;
use integration_tests::{
    TestServer,
    llms::{AnthropicMock, GoogleMock, OpenAIMock},
};
use serde_json::json;

/// Test OpenAI provider header rules execution order
#[tokio::test]
async fn openai_header_rules() {
    let mock = OpenAIMock::new("test")
        .with_models(vec!["gpt-4".to_string()])
        .with_response("test", "response");

    // Get the header recorder before spawning the mock
    let header_recorder = mock.header_recorder();

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let config = indoc! {r#"
        [llm]
        enabled = true
        
        # Additional header rules for the provider
        [[llm.providers.test.headers]]
        rule = "forward"
        pattern = "^X-Test-.*"
        
        [[llm.providers.test.headers]]
        rule = "remove"
        name = "X-Test-Secret"
        
        [[llm.providers.test.headers]]
        rule = "insert"
        name = "X-Custom"
        value = "custom-value"
    "#};

    let server = builder.build(config).await;

    // Make request with custom headers
    let response = server
        .client
        .request(reqwest::Method::POST, "/llm/v1/chat/completions")
        .header("Content-Type", "application/json")
        .header("X-Test-Public", "public-value")
        .header("X-Test-Secret", "secret-value")
        .header("X-Test-Data", "data-value")
        .header("X-Not-Forwarded", "should-not-appear")
        .json(&json!({
            "model": "test/gpt-4",
            "messages": [{"role": "user", "content": "test"}],
            "max_tokens": 10
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    // Get captured headers, filtering out standard HTTP headers
    let headers = header_recorder.captured_headers();

    // Should have forwarded X-Test-Public and X-Test-Data but not X-Test-Secret
    // Should have inserted X-Custom
    // Should not have X-Not-Forwarded
    insta::assert_debug_snapshot!(headers, @r#"
    [
        (
            "x-test-public",
            "public-value",
        ),
        (
            "x-test-data",
            "data-value",
        ),
        (
            "x-custom",
            "custom-value",
        ),
    ]
    "#);
}

/// Test Anthropic provider header rules
#[tokio::test]
async fn anthropic_header_rules() {
    let mock = AnthropicMock::new("claude")
        .with_models(vec!["claude-3-sonnet".to_string()])
        .with_response("test", "response");

    // Get the header recorder before spawning the mock
    let header_recorder = mock.header_recorder();

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let config = indoc! {r#"
        [llm]
        enabled = true
        
        # Header rules for Anthropic
        [[llm.providers.claude.headers]]
        rule = "insert"
        name = "X-Environment"
        value = "testing"
        
        [[llm.providers.claude.headers]]
        rule = "forward"
        name = "X-Request-ID"
        
        [[llm.providers.claude.headers]]
        rule = "rename_duplicate"
        name = "X-Original"
        rename = "X-Renamed"
    "#};

    let server = builder.build(config).await;

    // Make request with custom headers
    let response = server
        .client
        .request(reqwest::Method::POST, "/llm/v1/chat/completions")
        .header("Content-Type", "application/json")
        .header("X-Request-ID", "req-123")
        .header("X-Original", "original-value")
        .header("X-Other", "other-value")
        .json(&json!({
            "model": "claude/claude-3-sonnet",
            "messages": [{"role": "user", "content": "test"}],
            "max_tokens": 10
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    // Get captured headers, filtering out standard HTTP headers
    let headers = header_recorder.captured_headers();

    // Should have X-Environment (inserted), X-Request-ID (forwarded),
    // both X-Original and X-Renamed (rename_duplicate)
    // Should not have X-Other (not forwarded)
    insta::assert_debug_snapshot!(headers, @r#"
    [
        (
            "x-environment",
            "testing",
        ),
        (
            "x-request-id",
            "req-123",
        ),
        (
            "x-original",
            "original-value",
        ),
        (
            "x-renamed",
            "original-value",
        ),
    ]
    "#);
}

/// Test Google provider header rules
#[tokio::test]
async fn google_header_rules() {
    let mock = GoogleMock::new("gemini")
        .with_models(vec!["gemini-pro".to_string()])
        .with_response("test", "response");

    // Get the header recorder before spawning the mock
    let header_recorder = mock.header_recorder();

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let config = indoc! {r#"
        [llm]
        enabled = true
        
        # Header rules for Google
        [[llm.providers.gemini.headers]]
        rule = "forward"
        pattern = "^X-Trace-.*"
        
        [[llm.providers.gemini.headers]]
        rule = "insert"
        name = "X-API-Version"
        value = "v1beta"
        
        [[llm.providers.gemini.headers]]
        rule = "remove"
        name = "X-Trace-Secret"
    "#};

    let server = builder.build(config).await;

    // Make request with custom headers
    let response = server
        .client
        .request(reqwest::Method::POST, "/llm/v1/chat/completions")
        .header("Content-Type", "application/json")
        .header("X-Trace-ID", "trace-123")
        .header("X-Trace-Parent", "parent-456")
        .header("X-Trace-Secret", "secret-789")
        .header("X-Debug", "debug-value")
        .json(&json!({
            "model": "gemini/gemini-pro",
            "messages": [{"role": "user", "content": "test"}],
            "max_tokens": 10
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    // Get captured headers, filtering out standard HTTP headers
    let headers = header_recorder.captured_headers();

    // Should have X-Trace-ID and X-Trace-Parent (forwarded by pattern)
    // Should not have X-Trace-Secret (removed after forward)
    // Should have X-API-Version (inserted)
    // Should not have X-Debug (not forwarded)
    insta::assert_debug_snapshot!(headers, @r#"
    [
        (
            "x-trace-id",
            "trace-123",
        ),
        (
            "x-trace-parent",
            "parent-456",
        ),
        (
            "x-api-version",
            "v1beta",
        ),
    ]
    "#);
}

/// Test model-level header rules override provider-level rules
#[tokio::test]
async fn model_level_headers_override() {
    let mock = OpenAIMock::new("ai")
        .with_models(vec!["fast".to_string(), "smart".to_string()])
        .with_response("test", "response");

    // Get the header recorder before spawning the mock
    let header_recorder = mock.header_recorder();

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let config = indoc! {r#"
        [llm]
        enabled = true
        
        # Provider-level headers
        [[llm.providers.ai.headers]]
        rule = "insert"
        name = "X-Provider-Header"
        value = "provider-value"
        
        [[llm.providers.ai.headers]]
        rule = "forward"
        name = "X-Request-ID"
        
        # Model configs (required)
        [llm.providers.ai.models."fast"]
        
        [llm.providers.ai.models."smart"]
        
        # Model-level headers for "smart" model
        [[llm.providers.ai.models."smart".headers]]
        rule = "insert"
        name = "X-Model-Header"
        value = "smart-model-value"
        
        [[llm.providers.ai.models."smart".headers]]
        rule = "remove"
        name = "X-Request-ID"
    "#};

    let server = builder.build(config).await;

    // Test with "smart" model - should have model-level headers
    let response = server
        .client
        .request(reqwest::Method::POST, "/llm/v1/chat/completions")
        .header("Content-Type", "application/json")
        .header("X-Request-ID", "req-999")
        .json(&json!({
            "model": "ai/smart",
            "messages": [{"role": "user", "content": "test"}],
            "max_tokens": 10
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    // Get captured headers for smart model
    let headers = header_recorder.captured_headers();

    // Should have both provider and model headers
    // But X-Request-ID should be removed by model-level rule
    insta::assert_debug_snapshot!(headers, @r#"
    [
        (
            "x-provider-header",
            "provider-value",
        ),
        (
            "x-model-header",
            "smart-model-value",
        ),
    ]
    "#);

    // Test with "fast" model - should only have provider-level headers
    let response = server
        .client
        .request(reqwest::Method::POST, "/llm/v1/chat/completions")
        .header("Content-Type", "application/json")
        .header("X-Request-ID", "req-888")
        .json(&json!({
            "model": "ai/fast",
            "messages": [{"role": "user", "content": "test"}],
            "max_tokens": 10
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    // Get captured headers for fast model and filter out dynamic ones
    let headers = header_recorder.captured_headers();

    // Should only have provider-level headers
    // X-Request-ID should be forwarded (no model-level override)
    insta::assert_debug_snapshot!(headers, @r#"
    [
        (
            "x-provider-header",
            "provider-value",
        ),
        (
            "x-request-id",
            "req-888",
        ),
    ]
    "#);
}

// Tests from Grafbase gateway implementation
// Based on https://github.com/grafbase/grafbase/blob/main/crates/integration-tests/tests/gateway/basic/headers.rs

/// Test header forwarding with default value when header is missing
#[tokio::test]
async fn header_forwarding_with_default() {
    let mock = OpenAIMock::new("test")
        .with_models(vec!["gpt-4".to_string()])
        .with_response("test", "response");

    // Get the header recorder before spawning the mock
    let header_recorder = mock.header_recorder();

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let config = indoc! {r#"
        [llm]
        enabled = true
        
        [[llm.providers.test.headers]]
        rule = "forward"
        name = "X-Request-ID"
        default = "default-request-id"
    "#};

    let server = builder.build(config).await;

    // Send request WITHOUT X-Request-ID header
    let response = server
        .client
        .request(reqwest::Method::POST, "/llm/v1/chat/completions")
        .header("Content-Type", "application/json")
        .json(&json!({
            "model": "test/gpt-4",
            "messages": [{"role": "user", "content": "test"}],
            "max_tokens": 10
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    // Filter out dynamic headers
    let headers = header_recorder.captured_headers();

    // Should have X-Request-ID with default value
    insta::assert_debug_snapshot!(headers, @r#"
    [
        (
            "x-request-id",
            "default-request-id",
        ),
    ]
    "#);
}

/// Test header forwarding with default value when header exists
#[tokio::test]
async fn header_forwarding_with_default_and_existing_header() {
    let mock = OpenAIMock::new("test")
        .with_models(vec!["gpt-4".to_string()])
        .with_response("test", "response");

    // Get the header recorder before spawning the mock
    let header_recorder = mock.header_recorder();

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let config = indoc! {r#"
        [llm]
        enabled = true
        
        [[llm.providers.test.headers]]
        rule = "forward"
        name = "X-Request-ID"
        default = "default-request-id"
    "#};

    let server = builder.build(config).await;

    // Send request WITH X-Request-ID header
    let response = server
        .client
        .request(reqwest::Method::POST, "/llm/v1/chat/completions")
        .header("Content-Type", "application/json")
        .header("X-Request-ID", "actual-request-id")
        .json(&json!({
            "model": "test/gpt-4",
            "messages": [{"role": "user", "content": "test"}],
            "max_tokens": 10
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let headers = header_recorder.captured_headers();

    // Should have X-Request-ID with actual value (not default)
    insta::assert_debug_snapshot!(headers, @r#"
    [
        (
            "x-request-id",
            "actual-request-id",
        ),
    ]
    "#);
}

/// Test that regex forward + explicit forward doesn't duplicate headers
#[tokio::test]
async fn regex_header_forwarding_should_not_duplicate() {
    let mock = OpenAIMock::new("test")
        .with_models(vec!["gpt-4".to_string()])
        .with_response("test", "response");

    // Get the header recorder before spawning the mock
    let header_recorder = mock.header_recorder();

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let config = indoc! {r#"
        [llm]
        enabled = true
        
        # Forward all X- headers
        [[llm.providers.test.headers]]
        rule = "forward"
        pattern = "^[Xx]-.*"
        
        # Also explicitly forward and rename X-Source
        [[llm.providers.test.headers]]
        rule = "forward"
        name = "X-Source"
        rename = "Y-Source"
    "#};

    let server = builder.build(config).await;

    let response = server
        .client
        .request(reqwest::Method::POST, "/llm/v1/chat/completions")
        .header("Content-Type", "application/json")
        .header("X-Source", "test-source")
        .header("X-Other", "other-value")
        .json(&json!({
            "model": "test/gpt-4",
            "messages": [{"role": "user", "content": "test"}],
            "max_tokens": 10
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let headers = header_recorder.captured_headers();

    // Should have Y-Source (renamed) but NOT X-Source (avoided duplication)
    // Should have X-Other from regex pattern
    insta::assert_debug_snapshot!(headers, @r#"
    [
        (
            "x-other",
            "other-value",
        ),
        (
            "y-source",
            "test-source",
        ),
    ]
    "#);
}

/// Test forward with regex then remove specific header
#[tokio::test]
async fn regex_header_forwarding_then_delete() {
    let mock = AnthropicMock::new("claude")
        .with_models(vec!["claude-3".to_string()])
        .with_response("test", "response");

    // Get the header recorder before spawning the mock
    let header_recorder = mock.header_recorder();

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let config = indoc! {r#"
        [llm]
        enabled = true
        
        # Forward all X- headers
        [[llm.providers.claude.headers]]
        rule = "forward"
        pattern = "^[Xx]-.*"
        
        # Then remove specific ones
        [[llm.providers.claude.headers]]
        rule = "remove"
        name = "X-Secret"
        
        [[llm.providers.claude.headers]]
        rule = "remove"
        name = "X-Internal"
    "#};

    let server = builder.build(config).await;

    let response = server
        .client
        .request(reqwest::Method::POST, "/llm/v1/chat/completions")
        .header("Content-Type", "application/json")
        .header("X-Public", "public-value")
        .header("X-Secret", "secret-value")
        .header("X-Internal", "internal-value")
        .header("X-Allowed", "allowed-value")
        .json(&json!({
            "model": "claude/claude-3",
            "messages": [{"role": "user", "content": "test"}],
            "max_tokens": 10
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let headers = header_recorder.captured_headers();

    // Should have X-Public and X-Allowed but not X-Secret or X-Internal
    insta::assert_debug_snapshot!(headers, @r#"
    [
        (
            "x-public",
            "public-value",
        ),
        (
            "x-allowed",
            "allowed-value",
        ),
    ]
    "#);
}

/// Test forward with regex then remove with another regex
#[tokio::test]
async fn regex_header_forwarding_then_delete_with_regex() {
    let mock = OpenAIMock::new("test")
        .with_models(vec!["gpt-4".to_string()])
        .with_response("test", "response");

    // Get the header recorder before spawning the mock
    let header_recorder = mock.header_recorder();

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let config = indoc! {r#"
        [llm]
        enabled = true
        
        # Forward all X- headers
        [[llm.providers.test.headers]]
        rule = "forward"
        pattern = "^[Xx]-.*"
        
        # Then remove headers matching pattern
        [[llm.providers.test.headers]]
        rule = "remove"
        pattern = "^[Xx]-[Ss]ecret-.*"
    "#};

    let server = builder.build(config).await;

    let response = server
        .client
        .request(reqwest::Method::POST, "/llm/v1/chat/completions")
        .header("Content-Type", "application/json")
        .header("X-Public", "public")
        .header("X-Secret-Key", "secret1")
        .header("X-Secret-Token", "secret2")
        .header("X-secret-data", "secret3") // lowercase
        .header("X-Safe", "safe")
        .json(&json!({
            "model": "test/gpt-4",
            "messages": [{"role": "user", "content": "test"}],
            "max_tokens": 10
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let headers = header_recorder.captured_headers();

    // Should have X-Public and X-Safe but no X-Secret-* headers
    insta::assert_debug_snapshot!(headers, @r#"
    [
        (
            "x-public",
            "public",
        ),
        (
            "x-safe",
            "safe",
        ),
    ]
    "#);
}

/// Test rename_duplicate without default value
#[tokio::test]
async fn rename_duplicate_no_default() {
    let mock = OpenAIMock::new("test")
        .with_models(vec!["gpt-4".to_string()])
        .with_response("test", "response");

    // Get the header recorder before spawning the mock
    let header_recorder = mock.header_recorder();

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let config = indoc! {r#"
        [llm]
        enabled = true
        
        [[llm.providers.test.headers]]
        rule = "rename_duplicate"
        name = "X-Original"
        rename = "X-Duplicate"
    "#};

    let server = builder.build(config).await;

    // Test with header present
    let response = server
        .client
        .request(reqwest::Method::POST, "/llm/v1/chat/completions")
        .header("Content-Type", "application/json")
        .header("X-Original", "original-value")
        .json(&json!({
            "model": "test/gpt-4",
            "messages": [{"role": "user", "content": "test"}],
            "max_tokens": 10
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let headers = header_recorder.captured_headers();

    // Should have both X-Original and X-Duplicate with same value
    insta::assert_debug_snapshot!(headers, @r#"
    [
        (
            "x-original",
            "original-value",
        ),
        (
            "x-duplicate",
            "original-value",
        ),
    ]
    "#);
}

/// Test rename_duplicate with default value when original missing
#[tokio::test]
async fn rename_duplicate_default() {
    let mock = AnthropicMock::new("claude")
        .with_models(vec!["claude-3".to_string()])
        .with_response("test", "response");

    // Get the header recorder before spawning the mock
    let header_recorder = mock.header_recorder();

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let config = indoc! {r#"
        [llm]
        enabled = true
        
        [[llm.providers.claude.headers]]
        rule = "rename_duplicate"
        name = "X-Original"
        rename = "X-Duplicate"
        default = "default-value"
    "#};

    let server = builder.build(config).await;

    // Test WITHOUT original header
    let response = server
        .client
        .request(reqwest::Method::POST, "/llm/v1/chat/completions")
        .header("Content-Type", "application/json")
        .json(&json!({
            "model": "claude/claude-3",
            "messages": [{"role": "user", "content": "test"}],
            "max_tokens": 10
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let headers = header_recorder.captured_headers();

    // Should have both headers with default value
    insta::assert_debug_snapshot!(headers, @r#"
    [
        (
            "x-original",
            "default-value",
        ),
        (
            "x-duplicate",
            "default-value",
        ),
    ]
    "#);
}

/// Test rename_duplicate with default value behavior
#[tokio::test]
async fn rename_duplicate_default_with_existing_value() {
    let mock = OpenAIMock::new("test")
        .with_models(vec!["gpt-4".to_string()])
        .with_response("test", "response");

    // Get the header recorder before spawning the mock
    let header_recorder = mock.header_recorder();

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let config = indoc! {r#"
        [llm]
        enabled = true
        
        [[llm.providers.test.headers]]
        rule = "rename_duplicate"
        name = "X-Original"
        rename = "X-Duplicate"
        default = "default-value"
    "#};

    let server = builder.build(config).await;

    // Test WITH original header (should override default)
    let response = server
        .client
        .request(reqwest::Method::POST, "/llm/v1/chat/completions")
        .header("Content-Type", "application/json")
        .header("X-Original", "actual-value")
        .json(&json!({
            "model": "test/gpt-4",
            "messages": [{"role": "user", "content": "test"}],
            "max_tokens": 10
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let headers = header_recorder.captured_headers();

    // Should have both headers with actual value (not default)
    insta::assert_debug_snapshot!(headers, @r#"
    [
        (
            "x-original",
            "actual-value",
        ),
        (
            "x-duplicate",
            "actual-value",
        ),
    ]
    "#);
}
