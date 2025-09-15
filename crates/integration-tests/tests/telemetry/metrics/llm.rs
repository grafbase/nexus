//! LLM endpoint metrics tests

use std::collections::BTreeMap;

use indoc::formatdoc;
use integration_tests::{TestServer, llms::OpenAIMock, telemetry::*};
use reqwest::header::HeaderMap;

use crate::telemetry::metrics::{HistogramMetricRow, SumMetricRow};

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

        [llm.protocols.openai]
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
    let attrs: BTreeMap<_, _> = first_histogram
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
    let operation_histograms =
        wait_for_metrics_matching::<HistogramMetricRow, _>(&clickhouse, &operation_query, |rows| {
            let total_count: u64 = rows.iter().map(|row| row.count).sum();
            total_count == 1
        })
        .await
        .expect("Failed to get LLM operation metrics");

    // Verify operation metrics - check first row attributes
    let first_row = &operation_histograms[0];
    assert_eq!(first_row.metric_name, "gen_ai.client.operation.duration");

    let attrs: BTreeMap<_, _> = first_row
        .attributes
        .iter()
        .filter(|(k, _)| k.as_str() != "client.id") // Filter out dynamic client_id
        .cloned()
        .collect();

    // Use snapshot for attributes - now includes finish_reason
    insta::assert_debug_snapshot!(attrs, @r#"
    {
        "client.group": "llm-test-group",
        "gen_ai.operation.name": "chat.completions",
        "gen_ai.request.model": "test_openai/gpt-3.5-turbo",
        "gen_ai.response.finish_reason": "stop",
        "gen_ai.system": "nexus.llm",
    }
    "#);

    // Check dynamic field separately with assert_eq!
    let full_attrs: BTreeMap<_, _> = first_row.attributes.iter().cloned().collect();
    // Expected: client.id matches the one we sent in the request headers
    assert_eq!(full_attrs.get("client.id"), Some(&client_id));
}

#[tokio::test]
async fn llm_token_usage_metrics() {
    let service_name = unique_service_name("llm-token-metrics");
    let config = create_test_config_with_metrics(&service_name);

    // Setup mock LLM provider that returns specific token counts
    let mut builder = TestServer::builder();
    builder.spawn_llm(OpenAIMock::new("test_openai")).await;
    let test_server = builder.build(&config).await;

    let client_id = format!("test-token-client-{}", uuid::Uuid::new_v4());
    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", client_id.parse().unwrap());
    headers.insert("x-client-group", "token-test-group".parse().unwrap());

    // Make a request that will use tokens
    let request = serde_json::json!({
        "model": "test_openai/gpt-3.5-turbo",
        "messages": [
            {"role": "system", "content": "You are a helpful assistant."},
            {"role": "user", "content": "What is the weather today?"}
        ],
        "max_tokens": 50
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

    // Query ClickHouse for token metrics
    let clickhouse = create_clickhouse_client().await;

    // Check for input token counter metrics
    let input_token_query = formatdoc! {r#"
        SELECT
            MetricName,
            Attributes,
            Value
        FROM otel_metrics_sum
        WHERE
            MetricName = 'gen_ai.client.input.token.usage'
            AND ServiceName = '{service_name}'
            AND Attributes['client.id'] = '{client_id}'
        ORDER BY TimeUnix DESC
    "#};

    // Wait for input token metrics
    let input_token_metrics =
        wait_for_metrics_matching::<SumMetricRow, _>(&clickhouse, &input_token_query, |rows| !rows.is_empty())
            .await
            .expect("Failed to get input token metrics");

    // Verify input token metrics
    assert!(!input_token_metrics.is_empty());
    let first_row = &input_token_metrics[0];
    assert_eq!(first_row.metric_name, "gen_ai.client.input.token.usage");
    assert!(first_row.value > 0.0); // Should have counted some input tokens

    // Check attributes
    let attrs: BTreeMap<_, _> = first_row
        .attributes
        .iter()
        .filter(|(k, _)| k.as_str() != "client.id")
        .cloned()
        .collect();

    insta::assert_debug_snapshot!(attrs, @r###"
    {
        "client.group": "token-test-group",
        "gen_ai.request.model": "test_openai/gpt-3.5-turbo",
        "gen_ai.system": "nexus.llm",
    }
    "###);

    // Verify the client.id was correctly recorded
    let full_attrs: BTreeMap<_, _> = first_row.attributes.iter().cloned().collect();
    assert_eq!(full_attrs.get("client.id"), Some(&client_id));

    // Check for output token counter metrics
    let output_token_query = formatdoc! {r#"
        SELECT
            MetricName,
            Attributes,
            Value
        FROM otel_metrics_sum
        WHERE
            MetricName = 'gen_ai.client.output.token.usage'
            AND ServiceName = '{service_name}'
            AND Attributes['client.id'] = '{client_id}'
        ORDER BY TimeUnix DESC
    "#};

    let output_token_metrics =
        wait_for_metrics_matching::<SumMetricRow, _>(&clickhouse, &output_token_query, |rows| !rows.is_empty())
            .await
            .expect("Failed to get output token metrics");

    assert!(!output_token_metrics.is_empty());
    let output_row = &output_token_metrics[0];
    assert_eq!(output_row.metric_name, "gen_ai.client.output.token.usage");
    assert!(output_row.value > 0.0); // Should have counted some output tokens

    // Check for total token counter metrics
    let total_token_query = formatdoc! {r#"
        SELECT
            MetricName,
            Attributes,
            Value
        FROM otel_metrics_sum
        WHERE
            MetricName = 'gen_ai.client.total.token.usage'
            AND ServiceName = '{service_name}'
            AND Attributes['client.id'] = '{client_id}'
        ORDER BY TimeUnix DESC
    "#};

    let total_token_metrics =
        wait_for_metrics_matching::<SumMetricRow, _>(&clickhouse, &total_token_query, |rows| !rows.is_empty())
            .await
            .expect("Failed to get total token metrics");

    assert!(!total_token_metrics.is_empty());
    let total_row = &total_token_metrics[0];
    assert_eq!(total_row.metric_name, "gen_ai.client.total.token.usage");

    // Total should be sum of input and output
    let expected_total = first_row.value + output_row.value;
    assert_eq!(total_row.value, expected_total);
}

#[tokio::test]
async fn llm_streaming_token_usage_metrics() {
    let service_name = unique_service_name("llm-streaming-token-metrics");
    let config = create_test_config_with_metrics(&service_name);

    // Setup mock LLM provider with streaming support
    let mut builder = TestServer::builder();
    builder.spawn_llm(OpenAIMock::new("test_openai").with_streaming()).await;
    let test_server = builder.build(&config).await;

    let client_id = format!("test-streaming-token-client-{}", uuid::Uuid::new_v4());
    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", client_id.parse().unwrap());
    headers.insert("x-client-group", "streaming-token-group".parse().unwrap());

    // Make a streaming request using the llm_client helper
    let mut llm_client = test_server.llm_client("/llm");
    llm_client.push_header("x-client-id", client_id.clone());
    llm_client.push_header("x-client-group", "streaming-token-group");

    let streaming_request = serde_json::json!({
        "model": "test_openai/gpt-4",
        "messages": [
            {"role": "system", "content": "You are a helpful assistant."},
            {"role": "user", "content": "Tell me a short story."}
        ],
        "stream": true,
        "max_tokens": 100
    });

    // Stream the completion and collect all chunks
    let chunks = llm_client.stream_completions(streaming_request).await;
    assert!(!chunks.is_empty());

    // Query ClickHouse for token metrics
    let clickhouse = create_clickhouse_client().await;

    // Check for input token counter metrics from streaming
    let input_token_query = formatdoc! {r#"
        SELECT
            MetricName,
            Attributes,
            Value
        FROM otel_metrics_sum
        WHERE
            MetricName = 'gen_ai.client.input.token.usage'
            AND ServiceName = '{service_name}'
            AND Attributes['client.id'] = '{client_id}'
        ORDER BY TimeUnix DESC
    "#};

    let input_token_metrics =
        wait_for_metrics_matching::<SumMetricRow, _>(&clickhouse, &input_token_query, |rows| !rows.is_empty())
            .await
            .expect("Failed to get streaming input token metrics");

    assert!(!input_token_metrics.is_empty());
    let input_row = &input_token_metrics[0];
    assert_eq!(input_row.metric_name, "gen_ai.client.input.token.usage");
    assert!(input_row.value > 0.0); // Should have counted input tokens

    // Check for output token counter metrics from streaming
    let output_token_query = formatdoc! {r#"
        SELECT
            MetricName,
            Attributes,
            Value
        FROM otel_metrics_sum
        WHERE
            MetricName = 'gen_ai.client.output.token.usage'
            AND ServiceName = '{service_name}'
            AND Attributes['client.id'] = '{client_id}'
        ORDER BY TimeUnix DESC
    "#};

    let output_token_metrics =
        wait_for_metrics_matching::<SumMetricRow, _>(&clickhouse, &output_token_query, |rows| !rows.is_empty())
            .await
            .expect("Failed to get streaming output token metrics");

    assert!(!output_token_metrics.is_empty());
    let output_row = &output_token_metrics[0];
    assert_eq!(output_row.metric_name, "gen_ai.client.output.token.usage");
    assert!(output_row.value > 0.0); // Should have counted output tokens from streaming

    // Check for total token counter metrics from streaming
    let total_token_query = formatdoc! {r#"
        SELECT
            MetricName,
            Attributes,
            Value
        FROM otel_metrics_sum
        WHERE
            MetricName = 'gen_ai.client.total.token.usage'
            AND ServiceName = '{service_name}'
            AND Attributes['client.id'] = '{client_id}'
        ORDER BY TimeUnix DESC
    "#};

    let total_token_metrics =
        wait_for_metrics_matching::<SumMetricRow, _>(&clickhouse, &total_token_query, |rows| !rows.is_empty())
            .await
            .expect("Failed to get streaming total token metrics");

    assert!(!total_token_metrics.is_empty());
    let total_row = &total_token_metrics[0];
    assert_eq!(total_row.metric_name, "gen_ai.client.total.token.usage");

    // Verify total = input + output
    let expected_total = input_row.value + output_row.value;
    assert_eq!(total_row.value, expected_total);

    // Verify attributes include the group and model
    let attrs: BTreeMap<_, _> = total_row
        .attributes
        .iter()
        .filter(|(k, _)| k.as_str() != "client.id")
        .cloned()
        .collect();

    insta::assert_debug_snapshot!(attrs, @r###"
    {
        "client.group": "streaming-token-group",
        "gen_ai.request.model": "test_openai/gpt-4",
        "gen_ai.system": "nexus.llm",
    }
    "###);

    // Verify the client.id was correctly recorded
    let full_attrs_for_validation: BTreeMap<_, _> = total_row.attributes.iter().cloned().collect();
    assert_eq!(full_attrs_for_validation.get("client.id"), Some(&client_id));
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

    // Make a streaming request using the llm_client helper
    let mut llm_client = test_server.llm_client("/llm");
    llm_client.push_header("x-client-id", client_id.clone());
    llm_client.push_header("x-client-group", "streaming-test-group");

    let streaming_request = serde_json::json!({
        "model": "test_openai/gpt-4",
        "messages": [{"role": "user", "content": "Hello"}],
        "stream": true
    });

    // Stream the completion and collect all chunks
    let chunks = llm_client.stream_completions(streaming_request).await;
    assert!(!chunks.is_empty());

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
    let operation_histograms =
        wait_for_metrics_matching::<HistogramMetricRow, _>(&clickhouse, &operation_query, |rows| {
            let total_count: u64 = rows.iter().map(|row| row.count).sum();
            total_count == 1
        })
        .await
        .expect("Failed to get LLM operation metrics");

    // Verify operation metrics
    let first_row = &operation_histograms[0];
    assert_eq!(first_row.metric_name, "gen_ai.client.operation.duration");

    let attrs: BTreeMap<_, _> = first_row
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
        "gen_ai.response.finish_reason": "stop",
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

    let ttft_attrs: BTreeMap<_, _> = ttft_row
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
    let full_attrs: BTreeMap<_, _> = first_row.attributes.iter().cloned().collect();
    assert_eq!(full_attrs.get("client.id"), Some(&client_id));
}

#[tokio::test]
async fn llm_rate_limit_metrics() {
    let service_name = unique_service_name("llm-rate-limit-metrics");

    // Configure rate limiting that will be exceeded
    let config = formatdoc! {r#"
        [server]
        listen_address = "127.0.0.1:0"

        [server.client_identification]
        enabled = true
        client_id = {{ source = "http_header", http_header = "x-client-id" }}

        [telemetry]
        service_name = "{service_name}"

        [telemetry.resource_attributes]
        environment = "test"

        [telemetry.exporters.otlp]
        enabled = true
        endpoint = "http://localhost:4317"
        protocol = "grpc"

        [telemetry.exporters.otlp.batch_export]
        scheduled_delay = "1s"
        max_export_batch_size = 100

        [llm]
        enabled = true

        [llm.protocols.openai]
enabled = true
path = "/llm"
        # Configure rate limiting to trigger on second request
        [llm.providers.test_openai.rate_limits.per_user]
        input_token_limit = 20
        interval = "60s"
    "#};

    // Setup mock LLM provider
    let mut builder = TestServer::builder();
    builder.spawn_llm(OpenAIMock::new("test_openai")).await;
    let test_server = builder.build(&config).await;

    let client_id = format!("test-rate-limit-client-{}", uuid::Uuid::new_v4());
    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", client_id.parse().unwrap());

    // Make a request that will exceed rate limit
    let request = serde_json::json!({
        "model": "test_openai/gpt-3.5-turbo",
        "messages": [{"role": "user", "content": "This is a long message to exceed the rate limit immediately"}],
        "max_tokens": 10
    });

    // First request should succeed
    let response = test_server
        .client
        .request(reqwest::Method::POST, "/llm/v1/chat/completions")
        .headers(headers.clone())
        .json(&request)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    // Second request should be rate limited
    let response = test_server
        .client
        .request(reqwest::Method::POST, "/llm/v1/chat/completions")
        .headers(headers.clone())
        .json(&request)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 429);

    // Query ClickHouse for operation duration metrics with rate limit error
    let clickhouse = create_clickhouse_client().await;

    let operation_query = formatdoc! {r#"
        SELECT
            MetricName,
            Attributes,
            Count
        FROM otel_metrics_histogram
        WHERE
            MetricName = 'gen_ai.client.operation.duration'
            AND ServiceName = '{service_name}'
            AND hasAny(Attributes.keys, ['error.type'])
        ORDER BY TimeUnix DESC
    "#};

    // Wait for rate limit error metrics
    let operation_metrics = wait_for_metrics_matching::<HistogramMetricRow, _>(&clickhouse, &operation_query, |rows| {
        // We expect at least one rate limited request
        rows.iter().any(|row| {
            row.attributes
                .iter()
                .any(|(k, v)| k == "error.type" && v == "rate_limit_exceeded")
        })
    })
    .await
    .expect("Failed to get operation metrics with rate limit error");

    // Find the rate limited request
    let rate_limited_row = operation_metrics
        .iter()
        .find(|row| {
            row.attributes
                .iter()
                .any(|(k, v)| k == "error.type" && v == "rate_limit_exceeded")
        })
        .expect("Could not find rate limited operation metric");

    assert_eq!(rate_limited_row.metric_name, "gen_ai.client.operation.duration");

    // Check attributes - filter out dynamic client.id
    let attrs: BTreeMap<_, _> = rate_limited_row
        .attributes
        .iter()
        .filter(|(k, _)| k.as_str() != "client.id")
        .cloned()
        .collect();

    insta::assert_debug_snapshot!(attrs, @r###"
    {
        "error.type": "rate_limit_exceeded",
        "gen_ai.operation.name": "chat.completions",
        "gen_ai.request.model": "test_openai/gpt-3.5-turbo",
        "gen_ai.system": "nexus.llm",
    }
    "###);

    // Verify the client.id was correctly recorded even for rate limited requests
    let full_attrs: BTreeMap<_, _> = rate_limited_row.attributes.iter().cloned().collect();
    assert_eq!(full_attrs.get("client.id"), Some(&client_id));
}

#[tokio::test]
async fn llm_finish_reason_metrics() {
    let service_name = unique_service_name("llm-finish-reason-metrics");
    let config = create_test_config_with_metrics(&service_name);

    // Setup mock LLM provider
    let mut builder = TestServer::builder();
    builder.spawn_llm(OpenAIMock::new("test_openai")).await;
    let test_server = builder.build(&config).await;

    let client_id = format!("test-finish-reason-client-{}", uuid::Uuid::new_v4());
    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", client_id.parse().unwrap());
    headers.insert("x-client-group", "finish-reason-group".parse().unwrap());

    // Make multiple requests to generate different finish reasons
    let request1 = serde_json::json!({
        "model": "test_openai/gpt-3.5-turbo",
        "messages": [{"role": "user", "content": "Hello"}],
        "max_tokens": 10
    });

    let request2 = serde_json::json!({
        "model": "test_openai/gpt-3.5-turbo",
        "messages": [{"role": "user", "content": "Tell me a very long story"}],
        "max_tokens": 5  // Low limit to trigger length finish reason
    });

    // Send requests
    for request in [request1, request2] {
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

    // Query ClickHouse for operation duration metrics that include finish reason
    let clickhouse = create_clickhouse_client().await;

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

    // Wait for operation metrics with finish reason
    let operation_metrics = wait_for_metrics_matching::<HistogramMetricRow, _>(&clickhouse, &operation_query, |rows| {
        let total_count: u64 = rows.iter().map(|row| row.count).sum();
        total_count >= 2 // We made 2 requests
    })
    .await
    .expect("Failed to get operation duration metrics with finish reason");

    // Verify operation metrics include finish reason
    // Metrics may be aggregated or separate depending on timing
    let total_count: u64 = operation_metrics.iter().map(|row| row.count).sum();
    assert!(
        total_count >= 2,
        "Expected at least 2 operations recorded, got {}",
        total_count
    );

    // Check that each operation metric has a finish reason attribute
    for row in &operation_metrics {
        assert_eq!(row.metric_name, "gen_ai.client.operation.duration");

        // Verify attributes include gen_ai.response.finish_reason
        let finish_reason_attr = row
            .attributes
            .iter()
            .find(|(key, _)| key == "gen_ai.response.finish_reason")
            .map(|(_, value)| value);
        assert!(finish_reason_attr.is_some());
        let finish_reason = finish_reason_attr.unwrap();
        assert!(finish_reason == "stop" || finish_reason == "length");
    }

    // Verify we have the expected attributes structure
    let first_row = &operation_metrics[0];
    let attrs: BTreeMap<_, _> = first_row
        .attributes
        .iter()
        .filter(|(k, _)| k.as_str() != "client.id" && k.as_str() != "gen_ai.response.finish_reason")
        .cloned()
        .collect();

    insta::assert_debug_snapshot!(attrs, @r###"
    {
        "client.group": "finish-reason-group",
        "gen_ai.operation.name": "chat.completions",
        "gen_ai.request.model": "test_openai/gpt-3.5-turbo",
        "gen_ai.system": "nexus.llm",
    }
    "###);

    // Verify the client.id was correctly recorded
    let full_attrs: BTreeMap<_, _> = first_row.attributes.iter().cloned().collect();
    assert_eq!(full_attrs.get("client.id"), Some(&client_id));
}

#[tokio::test]
async fn llm_streaming_finish_reason_metrics() {
    let service_name = unique_service_name("llm-streaming-finish-reason");
    let config = create_test_config_with_metrics(&service_name);

    // Setup mock LLM provider with streaming
    let mut builder = TestServer::builder();
    builder.spawn_llm(OpenAIMock::new("test_openai").with_streaming()).await;
    let test_server = builder.build(&config).await;

    let client_id = format!("test-streaming-finish-{}", uuid::Uuid::new_v4());
    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", client_id.parse().unwrap());
    headers.insert("x-client-group", "streaming-finish-group".parse().unwrap());

    // Make a streaming request using the llm_client helper
    let mut llm_client = test_server.llm_client("/llm");
    llm_client.push_header("x-client-id", client_id.clone());
    llm_client.push_header("x-client-group", "streaming-finish-group");

    let request = serde_json::json!({
        "model": "test_openai/gpt-4",
        "messages": [{"role": "user", "content": "Hello"}],
        "stream": true,
        "max_tokens": 10
    });

    // Stream the completion and collect all chunks
    let chunks = llm_client.stream_completions(request).await;
    assert!(!chunks.is_empty());

    // Verify that the stream includes finish_reason in the last chunk
    let last_chunk = chunks.last().unwrap();
    assert!(
        last_chunk
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|choice| choice.get("finish_reason"))
            .is_some()
    );

    // Query ClickHouse for operation duration metrics with finish reason
    let clickhouse = create_clickhouse_client().await;

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

    // Wait for operation metrics from streaming
    let operation_metrics = wait_for_metrics_matching::<HistogramMetricRow, _>(&clickhouse, &operation_query, |rows| {
        let total_count: u64 = rows.iter().map(|row| row.count).sum();
        total_count == 1 // We made 1 streaming request
    })
    .await
    .expect("Failed to get streaming operation metrics with finish reason");

    // Verify operation metrics include finish reason for streaming
    assert!(!operation_metrics.is_empty());
    let first_row = &operation_metrics[0];
    assert_eq!(first_row.metric_name, "gen_ai.client.operation.duration");

    let total_count: u64 = operation_metrics.iter().map(|row| row.count).sum();
    assert_eq!(total_count, 1, "Should have recorded 1 streaming completion");

    // Verify attributes include gen_ai.response.finish_reason
    let has_finish_reason = first_row
        .attributes
        .iter()
        .any(|(key, _)| key == "gen_ai.response.finish_reason");
    assert!(has_finish_reason);

    let attrs: BTreeMap<_, _> = first_row
        .attributes
        .iter()
        .filter(|(k, _)| k.as_str() != "client.id" && k.as_str() != "gen_ai.response.finish_reason")
        .cloned()
        .collect();

    insta::assert_debug_snapshot!(attrs, @r###"
    {
        "client.group": "streaming-finish-group",
        "gen_ai.operation.name": "chat.completions",
        "gen_ai.request.model": "test_openai/gpt-4",
        "gen_ai.system": "nexus.llm",
    }
    "###);

    // Verify the client.id was correctly recorded
    let full_attrs: BTreeMap<_, _> = first_row.attributes.iter().cloned().collect();
    assert_eq!(full_attrs.get("client.id"), Some(&client_id));
}
