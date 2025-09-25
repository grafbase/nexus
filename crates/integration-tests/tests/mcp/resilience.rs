use indoc::indoc;
use integration_tests::TestServer;

#[tokio::test]
async fn startup_with_two_failing_servers() {
    // Configuration with two servers that will both fail
    // (cat is not a valid MCP server, and the other doesn't exist)
    let config = indoc! {r#"
        [mcp]
        path = "/mcp"
        
        [mcp.servers.invalid_mcp_server]
        cmd = ["cat"]
        
        [mcp.servers.nonexistent_server]
        cmd = ["/nonexistent/binary/that/will/fail"]
    "#};

    // Server should start successfully despite both downstreams failing
    let server = TestServer::builder().build(config).await;

    // Create MCP client and verify server is running
    let mcp_client = server.mcp_client("/mcp").await;
    let response = mcp_client.list_tools().await;

    // Snapshot the full tools response to verify exactly 2 tools (search and execute)
    let tool_names: Vec<&str> = response.tools.iter().map(|t| t.name.as_ref()).collect();
    insta::assert_debug_snapshot!(tool_names, @r###"
    [
        "search",
        "execute",
    ]
    "###);

    // Search should return empty results since both servers failed to initialize
    let search_results = mcp_client.search(&["test"]).await;
    insta::assert_json_snapshot!(search_results, @"[]");
}

#[tokio::test]
async fn startup_with_all_servers_failing() {
    // Configuration with only invalid servers
    let config = indoc! {r#"
        [mcp]
        path = "/mcp"
        
        [mcp.servers.failing_server_1]
        cmd = ["/nonexistent/binary/one"]
        
        [mcp.servers.failing_server_2]
        cmd = ["/nonexistent/binary/two"]
    "#};

    // Server should still start even with all downstreams failing
    let server = TestServer::builder().build(config).await;

    // Create MCP client and verify server is running
    let mcp_client = server.mcp_client("/mcp").await;
    let response = mcp_client.list_tools().await;

    // Snapshot the tools list - always exactly 2 tools exposed by the router
    let tool_names: Vec<&str> = response.tools.iter().map(|t| t.name.as_ref()).collect();
    insta::assert_debug_snapshot!(tool_names, @r###"
    [
        "search",
        "execute",
    ]
    "###);

    // Search should return empty results
    let search_results = mcp_client.search(&["test"]).await;
    insta::assert_json_snapshot!(search_results, @"[]");
}

#[tokio::test]
async fn startup_with_no_servers_configured() {
    // Configuration with no MCP servers at all - add dummy server for validation
    let config = indoc! {r#"
        [mcp]
        enabled = true
        path = "/mcp"

        # Dummy server to satisfy validation (will fail to start, which is fine)
        [mcp.servers.dummy]
        cmd = ["/nonexistent/dummy"]
    "#};

    // Server should start successfully with no downstreams
    let server = TestServer::builder().build(config).await;

    // Create MCP client and verify server is running
    let mcp_client = server.mcp_client("/mcp").await;
    let response = mcp_client.list_tools().await;

    // Snapshot the full tools list - always exactly 2 tools
    let tool_names: Vec<&str> = response.tools.iter().map(|t| t.name.as_ref()).collect();
    insta::assert_debug_snapshot!(tool_names, @r###"
    [
        "search",
        "execute",
    ]
    "###);

    // Search should also return empty results with no servers
    let search_results = mcp_client.search(&["anything"]).await;
    insta::assert_json_snapshot!(search_results, @"[]");
}

#[tokio::test]
async fn mixed_success_and_failure_servers() {
    use tokio::time::{Duration, sleep};

    // Configuration with one working server and one failing server
    let config = indoc! {r#"
        [mcp]
        path = "/mcp"
        
        [mcp.servers.working_server]
        cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
        
        [mcp.servers.failing_server]
        cmd = ["/nonexistent/binary/that/will/fail"]
    "#};

    // Server should start successfully with partial failures
    let server = TestServer::builder().build(config).await;

    // Create MCP client and verify server is running
    let mcp_client = server.mcp_client("/mcp").await;

    // Wait a bit for STDIO server to be fully ready
    sleep(Duration::from_millis(100)).await;

    // The router always exposes exactly 2 tools regardless of downstream health
    let response = mcp_client.list_tools().await;
    let tool_names: Vec<&str> = response.tools.iter().map(|t| t.name.as_ref()).collect();
    insta::assert_debug_snapshot!(tool_names, @r###"
    [
        "search",
        "execute",
    ]
    "###);

    // Search should return results from the working server only
    let search_results = mcp_client.search(&["echo"]).await;

    // Snapshot the search results - should have the echo tool from working_server
    insta::assert_json_snapshot!(search_results, @r#"
    [
      {
        "name": "working_server__echo",
        "description": "Echoes back the input text message verbatim for testing and debugging purposes",
        "input_schema": {
          "type": "object",
          "properties": {
            "text": {
              "type": "string",
              "description": "Text message to echo back verbatim"
            }
          },
          "required": [
            "text"
          ]
        },
        "score": 3.611918449401855
      }
    ]
    "#);

    // Test that we can execute the tool from the working server
    let result = mcp_client
        .execute(
            "working_server__echo",
            serde_json::json!({
                "text": "Hello from partial startup!"
            }),
        )
        .await;

    insta::assert_json_snapshot!(result, @r###"
    {
      "content": [
        {
          "type": "text",
          "text": "Echo: Hello from partial startup!"
        }
      ]
    }
    "###);

    // Search for something that doesn't exist should be empty
    let empty_results = mcp_client.search(&["nonexistent_tool"]).await;
    insta::assert_json_snapshot!(empty_results, @"[]");
}
