//! Distributed tracing integration tests

mod llm;
mod mcp;
mod rate_limit;

use clickhouse::Row;
use indoc::formatdoc;
use integration_tests::{TestServer, TestService, telemetry::*, tools::AdderTool};
use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize};
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

fn create_test_config_with_tracing(service_name: &str) -> String {
    formatdoc! {r#"
        [server]
        listen_address = "127.0.0.1:0"

        [server.client_identification]
        enabled = true
        client_id = {{ source = "http_header", http_header = "x-client-id" }}

        [telemetry]
        service_name = "{service_name}"

        [telemetry.resource_attributes]
        environment = "test"

        [telemetry.tracing]
        sampling = 1.0  # Sample all traces for testing
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
async fn basic_trace_creation() {
    let service_name = unique_service_name("trace-basic");
    let config = create_test_config_with_tracing(&service_name);

    let mut builder = TestServer::builder();
    let mut service = TestService::streamable_http("test_mcp_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let test_server = builder.build(&config).await;

    // Generate a unique trace ID for this test
    let trace_id = format!("{:032x}", uuid::Uuid::new_v4().as_u128());
    let span_id = format!("{:016x}", rand::random::<u64>());
    let traceparent = format!("00-{}-{}-01", trace_id, span_id);

    let client_id = format!("test-client-{}", uuid::Uuid::new_v4());
    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", client_id.parse().unwrap());
    headers.insert("traceparent", traceparent.parse().unwrap());

    let mcp = test_server.mcp_client_with_headers("/mcp", headers).await;

    // Make a single MCP request to generate a trace
    let _tools = mcp.list_tools().await;

    let clickhouse = create_clickhouse_client().await;

    // Query for trace spans with our specific trace ID
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
        ORDER BY Timestamp DESC
    "#};

    // Wait for trace spans to appear
    let spans = wait_for_metrics_matching::<TraceSpanRow, _>(&clickhouse, &query, |rows| !rows.is_empty())
        .await
        .expect("Failed to get trace spans");

    // Verify trace propagation worked - all spans should have our trace ID
    for span in &spans {
        assert_eq!(span.trace_id, trace_id, "Trace ID should be propagated");
    }

    // The first span should have our parent span ID
    assert_eq!(
        spans[0].parent_span_id, span_id,
        "Parent span ID should be propagated to first span"
    );

    // Filter out dynamic attributes before snapshot
    let mut filtered_spans = spans;
    for span in &mut filtered_spans {
        span.span_attributes
            .retain(|(k, _)| k != "url.full" && k != "server.address");
    }

    // Use insta with field redactions for dynamic values
    insta::assert_json_snapshot!(filtered_spans, {
        "[].TraceId" => "[TRACE_ID]",
        "[].SpanId" => "[SPAN_ID]",  
        "[].ParentSpanId" => "[PARENT_SPAN_ID]",
        "[].ServiceName" => "[SERVICE_NAME]",
        "[0].SpanAttributes[0][1]" => "[CLIENT_ID]",  // Redact the client_id in MCP span
        "[1].SpanAttributes[0][1]" => "[CLIENT_ID]",  // Redact the client_id in first HTTP span
        "[2].SpanAttributes[0][1]" => "[CLIENT_ID]",  // Redact the client_id in second HTTP span  
        "[3].SpanAttributes[0][1]" => "[CLIENT_ID]"   // Redact the client_id in third HTTP span
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
            "client.id",
            "[CLIENT_ID]"
          ],
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
      },
      {
        "TraceId": "[TRACE_ID]",
        "SpanId": "[SPAN_ID]",
        "ParentSpanId": "[PARENT_SPAN_ID]",
        "SpanName": "POST /mcp",
        "ServiceName": "[SERVICE_NAME]",
        "SpanAttributes": [
          [
            "client.id",
            "[CLIENT_ID]"
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
        ],
        "StatusCode": "Unset"
      },
      {
        "TraceId": "[TRACE_ID]",
        "SpanId": "[SPAN_ID]",
        "ParentSpanId": "[PARENT_SPAN_ID]",
        "SpanName": "POST /mcp",
        "ServiceName": "[SERVICE_NAME]",
        "SpanAttributes": [
          [
            "client.id",
            "[CLIENT_ID]"
          ],
          [
            "http.request.method",
            "POST"
          ],
          [
            "http.response.status_code",
            "202"
          ],
          [
            "http.route",
            "/mcp"
          ],
          [
            "url.scheme",
            "http"
          ]
        ],
        "StatusCode": "Unset"
      },
      {
        "TraceId": "[TRACE_ID]",
        "SpanId": "[SPAN_ID]",
        "ParentSpanId": "[PARENT_SPAN_ID]",
        "SpanName": "POST /mcp",
        "ServiceName": "[SERVICE_NAME]",
        "SpanAttributes": [
          [
            "client.id",
            "[CLIENT_ID]"
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
        ],
        "StatusCode": "Unset"
      }
    ]
    "#);
}

#[tokio::test]
async fn trace_propagation_with_traceparent() {
    let service_name = unique_service_name("trace-propagation");
    let config = create_test_config_with_tracing(&service_name);

    let mut builder = TestServer::builder();
    let mut service = TestService::streamable_http("test_mcp_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let test_server = builder.build(&config).await;

    // Generate a unique trace ID for this test
    let trace_id = format!("{:032x}", uuid::Uuid::new_v4().as_u128());
    let parent_span_id = format!("{:016x}", rand::random::<u64>());
    let traceparent = format!("00-{}-{}-01", trace_id, parent_span_id);

    let mut headers = HeaderMap::new();
    headers.insert("traceparent", traceparent.parse().unwrap());
    headers.insert("x-client-id", "test-client".parse().unwrap());

    let mcp = test_server.mcp_client_with_headers("/mcp", headers).await;

    // Make a request with trace context
    let _tools = mcp.list_tools().await;

    let clickhouse = create_clickhouse_client().await;

    // Query for spans with our specific trace ID
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
        ORDER BY Timestamp DESC
    "#};

    // Wait for trace spans with our trace ID
    let spans = wait_for_metrics_matching::<TraceSpanRow, _>(&clickhouse, &query, |rows| !rows.is_empty())
        .await
        .expect("Failed to get trace spans");

    // We should get multiple spans since list_tools makes multiple requests
    assert!(!spans.is_empty());

    // Verify trace ID was propagated
    for span in &spans {
        assert_eq!(span.trace_id, trace_id, "Trace ID should be propagated");
    }

    // Verify parent span ID was propagated to at least the first span
    assert_eq!(
        spans[0].parent_span_id, parent_span_id,
        "Parent span ID should be propagated"
    );

    // Filter out dynamic attributes before snapshot
    let mut filtered_spans = spans;
    for span in &mut filtered_spans {
        span.span_attributes
            .retain(|(k, _)| k != "url.full" && k != "server.address");
    }

    // Use insta with field redactions for dynamic values
    insta::assert_json_snapshot!(filtered_spans, {
        "[].TraceId" => "[TRACE_ID]",
        "[].SpanId" => "[SPAN_ID]",
        "[].ParentSpanId" => "[PARENT_SPAN_ID]",
        "[].ServiceName" => "[SERVICE_NAME]"
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
            "client.id",
            "test-client"
          ],
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
      },
      {
        "TraceId": "[TRACE_ID]",
        "SpanId": "[SPAN_ID]",
        "ParentSpanId": "[PARENT_SPAN_ID]",
        "SpanName": "POST /mcp",
        "ServiceName": "[SERVICE_NAME]",
        "SpanAttributes": [
          [
            "client.id",
            "test-client"
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
        ],
        "StatusCode": "Unset"
      },
      {
        "TraceId": "[TRACE_ID]",
        "SpanId": "[SPAN_ID]",
        "ParentSpanId": "[PARENT_SPAN_ID]",
        "SpanName": "POST /mcp",
        "ServiceName": "[SERVICE_NAME]",
        "SpanAttributes": [
          [
            "client.id",
            "test-client"
          ],
          [
            "http.request.method",
            "POST"
          ],
          [
            "http.response.status_code",
            "202"
          ],
          [
            "http.route",
            "/mcp"
          ],
          [
            "url.scheme",
            "http"
          ]
        ],
        "StatusCode": "Unset"
      },
      {
        "TraceId": "[TRACE_ID]",
        "SpanId": "[SPAN_ID]",
        "ParentSpanId": "[PARENT_SPAN_ID]",
        "SpanName": "POST /mcp",
        "ServiceName": "[SERVICE_NAME]",
        "SpanAttributes": [
          [
            "client.id",
            "test-client"
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
        ],
        "StatusCode": "Unset"
      }
    ]
    "#);
}

#[tokio::test]
async fn sampling_configuration() {
    // Test with 0% sampling (no traces should be created)
    let service_name = unique_service_name("trace-sampling-0");
    let config = formatdoc! {r#"
        [server]
        listen_address = "127.0.0.1:0"

        [server.client_identification]
        enabled = true
        client_id = {{ source = "http_header", http_header = "x-client-id" }}

        [telemetry]
        service_name = "{service_name}"

        [telemetry.tracing]
        sampling = 0.0  # Sample no traces
        parent_based_sampler = false

        [telemetry.exporters.otlp]
        enabled = true
        endpoint = "http://localhost:4317"
        protocol = "grpc"

        [mcp]
        enabled = true
        path = "/mcp"
    "#};

    let mut builder = TestServer::builder();
    let mut service = TestService::streamable_http("test_mcp_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let test_server = builder.build(&config).await;

    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", "test-client".parse().unwrap());

    let mcp = test_server.mcp_client_with_headers("/mcp", headers).await;

    // Make a single request (should not be traced with 0% sampling)
    let _tools = mcp.list_tools().await;

    let clickhouse = create_clickhouse_client().await;

    // Wait a bit to ensure any spans would have been exported if they existed
    // We can't use wait_for_metrics_matching since we expect NO results
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // Query for any spans
    let query = formatdoc! {r#"
        SELECT COUNT(*) as count
        FROM otel_traces
        WHERE ServiceName = '{service_name}'
    "#};

    #[derive(Debug, Deserialize, Row)]
    #[allow(dead_code)]
    struct CountRow {
        count: u64,
    }

    let result = clickhouse
        .query(&query)
        .fetch_all::<CountRow>()
        .await
        .expect("Failed to query");

    // With 0% sampling, we should have no spans
    insta::assert_debug_snapshot!(result, @r###"
    [
        CountRow {
            count: 0,
        },
    ]
    "###);
}

#[tokio::test]
async fn aws_xray_trace_propagation() {
    let service_name = unique_service_name("trace-xray");
    let config = create_test_config_with_tracing(&service_name);

    let mut builder = TestServer::builder();
    let mut service = TestService::streamable_http("test_mcp_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let test_server = builder.build(&config).await;

    // Generate AWS X-Ray format trace ID
    // Format: Root=1-{8 hex time}-{24 hex random};Parent={16 hex};Sampled=1
    let timestamp = format!(
        "{:08x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    );
    let random_part = format!("{:024x}", rand::random::<u128>() & 0xffffff_ffffffff_ffffffff);
    let parent_span_id = format!("{:016x}", rand::random::<u64>());

    let xray_trace_id = format!(
        "Root=1-{}-{};Parent={};Sampled=1",
        &timestamp[..8],
        &random_part[..24],
        parent_span_id
    );

    // Extract the actual trace ID we expect to see (combining timestamp and random)
    let expected_trace_id = format!("{}{}", &timestamp[..8], &random_part[..24]);

    let mut headers = HeaderMap::new();
    headers.insert("x-amzn-trace-id", xray_trace_id.parse().unwrap());
    headers.insert("x-client-id", "test-client".parse().unwrap());

    let mcp = test_server.mcp_client_with_headers("/mcp", headers).await;

    // Make a request with X-Ray trace context
    let _tools = mcp.list_tools().await;

    let clickhouse = create_clickhouse_client().await;

    // Query for spans with our trace ID
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
            AND lower(TraceId) = lower('{expected_trace_id}')
        ORDER BY Timestamp DESC
        LIMIT 10
    "#};

    // Wait for trace spans
    let spans = wait_for_metrics_matching::<TraceSpanRow, _>(&clickhouse, &query, |rows| !rows.is_empty())
        .await
        .expect("Failed to get trace spans with X-Ray propagation");

    // Verify we got spans
    assert!(!spans.is_empty(), "Should have trace spans with X-Ray propagation");

    // Verify trace ID was propagated (case-insensitive comparison)
    for span in &spans {
        assert_eq!(
            span.trace_id.to_lowercase(),
            expected_trace_id.to_lowercase(),
            "X-Ray trace ID should be propagated"
        );
    }

    // Verify parent span ID was propagated to at least the first span
    assert_eq!(
        spans[0].parent_span_id.to_lowercase(),
        parent_span_id.to_lowercase(),
        "X-Ray parent span ID should be propagated"
    );

    // Filter out dynamic attributes
    let mut filtered_spans = spans;
    for span in &mut filtered_spans {
        span.span_attributes
            .retain(|(k, _)| k != "url.full" && k != "server.address");
    }

    // Snapshot test with redactions
    insta::assert_json_snapshot!(filtered_spans, {
        "[].TraceId" => "[TRACE_ID]",
        "[].SpanId" => "[SPAN_ID]",
        "[].ParentSpanId" => "[PARENT_SPAN_ID]",
        "[].ServiceName" => "[SERVICE_NAME]"
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
            "client.id",
            "test-client"
          ],
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
      },
      {
        "TraceId": "[TRACE_ID]",
        "SpanId": "[SPAN_ID]",
        "ParentSpanId": "[PARENT_SPAN_ID]",
        "SpanName": "POST /mcp",
        "ServiceName": "[SERVICE_NAME]",
        "SpanAttributes": [
          [
            "client.id",
            "test-client"
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
        ],
        "StatusCode": "Unset"
      },
      {
        "TraceId": "[TRACE_ID]",
        "SpanId": "[SPAN_ID]",
        "ParentSpanId": "[PARENT_SPAN_ID]",
        "SpanName": "POST /mcp",
        "ServiceName": "[SERVICE_NAME]",
        "SpanAttributes": [
          [
            "client.id",
            "test-client"
          ],
          [
            "http.request.method",
            "POST"
          ],
          [
            "http.response.status_code",
            "202"
          ],
          [
            "http.route",
            "/mcp"
          ],
          [
            "url.scheme",
            "http"
          ]
        ],
        "StatusCode": "Unset"
      },
      {
        "TraceId": "[TRACE_ID]",
        "SpanId": "[SPAN_ID]",
        "ParentSpanId": "[PARENT_SPAN_ID]",
        "SpanName": "POST /mcp",
        "ServiceName": "[SERVICE_NAME]",
        "SpanAttributes": [
          [
            "client.id",
            "test-client"
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
        ],
        "StatusCode": "Unset"
      }
    ]
    "#);
}

#[tokio::test]
async fn trace_propagation_priority() {
    // Test that W3C traceparent takes priority over X-Ray when both are present
    let service_name = unique_service_name("trace-priority");
    let config = create_test_config_with_tracing(&service_name);

    let mut builder = TestServer::builder();
    let mut service = TestService::streamable_http("test_mcp_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let test_server = builder.build(&config).await;

    // Generate W3C trace ID
    let w3c_trace_id = format!("{:032x}", uuid::Uuid::new_v4().as_u128());
    let w3c_parent_span_id = format!("{:016x}", rand::random::<u64>());
    let traceparent = format!("00-{}-{}-01", w3c_trace_id, w3c_parent_span_id);

    // Generate different X-Ray trace ID
    let timestamp = format!(
        "{:08x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    );
    let random_part = format!("{:024x}", rand::random::<u128>() & 0xffffff_ffffffff_ffffffff);
    let xray_parent_span_id = format!("{:016x}", rand::random::<u64>());
    let xray_trace_id = format!(
        "Root=1-{}-{};Parent={};Sampled=1",
        &timestamp[..8],
        &random_part[..24],
        xray_parent_span_id
    );

    let mut headers = HeaderMap::new();
    // Add both headers - W3C should take priority
    headers.insert("traceparent", traceparent.parse().unwrap());
    headers.insert("x-amzn-trace-id", xray_trace_id.parse().unwrap());
    headers.insert("x-client-id", "test-client".parse().unwrap());

    let mcp = test_server.mcp_client_with_headers("/mcp", headers).await;

    // Make a request
    let _tools = mcp.list_tools().await;

    let clickhouse = create_clickhouse_client().await;

    // Query for spans with W3C trace ID (should be used due to priority)
    let query = formatdoc! {r#"
        SELECT
            TraceId,
            ParentSpanId
        FROM otel_traces
        WHERE
            ServiceName = '{service_name}'
            AND TraceId = '{w3c_trace_id}'
        ORDER BY Timestamp DESC
        LIMIT 1
    "#};

    #[derive(Debug, Deserialize, Row)]
    struct SimpleSpanRow {
        #[serde(rename = "TraceId")]
        trace_id: String,
        #[serde(rename = "ParentSpanId")]
        parent_span_id: String,
    }

    // Wait for trace spans with W3C trace ID
    let spans = wait_for_metrics_matching::<SimpleSpanRow, _>(&clickhouse, &query, |rows| !rows.is_empty())
        .await
        .expect("Failed to get trace spans - W3C should take priority");

    // Verify W3C trace was used (not X-Ray)
    assert_eq!(
        spans[0].trace_id, w3c_trace_id,
        "W3C trace ID should be used when both headers present"
    );
    assert_eq!(
        spans[0].parent_span_id, w3c_parent_span_id,
        "W3C parent span ID should be used"
    );
}

#[tokio::test]
async fn tracing_disabled() {
    // Test with tracing disabled
    let service_name = unique_service_name("trace-disabled");
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
        
        # Tracing is disabled because no exporters are configured
        # (telemetry.exporters.otlp.enabled = false by default)

        [mcp]
        enabled = true
        path = "/mcp"
    "#};

    let mut builder = TestServer::builder();
    let mut service = TestService::streamable_http("test_mcp_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let test_server = builder.build(&config).await;

    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", "test-client".parse().unwrap());

    let mcp = test_server.mcp_client_with_headers("/mcp", headers).await;

    // Make a single request (should not be traced when disabled)
    let _tools = mcp.list_tools().await;

    let clickhouse = create_clickhouse_client().await;

    // Wait a bit to ensure any spans would have been exported if they existed
    // We can't use wait_for_metrics_matching since we expect NO results
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // Query for any spans
    let query = formatdoc! {r#"
        SELECT COUNT(*) as count
        FROM otel_traces
        WHERE ServiceName = '{service_name}'
    "#};

    #[derive(Debug, Deserialize, Row)]
    #[allow(dead_code)]
    struct CountRow {
        count: u64,
    }

    let result = clickhouse
        .query(&query)
        .fetch_all::<CountRow>()
        .await
        .expect("Failed to query");

    // With tracing disabled, we should have no spans
    insta::assert_debug_snapshot!(result, @r###"
    [
        CountRow {
            count: 0,
        },
    ]
    "###);
}
