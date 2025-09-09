//! Rate limiting storage tracing tests with inline snapshots
//!
//! These tests verify that rate limiting operations create proper trace spans
//! when using either in-memory or Redis storage backends.

use clickhouse::Row;
use indoc::formatdoc;
use integration_tests::{TestServer, telemetry::*};
use reqwest::header::HeaderMap;
use serde::Deserialize;
use serde_json::json;

fn create_rate_limit_tracing_config(service_name: &str, key_prefix: &str) -> String {
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
        limit = 10
        interval = "60s"
        
        [server.rate_limits.per_ip]
        limit = 5
        interval = "60s"

        [telemetry]
        service_name = "{service_name}"

        [telemetry.resource_attributes]
        environment = "test"
        deployment = "integration-test"

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
        
        [mcp.servers.dummy]
        cmd = ["echo", "dummy"]
    "#}
}

#[tokio::test]
async fn redis_global_rate_limit_creates_span() {
    let service_name = unique_service_name("redis-trace-global");
    let key_prefix = format!("test_redis_trace_{}:", uuid::Uuid::new_v4());
    let config = create_rate_limit_tracing_config(&service_name, &key_prefix);

    let test_server = TestServer::builder().build(&config).await;

    // Generate trace context
    let trace_id = format!("{:032x}", uuid::Uuid::new_v4().as_u128());
    let span_id = format!("{:016x}", rand::random::<u64>());
    let traceparent = format!("00-{}-{}-01", trace_id, span_id);

    let mut headers = HeaderMap::new();
    headers.insert("traceparent", traceparent.parse().unwrap());

    let mcp_client = test_server.mcp_client_with_headers("/mcp", headers).await;

    // List tools to trigger rate limiting check
    let tools = mcp_client.list_tools().await;

    // Verify the request succeeded
    assert!(!tools.tools.is_empty());

    // Wait for traces to be exported
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // Define a simple row struct for Redis spans
    #[derive(Debug, Deserialize, Row)]
    struct RedisSpanRow {
        #[serde(rename = "SpanName")]
        span_name: String,
        #[serde(rename = "SpanAttributes")]
        span_attributes: Vec<(String, String)>,
        #[serde(rename = "StatusCode")]
        status_code: String,
    }

    // Query ClickHouse for the spans
    let clickhouse = create_clickhouse_client().await;
    let query = format!(
        "SELECT SpanName, SpanAttributes, StatusCode FROM otel_traces WHERE ServiceName = '{}' AND SpanName LIKE 'redis:%' ORDER BY SpanName",
        service_name
    );

    let spans = wait_for_metrics_matching::<RedisSpanRow, _>(&clickhouse, &query, |rows| !rows.is_empty())
        .await
        .expect("Failed to get Redis trace spans");

    // Check that we have Redis spans
    let redis_spans: Vec<_> = spans
        .into_iter()
        .map(|span| {
            let mut attrs: std::collections::BTreeMap<String, String> = span
                .span_attributes
                .into_iter()
                .filter(|(k, _)| {
                    k.starts_with("redis.") || k.starts_with("rate_limit.") || k == "error" || k == "error.type"
                })
                .collect();

            // Remove dynamic values for snapshot stability
            attrs.remove("redis.pool.available");
            attrs.remove("redis.pool.in_use");

            json!({
                "span_name": span.span_name,
                "status": span.status_code,
                "attributes": attrs,
            })
        })
        .collect();

    insta::assert_json_snapshot!(redis_spans, @r###"
    [
      {
        "span_name": "redis:check_and_consume:global",
        "status": "Unset",
        "attributes": {
          "rate_limit.allowed": "true",
          "rate_limit.interval_ms": "60000",
          "rate_limit.limit": "10",
          "rate_limit.scope": "global",
          "redis.operation": "check_and_consume",
          "redis.pool.size": "1"
        }
      },
      {
        "span_name": "redis:check_and_consume:global",
        "status": "Unset",
        "attributes": {
          "rate_limit.allowed": "true",
          "rate_limit.interval_ms": "60000",
          "rate_limit.limit": "10",
          "rate_limit.scope": "global",
          "redis.operation": "check_and_consume",
          "redis.pool.size": "1"
        }
      },
      {
        "span_name": "redis:check_and_consume:global",
        "status": "Unset",
        "attributes": {
          "rate_limit.allowed": "true",
          "rate_limit.interval_ms": "60000",
          "rate_limit.limit": "10",
          "rate_limit.scope": "global",
          "redis.operation": "check_and_consume",
          "redis.pool.size": "1"
        }
      }
    ]
    "###);
}

#[tokio::test]
async fn redis_ip_rate_limit_creates_span() {
    let service_name = unique_service_name("redis-trace-ip");
    let key_prefix = format!("test_redis_trace_{}:", uuid::Uuid::new_v4());
    let config = create_rate_limit_tracing_config(&service_name, &key_prefix);

    let test_server = TestServer::builder().build(&config).await;

    // Generate trace context
    let trace_id = format!("{:032x}", uuid::Uuid::new_v4().as_u128());
    let span_id = format!("{:016x}", rand::random::<u64>());
    let traceparent = format!("00-{}-{}-01", trace_id, span_id);

    let mut headers = HeaderMap::new();
    headers.insert("traceparent", traceparent.parse().unwrap());
    headers.insert("x-forwarded-for", "192.168.1.100".parse().unwrap());

    let mcp_client = test_server.mcp_client_with_headers("/mcp", headers).await;

    // List tools to trigger IP rate limiting check
    let tools = mcp_client.list_tools().await;

    // Verify the request succeeded
    assert!(!tools.tools.is_empty());

    // Wait for traces to be exported
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // Define a simple row struct for Redis spans
    #[derive(Debug, Deserialize, Row)]
    struct RedisSpanRow {
        #[serde(rename = "SpanName")]
        span_name: String,
        #[serde(rename = "SpanAttributes")]
        span_attributes: Vec<(String, String)>,
        #[serde(rename = "StatusCode")]
        status_code: String,
    }

    // Query ClickHouse for the spans
    let clickhouse = create_clickhouse_client().await;
    let query = format!(
        "SELECT SpanName, SpanAttributes, StatusCode FROM otel_traces WHERE ServiceName = '{}' AND SpanName LIKE 'redis:%' ORDER BY SpanName",
        service_name
    );

    let spans = wait_for_metrics_matching::<RedisSpanRow, _>(&clickhouse, &query, |rows| !rows.is_empty())
        .await
        .expect("Failed to get Redis trace spans");

    // Check that we have Redis spans with IP context
    let redis_spans: Vec<_> = spans
        .into_iter()
        .map(|span| {
            let has_ip_hash = span.span_attributes.iter().any(|(k, _)| k == "client.address_hash");

            let attrs: std::collections::BTreeMap<String, String> = span
                .span_attributes
                .into_iter()
                .filter(|(k, _)| {
                    k.starts_with("redis.")
                        || k.starts_with("rate_limit.")
                        || k == "client.address_hash"
                        || k == "error"
                        || k == "error.type"
                })
                .filter(|(k, _)| {
                    // Remove dynamic values
                    k != "redis.pool.available" && k != "redis.pool.in_use" && k != "client.address_hash" // Hash will be different each time
                })
                .collect();

            json!({
                "span_name": span.span_name,
                "status": span.status_code,
                "attributes": attrs,
                "has_ip_hash": has_ip_hash,
            })
        })
        .collect();

    insta::assert_json_snapshot!(redis_spans, @r#"
    [
      {
        "span_name": "redis:check_and_consume:global",
        "status": "Unset",
        "attributes": {
          "rate_limit.allowed": "true",
          "rate_limit.interval_ms": "60000",
          "rate_limit.limit": "10",
          "rate_limit.scope": "global",
          "redis.operation": "check_and_consume",
          "redis.pool.size": "1"
        },
        "has_ip_hash": false
      },
      {
        "span_name": "redis:check_and_consume:global",
        "status": "Unset",
        "attributes": {
          "rate_limit.allowed": "true",
          "rate_limit.interval_ms": "60000",
          "rate_limit.limit": "10",
          "rate_limit.scope": "global",
          "redis.operation": "check_and_consume",
          "redis.pool.size": "1"
        },
        "has_ip_hash": false
      },
      {
        "span_name": "redis:check_and_consume:global",
        "status": "Unset",
        "attributes": {
          "rate_limit.allowed": "true",
          "rate_limit.interval_ms": "60000",
          "rate_limit.limit": "10",
          "rate_limit.scope": "global",
          "redis.operation": "check_and_consume",
          "redis.pool.size": "1"
        },
        "has_ip_hash": false
      },
      {
        "span_name": "redis:check_and_consume:ip",
        "status": "Unset",
        "attributes": {
          "rate_limit.allowed": "true",
          "rate_limit.interval_ms": "60000",
          "rate_limit.limit": "5",
          "rate_limit.scope": "ip",
          "redis.operation": "check_and_consume",
          "redis.pool.size": "1"
        },
        "has_ip_hash": true
      },
      {
        "span_name": "redis:check_and_consume:ip",
        "status": "Unset",
        "attributes": {
          "rate_limit.allowed": "true",
          "rate_limit.interval_ms": "60000",
          "rate_limit.limit": "5",
          "rate_limit.scope": "ip",
          "redis.operation": "check_and_consume",
          "redis.pool.size": "1"
        },
        "has_ip_hash": true
      },
      {
        "span_name": "redis:check_and_consume:ip",
        "status": "Unset",
        "attributes": {
          "rate_limit.allowed": "true",
          "rate_limit.interval_ms": "60000",
          "rate_limit.limit": "5",
          "rate_limit.scope": "ip",
          "redis.operation": "check_and_consume",
          "redis.pool.size": "1"
        },
        "has_ip_hash": true
      }
    ]
    "#);
}

#[tokio::test]
async fn redis_token_rate_limit_creates_span() {
    let service_name = unique_service_name("redis-trace-token");
    let key_prefix = format!("test_redis_trace_{}:", uuid::Uuid::new_v4());
    let config = formatdoc! {r#"
        [server]
        listen_address = "127.0.0.1:0"

        [server.client_identification]
        enabled = true
        client_id = {{ source = "http_header", http_header = "x-client-id" }}
        group_id = {{ source = "http_header", http_header = "x-client-group" }}
        
        [server.client_identification.validation]
        group_values = ["premium", "basic"]

        [server.rate_limits]
        enabled = true
        
        [server.rate_limits.storage]
        type = "redis"
        url = "redis://localhost:6379"
        key_prefix = "{key_prefix}"

        [telemetry]
        service_name = "{service_name}"

        [telemetry.tracing]
        sampling = 1.0

        [telemetry.exporters.otlp]
        enabled = true
        endpoint = "http://localhost:4317"
        protocol = "grpc"

        [telemetry.exporters.otlp.batch_export]
        scheduled_delay = "100ms"

        [llm]
        enabled = true
        
        [llm.providers.testprovider.rate_limits.per_user]
        input_token_limit = 1000
        interval = "60s"
        
        [llm.providers.testprovider.rate_limits.per_user.groups.premium]
        input_token_limit = 5000
        interval = "60s"
    "#};

    // Setup mock LLM provider (this adds the provider config automatically)
    let mut builder = TestServer::builder();
    let openai_mock = integration_tests::llms::OpenAIMock::new("testprovider").with_models(vec!["gpt-4".to_string()]);
    builder.spawn_llm(openai_mock).await;
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

    // Make a request that will trigger token rate limiting
    let response = test_server
        .client
        .request(reqwest::Method::POST, "/llm/v1/chat/completions")
        .headers(headers)
        .json(&json!({
            "model": "testprovider/gpt-4",
            "messages": [
                {"role": "user", "content": "Hello"}
            ],
            "max_tokens": 10
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    // Wait for traces to be exported
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // Define a simple row struct for Redis spans
    #[derive(Debug, Deserialize, Row)]
    struct RedisSpanRow {
        #[serde(rename = "SpanName")]
        span_name: String,
        #[serde(rename = "SpanAttributes")]
        span_attributes: Vec<(String, String)>,
        #[serde(rename = "StatusCode")]
        status_code: String,
    }

    // Query ClickHouse for the spans
    let clickhouse = create_clickhouse_client().await;
    let query = format!(
        "SELECT SpanName, SpanAttributes, StatusCode FROM otel_traces WHERE ServiceName = '{}' AND SpanName = 'redis:check_and_consume_tokens'",
        service_name
    );

    let spans = wait_for_metrics_matching::<RedisSpanRow, _>(&clickhouse, &query, |rows| !rows.is_empty())
        .await
        .expect("Failed to get Redis trace spans");

    // Check that we have Redis token span
    let redis_spans: Vec<_> = spans
        .into_iter()
        .map(|span| {
            let has_tokens = span.span_attributes.iter().any(|(k, _)| k == "rate_limit.tokens");

            let attrs: std::collections::BTreeMap<String, String> = span
                .span_attributes
                .into_iter()
                .filter(|(k, _)| {
                    k.starts_with("redis.")
                        || k.starts_with("rate_limit.")
                        || k.starts_with("llm.")
                        || k == "error"
                        || k == "error.type"
                    // Note: client.* attributes are no longer added to Redis spans
                })
                .filter(|(k, _)| {
                    // Remove dynamic values
                    k != "redis.pool.available" && k != "redis.pool.in_use" && k != "rate_limit.tokens" // Actual token count may vary
                })
                .collect();

            json!({
                "span_name": span.span_name,
                "status": span.status_code,
                "attributes": attrs,
                "has_tokens": has_tokens,
            })
        })
        .collect();

    insta::assert_json_snapshot!(redis_spans, @r###"
    [
      {
        "span_name": "redis:check_and_consume_tokens",
        "status": "Unset",
        "attributes": {
          "llm.model": "gpt-4",
          "llm.provider": "testprovider",
          "rate_limit.allowed": "true",
          "rate_limit.interval_ms": "60000",
          "rate_limit.limit": "5000",
          "rate_limit.scope": "token",
          "redis.operation": "check_and_consume_tokens",
          "redis.pool.size": "1"
        },
        "has_tokens": true
      }
    ]
    "###);
}

#[tokio::test]
async fn redis_rate_limit_exceeded_span_has_error() {
    let service_name = unique_service_name("redis-trace-exceeded");
    let key_prefix = format!("test_redis_trace_{}:", uuid::Uuid::new_v4());
    let config = formatdoc! {r#"
        [server]
        listen_address = "127.0.0.1:0"

        [server.rate_limits]
        enabled = true
        
        [server.rate_limits.storage]
        type = "redis"
        url = "redis://localhost:6379"
        key_prefix = "{key_prefix}"
        
        [server.rate_limits.global]
        limit = 1
        interval = "60s"

        [telemetry]
        service_name = "{service_name}"

        [telemetry.tracing]
        sampling = 1.0

        [telemetry.exporters.otlp]
        enabled = true
        endpoint = "http://localhost:4317"
        protocol = "grpc"

        [telemetry.exporters.otlp.batch_export]
        scheduled_delay = "100ms"

        [mcp]
        enabled = true
        
        [mcp.servers.dummy]
        cmd = ["echo", "dummy"]
    "#};

    let mut test_server = TestServer::builder().build(&config).await;

    // Generate trace context
    let trace_id = format!("{:032x}", uuid::Uuid::new_v4().as_u128());
    let span_id = format!("{:016x}", rand::random::<u64>());
    let traceparent = format!("00-{}-{}-01", trace_id, span_id);

    // Use raw HTTP requests for this test since we need to control exactly when rate limiting kicks in
    // and MCP client initialization itself makes requests
    test_server.client.push_header("traceparent", &traceparent);

    // Make first request - should succeed
    let response = test_server
        .client
        .post(
            "/mcp",
            &json!({
                "jsonrpc": "2.0",
                "method": "tools/list",
                "id": "test-1"
            }),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    // Make second request - should be rate limited
    let response = test_server
        .client
        .post(
            "/mcp",
            &json!({
                "jsonrpc": "2.0",
                "method": "tools/list",
                "id": "test-2"
            }),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 429);

    // Wait for traces to be exported
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // Define a simple row struct for Redis spans
    #[derive(Debug, Deserialize, Row)]
    struct RedisSpanRow {
        #[serde(rename = "SpanName")]
        span_name: String,
        #[serde(rename = "SpanAttributes")]
        span_attributes: Vec<(String, String)>,
        #[serde(rename = "StatusCode")]
        status_code: String,
    }

    // Query ClickHouse for the spans
    let clickhouse = create_clickhouse_client().await;
    let query = format!(
        "SELECT SpanName, SpanAttributes, StatusCode FROM otel_traces WHERE ServiceName = '{}' AND SpanName LIKE 'redis:%' ORDER BY SpanName",
        service_name
    );

    let spans = wait_for_metrics_matching::<RedisSpanRow, _>(&clickhouse, &query, |rows| !rows.is_empty())
        .await
        .expect("Failed to get Redis trace spans");

    // Check that we have both allowed and blocked spans
    let redis_spans: Vec<_> = spans
        .into_iter()
        .map(|span| {
            let rate_limit_allowed = span
                .span_attributes
                .iter()
                .find(|(k, _)| k == "rate_limit.allowed")
                .map(|(_, v)| v.as_str());

            let has_retry_after = span
                .span_attributes
                .iter()
                .any(|(k, _)| k == "rate_limit.retry_after_ms");

            json!({
                "span_name": span.span_name,
                "status": span.status_code,
                "allowed": rate_limit_allowed,
                "has_retry_after": has_retry_after,
            })
        })
        .collect();

    insta::assert_json_snapshot!(redis_spans, @r#"
    [
      {
        "span_name": "redis:check_and_consume:global",
        "status": "Unset",
        "allowed": "true",
        "has_retry_after": false
      },
      {
        "span_name": "redis:check_and_consume:global",
        "status": "Unset",
        "allowed": "false",
        "has_retry_after": true
      }
    ]
    "#);
}
