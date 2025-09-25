use axum::http::HeaderMap;
use indoc::indoc;
use integration_tests::TestServer;
use serde_json::json;

/// Test that empty allow denies all access
#[tokio::test]
async fn empty_allow_denies_all() {
    let config = indoc! {r#"
        [mcp]
        enabled = true

        [mcp.servers.restricted]
        cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
        allow = []
    "#};

    let server = TestServer::builder().build(config).await;

    let mut headers = HeaderMap::new();
    headers.insert("X-Client-ID", "user1".parse().unwrap());
    headers.insert("X-Client-Group", "premium".parse().unwrap());
    let client = server.mcp_client_with_headers("/mcp", headers).await;

    // Try to execute a tool from the restricted server
    let result = client
        .execute_expect_error("restricted__echo", json!({"text": "test"}))
        .await;

    // Should get method not found (security: don't leak that tool exists)
    insta::assert_snapshot!(result.to_string(), @"Mcp error: -32601: tools/call");
}

/// Test that allow restricts access to specific groups
#[tokio::test]
async fn allow_restricts_access() {
    let config = indoc! {r#"
        [server.client_identification]
        enabled = true
        client_id.http_header = "X-Client-ID"
        group_id.http_header = "X-Client-Group"

        [server.client_identification.validation]
        group_values = ["premium", "basic", "trial"]

        [mcp]
        enabled = true

        [mcp.servers.premium_only]
        cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
        allow = ["premium"]
    "#};

    let server = TestServer::builder().build(config).await;

    // Premium user can access
    let mut premium_headers = HeaderMap::new();
    premium_headers.insert("X-Client-ID", "premium_user".parse().unwrap());
    premium_headers.insert("X-Client-Group", "premium".parse().unwrap());
    let premium_client = server.mcp_client_with_headers("/mcp", premium_headers).await;

    // Premium user should be able to access the tool
    let _premium_result = premium_client
        .execute("premium_only__echo", json!({"text": "test"}))
        .await;
    // If we get here without error, access was granted

    // Basic user is denied
    let mut basic_headers = HeaderMap::new();
    basic_headers.insert("X-Client-ID", "basic_user".parse().unwrap());
    basic_headers.insert("X-Client-Group", "basic".parse().unwrap());
    let basic_client = server.mcp_client_with_headers("/mcp", basic_headers).await;

    let basic_result = basic_client
        .execute_expect_error("premium_only__echo", json!({"text": "test"}))
        .await;

    // Should get method not found (access denied looks like tool doesn't exist)
    insta::assert_snapshot!(basic_result.to_string(), @"Mcp error: -32601: tools/call");

    // User without group is also denied
    let mut no_group_headers = HeaderMap::new();
    no_group_headers.insert("X-Client-ID", "no_group_user".parse().unwrap());
    let no_group_client = server.mcp_client_with_headers("/mcp", no_group_headers).await;

    let no_group_result = no_group_client
        .execute_expect_error("premium_only__echo", json!({"text": "test"}))
        .await;

    // Should get method not found (no group means no access to restricted servers)
    insta::assert_snapshot!(no_group_result.to_string(), @"Mcp error: -32601: tools/call");
}

/// Test that deny blocks specific groups
#[tokio::test]
async fn deny_blocks_specific_groups() {
    let config = indoc! {r#"
        [server.client_identification]
        enabled = true
        client_id.http_header = "X-Client-ID"
        group_id.http_header = "X-Client-Group"

        [server.client_identification.validation]
        group_values = ["premium", "suspended", "trial"]

        [mcp]
        enabled = true

        [mcp.servers.no_suspended]
        cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
        deny = ["suspended"]
    "#};

    let server = TestServer::builder().build(config).await;

    // Premium user can access
    let mut premium_headers = HeaderMap::new();
    premium_headers.insert("X-Client-ID", "premium_user".parse().unwrap());
    premium_headers.insert("X-Client-Group", "premium".parse().unwrap());
    let premium_client = server.mcp_client_with_headers("/mcp", premium_headers).await;

    // Premium user should be able to access the tool (not in deny list)
    let _premium_result = premium_client
        .execute("no_suspended__echo", json!({"text": "test"}))
        .await;
    // If we get here without error, access was granted

    // Suspended user is denied
    let mut suspended_headers = HeaderMap::new();
    suspended_headers.insert("X-Client-ID", "suspended_user".parse().unwrap());
    suspended_headers.insert("X-Client-Group", "suspended".parse().unwrap());
    let suspended_client = server.mcp_client_with_headers("/mcp", suspended_headers).await;

    let suspended_result = suspended_client
        .execute_expect_error("no_suspended__echo", json!({"text": "test"}))
        .await;

    // Should get method not found (access denied looks like tool doesn't exist)
    insta::assert_snapshot!(suspended_result.to_string(), @"Mcp error: -32601: tools/call");
}

/// Test that tool-level access control overrides server-level
#[tokio::test]
async fn tool_level_overrides_server_level() {
    let config = indoc! {r#"
        [server.client_identification]
        enabled = true
        client_id.http_header = "X-Client-ID"
        group_id.http_header = "X-Client-Group"

        [server.client_identification.validation]
        group_values = ["basic", "premium"]

        [mcp]
        enabled = true

        [mcp.servers.mixed]
        cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
        allow = ["basic"]

        [mcp.servers.mixed.tools.echo]
        allow = ["premium"]
    "#};

    let server = TestServer::builder().build(config).await;

    // Basic user can access regular tools but not echo
    let mut basic_headers = HeaderMap::new();
    basic_headers.insert("X-Client-ID", "basic_user".parse().unwrap());
    basic_headers.insert("X-Client-Group", "basic".parse().unwrap());
    let basic_client = server.mcp_client_with_headers("/mcp", basic_headers).await;

    // Basic can access other tools (server-level allows basic)
    let _basic_math = basic_client.execute("mixed__add", json!({"a": 1, "b": 2})).await;
    // If we get here without error, access was granted

    // Basic cannot access echo (overridden to premium only)
    let basic_echo = basic_client
        .execute_expect_error("mixed__echo", json!({"text": "test"}))
        .await;

    // Should get method not found for tool-level restriction
    insta::assert_snapshot!(basic_echo.to_string(), @"Mcp error: -32601: tools/call");

    // Premium user can access echo but not other tools
    let mut premium_headers = HeaderMap::new();
    premium_headers.insert("X-Client-ID", "premium_user".parse().unwrap());
    premium_headers.insert("X-Client-Group", "premium".parse().unwrap());
    let premium_client = server.mcp_client_with_headers("/mcp", premium_headers).await;

    // Premium can access echo (tool-level override allows premium)
    premium_client.execute("mixed__echo", json!({"text": "test"})).await;

    // Premium cannot access other tools (server-level restriction allows only basic)
    let premium_math = premium_client
        .execute_expect_error("mixed__add", json!({"a": 1, "b": 2}))
        .await;

    // Should get method not found for server-level restriction
    insta::assert_snapshot!(premium_math.to_string(), @"Mcp error: -32601: tools/call");
}

/// Test that allow and deny groups work together
#[tokio::test]
async fn allow_and_deny_combined() {
    let config = indoc! {r#"
        [server.client_identification]
        enabled = true
        client_id.http_header = "X-Client-ID"
        group_id.http_header = "X-Client-Group"

        [server.client_identification.validation]
        group_values = ["premium", "suspended_premium", "basic"]

        [mcp]
        enabled = true

        [mcp.servers.premium_not_suspended]
        cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
        allow = ["premium", "suspended_premium"]
        deny = ["suspended_premium"]
    "#};

    let server = TestServer::builder().build(config).await;

    // Premium user can access
    let mut premium_headers = HeaderMap::new();
    premium_headers.insert("X-Client-ID", "premium_user".parse().unwrap());
    premium_headers.insert("X-Client-Group", "premium".parse().unwrap());
    let premium_client = server.mcp_client_with_headers("/mcp", premium_headers).await;

    // Premium user is in allow list and not in deny list, so access granted
    let _premium_result = premium_client
        .execute("premium_not_suspended__echo", json!({"text": "test"}))
        .await;
    // If we get here without error, access was granted

    // Suspended premium user is denied (deny takes precedence)
    let mut suspended_headers = HeaderMap::new();
    suspended_headers.insert("X-Client-ID", "suspended_user".parse().unwrap());
    suspended_headers.insert("X-Client-Group", "suspended_premium".parse().unwrap());
    let suspended_client = server.mcp_client_with_headers("/mcp", suspended_headers).await;

    let suspended_result = suspended_client
        .execute_expect_error("premium_not_suspended__echo", json!({"text": "test"}))
        .await;

    // Should get method not found (deny list takes precedence)
    insta::assert_snapshot!(suspended_result.to_string(), @"Mcp error: -32601: tools/call");
}
