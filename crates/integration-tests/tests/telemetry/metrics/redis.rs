//! Redis metrics tests

use clickhouse::Row;
use indoc::formatdoc;
use insta::assert_json_snapshot;
use integration_tests::{
    TestServer, TestService,
    telemetry::{create_clickhouse_client, unique_service_name, wait_for_metrics_matching},
    tools::AdderTool,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::telemetry::metrics::HistogramMetricRow;

// Custom row types for gauge queries that return calculated values
#[derive(Row, Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "PascalCase")]
struct GaugeMetricCheckRow {
    metric_name: String,
    attributes: Vec<(String, String)>,
    is_non_negative: bool,
}

// Custom row type for token operation queries
#[derive(Row, Deserialize, Serialize, Debug, Clone)]
struct TokenOperationRow {
    #[serde(rename = "MetricName")]
    metric_name: String,
    operation: String,
    status: String,
    has_tokens_attribute: bool,
    #[serde(rename = "Count")]
    count: u64,
}

// Helper function to create test config with Redis and telemetry enabled
fn create_test_config_with_redis_metrics(service_name: &str, key_prefix: &str) -> String {
    formatdoc! {r#"
        [server]
        listen_address = "127.0.0.1:0"

        [server.rate_limits]
        enabled = true

        [server.rate_limits.storage]
        type = "redis"
        url = "redis://localhost:6379/0"
        key_prefix = "{key_prefix}"

        [server.rate_limits.global]
        limit = 100
        interval = "60s"

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

        [mcp]
        enabled = true
        path = "/mcp"
    "#}
}

#[tokio::test]
async fn redis_command_duration_check_and_consume() {
    let service_name = unique_service_name("redis-cmd-check-consume");
    let key_prefix = format!("test_redis_cmd_{}:", uuid::Uuid::new_v4());
    let config = create_test_config_with_redis_metrics(&service_name, &key_prefix);

    let mut builder = TestServer::builder();
    let mut service = TestService::streamable_http("test_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let test_server = builder.build(&config).await;

    // Make requests that will trigger Redis rate limiting
    for _ in 0..5 {
        let response = test_server
            .client
            .post(
                "/mcp",
                &json!({
                    "jsonrpc": "2.0",
                    "method": "tools/list",
                    "id": 1
                }),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), 200);
    }

    // Wait for metrics to be exported
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let clickhouse = create_clickhouse_client().await;

    // Query for specific check_and_consume operation metrics
    // The Count field represents the actual number of observations
    let query = formatdoc! {r#"
        SELECT
            MetricName,
            Attributes,
            Count
        FROM otel_metrics_histogram
        WHERE ResourceAttributes['service.name'] = '{service_name}'
          AND MetricName = 'redis.command.duration'
          AND Attributes['operation'] = 'check_and_consume'
          AND Attributes['status'] = 'success'
        ORDER BY TimeUnix DESC
        LIMIT 1
    "#};

    let metrics = wait_for_metrics_matching::<HistogramMetricRow, _>(&clickhouse, &query, |rows| !rows.is_empty())
        .await
        .expect("Failed to get Redis check_and_consume metrics");

    // The Count field represents the actual number of operations (5 requests made)
    assert_json_snapshot!(metrics, @r#"
    [
      {
        "MetricName": "redis.command.duration",
        "Attributes": [
          [
            "operation",
            "check_and_consume"
          ],
          [
            "status",
            "success"
          ]
        ],
        "Count": 5
      }
    ]
    "#);
}

#[tokio::test]
async fn redis_pool_connections_in_use_gauge() {
    let service_name = unique_service_name("redis-pool-in-use");
    let key_prefix = format!("test_redis_pool_{}:", uuid::Uuid::new_v4());
    let config = create_test_config_with_redis_metrics(&service_name, &key_prefix);

    let mut builder = TestServer::builder();
    let mut service = TestService::streamable_http("test_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let test_server = builder.build(&config).await;

    // Make requests to exercise the connection pool
    for _ in 0..3 {
        let response = test_server
            .client
            .post(
                "/mcp",
                &json!({
                    "jsonrpc": "2.0",
                    "method": "tools/list",
                    "id": 1
                }),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), 200);
    }

    // Wait for metrics to be exported
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let clickhouse = create_clickhouse_client().await;

    // Query for the latest in_use gauge metric
    let query = formatdoc! {r#"
        SELECT
            MetricName,
            Attributes,
            Value >= 0 as is_non_negative
        FROM otel_metrics_gauge
        WHERE ResourceAttributes['service.name'] = '{service_name}'
          AND MetricName = 'redis.pool.connections.in_use'
        ORDER BY TimeUnix DESC
        LIMIT 1
    "#};

    let metrics = wait_for_metrics_matching::<GaugeMetricCheckRow, _>(&clickhouse, &query, |rows| !rows.is_empty())
        .await
        .expect("Failed to get Redis pool in_use metric");

    assert_json_snapshot!(metrics, @r#"
    [
      {
        "MetricName": "redis.pool.connections.in_use",
        "Attributes": [],
        "IsNonNegative": true
      }
    ]
    "#);
}

#[tokio::test]
async fn redis_pool_connections_available_gauge() {
    let service_name = unique_service_name("redis-pool-available");
    let key_prefix = format!("test_redis_pool_{}:", uuid::Uuid::new_v4());
    let config = create_test_config_with_redis_metrics(&service_name, &key_prefix);

    let mut builder = TestServer::builder();
    let mut service = TestService::streamable_http("test_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let test_server = builder.build(&config).await;

    // Make requests to exercise the connection pool
    for _ in 0..3 {
        let response = test_server
            .client
            .post(
                "/mcp",
                &json!({
                    "jsonrpc": "2.0",
                    "method": "tools/list",
                    "id": 1
                }),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), 200);
    }

    // Wait for metrics to be exported
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let clickhouse = create_clickhouse_client().await;

    // Query for the latest available gauge metric
    let query = formatdoc! {r#"
        SELECT
            MetricName,
            Attributes,
            Value >= 0 as is_non_negative
        FROM otel_metrics_gauge
        WHERE ResourceAttributes['service.name'] = '{service_name}'
          AND MetricName = 'redis.pool.connections.available'
        ORDER BY TimeUnix DESC
        LIMIT 1
    "#};

    let metrics = wait_for_metrics_matching::<GaugeMetricCheckRow, _>(&clickhouse, &query, |rows| !rows.is_empty())
        .await
        .expect("Failed to get Redis pool available metric");

    assert_json_snapshot!(metrics, @r#"
    [
      {
        "MetricName": "redis.pool.connections.available",
        "Attributes": [],
        "IsNonNegative": true
      }
    ]
    "#);
}

#[tokio::test]
async fn redis_metrics_with_errors() {
    let service_name = unique_service_name("redis-error-metrics");
    // Use an invalid Redis URL to trigger errors
    let config = formatdoc! {r#"
        [server]
        listen_address = "127.0.0.1:0"

        [server.rate_limits]
        enabled = true

        [server.rate_limits.storage]
        type = "redis"
        url = "redis://localhost:9999/0"  # Invalid port
        key_prefix = "test_error:"
        response_timeout = "100ms"

        [server.rate_limits.global]
        limit = 10
        interval = "60s"

        [telemetry]
        service_name = "{service_name}"

        [telemetry.exporters.otlp]
        enabled = true
        endpoint = "http://localhost:4317"
        protocol = "grpc"

        [telemetry.exporters.otlp.batch_export]
        scheduled_delay = "1s"

        [mcp]
        enabled = true

        [mcp.servers.dummy]
        cmd = ["echo", "dummy"]
    "#};

    // This should fail to start due to Redis connection error
    let result = std::panic::catch_unwind(|| {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { TestServer::builder().build(&config).await })
    });

    // Simple boolean check - OK to use assert for primitives
    assert!(result.is_err());
}

#[tokio::test]
async fn redis_check_and_consume_tokens_operation() {
    use integration_tests::llms::OpenAIMock;

    let service_name = unique_service_name("redis-token-operation");
    let key_prefix = format!("test_redis_token_{}:", uuid::Uuid::new_v4());

    // Spawn the mock LLM server
    let openai = OpenAIMock::new("test").with_models(vec!["gpt-4".to_string()]);
    let mut builder = TestServer::builder();
    builder.spawn_llm(openai).await;

    let config = formatdoc! {r#"
        [server]
        listen_address = "127.0.0.1:0"

        [server.client_identification]
        enabled = true
        client_id = {{ source = "http_header", http_header = "x-client-id" }}

        [server.rate_limits]
        enabled = true

        [server.rate_limits.storage]
        type = "redis"
        url = "redis://localhost:6379/0"
        key_prefix = "{key_prefix}"

        [telemetry]
        service_name = "{service_name}"

        [telemetry.exporters.otlp]
        enabled = true
        endpoint = "http://localhost:4317"
        protocol = "grpc"

        [telemetry.exporters.otlp.batch_export]
        scheduled_delay = "1s"

        [llm]
        enabled = true

        [llm.protocols.openai]
        enabled = true
        path = "/llm"

        [llm.providers.test.models."gpt-4".rate_limits.per_user]
        input_token_limit = 1000
        interval = "60s"
    "#};

    let test_server = builder.build(&config).await;

    // Make an LLM request that will trigger token rate limiting
    let (status, body) = test_server
        .openai_completions(json!({
            "model": "test/gpt-4",
            "messages": [{"role": "user", "content": "Hello"}]
        }))
        .header("x-client-id", "test-user-1")
        .send_raw()
        .await;

    // Verify the request succeeded
    assert_eq!(status, 200);
    assert!(body["choices"].is_array());

    // Wait for metrics to be exported
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let clickhouse = create_clickhouse_client().await;

    // Query for token rate limit operations
    // The Count field represents the actual number of operations
    let query = formatdoc! {r#"
        SELECT
            MetricName,
            Attributes['operation'] as operation,
            Attributes['status'] as status,
            mapContains(Attributes, 'tokens') as has_tokens_attribute,
            Count
        FROM otel_metrics_histogram
        WHERE ResourceAttributes['service.name'] = '{service_name}'
          AND MetricName = 'redis.command.duration'
          AND Attributes['operation'] = 'check_and_consume_tokens'
        ORDER BY TimeUnix DESC
        LIMIT 1
    "#};

    let metrics = wait_for_metrics_matching::<TokenOperationRow, _>(&clickhouse, &query, |rows| !rows.is_empty())
        .await
        .expect("Failed to get Redis check_and_consume_tokens metrics");

    // The Count field represents the actual number of operations (1 LLM request)
    assert_json_snapshot!(metrics, @r###"
    [
      {
        "MetricName": "redis.command.duration",
        "operation": "check_and_consume_tokens",
        "status": "success",
        "has_tokens_attribute": true,
        "Count": 1
      }
    ]
    "###);
}
