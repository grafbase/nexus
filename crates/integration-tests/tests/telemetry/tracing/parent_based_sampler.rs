//! Tests for parent-based sampling behavior

use clickhouse::Row;
use indoc::formatdoc;
use integration_tests::{TestServer, TestService, telemetry::*, tools::AdderTool};
use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize};

/// Row structure for count queries
#[derive(Debug, Deserialize, Serialize, Row)]
struct CountRow {
    count: u32,
}

fn create_test_config_with_parent_sampler(service_name: &str, parent_based: bool, sampling_rate: f64) -> String {
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
        sampling = {sampling_rate}
        parent_based_sampler = {parent_based}

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
async fn parent_based_sampler_respects_parent_sampled_flag() {
    let service_name = unique_service_name("parent-sampler-respects");
    let config = create_test_config_with_parent_sampler(&service_name, true, 0.0);

    let mut builder = TestServer::builder();
    let mut service = TestService::streamable_http("test_mcp_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let test_server = builder.build(&config).await;

    // Test 1: Send request with sampled=1 (should create trace despite sampling=0.0)
    let trace_id_sampled = format!("{:032x}", uuid::Uuid::new_v4().as_u128());
    let span_id_sampled = format!("{:016x}", rand::random::<u64>());
    let traceparent_sampled = format!("00-{}-{}-01", trace_id_sampled, span_id_sampled);

    let client_id = format!("test-client-{}", uuid::Uuid::new_v4());
    let mut headers_sampled = HeaderMap::new();
    headers_sampled.insert("x-client-id", client_id.parse().unwrap());
    headers_sampled.insert("traceparent", traceparent_sampled.parse().unwrap());

    let mcp_sampled = test_server.mcp_client_with_headers("/mcp", headers_sampled).await;
    let _tools_sampled = mcp_sampled.list_tools().await;

    // Test 2: Send request with sampled=0 (should NOT create trace)
    let trace_id_not_sampled = format!("{:032x}", uuid::Uuid::new_v4().as_u128());
    let span_id_not_sampled = format!("{:016x}", rand::random::<u64>());
    let traceparent_not_sampled = format!("00-{}-{}-00", trace_id_not_sampled, span_id_not_sampled);

    let mut headers_not_sampled = HeaderMap::new();
    headers_not_sampled.insert("x-client-id", client_id.parse().unwrap());
    headers_not_sampled.insert("traceparent", traceparent_not_sampled.parse().unwrap());

    let mcp_not_sampled = test_server.mcp_client_with_headers("/mcp", headers_not_sampled).await;
    let _tools_not_sampled = mcp_not_sampled.list_tools().await;

    // Wait for traces to be exported
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    let clickhouse = create_clickhouse_client().await;

    // Query for traces with sampled=1 flag
    let query_sampled = formatdoc! {r#"
        SELECT count(*) as count
        FROM otel_traces
        WHERE ServiceName = '{service_name}'
        AND TraceId = '{trace_id_sampled}'
    "#};

    let row: CountRow = clickhouse.query(&query_sampled).fetch_one().await.unwrap();
    let sampled_count = row.count;

    // Query for traces with sampled=0 flag
    let query_not_sampled = formatdoc! {r#"
        SELECT count(*) as count
        FROM otel_traces
        WHERE ServiceName = '{service_name}'
        AND TraceId = '{trace_id_not_sampled}'
    "#};

    let row: CountRow = clickhouse.query(&query_not_sampled).fetch_one().await.unwrap();
    let not_sampled_count = row.count;

    // With parent_based_sampler=true and sampling=0.0:
    // - Request with sampled=1 should create traces
    // - Request with sampled=0 should NOT create traces
    assert!(sampled_count > 0, "Expected traces for sampled=1 parent");
    assert_eq!(not_sampled_count, 0, "Expected no traces for sampled=0 parent");
}

#[tokio::test]
async fn parent_based_sampler_disabled_uses_ratio() {
    let service_name = unique_service_name("parent-sampler-disabled");
    let config = create_test_config_with_parent_sampler(&service_name, false, 1.0);

    let mut builder = TestServer::builder();
    let mut service = TestService::streamable_http("test_mcp_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let test_server = builder.build(&config).await;

    // Send request with sampled=0 (should still create trace because parent_based_sampler=false)
    let trace_id = format!("{:032x}", uuid::Uuid::new_v4().as_u128());
    let span_id = format!("{:016x}", rand::random::<u64>());
    let traceparent = format!("00-{}-{}-00", trace_id, span_id); // sampled=0

    let client_id = format!("test-client-{}", uuid::Uuid::new_v4());
    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", client_id.parse().unwrap());
    headers.insert("traceparent", traceparent.parse().unwrap());

    let mcp = test_server.mcp_client_with_headers("/mcp", headers).await;
    let _tools = mcp.list_tools().await;

    let clickhouse = create_clickhouse_client().await;

    // Query for traces with our specific trace ID - use same simple count approach as passing test
    let query = formatdoc! {r#"
        SELECT count(*) as count
        FROM otel_traces
        WHERE ServiceName = '{service_name}'
        AND TraceId = '{trace_id}'
    "#};

    // Wait for traces to appear
    let rows = wait_for_metrics_matching::<CountRow, _>(&clickhouse, &query, |rows| rows.iter().any(|r| r.count > 0))
        .await
        .expect("Failed to get trace count");

    let count = rows[0].count;

    // With parent_based_sampler=false and sampling=1.0:
    // - All requests should create traces regardless of parent's sampled flag
    assert!(
        count > 0,
        "Expected traces with trace ID {} even with sampled=0 parent when parent_based_sampler is disabled",
        trace_id
    );
}

#[tokio::test]
async fn parent_based_sampler_no_parent_uses_ratio() {
    let service_name = unique_service_name("parent-sampler-no-parent");
    let config = create_test_config_with_parent_sampler(&service_name, true, 1.0);

    let mut builder = TestServer::builder();
    let mut service = TestService::streamable_http("test_mcp_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let test_server = builder.build(&config).await;

    // Send request WITHOUT traceparent header (no parent)
    let client_id = format!("test-client-{}", uuid::Uuid::new_v4());
    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", client_id.parse().unwrap());
    // No traceparent header

    let mcp = test_server.mcp_client_with_headers("/mcp", headers).await;
    let _tools = mcp.list_tools().await;

    // Wait for traces to be exported
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    let clickhouse = create_clickhouse_client().await;

    // Query for traces from this service
    let query = formatdoc! {r#"
        SELECT count(*) as count
        FROM otel_traces
        WHERE ServiceName = '{service_name}'
    "#};

    let row: CountRow = clickhouse.query(&query).fetch_one().await.unwrap();
    let count = row.count;

    // With parent_based_sampler=true and sampling=1.0 and no parent:
    // - Should use ratio-based sampling (1.0 = always sample)
    assert!(count > 0, "Expected traces when no parent exists with sampling=1.0");
}

#[tokio::test]
async fn parent_based_sampler_xray_format() {
    let service_name = unique_service_name("parent-sampler-xray");
    let config = create_test_config_with_parent_sampler(&service_name, true, 0.0);

    let mut builder = TestServer::builder();
    let mut service = TestService::streamable_http("test_mcp_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let test_server = builder.build(&config).await;

    // Test with X-Ray format header with Sampled=1
    let trace_id_sampled = format!(
        "{:08x}{:024x}",
        rand::random::<u32>(),
        rand::random::<u128>() & ((1u128 << 96) - 1)
    );
    let parent_id_sampled = format!("{:016x}", rand::random::<u64>());
    let xray_header_sampled = format!(
        "Root=1-{}-{};Parent={};Sampled=1",
        &trace_id_sampled[..8],
        &trace_id_sampled[8..],
        parent_id_sampled
    );

    let client_id = format!("test-client-{}", uuid::Uuid::new_v4());
    let mut headers_sampled = HeaderMap::new();
    headers_sampled.insert("x-client-id", client_id.parse().unwrap());
    headers_sampled.insert("x-amzn-trace-id", xray_header_sampled.parse().unwrap());

    let mcp_sampled = test_server.mcp_client_with_headers("/mcp", headers_sampled).await;
    let _tools_sampled = mcp_sampled.list_tools().await;

    // Test with X-Ray format header with Sampled=0
    let trace_id_not_sampled = format!(
        "{:08x}{:024x}",
        rand::random::<u32>(),
        rand::random::<u128>() & ((1u128 << 96) - 1)
    );
    let parent_id_not_sampled = format!("{:016x}", rand::random::<u64>());
    let xray_header_not_sampled = format!(
        "Root=1-{}-{};Parent={};Sampled=0",
        &trace_id_not_sampled[..8],
        &trace_id_not_sampled[8..],
        parent_id_not_sampled
    );

    let mut headers_not_sampled = HeaderMap::new();
    headers_not_sampled.insert("x-client-id", client_id.parse().unwrap());
    headers_not_sampled.insert("x-amzn-trace-id", xray_header_not_sampled.parse().unwrap());

    let mcp_not_sampled = test_server.mcp_client_with_headers("/mcp", headers_not_sampled).await;
    let _tools_not_sampled = mcp_not_sampled.list_tools().await;

    // Wait for traces to be exported
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    let clickhouse = create_clickhouse_client().await;

    // Query for traces with Sampled=1
    let query_sampled = formatdoc! {r#"
        SELECT count(*) as count
        FROM otel_traces
        WHERE ServiceName = '{service_name}'
        AND TraceId = '{trace_id_sampled}'
    "#};

    let row: CountRow = clickhouse.query(&query_sampled).fetch_one().await.unwrap();
    let sampled_count = row.count;

    // Query for traces with Sampled=0
    let query_not_sampled = formatdoc! {r#"
        SELECT count(*) as count
        FROM otel_traces
        WHERE ServiceName = '{service_name}'
        AND TraceId = '{trace_id_not_sampled}'
    "#};

    let row: CountRow = clickhouse.query(&query_not_sampled).fetch_one().await.unwrap();
    let not_sampled_count = row.count;

    // With parent_based_sampler=true and X-Ray headers:
    // - Request with Sampled=1 should create traces
    // - Request with Sampled=0 should NOT create traces
    assert!(sampled_count > 0, "Expected traces for X-Ray Sampled=1");
    assert_eq!(not_sampled_count, 0, "Expected no traces for X-Ray Sampled=0");
}
