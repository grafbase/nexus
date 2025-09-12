//! MCP endpoint metrics tests

use indoc::{formatdoc, indoc};
use integration_tests::{
    TestServer, TestService,
    telemetry::*,
    tools::{AdderTool, FailingTool},
};
use reqwest::header::HeaderMap;

use crate::telemetry::metrics::HistogramMetricRow;

// Helper function to create test config with telemetry enabled
fn create_test_config_with_metrics(service_name: &str) -> String {
    formatdoc! {r#"
        [server]
        listen_address = "127.0.0.1:0"

        # Enable client identification for accurate metrics tracking
        [server.client_identification]
        enabled = true
        client_id = {{ source = "http_header", http_header = "x-client-id" }}
        group_id = {{ source = "http_header", http_header = "x-client-group" }}

        [telemetry]
        service_name = "{service_name}"

        [telemetry.resource_attributes]
        environment = "test"

        [telemetry.exporters.otlp]
        enabled = true
        endpoint = "http://localhost:4317"
        protocol = "grpc"

        # Export with reasonable delay to avoid duplication
        [telemetry.exporters.otlp.batch_export]
        scheduled_delay = "1s"
        max_export_batch_size = 100

        [mcp]
        enabled = true
        path = "/mcp"
    "#}
}

#[tokio::test]
async fn http_request_metrics() {
    let service_name = unique_service_name("mcp-http-metrics");
    let config = create_test_config_with_metrics(&service_name);

    let mut builder = TestServer::builder();
    let mut service = TestService::streamable_http("test_mcp_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let test_server = builder.build(&config).await;

    let client_id = format!("test-client-{}", uuid::Uuid::new_v4());
    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", client_id.parse().unwrap());
    headers.insert("x-client-group", "test-group".parse().unwrap());

    let mcp = test_server.mcp_client_with_headers("/mcp", headers).await;

    // Make search requests
    for _ in 0..3 {
        let _results = mcp.search(&["adder"]).await;
    }

    let clickhouse = create_clickhouse_client().await;

    // Build query for duration metrics - filter by service name which is unique per test run
    // Exclude health checks by filtering for POST method only
    let query = formatdoc! {r#"
        SELECT
            MetricName,
            Attributes,
            Count
        FROM otel_metrics_histogram
        WHERE
            MetricName = 'http.server.request.duration'
            AND ServiceName = '{service_name}'
            AND Attributes['http.route'] = '/mcp'
            AND Attributes['http.request.method'] = 'POST'
        ORDER BY TimeUnix DESC
    "#};

    // Wait for HTTP request metrics
    // Expected exactly 5 HTTP requests:
    // - 2 from MCP protocol initialization (initialize request + initialized notification)
    // - 3 from our explicit search requests in the test
    let mcp_histograms = wait_for_metrics_matching::<HistogramMetricRow, _>(&clickhouse, &query, |rows| {
        let mcp_count: u64 = rows.iter().map(|row| row.count).sum();
        mcp_count == 5
    })
    .await
    .expect("Failed to get metrics");

    // Verify HTTP metric attributes contain expected fields
    let first_histogram = &mcp_histograms[0];
    // Expected metric name: standard HTTP server duration metric name per OpenTelemetry conventions
    assert_eq!(first_histogram.metric_name, "http.server.request.duration");

    // Check that we have the expected attributes
    let attrs: std::collections::BTreeMap<_, _> = first_histogram
        .attributes
        .iter()
        .filter(|(k, _)| k.as_str() != "http.response.status_code") // Filter out status code as it varies
        .cloned()
        .collect();

    insta::assert_debug_snapshot!(attrs, @r###"
    {
        "http.request.method": "POST",
        "http.route": "/mcp",
    }
    "###);
}

#[tokio::test]
async fn search_tool_metrics() {
    let service_name = unique_service_name("mcp-search-metrics");
    let config = create_test_config_with_metrics(&service_name);

    let mut builder = TestServer::builder();
    let mut service = TestService::streamable_http("test_mcp_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let test_server = builder.build(&config).await;

    let client_id = format!("test-client-{}", uuid::Uuid::new_v4());
    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", client_id.parse().unwrap());
    headers.insert("x-client-group", "test-group".parse().unwrap());

    let mcp = test_server.mcp_client_with_headers("/mcp", headers).await;

    // Make search requests
    for _ in 0..3 {
        let _results = mcp.search(&["adder"]).await;
    }

    let clickhouse = create_clickhouse_client().await;

    // Check for the mcp.tool.call.duration metric
    // Filter by client_id to ensure we only count metrics from this specific test run
    let tool_query = formatdoc! {r#"
        SELECT
            MetricName,
            Attributes,
            Count
        FROM otel_metrics_histogram
        WHERE
            MetricName = 'mcp.tool.call.duration'
            AND ServiceName = '{service_name}'
            AND Attributes['client.id'] = '{client_id}'
        ORDER BY TimeUnix DESC
    "#};

    // Wait for tool call metrics
    // Expected exactly 3 tool calls: we made 3 explicit search requests in the test
    let tool_histograms = wait_for_metrics_matching::<HistogramMetricRow, _>(&clickhouse, &tool_query, |rows| {
        let total_count: u64 = rows.iter().map(|row| row.count).sum();
        total_count == 3
    })
    .await
    .expect("Failed to get tool call metrics");

    // Verify search metrics - check first row attributes (all should be the same for search)
    let first_row = &tool_histograms[0];
    let attrs: std::collections::BTreeMap<_, _> = first_row
        .attributes
        .iter()
        .filter(|(k, _)| k.as_str() != "client.id") // Filter out dynamic client.id
        .cloned()
        .collect();

    // Expected values:
    // - keyword_count: "1" - we searched with 1 keyword: "adder"
    // - result_count: "1" - exactly 1 tool matches: AdderTool from test_mcp_server
    insta::assert_debug_snapshot!(attrs, @r###"
    {
        "client.group": "test-group",
        "keyword_count": "1",
        "result_count": "1",
        "status": "success",
        "tool_name": "search",
        "tool_type": "builtin",
    }
    "###);
}

#[tokio::test]
async fn execute_tool_metrics() {
    let service_name = unique_service_name("mcp-execute-metrics");
    let config = create_test_config_with_metrics(&service_name);

    let mut builder = TestServer::builder();
    let mut service = TestService::streamable_http("test_mcp_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let test_server = builder.build(&config).await;

    let client_id = format!("test-execute-client-{}", uuid::Uuid::new_v4());
    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", client_id.parse().unwrap());
    headers.insert("x-client-group", "execute-test-group".parse().unwrap());

    let mcp = test_server.mcp_client_with_headers("/mcp", headers).await;

    // Make an execute request
    let execute_result = mcp
        .execute(
            "test_mcp_server__adder",
            serde_json::json!({
                "a": 5,
                "b": 3
            }),
        )
        .await;

    // Verify result
    let result_text = serde_json::to_string(&execute_result.content[0].raw).unwrap();
    assert!(result_text.contains("8"));

    let clickhouse = create_clickhouse_client().await;

    // Query for execute tool metrics
    let tool_query = formatdoc! {r#"
        SELECT
            MetricName,
            Attributes,
            Count
        FROM otel_metrics_histogram
        WHERE
            MetricName = 'mcp.tool.call.duration'
            AND ServiceName = '{service_name}'
            AND Attributes['client.id'] = '{client_id}'
        ORDER BY TimeUnix DESC
    "#};

    // Wait for tool call metrics
    // Expected exactly 1 execute call: we made a single execute request
    let tool_histograms = wait_for_metrics_matching::<HistogramMetricRow, _>(&clickhouse, &tool_query, |rows| {
        let total_count: u64 = rows.iter().map(|row| row.count).sum();
        total_count == 1
    })
    .await
    .expect("Failed to get tool call metrics");

    let attrs: std::collections::BTreeMap<_, _> = tool_histograms[0]
        .attributes
        .iter()
        .filter(|(k, _)| k.as_str() != "client.id") // Filter out dynamic client.id
        .cloned()
        .collect();

    // Use snapshot for static fields
    insta::assert_debug_snapshot!(attrs, @r###"
    {
        "client.group": "execute-test-group",
        "server_name": "test_mcp_server",
        "status": "success",
        "tool_name": "adder",
        "tool_type": "downstream",
    }
    "###);

    // Check dynamic field separately with assert_eq!
    let full_attrs: std::collections::BTreeMap<_, _> = tool_histograms[0].attributes.iter().cloned().collect();
    // Expected: client_id matches the one we sent in the request headers
    assert_eq!(full_attrs.get("client.id"), Some(&client_id));
}

#[tokio::test]
async fn list_tools_metrics() {
    let service_name = unique_service_name("mcp-list-tools-metrics");
    let config = create_test_config_with_metrics(&service_name);

    let mut builder = TestServer::builder();
    let mut service = TestService::streamable_http("test_mcp_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let test_server = builder.build(&config).await;

    let client_id = format!("test-method-client-{}", uuid::Uuid::new_v4());
    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", client_id.parse().unwrap());
    headers.insert("x-client-group", "method-test-group".parse().unwrap());

    let mcp = test_server.mcp_client_with_headers("/mcp", headers).await;

    // Test list_tools operation
    let _tools = mcp.list_tools().await;

    // Query metrics
    let clickhouse = create_clickhouse_client().await;

    // Query for tools list metrics
    let tools_query = formatdoc! {r#"
        SELECT
            MetricName,
            Attributes,
            Count
        FROM otel_metrics_histogram
        WHERE
            MetricName = 'mcp.tools.list.duration'
            AND ServiceName = '{service_name}'
            AND Attributes['client.id'] = '{client_id}'
        ORDER BY TimeUnix DESC
    "#};

    let tools_metrics = wait_for_metrics_matching::<HistogramMetricRow, _>(&clickhouse, &tools_query, |rows| {
        let total_count: u64 = rows.iter().map(|row| row.count).sum();
        total_count == 1
    })
    .await
    .expect("Failed to get tools list metrics");

    let tools_attrs: std::collections::BTreeMap<_, _> = tools_metrics[0].attributes.iter().cloned().collect();

    // Create a filtered version without dynamic client_id for snapshots
    let mut attrs_snapshot = tools_attrs.clone();
    attrs_snapshot.remove("client.id");

    // Snapshot the static attributes
    insta::assert_debug_snapshot!(attrs_snapshot, @r#"
    {
        "client.group": "method-test-group",
        "method": "list_tools",
        "status": "success",
    }
    "#);

    // Separately verify the dynamic client_id exists
    // Expected: client_id matches the one we sent in the request headers
    assert_eq!(tools_attrs.get("client.id"), Some(&client_id));
}

#[tokio::test]
async fn list_prompts_metrics() {
    let service_name = unique_service_name("mcp-list-prompts");
    let config = create_test_config_with_metrics(&service_name);

    let mut builder = TestServer::builder();
    let mut service = TestService::streamable_http("test_mcp_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let test_server = builder.build(&config).await;

    let client_id = format!("test-prompts-{}", uuid::Uuid::new_v4());
    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", client_id.parse().unwrap());
    headers.insert("x-client-group", "prompts-group".parse().unwrap());

    let mcp = test_server.mcp_client_with_headers("/mcp", headers).await;
    let _prompts = mcp.list_prompts().await;

    let clickhouse = create_clickhouse_client().await;
    let query = formatdoc! {r#"
        SELECT MetricName, Attributes, Count
        FROM otel_metrics_histogram
        WHERE
            MetricName = 'mcp.prompt.request.duration'
            AND ServiceName = '{service_name}'
            AND Attributes['client.id'] = '{client_id}'
        ORDER BY TimeUnix DESC
    "#};

    let metrics = wait_for_metrics_matching::<HistogramMetricRow, _>(&clickhouse, &query, |rows| {
        let total_count: u64 = rows.iter().map(|row| row.count).sum();
        total_count == 1
    })
    .await
    .expect("Failed to get prompts metrics");

    let attrs: std::collections::BTreeMap<_, _> = metrics[0]
        .attributes
        .iter()
        .filter(|(k, _)| k.as_str() != "client.id") // Filter out dynamic client.id
        .cloned()
        .collect();

    // Use snapshot for static fields
    insta::assert_debug_snapshot!(attrs, @r###"
    {
        "client.group": "prompts-group",
        "method": "list_prompts",
        "status": "success",
    }
    "###);

    // Check dynamic field separately with assert_eq!
    let full_attrs: std::collections::BTreeMap<_, _> = metrics[0].attributes.iter().cloned().collect();
    // Expected: client_id matches the one we sent in the request headers
    assert_eq!(full_attrs.get("client.id"), Some(&client_id));
}

#[tokio::test]
async fn list_resources_metrics() {
    let service_name = unique_service_name("mcp-list-resources");
    let config = create_test_config_with_metrics(&service_name);

    let mut builder = TestServer::builder();
    let mut service = TestService::streamable_http("test_mcp_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let test_server = builder.build(&config).await;

    let client_id = format!("test-resources-{}", uuid::Uuid::new_v4());
    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", client_id.parse().unwrap());
    headers.insert("x-client-group", "resources-group".parse().unwrap());

    let mcp = test_server.mcp_client_with_headers("/mcp", headers).await;
    let _resources = mcp.list_resources().await;

    let clickhouse = create_clickhouse_client().await;
    let query = formatdoc! {r#"
        SELECT MetricName, Attributes, Count
        FROM otel_metrics_histogram
        WHERE
            MetricName = 'mcp.resource.request.duration'
            AND ServiceName = '{service_name}'
            AND Attributes['client.id'] = '{client_id}'
        ORDER BY TimeUnix DESC
    "#};

    let metrics = wait_for_metrics_matching::<HistogramMetricRow, _>(&clickhouse, &query, |rows| {
        let total_count: u64 = rows.iter().map(|row| row.count).sum();
        total_count == 1
    })
    .await
    .expect("Failed to get resources metrics");

    let attrs: std::collections::BTreeMap<_, _> = metrics[0]
        .attributes
        .iter()
        .filter(|(k, _)| k.as_str() != "client.id") // Filter out dynamic client.id
        .cloned()
        .collect();

    // Use snapshot for static fields
    insta::assert_debug_snapshot!(attrs, @r###"
    {
        "client.group": "resources-group",
        "method": "list_resources",
        "status": "success",
    }
    "###);

    // Check dynamic field separately with assert_eq!
    let full_attrs: std::collections::BTreeMap<_, _> = metrics[0].attributes.iter().cloned().collect();
    // Expected: client_id matches the one we sent in the request headers
    assert_eq!(full_attrs.get("client.id"), Some(&client_id));
}

#[tokio::test]
async fn get_prompt_success_metrics() {
    let service_name = unique_service_name("mcp-get-prompt-success");
    let config = create_test_config_with_metrics(&service_name);

    let mut builder = TestServer::builder();
    let mut service = TestService::streamable_http("test_mcp_server".to_string());
    service.add_tool(AdderTool);
    // Add a test prompt
    service.add_prompt(rmcp::model::Prompt {
        name: "test_prompt".to_string(),
        description: Some("A test prompt".to_string()),
        arguments: None,
        title: None,
        icons: None,
    });
    builder.spawn_service(service).await;

    let test_server = builder.build(&config).await;

    let client_id = format!("test-get-prompt-success-{}", uuid::Uuid::new_v4());
    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", client_id.parse().unwrap());
    headers.insert("x-client-group", "prompt-success-group".parse().unwrap());

    let mcp = test_server.mcp_client_with_headers("/mcp", headers).await;
    let _prompt = mcp
        .get_prompt_result("test_mcp_server__test_prompt", None)
        .await
        .unwrap();

    let clickhouse = create_clickhouse_client().await;
    let query = formatdoc! {r#"
        SELECT MetricName, Attributes, Count
        FROM otel_metrics_histogram
        WHERE
            MetricName = 'mcp.prompt.request.duration'
            AND ServiceName = '{service_name}'
            AND Attributes['client.id'] = '{client_id}'
        ORDER BY TimeUnix DESC
    "#};

    let metrics = wait_for_metrics_matching::<HistogramMetricRow, _>(&clickhouse, &query, |rows| {
        let total_count: u64 = rows.iter().map(|row| row.count).sum();
        total_count == 1
    })
    .await
    .expect("Failed to get prompt success metrics");

    let attrs: std::collections::BTreeMap<_, _> = metrics[0]
        .attributes
        .iter()
        .filter(|(k, _)| k.as_str() != "client.id")
        .cloned()
        .collect();

    insta::assert_debug_snapshot!(attrs, @r###"
    {
        "client.group": "prompt-success-group",
        "method": "get_prompt",
        "status": "success",
    }
    "###);

    let full_attrs: std::collections::BTreeMap<_, _> = metrics[0].attributes.iter().cloned().collect();
    assert_eq!(full_attrs.get("client.id"), Some(&client_id));
}

#[tokio::test]
async fn read_resource_success_metrics() {
    let service_name = unique_service_name("mcp-read-resource-success");
    let config = create_test_config_with_metrics(&service_name);

    let mut builder = TestServer::builder();
    let mut service = TestService::streamable_http("test_mcp_server".to_string());
    service.add_tool(AdderTool);
    // Add a test resource
    use rmcp::model::{RawResource, Resource};
    service.add_resource(Resource::new(
        RawResource {
            uri: "file://test.txt".to_string(),
            name: "test_resource".to_string(),
            description: Some("A test resource".to_string()),
            mime_type: Some("text/plain".to_string()),
            size: None,
            title: None,
            icons: None,
        },
        None,
    ));
    builder.spawn_service(service).await;

    let test_server = builder.build(&config).await;

    let client_id = format!("test-read-resource-success-{}", uuid::Uuid::new_v4());
    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", client_id.parse().unwrap());
    headers.insert("x-client-group", "resource-success-group".parse().unwrap());

    let mcp = test_server.mcp_client_with_headers("/mcp", headers).await;
    let _resource = mcp.read_resource("file://test.txt").await;

    let clickhouse = create_clickhouse_client().await;
    let query = formatdoc! {r#"
        SELECT MetricName, Attributes, Count
        FROM otel_metrics_histogram
        WHERE
            MetricName = 'mcp.resource.request.duration'
            AND ServiceName = '{service_name}'
            AND Attributes['client.id'] = '{client_id}'
        ORDER BY TimeUnix DESC
    "#};

    let metrics = wait_for_metrics_matching::<HistogramMetricRow, _>(&clickhouse, &query, |rows| {
        let total_count: u64 = rows.iter().map(|row| row.count).sum();
        total_count == 1
    })
    .await
    .expect("Failed to get resource success metrics");

    let attrs: std::collections::BTreeMap<_, _> = metrics[0]
        .attributes
        .iter()
        .filter(|(k, _)| k.as_str() != "client.id")
        .cloned()
        .collect();

    insta::assert_debug_snapshot!(attrs, @r###"
    {
        "client.group": "resource-success-group",
        "method": "read_resource",
        "status": "success",
    }
    "###);

    let full_attrs: std::collections::BTreeMap<_, _> = metrics[0].attributes.iter().cloned().collect();
    assert_eq!(full_attrs.get("client.id"), Some(&client_id));
}

#[tokio::test]
async fn get_prompt_error_metrics() {
    let service_name = unique_service_name("mcp-get-prompt-error");
    let config = create_test_config_with_metrics(&service_name);

    let mut builder = TestServer::builder();
    let mut service = TestService::streamable_http("test_mcp_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let test_server = builder.build(&config).await;

    let client_id = format!("test-get-prompt-{}", uuid::Uuid::new_v4());
    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", client_id.parse().unwrap());
    headers.insert("x-client-group", "error-group".parse().unwrap());

    let mcp = test_server.mcp_client_with_headers("/mcp", headers).await;
    let _error = mcp.get_prompt_result("nonexistent", None).await.unwrap_err();

    let clickhouse = create_clickhouse_client().await;
    let query = formatdoc! {r#"
        SELECT MetricName, Attributes, Count
        FROM otel_metrics_histogram
        WHERE
            MetricName = 'mcp.prompt.request.duration'
            AND ServiceName = '{service_name}'
            AND Attributes['client.id'] = '{client_id}'
        ORDER BY TimeUnix DESC
    "#};

    let metrics = wait_for_metrics_matching::<HistogramMetricRow, _>(&clickhouse, &query, |rows| {
        let total_count: u64 = rows.iter().map(|row| row.count).sum();
        total_count == 1
    })
    .await
    .expect("Failed to get error metrics");

    let attrs: std::collections::BTreeMap<_, _> = metrics[0]
        .attributes
        .iter()
        .filter(|(k, _)| k.as_str() != "client.id") // Filter out dynamic client.id
        .cloned()
        .collect();

    // Use snapshot for static fields
    insta::assert_debug_snapshot!(attrs, @r#"
    {
        "client.group": "error-group",
        "error.type": "method_not_found",
        "method": "get_prompt",
        "status": "error",
    }
    "#);

    // Check dynamic field separately with assert_eq!
    let full_attrs: std::collections::BTreeMap<_, _> = metrics[0].attributes.iter().cloned().collect();
    // Expected: client_id matches the one we sent in the request headers
    assert_eq!(full_attrs.get("client.id"), Some(&client_id));
}

#[tokio::test]
async fn read_resource_error_metrics() {
    let service_name = unique_service_name("mcp-read-resource-error");
    let config = create_test_config_with_metrics(&service_name);

    let mut builder = TestServer::builder();
    let mut service = TestService::streamable_http("test_mcp_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let test_server = builder.build(&config).await;

    let client_id = format!("test-read-resource-{}", uuid::Uuid::new_v4());
    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", client_id.parse().unwrap());
    headers.insert("x-client-group", "error-group".parse().unwrap());

    let mcp = test_server.mcp_client_with_headers("/mcp", headers).await;
    let _error = mcp.read_resource_result("nonexistent://resource").await.unwrap_err();

    let clickhouse = create_clickhouse_client().await;
    let query = formatdoc! {r#"
        SELECT MetricName, Attributes, Count
        FROM otel_metrics_histogram
        WHERE
            MetricName = 'mcp.resource.request.duration'
            AND ServiceName = '{service_name}'
            AND Attributes['client.id'] = '{client_id}'
        ORDER BY TimeUnix DESC
    "#};

    let metrics = wait_for_metrics_matching::<HistogramMetricRow, _>(&clickhouse, &query, |rows| {
        let total_count: u64 = rows.iter().map(|row| row.count).sum();
        total_count == 1
    })
    .await
    .expect("Failed to get error metrics");

    let attrs: std::collections::BTreeMap<_, _> = metrics[0]
        .attributes
        .iter()
        .filter(|(k, _)| k.as_str() != "client.id") // Filter out dynamic client.id
        .cloned()
        .collect();

    // Use snapshot for static fields
    insta::assert_debug_snapshot!(attrs, @r#"
    {
        "client.group": "error-group",
        "error.type": "method_not_found",
        "method": "read_resource",
        "status": "error",
    }
    "#);

    // Check dynamic field separately with assert_eq!
    let full_attrs: std::collections::BTreeMap<_, _> = metrics[0].attributes.iter().cloned().collect();
    // Expected: client_id matches the one we sent in the request headers
    assert_eq!(full_attrs.get("client.id"), Some(&client_id));
}

#[tokio::test]
async fn execute_invalid_tool_name_metrics() {
    // Test invalid tool name format (missing __ separator)
    let service_name = unique_service_name("mcp-invalid-tool-name");
    let config = create_test_config_with_metrics(&service_name);

    let mut builder = TestServer::builder();
    let mut service = TestService::streamable_http("test_mcp_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let test_server = builder.build(&config).await;

    let client_id = format!("test-invalid-{}", uuid::Uuid::new_v4());
    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", client_id.parse().unwrap());
    headers.insert("x-client-group", "error-group".parse().unwrap());

    let mcp = test_server.mcp_client_with_headers("/mcp", headers).await;

    // Invalid tool name (missing __ separator)
    let _error = mcp.execute_expect_error("invalidtoolname", serde_json::json!({})).await;

    let clickhouse = create_clickhouse_client().await;
    let error_query = formatdoc! {r#"
        SELECT MetricName, Attributes, Count
        FROM otel_metrics_histogram
        WHERE
            MetricName = 'mcp.tool.call.duration'
            AND ServiceName = '{service_name}'
            AND Attributes['client.id'] = '{client_id}'
            AND Attributes['error.type'] = 'method_not_found'
        ORDER BY TimeUnix DESC
    "#};

    let error_metrics = wait_for_metrics_matching::<HistogramMetricRow, _>(&clickhouse, &error_query, |rows| {
        let total_count: u64 = rows.iter().map(|row| row.count).sum();
        total_count == 1
    })
    .await
    .expect("Failed to get error metrics");

    let attrs: std::collections::BTreeMap<_, _> = error_metrics[0]
        .attributes
        .iter()
        .filter(|(k, _)| k.as_str() != "client.id") // Filter out dynamic client.id
        .cloned()
        .collect();

    // Use snapshot for static fields
    insta::assert_debug_snapshot!(attrs, @r#"
    {
        "client.group": "error-group",
        "error.type": "method_not_found",
        "status": "error",
        "tool_name": "invalidtoolname",
        "tool_type": "downstream",
    }
    "#);

    // Check dynamic field separately with assert_eq!
    let full_attrs: std::collections::BTreeMap<_, _> = error_metrics[0].attributes.iter().cloned().collect();
    // Expected: client_id matches the one we sent in the request headers
    assert_eq!(full_attrs.get("client.id"), Some(&client_id));
}

#[tokio::test]
async fn execute_server_not_found_metrics() {
    // Test nonexistent server in tool name
    let service_name = unique_service_name("mcp-server-not-found");
    let config = create_test_config_with_metrics(&service_name);

    let mut builder = TestServer::builder();
    let mut service = TestService::streamable_http("test_mcp_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let test_server = builder.build(&config).await;

    let client_id = format!("test-server-404-{}", uuid::Uuid::new_v4());
    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", client_id.parse().unwrap());
    headers.insert("x-client-group", "error-group".parse().unwrap());

    let mcp = test_server.mcp_client_with_headers("/mcp", headers).await;

    // Server doesn't exist
    let _error = mcp
        .execute_expect_error("nonexistent_server__some_tool", serde_json::json!({}))
        .await;

    let clickhouse = create_clickhouse_client().await;
    let error_query = formatdoc! {r#"
        SELECT MetricName, Attributes, Count
        FROM otel_metrics_histogram
        WHERE
            MetricName = 'mcp.tool.call.duration'
            AND ServiceName = '{service_name}'
            AND Attributes['client.id'] = '{client_id}'
            AND Attributes['error.type'] = 'method_not_found'
        ORDER BY TimeUnix DESC
    "#};

    let error_metrics = wait_for_metrics_matching::<HistogramMetricRow, _>(&clickhouse, &error_query, |rows| {
        let total_count: u64 = rows.iter().map(|row| row.count).sum();
        total_count == 1
    })
    .await
    .expect("Failed to get error metrics");

    let attrs: std::collections::BTreeMap<_, _> = error_metrics[0]
        .attributes
        .iter()
        .filter(|(k, _)| k.as_str() != "client.id") // Filter out dynamic client.id
        .cloned()
        .collect();

    // Use snapshot for static fields
    insta::assert_debug_snapshot!(attrs, @r#"
    {
        "client.group": "error-group",
        "error.type": "method_not_found",
        "status": "error",
        "tool_name": "nonexistent_server__some_tool",
        "tool_type": "downstream",
    }
    "#);

    // Check dynamic field separately with assert_eq!
    let full_attrs: std::collections::BTreeMap<_, _> = error_metrics[0].attributes.iter().cloned().collect();
    // Expected: client_id matches the one we sent in the request headers
    assert_eq!(full_attrs.get("client.id"), Some(&client_id));
}

#[tokio::test]
async fn execute_tool_not_found_metrics() {
    // Test nonexistent tool on existing server
    let service_name = unique_service_name("mcp-tool-not-found");
    let config = create_test_config_with_metrics(&service_name);

    let mut builder = TestServer::builder();
    let mut service = TestService::streamable_http("test_mcp_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let test_server = builder.build(&config).await;

    let client_id = format!("test-tool-404-{}", uuid::Uuid::new_v4());
    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", client_id.parse().unwrap());
    headers.insert("x-client-group", "error-group".parse().unwrap());

    let mcp = test_server.mcp_client_with_headers("/mcp", headers).await;

    // Server exists but tool doesn't
    let _error = mcp
        .execute_expect_error("test_mcp_server__nonexistent_tool", serde_json::json!({}))
        .await;

    let clickhouse = create_clickhouse_client().await;
    let error_query = formatdoc! {r#"
        SELECT MetricName, Attributes, Count
        FROM otel_metrics_histogram
        WHERE
            MetricName = 'mcp.tool.call.duration'
            AND ServiceName = '{service_name}'
            AND Attributes['client.id'] = '{client_id}'
            AND Attributes['error.type'] = 'method_not_found'
        ORDER BY TimeUnix DESC
    "#};

    let error_metrics = wait_for_metrics_matching::<HistogramMetricRow, _>(&clickhouse, &error_query, |rows| {
        let total_count: u64 = rows.iter().map(|row| row.count).sum();
        total_count == 1
    })
    .await
    .expect("Failed to get error metrics");

    let attrs: std::collections::BTreeMap<_, _> = error_metrics[0]
        .attributes
        .iter()
        .filter(|(k, _)| k.as_str() != "client.id") // Filter out dynamic client.id
        .cloned()
        .collect();

    // Use snapshot for static fields
    insta::assert_debug_snapshot!(attrs, @r#"
    {
        "client.group": "error-group",
        "error.type": "method_not_found",
        "status": "error",
        "tool_name": "test_mcp_server__nonexistent_tool",
        "tool_type": "downstream",
    }
    "#);

    // Check dynamic field separately with assert_eq!
    let full_attrs: std::collections::BTreeMap<_, _> = error_metrics[0].attributes.iter().cloned().collect();
    // Expected: client_id matches the one we sent in the request headers
    assert_eq!(full_attrs.get("client.id"), Some(&client_id));
}

#[tokio::test]
async fn downstream_rate_limit_exceeded_metrics() {
    // This test verifies that downstream rate limit errors are properly tracked in metrics
    // We'll use a special failing tool that returns -32000 error code
    let service_name = unique_service_name("mcp-downstream-rate-limit");
    let config = create_test_config_with_metrics(&service_name);

    let mut builder = TestServer::builder();
    let mut service = TestService::streamable_http("rate_limit_server".to_string());
    // Use FailingTool which returns -32000 error code (rate limit error)
    service.add_tool(FailingTool);
    builder.spawn_service(service).await;

    let test_server = builder.build(&config).await;

    let client_id = format!("test-rate-limit-{}", uuid::Uuid::new_v4());
    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", client_id.parse().unwrap());
    headers.insert("x-client-group", "rate-limit-group".parse().unwrap());

    let mcp = test_server.mcp_client_with_headers("/mcp", headers).await;

    // Call the failing tool which returns -32000 (rate limit error code)
    let _error = mcp
        .execute_expect_error("rate_limit_server__failing_tool", serde_json::json!({}))
        .await;

    // Query metrics for rate limit error
    let clickhouse = create_clickhouse_client().await;
    let query = formatdoc! {r#"
        SELECT MetricName, Attributes, Count
        FROM otel_metrics_histogram
        WHERE
            MetricName = 'mcp.tool.call.duration'
            AND ServiceName = '{service_name}'
            AND Attributes['client.id'] = '{client_id}'
            AND Attributes['error.type'] = 'rate_limit_exceeded'
        ORDER BY TimeUnix DESC
    "#};

    let error_metrics = wait_for_metrics_matching::<HistogramMetricRow, _>(&clickhouse, &query, |rows| {
        let total_count: u64 = rows.iter().map(|row| row.count).sum();
        total_count == 1
    })
    .await
    .expect("Failed to get rate limit error metrics");

    let attrs: std::collections::BTreeMap<_, _> = error_metrics[0]
        .attributes
        .iter()
        .filter(|(k, _)| k.as_str() != "client.id")
        .cloned()
        .collect();

    // Verify the error is tracked as rate_limit_exceeded
    insta::assert_debug_snapshot!(attrs, @r#"
    {
        "client.group": "rate-limit-group",
        "error.type": "rate_limit_exceeded",
        "status": "error",
        "tool_name": "rate_limit_server__failing_tool",
        "tool_type": "downstream",
    }
    "#);

    // Verify client_id is present
    let full_attrs: std::collections::BTreeMap<_, _> = error_metrics[0].attributes.iter().cloned().collect();
    // Expected: client_id matches the one we sent in the request headers
    assert_eq!(full_attrs.get("client.id"), Some(&client_id));
}

#[tokio::test]
async fn nexus_rate_limit_exceeded_metrics() {
    // This test verifies that Nexus's own rate limiting is properly tracked in metrics
    // We configure a very low rate limit and exceed it
    let service_name = unique_service_name("mcp-nexus-rate-limit");

    let mut builder = TestServer::builder();
    let mut service = TestService::streamable_http("test_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let base_config = create_test_config_with_metrics(&service_name);

    // Config with rate limiting enabled for MCP server
    let rate_limit_config = indoc! {r#"
        # Enable rate limiting (memory storage is default)
        [server.rate_limits]
        enabled = true

        # Configure very low rate limit that we'll exceed
        [mcp.servers.test_server.rate_limits]
        limit = 1
        interval = "10s"
    "#};

    let config = formatdoc! {r#"
        {base_config}

        {rate_limit_config}
    "#};

    let test_server = builder.build(&config).await;

    let client_id = format!("test-nexus-limit-{}", uuid::Uuid::new_v4());
    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", client_id.parse().unwrap());
    headers.insert("x-client-group", "nexus-limit-group".parse().unwrap());

    let mcp = test_server.mcp_client_with_headers("/mcp", headers).await;

    // First call should succeed
    let first_result = mcp
        .execute(
            "test_server__adder",
            serde_json::json!({
                "a": 5,
                "b": 3
            }),
        )
        .await;

    // Verify first call succeeded
    let result_text = serde_json::to_string(&first_result.content[0].raw).unwrap();
    assert!(result_text.contains("8"));

    // Second call should be rate limited by Nexus
    let _error = mcp
        .execute_expect_error("test_server__adder", serde_json::json!({"a": 1, "b": 2}))
        .await;

    // Query metrics for rate limit error
    let clickhouse = create_clickhouse_client().await;
    let query = formatdoc! {r#"
        SELECT MetricName, Attributes, Count
        FROM otel_metrics_histogram
        WHERE
            MetricName = 'mcp.tool.call.duration'
            AND ServiceName = '{service_name}'
            AND Attributes['client.id'] = '{client_id}'
            AND Attributes['error.type'] = 'rate_limit_exceeded'
        ORDER BY TimeUnix DESC
    "#};

    let error_metrics = wait_for_metrics_matching::<HistogramMetricRow, _>(&clickhouse, &query, |rows| {
        let total_count: u64 = rows.iter().map(|row| row.count).sum();
        total_count == 1
    })
    .await
    .expect("Failed to get Nexus rate limit error metrics");

    let attrs: std::collections::BTreeMap<_, _> = error_metrics[0]
        .attributes
        .iter()
        .filter(|(k, _)| k.as_str() != "client.id")
        .cloned()
        .collect();

    // Verify the error is tracked as rate_limit_exceeded from Nexus itself
    // Note: server_name is not included when Nexus rate limits before reaching the server
    insta::assert_debug_snapshot!(attrs, @r#"
    {
        "client.group": "nexus-limit-group",
        "error.type": "rate_limit_exceeded",
        "status": "error",
        "tool_name": "test_server__adder",
        "tool_type": "downstream",
    }
    "#);

    // Verify client_id is present
    let full_attrs: std::collections::BTreeMap<_, _> = error_metrics[0].attributes.iter().cloned().collect();
    // Expected: client_id matches the one we sent in the request headers
    assert_eq!(full_attrs.get("client.id"), Some(&client_id));
}
