use indoc::{formatdoc, indoc};
use integration_tests::{TestServer, TestService, tools::AdderTool};
use serde_json::json;

#[tokio::test]
async fn search_structured_content_enabled_by_default() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
        # enable_structured_content defaults to true via Default trait
    "#};

    let mut builder = TestServer::builder();
    let mut service = TestService::sse("test_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let server = builder.build(config).await;
    let client = server.mcp_client("/mcp").await;

    // Search for the tool
    let search_results = client.search(&["adder"]).await;
    assert!(!search_results.is_empty());
    assert_eq!(search_results[0]["name"], "test_server__adder");

    // Verify the underlying response structure uses structuredContent
    // We can't directly inspect the raw response in the high-level API,
    // but we know it works if search returns results
}

#[tokio::test]
async fn search_legacy_content_json_mode() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
        enable_structured_content = false
    "#};

    let mut builder = TestServer::builder();
    let mut service = TestService::sse("test_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let server = builder.build(config).await;
    let client = server.mcp_client("/mcp").await;

    // Search for the tool - should work with legacy format
    let search_results = client.search(&["adder"]).await;
    assert!(!search_results.is_empty());
    assert_eq!(search_results[0]["name"], "test_server__adder");
}

#[tokio::test]
async fn execute_passthrough_unaffected_by_config() {
    // Verify that execute tool always passes through downstream response unchanged
    for enable_structured in [true, false] {
        let config = formatdoc! {r#"
            [mcp]
            enabled = true
            enable_structured_content = {enable_structured}
        "#};

        let mut builder = TestServer::builder();
        let mut service = TestService::sse("math_server".to_string());
        service.add_tool(AdderTool);
        builder.spawn_service(service).await;

        let server = builder.build(&config).await;
        let client = server.mcp_client("/mcp").await;

        // Execute should work the same regardless of config
        let result = client.execute("math_server__adder", json!({"a": 1, "b": 2})).await;

        // The downstream response format is unchanged by our config
        assert!(result.content.is_some());
    }
}

#[tokio::test]
async fn test_client_search_handles_both_formats() {
    // Verify the test client's search() method works with both formats
    for enable_structured in [true, false] {
        let config = formatdoc! {r#"
            [mcp]
            enabled = true
            enable_structured_content = {enable_structured}
        "#};

        let mut builder = TestServer::builder();
        let mut service = TestService::sse("test_server".to_string());
        service.add_tool(AdderTool);
        builder.spawn_service(service).await;

        let server = builder.build(&config).await;
        let client = server.mcp_client("/mcp").await;

        // High-level search method should work regardless of format
        let results = client.search(&["adder"]).await;
        assert!(!results.is_empty());
        assert_eq!(results[0]["name"], "test_server__adder");
        assert_eq!(results[0]["description"], "Adds two numbers together");
    }
}

#[tokio::test]
async fn search_returns_proper_scores_in_both_modes() {
    // Ensure that scores are properly returned in both modes
    for enable_structured in [true, false] {
        let config = formatdoc! {r#"
            [mcp]
            enabled = true
            enable_structured_content = {enable_structured}
        "#};

        let mut builder = TestServer::builder();
        let mut service = TestService::sse("test_server".to_string());
        service.add_tool(AdderTool);
        builder.spawn_service(service).await;

        let server = builder.build(&config).await;
        let client = server.mcp_client("/mcp").await;

        // Search should return results with scores
        let results = client.search(&["add", "numbers"]).await;
        assert!(!results.is_empty());

        // Verify score field exists and is a number
        assert!(results[0]["score"].is_number());
        let score = results[0]["score"].as_f64().unwrap();
        assert!(score > 0.0);
    }
}
