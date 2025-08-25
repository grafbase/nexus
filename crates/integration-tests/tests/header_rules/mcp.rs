//! MCP header rules integration tests using HeaderInspectorTool

use indoc::indoc;
use integration_tests::{TestServer, TestService, tools::HeaderInspectorTool};
use serde_json::json;

/// Test header insertion with static values  
#[tokio::test]
async fn mcp_header_insertion() {
    let config = indoc! {r#"
        [mcp]
        enabled = true

        [[mcp.headers]]
        rule = "insert"
        name = "X-Service-Name"
        value = "nexus-router"

        [[mcp.headers]]
        rule = "insert"
        name = "X-API-Version"
        value = "v1.0"
        
        [[mcp.headers]]
        rule = "insert"
        name = "X-Environment"
        value = "test"

        [[mcp.headers]]
        rule = "insert"
        name = "Authorization"
        value = "Bearer inserted-token"
        
        [[mcp.headers]]
        rule = "insert"
        name = "X-System"
        value = "nexus"

        [[mcp.headers]]
        rule = "insert"
        name = "X-Team"
        value = "platform"
    "#};

    // Create a service with HeaderInspectorTool
    let mut service = TestService::sse("test_service".to_string());
    let inspector = HeaderInspectorTool::new();
    let header_recorder = inspector.header_recorder();
    service.add_tool(inspector);

    let mut builder = TestServer::builder();
    builder.spawn_service(service).await;
    let server = builder.build(config).await;

    let mcp = server.mcp_client("/mcp").await;

    // Execute the header inspector tool to trigger header capture
    let _result = mcp
        .execute("test_service__header_inspector", json!({ "echo": false }))
        .await;

    // Get captured headers using the clean API
    let headers = header_recorder.captured_headers();

    // Verify headers with snapshot - all inserted headers should be present
    insta::assert_debug_snapshot!(headers, @r#"
    [
        (
            "x-service-name",
            "nexus-router",
        ),
        (
            "x-api-version",
            "v1.0",
        ),
        (
            "x-environment",
            "test",
        ),
        (
            "x-system",
            "nexus",
        ),
        (
            "x-team",
            "platform",
        ),
    ]
    "#);

    mcp.disconnect().await;
}

/// Test global headers only (server-specific headers would require manual configuration)
#[tokio::test]
async fn mcp_global_headers_only() {
    let config = indoc! {r#"
        [mcp]
        enabled = true

        # Global headers applied to all MCP servers
        [[mcp.headers]]
        rule = "insert"
        name = "X-Global-Header"
        value = "global-value"

        [[mcp.headers]]
        rule = "insert"
        name = "X-System"
        value = "nexus"
    "#};

    // Create a service with HeaderInspectorTool
    let mut service = TestService::sse("test_service".to_string());
    let inspector = HeaderInspectorTool::new();
    let header_recorder = inspector.header_recorder();
    service.add_tool(inspector);

    let mut builder = TestServer::builder();
    builder.spawn_service(service).await;
    let server = builder.build(config).await;

    let mcp = server.mcp_client("/mcp").await;

    // Execute the header inspector tool to trigger header capture
    let _result = mcp
        .execute("test_service__header_inspector", json!({ "echo": false }))
        .await;

    // Get captured headers using the clean API
    let headers = header_recorder.captured_headers();

    // Verify headers with snapshot - global headers should be present
    insta::assert_debug_snapshot!(headers, @r###"
    [
        (
            "x-global-header",
            "global-value",
        ),
        (
            "x-system",
            "nexus",
        ),
    ]
    "###);

    mcp.disconnect().await;
}

/// Test case-insensitive header names (ASCII validation)
#[tokio::test]
async fn mcp_case_insensitive_headers() {
    let config = indoc! {r#"
        [mcp]
        enabled = true

        [[mcp.headers]]
        rule = "insert"
        name = "X-UPPERCASE-HEADER"
        value = "uppercase-value"

        [[mcp.headers]]
        rule = "insert"
        name = "x-lowercase-header"
        value = "lowercase-value"
    "#};

    // Create a service with HeaderInspectorTool
    let mut service = TestService::sse("case_test".to_string());
    let inspector = HeaderInspectorTool::new();
    let header_recorder = inspector.header_recorder();
    service.add_tool(inspector);

    let mut builder = TestServer::builder();
    builder.spawn_service(service).await;
    let server = builder.build(config).await;

    let mcp = server.mcp_client("/mcp").await;

    // Execute the header inspector tool to trigger header capture
    let _result = mcp
        .execute("case_test__header_inspector", json!({ "echo": false }))
        .await;

    // Get captured headers using the clean API
    let headers = header_recorder.captured_headers();

    // Verify headers with snapshot (HTTP headers are case-insensitive)
    insta::assert_debug_snapshot!(headers, @r###"
    [
        (
            "x-uppercase-header",
            "uppercase-value",
        ),
        (
            "x-lowercase-header",
            "lowercase-value",
        ),
    ]
    "###);

    mcp.disconnect().await;
}

/// Test that only configured headers are inserted (no automatic forwarding)
#[tokio::test]
async fn validate_no_automatic_forwarding() {
    let config = indoc! {r#"
        [mcp]
        enabled = true

        # Only explicit insert rules - no forwarding
        [[mcp.headers]]
        rule = "insert"
        name = "X-Configured"
        value = "configured-value"
    "#};

    // Create a service with header inspector tool
    let mut service = TestService::sse("no_forward_test".to_string());
    let inspector = HeaderInspectorTool::new();
    let header_recorder = inspector.header_recorder();
    service.add_tool(inspector);

    let mut builder = TestServer::builder();
    builder.spawn_service(service).await;
    let server = builder.build(config).await;

    // Send request with headers that should NOT be forwarded automatically
    use reqwest::header::{HeaderMap, HeaderValue};
    let mut headers = HeaderMap::new();
    headers.insert("X-Client-ID", HeaderValue::from_static("client-123"));
    headers.insert("X-Session-Token", HeaderValue::from_static("session-456"));
    headers.insert("Authorization", HeaderValue::from_static("Bearer client-token"));

    let mcp = server.mcp_client_with_headers("/mcp", headers).await;

    // Execute the header inspector tool to trigger header capture
    let _result = mcp
        .execute("no_forward_test__header_inspector", json!({ "echo": false }))
        .await;

    // Get captured headers using the clean API
    let headers = header_recorder.captured_headers();

    // Should only have the configured header, not the client headers
    insta::assert_debug_snapshot!(headers, @r###"
    [
        (
            "x-configured",
            "configured-value",
        ),
    ]
    "###);

    mcp.disconnect().await;
}
