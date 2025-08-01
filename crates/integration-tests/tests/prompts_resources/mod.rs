use indoc::indoc;
use integration_tests::{TestServer, TestService};
use rmcp::model::{Prompt, Resource};

#[tokio::test]
async fn test_prompts_aggregation() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    // Create downstream servers with prompts
    let mut prompt_server = TestService::sse("prompt_server".to_string());

    // Add test prompts
    let prompt1 = Prompt {
        name: "test_prompt".to_string(),
        description: Some("A test prompt".to_string()),
        arguments: None,
    };
    prompt_server.add_prompt(prompt1);

    let prompt2 = Prompt {
        name: "another_prompt".to_string(),
        description: Some("Another test prompt".to_string()),
        arguments: None,
    };
    prompt_server.add_prompt(prompt2);

    // Build nexus server with downstream server
    let mut builder = TestServer::builder();
    builder.spawn_service(prompt_server).await;
    let server = builder.build(config).await;

    let mcp_client = server.mcp_client("/mcp").await;

    // Test listing prompts
    let prompts_result = mcp_client.list_prompts().await;
    insta::assert_json_snapshot!(prompts_result, @r###"
    {
      "prompts": [
        {
          "name": "prompt_server__another_prompt",
          "description": "Another test prompt"
        },
        {
          "name": "prompt_server__test_prompt",
          "description": "A test prompt"
        }
      ]
    }
    "###);

    // Test getting a specific prompt
    let prompt_result = mcp_client.get_prompt("prompt_server__test_prompt", None).await;
    insta::assert_json_snapshot!(prompt_result, @r###"
    {
      "description": "Test prompt: test_prompt",
      "messages": [
        {
          "role": "user",
          "content": {
            "type": "text",
            "text": "This is a test prompt named test_prompt"
          }
        }
      ]
    }
    "###);

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn test_resources_aggregation() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    // Create downstream servers with resources
    let mut resource_server = TestService::sse("resource_server".to_string());

    // Add test resources - create using the new method
    let resource1 = Resource::new(
        rmcp::model::RawResource {
            uri: "file://test.txt".to_string(),
            name: "Test File".to_string(),
            description: Some("A test file resource".to_string()),
            mime_type: Some("text/plain".to_string()),
            size: None,
        },
        None,
    );
    resource_server.add_resource(resource1);

    let resource2 = Resource::new(
        rmcp::model::RawResource {
            uri: "http://example.com/data.json".to_string(),
            name: "Example Data".to_string(),
            description: Some("Example JSON data".to_string()),
            mime_type: Some("application/json".to_string()),
            size: None,
        },
        None,
    );
    resource_server.add_resource(resource2);

    // Build nexus server with downstream server
    let mut builder = TestServer::builder();
    builder.spawn_service(resource_server).await;
    let server = builder.build(config).await;

    let mcp_client = server.mcp_client("/mcp").await;

    // Test listing resources
    let resources_result = mcp_client.list_resources().await;
    insta::assert_json_snapshot!(resources_result, @r#"
    {
      "resources": [
        {
          "uri": "file://test.txt",
          "name": "Test File",
          "description": "A test file resource",
          "mimeType": "text/plain"
        },
        {
          "uri": "http://example.com/data.json",
          "name": "Example Data",
          "description": "Example JSON data",
          "mimeType": "application/json"
        }
      ]
    }
    "#);

    // Test reading a specific resource
    let resource_result = mcp_client.read_resource("file://test.txt").await;
    insta::assert_json_snapshot!(resource_result, @r###"
    {
      "contents": []
    }
    "###);

    mcp_client.disconnect().await;
}

#[tokio::test]
async fn test_multiple_servers_prompts_resources() {
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    // Create multiple downstream servers
    let mut server1 = TestService::sse("server1".to_string());
    let mut server2 = TestService::streamable_http("server2".to_string());

    // Add prompts to server1
    let prompt = Prompt {
        name: "server1_prompt".to_string(),
        description: Some("Prompt from server1".to_string()),
        arguments: None,
    };
    server1.add_prompt(prompt);

    // Add resources to server2
    let resource = Resource::new(
        rmcp::model::RawResource {
            uri: "data://test".to_string(),
            name: "Test Data".to_string(),
            description: Some("Test data resource".to_string()),
            mime_type: Some("application/octet-stream".to_string()),
            size: None,
        },
        None,
    );
    server2.add_resource(resource);

    // Build nexus server with both servers
    let mut builder = TestServer::builder();
    builder.spawn_service(server1).await;
    builder.spawn_service(server2).await;
    let server = builder.build(config).await;

    let mcp_client = server.mcp_client("/mcp").await;

    // Test that prompts from server1 are available
    let prompts_result = mcp_client.list_prompts().await;
    insta::assert_json_snapshot!(prompts_result, @r###"
    {
      "prompts": [
        {
          "name": "server1__server1_prompt",
          "description": "Prompt from server1"
        }
      ]
    }
    "###);

    // Test that resources from server2 are available
    let resources_result = mcp_client.list_resources().await;
    insta::assert_json_snapshot!(resources_result, @r#"
    {
      "resources": [
        {
          "uri": "data://test",
          "name": "Test Data",
          "description": "Test data resource",
          "mimeType": "application/octet-stream"
        }
      ]
    }
    "#);

    mcp_client.disconnect().await;
}
