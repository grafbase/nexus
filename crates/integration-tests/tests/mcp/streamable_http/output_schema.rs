use indoc::indoc;
use integration_tests::tools::AdderTool;
use integration_tests::{TestServer, TestService};

#[tokio::test]
async fn search_tool_output_schema_validation() {
    // Test that validates the search tool's output schema is properly formatted
    // to work with MCP Inspector and other validation tools
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let mut test_service = TestService::streamable_http("test_service".to_string());
    test_service.add_tool(AdderTool);

    let mut builder = TestServer::builder();
    builder.spawn_service(test_service).await;
    let server = builder.build(config).await;

    let mcp_client = server.mcp_client("/mcp").await;
    let tools_result = mcp_client.list_tools().await;

    // Find the search tool
    let search_tool = tools_result
        .tools
        .iter()
        .find(|t| t.name == "search")
        .expect("search tool should exist");

    // Verify outputSchema exists and has the correct structure
    let output_schema = search_tool
        .output_schema
        .as_ref()
        .expect("search tool should have outputSchema");

    // Snapshot the outputSchema to ensure:
    // 1. Root type is "object" (not "array") to work around MCP Inspector bug
    // 2. No "$schema" field that MCP Inspector doesn't recognize
    // 3. No "format" fields (like "float" or "double") that cause validation errors
    // 4. Proper $defs structure for SearchResult type reference
    insta::assert_json_snapshot!(output_schema, @r##"
    {
      "$defs": {
        "SearchResult": {
          "properties": {
            "description": {
              "description": "Description of what the tool does",
              "type": "string"
            },
            "input_schema": {
              "description": "The input schema for the tool's parameters"
            },
            "name": {
              "description": "The name of the tool (format: \"server__tool\")",
              "type": "string"
            },
            "score": {
              "description": "The relevance score for this result (higher is more relevant)",
              "type": "number"
            }
          },
          "required": [
            "name",
            "description",
            "input_schema",
            "score"
          ],
          "type": "object"
        }
      },
      "properties": {
        "results": {
          "description": "The list of search results",
          "items": {
            "$ref": "#/$defs/SearchResult"
          },
          "type": "array"
        }
      },
      "required": [
        "results"
      ],
      "title": "SearchResponse",
      "type": "object"
    }
    "##);
}

#[tokio::test]
async fn execute_tool_has_no_output_schema() {
    // The execute tool should not have an outputSchema as it returns dynamic results
    let config = indoc! {r#"
        [mcp]
        enabled = true
    "#};

    let mut test_service = TestService::streamable_http("test_service".to_string());
    test_service.add_tool(AdderTool);

    let mut builder = TestServer::builder();
    builder.spawn_service(test_service).await;
    let server = builder.build(config).await;
    let mcp_client = server.mcp_client("/mcp").await;
    let tools_result = mcp_client.list_tools().await;

    // Find the execute tool
    let execute_tool = tools_result
        .tools
        .iter()
        .find(|t| t.name == "execute")
        .expect("execute tool should exist");

    // Execute tool should not have outputSchema since responses are dynamic
    assert!(
        execute_tool.output_schema.is_none(),
        "execute tool should not have outputSchema"
    );
}
