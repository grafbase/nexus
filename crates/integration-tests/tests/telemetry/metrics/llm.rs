//! LLM endpoint metrics tests

use indoc::formatdoc;
use integration_tests::{TestServer, llms::OpenAIMock, telemetry::*};
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

        [llm]
        enabled = true
        path = "/llm"
    "#}
}

#[tokio::test]
async fn llm_endpoint_metrics() {
    let service_name = unique_service_name("llm-http-metrics");
    let config = create_test_config_with_metrics(&service_name);

    // Setup mock LLM provider
    let mut builder = TestServer::builder();
    builder.spawn_llm(OpenAIMock::new("test_openai")).await;
    let test_server = builder.build(&config).await;

    let client_id = format!("test-client-{}", uuid::Uuid::new_v4());
    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", client_id.parse().unwrap());
    headers.insert("x-client-group", "test-group".parse().unwrap());

    // Make multiple requests to the LLM endpoint
    let request = serde_json::json!({
        "model": "test_openai/gpt-3.5-turbo",
        "messages": [{"role": "user", "content": "Hello"}],
        "max_tokens": 10
    });

    for _ in 0..2 {
        let response = test_server
            .client
            .request(reqwest::Method::POST, "/llm/v1/chat/completions")
            .headers(headers.clone())
            .json(&request)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
    }

    // Drop the test server to force flush metrics
    drop(test_server);
    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // Query ClickHouse for metrics
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
            AND Attributes['http.route'] = '/llm/v1/chat/completions'
            AND Attributes['http.request.method'] = 'POST'
        ORDER BY TimeUnix DESC
    "#};

    // Wait for HTTP request metrics
    // Expected exactly 2 HTTP POST requests: we made 2 explicit POST requests to /llm/v1/chat/completions
    let llm_histograms = wait_for_metrics_matching::<HistogramMetricRow, _>(&clickhouse, &query, |rows| {
        let total_count: u64 = rows.iter().map(|row| row.count).sum();
        total_count == 2
    })
    .await
    .expect("Failed to get LLM metrics");

    // Verify HTTP metric attributes contain expected fields
    let first_histogram = &llm_histograms[0];
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
        "http.route": "/llm/v1/chat/completions",
    }
    "###);
}

#[tokio::test]
async fn llm_non_streaming_operation_metrics() {
    let service_name = unique_service_name("llm-non-streaming-metrics");
    let config = create_test_config_with_metrics(&service_name);

    // Setup mock LLM provider
    let mut builder = TestServer::builder();
    builder.spawn_llm(OpenAIMock::new("test_openai")).await;
    let test_server = builder.build(&config).await;

    let client_id = format!("test-llm-client-{}", uuid::Uuid::new_v4());
    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", client_id.parse().unwrap());
    headers.insert("x-client-group", "llm-test-group".parse().unwrap());

    // Make a non-streaming request
    let request = serde_json::json!({
        "model": "test_openai/gpt-3.5-turbo",
        "messages": [{"role": "user", "content": "Hello"}],
        "max_tokens": 10
    });

    let response = test_server
        .client
        .request(reqwest::Method::POST, "/llm/v1/chat/completions")
        .headers(headers.clone())
        .json(&request)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    // Drop the test server to force flush metrics
    drop(test_server);
    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // Query ClickHouse for metrics
    let clickhouse = create_clickhouse_client().await;

    // Check for the gen_ai.client.operation.duration metric
    // Filter by client_id to ensure we only count metrics from this specific test run
    let operation_query = formatdoc! {r#"
        SELECT
            MetricName,
            Attributes,
            Count
        FROM otel_metrics_histogram
        WHERE
            MetricName = 'gen_ai.client.operation.duration'
            AND ServiceName = '{service_name}'
            AND Attributes['client.id'] = '{client_id}'
        ORDER BY TimeUnix DESC
    "#};

    // Wait for LLM operation metrics
    // Expected exactly 1 operation: 1 non-streaming completion
    let operation_histograms = wait_for_metrics_matching::<HistogramMetricRow, _>(&clickhouse, &operation_query, |rows| {
        let total_count: u64 = rows.iter().map(|row| row.count).sum();
        total_count == 1
    })
    .await
    .expect("Failed to get LLM operation metrics");

    // Verify operation metrics - check first row attributes
    let first_row = &operation_histograms[0];
    assert_eq!(first_row.metric_name, "gen_ai.client.operation.duration");
    
    let attrs: std::collections::BTreeMap<_, _> = first_row
        .attributes
        .iter()
        .filter(|(k, _)| k.as_str() != "client.id") // Filter out dynamic client_id
        .cloned()
        .collect();
    
    // Use snapshot for attributes
    insta::assert_debug_snapshot!(attrs, @r#"
    {
        "client.group": "llm-test-group",
        "gen_ai.operation.name": "chat.completions",
        "gen_ai.request.model": "test_openai/gpt-3.5-turbo",
        "gen_ai.system": "nexus.llm",
    }
    "#);

    // Check dynamic field separately with assert_eq!
    let full_attrs: std::collections::BTreeMap<_, _> = first_row.attributes.iter().cloned().collect();
    // Expected: client.id matches the one we sent in the request headers
    assert_eq!(full_attrs.get("client.id"), Some(&client_id));
}

#[tokio::test]
async fn llm_streaming_operation_metrics() {
    let service_name = unique_service_name("llm-streaming-metrics");
    let config = create_test_config_with_metrics(&service_name);

    // Setup mock LLM provider with streaming support
    let mut builder = TestServer::builder();
    builder.spawn_llm(OpenAIMock::new("test_openai").with_streaming()).await;
    let test_server = builder.build(&config).await;

    let client_id = format!("test-streaming-client-{}", uuid::Uuid::new_v4());
    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", client_id.parse().unwrap());
    headers.insert("x-client-group", "streaming-test-group".parse().unwrap());

    // Make a streaming request
    let streaming_request = serde_json::json!({
        "model": "test_openai/gpt-4",
        "messages": [{"role": "user", "content": "Hello"}],
        "stream": true
    });

    let streaming_response = test_server
        .client
        .request(reqwest::Method::POST, "/llm/v1/chat/completions")
        .headers(headers.clone())
        .json(&streaming_request)
        .send()
        .await
        .unwrap();
    assert_eq!(streaming_response.status(), 200);
    
    // Consume the stream
    let _body = streaming_response.text().await.unwrap();

    // Drop the test server to force flush metrics
    drop(test_server);
    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // Query ClickHouse for metrics
    let clickhouse = create_clickhouse_client().await;

    // Check for the gen_ai.client.operation.duration metric
    let operation_query = formatdoc! {r#"
        SELECT
            MetricName,
            Attributes,
            Count
        FROM otel_metrics_histogram
        WHERE
            MetricName = 'gen_ai.client.operation.duration'
            AND ServiceName = '{service_name}'
            AND Attributes['client.id'] = '{client_id}'
        ORDER BY TimeUnix DESC
    "#};

    // Wait for LLM operation metrics
    // Expected exactly 1 operation: 1 streaming completion
    let operation_histograms = wait_for_metrics_matching::<HistogramMetricRow, _>(&clickhouse, &operation_query, |rows| {
        let total_count: u64 = rows.iter().map(|row| row.count).sum();
        total_count == 1
    })
    .await
    .expect("Failed to get LLM operation metrics");

    // Verify operation metrics
    let first_row = &operation_histograms[0];
    assert_eq!(first_row.metric_name, "gen_ai.client.operation.duration");
    
    let attrs: std::collections::BTreeMap<_, _> = first_row
        .attributes
        .iter()
        .filter(|(k, _)| k.as_str() != "client.id")
        .cloned()
        .collect();
    
    insta::assert_debug_snapshot!(attrs, @r###"
    {
        "client.group": "streaming-test-group",
        "gen_ai.operation.name": "chat.completions",
        "gen_ai.request.model": "test_openai/gpt-4",
        "gen_ai.system": "nexus.llm",
    }
    "###);

    // Also check for time to first token metric
    let ttft_query = formatdoc! {r#"
        SELECT
            MetricName,
            Attributes,
            Count
        FROM otel_metrics_histogram
        WHERE
            MetricName = 'gen_ai.client.time_to_first_token'
            AND ServiceName = '{service_name}'
            AND Attributes['client.id'] = '{client_id}'
        ORDER BY TimeUnix DESC
    "#};

    // Wait for time to first token metrics
    // Expected exactly 1 TTFT metric for the streaming request
    let ttft_histograms = wait_for_metrics_matching::<HistogramMetricRow, _>(&clickhouse, &ttft_query, |rows| {
        let total_count: u64 = rows.iter().map(|row| row.count).sum();
        total_count == 1
    })
    .await
    .expect("Failed to get time to first token metrics");

    // Verify TTFT metrics
    let ttft_row = &ttft_histograms[0];
    assert_eq!(ttft_row.metric_name, "gen_ai.client.time_to_first_token");
    
    let ttft_attrs: std::collections::BTreeMap<_, _> = ttft_row
        .attributes
        .iter()
        .filter(|(k, _)| k.as_str() != "client.id")
        .cloned()
        .collect();
    
    insta::assert_debug_snapshot!(ttft_attrs, @r###"
    {
        "client.group": "streaming-test-group",
        "gen_ai.operation.name": "chat.completions",
        "gen_ai.request.model": "test_openai/gpt-4",
        "gen_ai.system": "nexus.llm",
    }
    "###);

    // Check dynamic field
    let full_attrs: std::collections::BTreeMap<_, _> = first_row.attributes.iter().cloned().collect();
    assert_eq!(full_attrs.get("client.id"), Some(&client_id));
}
