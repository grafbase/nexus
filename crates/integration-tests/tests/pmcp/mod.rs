//! Tests for PMCP client functionality with Nexus server

use indoc::indoc;
use integration_tests::TestServer;

#[tokio::test]
async fn pmcp_client_can_list_tools() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
        path = "/mcp"

        # Dummy server to satisfy validation
        [mcp.servers.dummy]
        cmd = ["echo", "dummy"]
    "#};

    let server = TestServer::builder().build(config).await;

    // Use pmcp client instead of rmcp
    let mut mcp_client = server.pmcp_client("/mcp").await;

    let tools_result = mcp_client.list_tools().await;
    
    // Should have search and execute tools even with no downstream servers
    assert_eq!(tools_result.tools.len(), 2);
    assert!(tools_result.tools.iter().any(|t| t.name == "search"));
    assert!(tools_result.tools.iter().any(|t| t.name == "execute"));

    println!("✓ PMCP client successfully connected and listed tools");
}

#[tokio::test]
async fn pmcp_client_can_search_tools() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
        path = "/mcp"

        # Dummy server to satisfy validation
        [mcp.servers.dummy]
        cmd = ["echo", "dummy"]
    "#};

    let server = TestServer::builder().build(config).await;
    let mut mcp_client = server.pmcp_client("/mcp").await;

    // Search should work even with no downstream tools
    let search_result = mcp_client.search(&["search"]).await;
    
    // Should return empty list since no downstream servers are configured
    assert!(search_result.is_empty());

    println!("✓ PMCP client can perform search");
}