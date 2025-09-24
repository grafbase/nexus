//! MCP-specific tracing integration tests

use clickhouse::Row;
use indoc::formatdoc;
use integration_tests::{TestServer, TestService, telemetry::*, tools::AdderTool};
use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Row structure for trace spans in ClickHouse
#[derive(Debug, Deserialize, Serialize, Row)]
struct TraceSpanRow {
    #[serde(rename = "TraceId")]
    trace_id: String,
    #[serde(rename = "SpanId")]
    span_id: String,
    #[serde(rename = "ParentSpanId")]
    parent_span_id: String,
    #[serde(rename = "SpanName")]
    span_name: String,
    #[serde(rename = "ServiceName")]
    service_name: String,
    #[serde(rename = "SpanAttributes")]
    span_attributes: Vec<(String, String)>,
    #[serde(rename = "StatusCode")]
    status_code: String,
}

fn create_mcp_tracing_config(service_name: &str) -> String {
    formatdoc! {r#"
        [server]
        listen_address = "127.0.0.1:0"

        [server.client_identification]
        enabled = true
        client_id = {{ source = "http_header", http_header = "x-client-id" }}
        group_id = {{ source = "http_header", http_header = "x-client-group" }}

        [server.client_identification.validation]
        group_values = ["premium", "basic", "free"]

        [telemetry]
        service_name = "{service_name}"

        [telemetry.resource_attributes]
        environment = "test"

        [telemetry.tracing]
        sampling = 1.0
        parent_based_sampler = false

        [telemetry.exporters.otlp]
        enabled = true
        endpoint = "http://localhost:4317"
        protocol = "grpc"

        [telemetry.exporters.otlp.batch_export]
        scheduled_delay = "100ms"
        max_export_batch_size = 100

        [mcp]
        enabled = true
        path = "/mcp"
    "#}
}

#[tokio::test]
async fn mcp_tools_list_creates_span() {
    let service_name = unique_service_name("mcp-tools-list");
    let config = create_mcp_tracing_config(&service_name);

    let mut builder = TestServer::builder();
    let mut service = TestService::streamable_http("test_mcp_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let test_server = builder.build(&config).await;

    // Generate trace context
    let trace_id = format!("{:032x}", uuid::Uuid::new_v4().as_u128());
    let span_id = format!("{:016x}", rand::random::<u64>());
    let traceparent = format!("00-{}-{}-01", trace_id, span_id);

    let client_id = format!("test-client-{}", uuid::Uuid::new_v4());
    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", client_id.parse().unwrap());
    headers.insert("x-client-group", "premium".parse().unwrap());
    headers.insert("traceparent", traceparent.parse().unwrap());

    let mcp = test_server.mcp_client_with_headers("/mcp", headers).await;

    // Make a list_tools request
    let _tools = mcp.list_tools().await;

    let clickhouse = create_clickhouse_client().await;

    // Query for MCP-specific spans - first check if ANY spans exist
    let query = formatdoc! {r#"
        SELECT
            TraceId,
            SpanId,
            ParentSpanId,
            SpanName,
            ServiceName,
            SpanAttributes,
            StatusCode
        FROM otel_traces
        WHERE
            ServiceName = '{service_name}'
            AND SpanName = 'tools/list'
        ORDER BY Timestamp DESC
        LIMIT 10
    "#};

    let spans = wait_for_metrics_matching::<TraceSpanRow, _>(&clickhouse, &query, |rows| {
        // Wait for both HTTP and MCP spans
        let has_tools_list = rows.iter().any(|r| r.span_name == "tools/list");
        if !has_tools_list && !rows.is_empty() {
            log::debug!(
                "Found spans but no tools/list: {:?}",
                rows.iter().map(|r| &r.span_name).collect::<Vec<_>>()
            );
        }
        has_tools_list
    })
    .await
    .expect("Failed to get MCP trace spans");

    // Filter to get MCP-specific attributes
    let mut mcp_spans: Vec<_> = spans.into_iter().filter(|s| s.span_name == "tools/list").collect();

    // Clean up dynamic attributes for snapshot
    for span in &mut mcp_spans {
        span.span_attributes
            .retain(|(k, _)| k.starts_with("mcp.") || k == "error");
        // Sort attributes for consistent snapshots
        span.span_attributes.sort_by(|a, b| a.0.cmp(&b.0));
    }

    insta::assert_json_snapshot!(mcp_spans, {
        "[].TraceId" => "[TRACE_ID]",
        "[].SpanId" => "[SPAN_ID]",
        "[].ParentSpanId" => "[PARENT_SPAN_ID]",
        "[].ServiceName" => "[SERVICE_NAME]",
        "[].SpanAttributes[2][1]" => "[CLIENT_ID]"  // The client_id is the 3rd attribute (index 2)
    }, @r#"
    [
      {
        "TraceId": "[TRACE_ID]",
        "SpanId": "[SPAN_ID]",
        "ParentSpanId": "[PARENT_SPAN_ID]",
        "SpanName": "tools/list",
        "ServiceName": "[SERVICE_NAME]",
        "SpanAttributes": [
          [
            "mcp.auth_forwarded",
            "false"
          ],
          [
            "mcp.method",
            "tools/list"
          ]
        ],
        "StatusCode": "Unset"
      }
    ]
    "#);
}

#[tokio::test]
async fn mcp_tools_call_with_search() {
    let service_name = unique_service_name("mcp-tools-search");
    let config = create_mcp_tracing_config(&service_name);

    let mut builder = TestServer::builder();
    let mut service = TestService::streamable_http("test_mcp_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let test_server = builder.build(&config).await;

    // Generate trace context
    let trace_id = format!("{:032x}", uuid::Uuid::new_v4().as_u128());
    let span_id = format!("{:016x}", rand::random::<u64>());
    let traceparent = format!("00-{}-{}-01", trace_id, span_id);

    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", "test-client".parse().unwrap());
    headers.insert("traceparent", traceparent.parse().unwrap());

    let mcp = test_server.mcp_client_with_headers("/mcp", headers).await;

    // Call the search tool
    let keywords = &["math", "calculator", "addition"];
    let _result = mcp.search(keywords).await;

    let clickhouse = create_clickhouse_client().await;

    // Query for MCP tool call spans
    let query = formatdoc! {r#"
        SELECT
            TraceId,
            SpanId,
            ParentSpanId,
            SpanName,
            ServiceName,
            SpanAttributes,
            StatusCode
        FROM otel_traces
        WHERE
            ServiceName = '{service_name}'
            AND TraceId = '{trace_id}'
            AND SpanName = 'tools/call'
        ORDER BY Timestamp DESC
    "#};

    let spans = wait_for_metrics_matching::<TraceSpanRow, _>(&clickhouse, &query, |rows| !rows.is_empty())
        .await
        .expect("Failed to get search tool trace spans");

    // Get the first tools/call span
    let mut search_span = spans.into_iter().next().unwrap();

    // Clean up and filter attributes
    search_span
        .span_attributes
        .retain(|(k, _)| k.starts_with("mcp.") || k == "error");
    search_span.span_attributes.sort_by(|a, b| a.0.cmp(&b.0));

    insta::assert_json_snapshot!(search_span, {
        ".TraceId" => "[TRACE_ID]",
        ".SpanId" => "[SPAN_ID]",
        ".ParentSpanId" => "[PARENT_SPAN_ID]",
        ".ServiceName" => "[SERVICE_NAME]"
    }, @r#"
    {
      "TraceId": "[TRACE_ID]",
      "SpanId": "[SPAN_ID]",
      "ParentSpanId": "[PARENT_SPAN_ID]",
      "SpanName": "tools/call",
      "ServiceName": "[SERVICE_NAME]",
      "SpanAttributes": [
        [
          "mcp.auth_forwarded",
          "false"
        ],
        [
          "mcp.method",
          "tools/call"
        ],
        [
          "mcp.search.keyword_count",
          "3"
        ],
        [
          "mcp.search.keywords",
          "math,calculator,addition"
        ],
        [
          "mcp.tool.name",
          "search"
        ],
        [
          "mcp.tool.type",
          "builtin"
        ]
      ],
      "StatusCode": "Unset"
    }
    "#);
}

#[tokio::test]
async fn mcp_tools_call_with_execute() {
    let service_name = unique_service_name("mcp-tools-execute");
    let config = create_mcp_tracing_config(&service_name);

    let mut builder = TestServer::builder();
    let mut service = TestService::streamable_http("test_mcp_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let test_server = builder.build(&config).await;

    // Generate trace context
    let trace_id = format!("{:032x}", uuid::Uuid::new_v4().as_u128());
    let span_id = format!("{:016x}", rand::random::<u64>());
    let traceparent = format!("00-{}-{}-01", trace_id, span_id);

    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", "test-client".parse().unwrap());
    headers.insert("x-client-group", "basic".parse().unwrap());
    headers.insert("traceparent", traceparent.parse().unwrap());

    let mcp = test_server.mcp_client_with_headers("/mcp", headers).await;

    // Call the execute tool
    let execute_params = json!({
        "a": 5,
        "b": 3
    });

    let _result = mcp.execute("test_mcp_server__adder", execute_params).await;

    let clickhouse = create_clickhouse_client().await;

    // Query for MCP execute tool spans
    let query = formatdoc! {r#"
        SELECT
            TraceId,
            SpanId,
            ParentSpanId,
            SpanName,
            ServiceName,
            SpanAttributes,
            StatusCode
        FROM otel_traces
        WHERE
            ServiceName = '{service_name}'
            AND TraceId = '{trace_id}'
            AND SpanName = 'tools/call'
        ORDER BY Timestamp DESC
    "#};

    let spans = wait_for_metrics_matching::<TraceSpanRow, _>(&clickhouse, &query, |rows| {
        rows.iter().any(|r| {
            r.span_attributes
                .iter()
                .any(|(k, v)| k == "mcp.tool.name" && v == "execute")
        })
    })
    .await
    .expect("Failed to get execute tool trace spans");

    // Find the execute span
    let mut execute_span = spans
        .into_iter()
        .find(|s| {
            s.span_attributes
                .iter()
                .any(|(k, v)| k == "mcp.tool.name" && v == "execute")
        })
        .unwrap();

    // Clean up and filter attributes
    execute_span
        .span_attributes
        .retain(|(k, _)| k.starts_with("mcp.") || k == "error");
    execute_span.span_attributes.sort_by(|a, b| a.0.cmp(&b.0));

    insta::assert_json_snapshot!(execute_span, {
        ".TraceId" => "[TRACE_ID]",
        ".SpanId" => "[SPAN_ID]",
        ".ParentSpanId" => "[PARENT_SPAN_ID]",
        ".ServiceName" => "[SERVICE_NAME]"
    }, @r#"
    {
      "TraceId": "[TRACE_ID]",
      "SpanId": "[SPAN_ID]",
      "ParentSpanId": "[PARENT_SPAN_ID]",
      "SpanName": "tools/call",
      "ServiceName": "[SERVICE_NAME]",
      "SpanAttributes": [
        [
          "mcp.auth_forwarded",
          "false"
        ],
        [
          "mcp.execute.target_server",
          "test_mcp_server"
        ],
        [
          "mcp.execute.target_tool",
          "test_mcp_server__adder"
        ],
        [
          "mcp.method",
          "tools/call"
        ],
        [
          "mcp.tool.name",
          "execute"
        ],
        [
          "mcp.tool.type",
          "builtin"
        ]
      ],
      "StatusCode": "Unset"
    }
    "#);
}

#[tokio::test]
async fn mcp_span_has_http_parent() {
    let service_name = unique_service_name("mcp-trace-hierarchy");
    let config = create_mcp_tracing_config(&service_name);

    let mut builder = TestServer::builder();
    let mut service = TestService::streamable_http("test_mcp_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let test_server = builder.build(&config).await;

    // Generate trace context
    let trace_id = format!("{:032x}", uuid::Uuid::new_v4().as_u128());
    let parent_span_id = format!("{:016x}", rand::random::<u64>());
    let traceparent = format!("00-{}-{}-01", trace_id, parent_span_id);

    let mut headers = HeaderMap::new();
    headers.insert("traceparent", traceparent.parse().unwrap());
    headers.insert("x-client-id", "test-client".parse().unwrap());

    let mcp = test_server.mcp_client_with_headers("/mcp", headers).await;

    // Make an MCP request (search for tools)
    let _result = mcp.search(&["math"]).await;

    let clickhouse = create_clickhouse_client().await;

    // Simple row structure for hierarchy test
    #[derive(Debug, Deserialize, Row)]
    #[allow(dead_code)]
    struct SimpleSpanRow {
        #[serde(rename = "TraceId")]
        trace_id: String,
        #[serde(rename = "SpanId")]
        span_id: String,
        #[serde(rename = "ParentSpanId")]
        parent_span_id: String,
        #[serde(rename = "SpanName")]
        span_name: String,
        #[serde(rename = "ServiceName")]
        service_name: String,
    }

    // Query for all spans in this trace
    let query = formatdoc! {r#"
        SELECT
            TraceId,
            SpanId,
            ParentSpanId,
            SpanName,
            ServiceName
        FROM otel_traces
        WHERE
            ServiceName = '{service_name}'
            AND TraceId = '{trace_id}'
        ORDER BY Timestamp ASC
        LIMIT 10
    "#};

    let spans = wait_for_metrics_matching::<SimpleSpanRow, _>(&clickhouse, &query, |rows| {
        // We need at least 2 spans: HTTP and MCP
        rows.len() >= 2
    })
    .await
    .expect("Failed to get trace spans");

    // Find the MCP tools/call span
    let mcp_span = spans
        .iter()
        .find(|s| s.span_name == "tools/call")
        .expect("MCP span not found");

    // Find the HTTP span that is the parent of the MCP span
    let http_parent = spans
        .iter()
        .find(|s| s.span_name.starts_with("POST ") && s.span_id == mcp_span.parent_span_id)
        .expect("HTTP parent span not found");

    // Find the root HTTP span (the one with external parent)
    let root_http_span = spans
        .iter()
        .find(|s| s.span_name.starts_with("POST ") && s.parent_span_id == parent_span_id)
        .expect("Root HTTP span not found");

    // Verify trace hierarchy:
    // 1. All spans should have the same trace ID
    assert_eq!(root_http_span.trace_id, trace_id);
    assert_eq!(http_parent.trace_id, trace_id);
    assert_eq!(mcp_span.trace_id, trace_id);

    // 2. Root HTTP span should have the external parent
    assert_eq!(root_http_span.parent_span_id, parent_span_id);

    // 3. MCP span should have an HTTP span as parent (verified above by finding http_parent)
    // The fact that we found http_parent means the parent-child relationship is correct
    assert_eq!(
        mcp_span.parent_span_id, http_parent.span_id,
        "MCP span should be a child of an HTTP span"
    );
}

#[tokio::test]
async fn mcp_downstream_tool_call() {
    let service_name = unique_service_name("mcp-downstream");
    let config = create_mcp_tracing_config(&service_name);

    let mut builder = TestServer::builder();
    let mut service = TestService::streamable_http("test_mcp_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let test_server = builder.build(&config).await;

    // Generate trace context
    let trace_id = format!("{:032x}", uuid::Uuid::new_v4().as_u128());
    let span_id = format!("{:016x}", rand::random::<u64>());
    let traceparent = format!("00-{}-{}-01", trace_id, span_id);

    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", "test-client".parse().unwrap());
    headers.insert("traceparent", traceparent.parse().unwrap());

    let mcp = test_server.mcp_client_with_headers("/mcp", headers).await;

    // Call a downstream tool directly using execute
    let params = json!({
        "a": 10,
        "b": 20
    });

    let _result = mcp.execute("test_mcp_server__adder", params).await;

    let clickhouse = create_clickhouse_client().await;

    // Query for downstream tool call spans
    let query = formatdoc! {r#"
        SELECT
            TraceId,
            SpanId,
            ParentSpanId,
            SpanName,
            ServiceName,
            SpanAttributes,
            StatusCode
        FROM otel_traces
        WHERE
            ServiceName = '{service_name}'
            AND TraceId = '{trace_id}'
            AND SpanName = 'tools/call'
        ORDER BY Timestamp DESC
    "#};

    let spans = wait_for_metrics_matching::<TraceSpanRow, _>(&clickhouse, &query, |rows| {
        rows.iter().any(|r| {
            r.span_attributes
                .iter()
                .any(|(k, v)| k == "mcp.tool.name" && v == "execute")
                && r.span_attributes
                    .iter()
                    .any(|(k, v)| k == "mcp.execute.target_tool" && v == "test_mcp_server__adder")
        })
    })
    .await
    .expect("Failed to get downstream tool trace spans");

    // Find the execute span for the downstream tool
    let mut downstream_span = spans
        .into_iter()
        .find(|s| {
            s.span_attributes
                .iter()
                .any(|(k, v)| k == "mcp.execute.target_tool" && v == "test_mcp_server__adder")
        })
        .unwrap();

    // Clean up and filter attributes
    downstream_span
        .span_attributes
        .retain(|(k, _)| k.starts_with("mcp.") || k == "error");
    downstream_span.span_attributes.sort_by(|a, b| a.0.cmp(&b.0));

    insta::assert_json_snapshot!(downstream_span, {
        ".TraceId" => "[TRACE_ID]",
        ".SpanId" => "[SPAN_ID]",
        ".ParentSpanId" => "[PARENT_SPAN_ID]",
        ".ServiceName" => "[SERVICE_NAME]"
    }, @r#"
    {
      "TraceId": "[TRACE_ID]",
      "SpanId": "[SPAN_ID]",
      "ParentSpanId": "[PARENT_SPAN_ID]",
      "SpanName": "tools/call",
      "ServiceName": "[SERVICE_NAME]",
      "SpanAttributes": [
        [
          "mcp.auth_forwarded",
          "false"
        ],
        [
          "mcp.execute.target_server",
          "test_mcp_server"
        ],
        [
          "mcp.execute.target_tool",
          "test_mcp_server__adder"
        ],
        [
          "mcp.method",
          "tools/call"
        ],
        [
          "mcp.tool.name",
          "execute"
        ],
        [
          "mcp.tool.type",
          "builtin"
        ]
      ],
      "StatusCode": "Unset"
    }
    "#);
}

#[tokio::test]
#[ignore = "Needs proper OAuth setup to work"]
async fn mcp_auth_forwarding_tracked() {
    let service_name = unique_service_name("mcp-auth-forward");
    let config = formatdoc! {r#"
        [server]
        listen_address = "127.0.0.1:0"

        [telemetry]
        service_name = "{service_name}"

        [telemetry.tracing]
        sampling = 1.0

        [telemetry.exporters.otlp]
        enabled = true
        endpoint = "http://localhost:4317"
        protocol = "grpc"

        [mcp]
        enabled = true
        path = "/mcp"

        # Dummy server to satisfy validation
        [mcp.servers.dummy]
        cmd = ["echo", "dummy"]
    "#};

    let test_server = TestServer::builder().build(&config).await;

    // Use a dummy token for testing (auth forwarding tracks presence, not validity)
    let token = "test-jwt-token";

    // Generate trace context
    let trace_id = format!("{:032x}", uuid::Uuid::new_v4().as_u128());
    let span_id = format!("{:016x}", rand::random::<u64>());
    let traceparent = format!("00-{}-{}-01", trace_id, span_id);

    let mut headers = HeaderMap::new();
    headers.insert("authorization", format!("Bearer {}", token).parse().unwrap());
    headers.insert("traceparent", traceparent.parse().unwrap());

    let mcp = test_server.mcp_client_with_headers("/mcp", headers).await;

    // Make a request with auth
    let _tools = mcp.list_tools().await;

    let clickhouse = create_clickhouse_client().await;

    // Query for spans with auth forwarding
    let query = formatdoc! {r#"
        SELECT
            SpanName,
            SpanAttributes
        FROM otel_traces
        WHERE
            ServiceName = '{service_name}'
            AND TraceId = '{trace_id}'
            AND SpanName = 'tools/list'
        ORDER BY Timestamp DESC
        LIMIT 1
    "#};

    #[derive(Debug, Deserialize, Row, serde::Serialize)]
    struct AuthSpanRow {
        #[serde(rename = "SpanName")]
        span_name: String,
        #[serde(rename = "SpanAttributes")]
        span_attributes: Vec<(String, String)>,
    }

    let spans = wait_for_metrics_matching::<AuthSpanRow, _>(&clickhouse, &query, |rows| !rows.is_empty())
        .await
        .expect("Failed to get auth forwarding trace spans");

    let mut auth_span = spans.into_iter().next().unwrap();

    // Check for auth_forwarded attribute
    let auth_forwarded = auth_span
        .span_attributes
        .iter()
        .find(|(k, _)| k == "mcp.auth_forwarded")
        .map(|(_, v)| v.as_str())
        .unwrap_or("false");

    // With authorization header present, this should be true
    assert_eq!(
        auth_forwarded, "true",
        "Auth header presence should be tracked in traces"
    );

    // Clean up attributes for snapshot
    auth_span
        .span_attributes
        .retain(|(k, _)| k == "mcp.auth_forwarded" || k == "mcp.method");
    auth_span.span_attributes.sort_by(|a, b| a.0.cmp(&b.0));

    insta::assert_json_snapshot!(auth_span, @r#"
    {
      "SpanName": "tools/list",
      "SpanAttributes": [
        [
          "mcp.auth_forwarded",
          "true"
        ],
        [
          "mcp.method",
          "tools/list"
        ]
      ]
    }
    "#);
}

#[tokio::test]
#[ignore = "Error might occur before span creation"]
async fn mcp_error_tracking() {
    let service_name = unique_service_name("mcp-error");
    let config = formatdoc! {r#"
        [server]
        listen_address = "127.0.0.1:0"

        [server.client_identification]
        enabled = true
        client_id = {{ source = "http_header", http_header = "x-client-id" }}

        [telemetry]
        service_name = "{service_name}"

        [telemetry.tracing]
        sampling = 1.0

        [telemetry.exporters.otlp]
        enabled = true
        endpoint = "http://localhost:4317"
        protocol = "grpc"

        [mcp]
        enabled = true
        path = "/mcp"

        # Dummy server to satisfy validation
        [mcp.servers.dummy]
        cmd = ["echo", "dummy"]
    "#};

    let test_server = TestServer::builder().build(&config).await;

    // Generate trace context
    let trace_id = format!("{:032x}", uuid::Uuid::new_v4().as_u128());
    let span_id = format!("{:016x}", rand::random::<u64>());
    let traceparent = format!("00-{}-{}-01", trace_id, span_id);

    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", "test-client".parse().unwrap());
    headers.insert("traceparent", traceparent.parse().unwrap());

    let mcp = test_server.mcp_client_with_headers("/mcp", headers).await;

    // Call a non-existent tool to trigger an error
    let params = json!({
        "invalid": "params"
    });

    let _result = mcp.execute_expect_error("non_existent_tool", params).await;
    // Error is expected, just checking it was tracked

    let clickhouse = create_clickhouse_client().await;

    // Query for error spans
    let query = formatdoc! {r#"
        SELECT
            SpanName,
            SpanAttributes,
            StatusCode
        FROM otel_traces
        WHERE
            ServiceName = '{service_name}'
            AND TraceId = '{trace_id}'
            AND SpanName = 'tools/call'
        ORDER BY Timestamp DESC
        LIMIT 1
    "#};

    #[derive(Debug, Deserialize, Row, serde::Serialize)]
    struct ErrorSpanRow {
        #[serde(rename = "SpanName")]
        span_name: String,
        #[serde(rename = "SpanAttributes")]
        span_attributes: Vec<(String, String)>,
        #[serde(rename = "StatusCode")]
        status_code: String,
    }

    let spans = wait_for_metrics_matching::<ErrorSpanRow, _>(&clickhouse, &query, |rows| {
        rows.iter().any(|r| r.span_attributes.iter().any(|(k, _)| k == "error"))
    })
    .await
    .expect("Failed to get error trace spans");

    let mut error_span = spans.into_iter().next().unwrap();

    // Filter to error-related attributes
    error_span
        .span_attributes
        .retain(|(k, _)| k == "error" || k == "mcp.error.code" || k == "mcp.tool.name");
    error_span.span_attributes.sort_by(|a, b| a.0.cmp(&b.0));

    insta::assert_json_snapshot!(error_span, @r#"
    {
      "SpanName": "tools/call",
      "SpanAttributes": [
        [
          "error",
          "true"
        ],
        [
          "mcp.error.code",
          "-32601"
        ],
        [
          "mcp.tool.name",
          "execute"
        ]
      ],
      "StatusCode": "Unset"
    }
    "#);
}

#[tokio::test]
async fn mcp_and_http_spans_same_trace() {
    let service_name = unique_service_name("mcp-http-trace");
    let config = create_mcp_tracing_config(&service_name);

    let mut builder = TestServer::builder();
    let mut service = TestService::streamable_http("test_mcp_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let test_server = builder.build(&config).await;

    // Generate trace context
    let trace_id = format!("{:032x}", uuid::Uuid::new_v4().as_u128());
    let span_id = format!("{:016x}", rand::random::<u64>());
    let traceparent = format!("00-{}-{}-01", trace_id, span_id);

    let client_id = format!("test-client-{}", uuid::Uuid::new_v4());
    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", client_id.parse().unwrap());
    headers.insert("traceparent", traceparent.parse().unwrap());

    let mcp = test_server.mcp_client_with_headers("/mcp", headers).await;

    // Make a request to generate both HTTP and MCP spans
    let _tools = mcp.list_tools().await;

    let clickhouse = create_clickhouse_client().await;

    // Query for ALL spans in the trace
    let query = formatdoc! {r#"
        SELECT
            SpanName,
            TraceId,
            ParentSpanId
        FROM otel_traces
        WHERE
            ServiceName = '{service_name}'
            AND TraceId = '{trace_id}'
        ORDER BY Timestamp ASC
    "#};

    #[derive(Debug, Deserialize, Row, Serialize)]
    struct SimpleSpanRow {
        #[serde(rename = "SpanName")]
        span_name: String,
        #[serde(rename = "TraceId")]
        trace_id: String,
        #[serde(rename = "ParentSpanId")]
        parent_span_id: String,
    }

    let spans = wait_for_metrics_matching::<SimpleSpanRow, _>(&clickhouse, &query, |rows| {
        // We should have at least HTTP and MCP spans
        rows.iter().any(|r| r.span_name.contains("POST")) && rows.iter().any(|r| r.span_name == "tools/list")
    })
    .await
    .expect("Failed to get trace spans");

    // Verify all spans are in the same trace
    assert!(spans.len() >= 2, "Should have at least HTTP and MCP spans");

    let first_trace_id = &spans[0].trace_id;
    for span in &spans {
        assert_eq!(&span.trace_id, first_trace_id, "All spans should be in the same trace");
    }

    // Check we have both HTTP and MCP spans
    let has_http = spans.iter().any(|s| s.span_name.contains("POST"));
    let has_mcp = spans.iter().any(|s| s.span_name == "tools/list");

    assert!(has_http, "Should have HTTP span");
    assert!(has_mcp, "Should have MCP span");

    // Note: Due to the task spawning in StreamableHttpService,
    // the MCP span will be a sibling of the HTTP span, not a child
    // Both will have the same parent from the traceparent header
}

#[tokio::test]
async fn http_span_contains_client_attributes() {
    // CRITICAL: This test verifies that client.id and client.group are present in HTTP spans
    // This is essential for production tracing and debugging
    let service_name = unique_service_name("http-client-attrs");
    let config = create_mcp_tracing_config(&service_name);

    let mut builder = TestServer::builder();
    let mut service = TestService::streamable_http("test_mcp_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let test_server = builder.build(&config).await;

    // Generate trace context
    let trace_id = format!("{:032x}", uuid::Uuid::new_v4().as_u128());
    let span_id = format!("{:016x}", rand::random::<u64>());
    let traceparent = format!("00-{}-{}-01", trace_id, span_id);

    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", "production-client-123".parse().unwrap());
    headers.insert("x-client-group", "premium".parse().unwrap());
    headers.insert("traceparent", traceparent.parse().unwrap());

    let mcp = test_server.mcp_client_with_headers("/mcp", headers).await;

    // Make a request to generate HTTP span
    let _tools = mcp.list_tools().await;

    let clickhouse = create_clickhouse_client().await;

    // Query specifically for the HTTP span
    let query = formatdoc! {r#"
        SELECT
            SpanName,
            SpanAttributes
        FROM otel_traces
        WHERE
            ServiceName = '{service_name}'
            AND TraceId = '{trace_id}'
            AND SpanName = 'POST /mcp'
        ORDER BY Timestamp DESC
        LIMIT 1
    "#};

    #[derive(Debug, Deserialize, Row, Serialize)]
    struct HttpSpanRow {
        #[serde(rename = "SpanName")]
        span_name: String,
        #[serde(rename = "SpanAttributes")]
        span_attributes: Vec<(String, String)>,
    }

    let spans = wait_for_metrics_matching::<HttpSpanRow, _>(&clickhouse, &query, |rows| !rows.is_empty())
        .await
        .expect("Failed to get HTTP span");

    assert!(spans.len() == 1, "Should have exactly one HTTP span");

    let http_span = &spans[0];

    // CRITICAL VERIFICATION: Check that client.id and client.group are present in the HTTP span
    let attrs: std::collections::HashMap<String, String> = http_span.span_attributes.iter().cloned().collect();

    // These assertions are CRITICAL for production
    assert!(
        attrs.contains_key("client.id"),
        "CRITICAL: HTTP span MUST have client.id attribute for production tracing. Found attributes: {:?}",
        attrs.keys().collect::<Vec<_>>()
    );

    assert!(
        attrs.contains_key("client.group"),
        "CRITICAL: HTTP span MUST have client.group attribute for production tracing. Found attributes: {:?}",
        attrs.keys().collect::<Vec<_>>()
    );

    // Also verify standard HTTP attributes are still present
    assert!(
        attrs.contains_key("http.request.method"),
        "Should have http.request.method"
    );
    assert!(attrs.contains_key("http.route"), "Should have http.route");
    assert!(
        attrs.contains_key("http.response.status_code"),
        "Should have http.response.status_code"
    );

    // Use snapshot for the full set of attributes (filtering out dynamic values)
    let mut filtered_attrs: Vec<(String, String)> = http_span
        .span_attributes
        .iter()
        .filter(|(k, _)| k != "url.full" && k != "server.address")
        .cloned()
        .collect();
    filtered_attrs.sort_by(|a, b| a.0.cmp(&b.0));

    insta::assert_json_snapshot!(filtered_attrs, @r###"
    [
      [
        "client.group",
        "premium"
      ],
      [
        "client.id",
        "production-client-123"
      ],
      [
        "http.request.method",
        "POST"
      ],
      [
        "http.response.status_code",
        "200"
      ],
      [
        "http.route",
        "/mcp"
      ],
      [
        "url.scheme",
        "http"
      ]
    ]
    "###);

    eprintln!("✅ PRODUCTION CRITICAL: HTTP span correctly contains client.id and client.group attributes");
}
