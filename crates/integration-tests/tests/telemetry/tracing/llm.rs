//! Comprehensive LLM tracing tests with inline snapshots

use clickhouse::Row;
use indoc::formatdoc;
use integration_tests::{TestServer, llms::OpenAIMock, telemetry::*};
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

fn create_llm_tracing_config(service_name: &str) -> String {
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

        [llm]
        enabled = true

        [llm.protocols.openai]
enabled = true
path = "/llm"
    "#}
}

#[tokio::test]
async fn llm_chat_completion_creates_span() {
    let service_name = unique_service_name("llm-trace-basic");
    let config = create_llm_tracing_config(&service_name);

    // Setup mock LLM provider
    let mut builder = TestServer::builder();
    builder.spawn_llm(OpenAIMock::new("test_openai")).await;
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

    // Make a chat completion request
    let response = test_server
        .client
        .request(reqwest::Method::POST, "/llm/v1/chat/completions")
        .headers(headers)
        .json(&json!({
            "model": "test_openai/gpt-4",
            "messages": [
                {"role": "system", "content": "You are a helpful assistant."},
                {"role": "user", "content": "What is 2+2?"}
            ],
            "temperature": 0.7,
            "max_tokens": 150
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let clickhouse = create_clickhouse_client().await;

    // Query for LLM-specific spans
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
            AND SpanName = 'llm:chat_completion'
        ORDER BY Timestamp DESC
        LIMIT 10
    "#};

    let spans = wait_for_metrics_matching::<TraceSpanRow, _>(&clickhouse, &query, |rows| {
        rows.iter().any(|r| r.span_name == "llm:chat_completion")
    })
    .await
    .expect("Failed to get LLM trace spans");

    // Filter to get LLM-specific attributes
    let mut llm_spans: Vec<_> = spans
        .into_iter()
        .filter(|s| s.span_name == "llm:chat_completion")
        .collect();

    // Clean up dynamic attributes for snapshot
    for span in &mut llm_spans {
        span.span_attributes
            .retain(|(k, _)| k.starts_with("gen_ai.") || k.starts_with("client.") || k == "llm.auth_forwarded");
        // Sort attributes for consistent snapshots
        span.span_attributes.sort_by(|a, b| a.0.cmp(&b.0));
    }

    insta::assert_json_snapshot!(llm_spans, {
        "[].TraceId" => "[TRACE_ID]",
        "[].SpanId" => "[SPAN_ID]",
        "[].ParentSpanId" => "[PARENT_SPAN_ID]",
        "[].ServiceName" => "[SERVICE_NAME]",
        "[].SpanAttributes[0][1]" => "[CLIENT_GROUP]",
        "[].SpanAttributes[1][1]" => "[CLIENT_ID]"
    }, @r#"
    [
      {
        "TraceId": "[TRACE_ID]",
        "SpanId": "[SPAN_ID]",
        "ParentSpanId": "[PARENT_SPAN_ID]",
        "SpanName": "llm:chat_completion",
        "ServiceName": "[SERVICE_NAME]",
        "SpanAttributes": [
          [
            "client.group",
            "[CLIENT_GROUP]"
          ],
          [
            "client.id",
            "[CLIENT_ID]"
          ],
          [
            "gen_ai.request.max_tokens",
            "150"
          ],
          [
            "gen_ai.request.model",
            "test_openai/gpt-4"
          ],
          [
            "gen_ai.request.temperature",
            "0.7"
          ],
          [
            "gen_ai.response.finish_reason",
            "stop"
          ],
          [
            "gen_ai.response.model",
            "test_openai/gpt-4"
          ],
          [
            "gen_ai.usage.input_tokens",
            "10"
          ],
          [
            "gen_ai.usage.output_tokens",
            "15"
          ],
          [
            "gen_ai.usage.total_tokens",
            "25"
          ],
          [
            "llm.auth_forwarded",
            "false"
          ]
        ],
        "StatusCode": "Unset"
      }
    ]
    "#);

    // Also verify HTTP span exists and is connected
    let http_query = formatdoc! {r#"
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
            AND SpanName LIKE 'POST%'
        LIMIT 1
    "#};

    let http_spans = wait_for_metrics_matching::<TraceSpanRow, _>(&clickhouse, &http_query, |rows| !rows.is_empty())
        .await
        .expect("Failed to get HTTP trace span");

    assert_eq!(http_spans.len(), 1);
    assert_eq!(http_spans[0].trace_id, trace_id);
}

#[tokio::test]
async fn llm_streaming_completion_creates_span() {
    let service_name = unique_service_name("llm-trace-stream");
    let config = create_llm_tracing_config(&service_name);

    // Setup mock LLM provider with streaming support
    let mut builder = TestServer::builder();
    let mock = OpenAIMock::new("test_openai").with_streaming();
    builder.spawn_llm(mock).await;
    let test_server = builder.build(&config).await;

    // Generate trace context
    let trace_id = format!("{:032x}", uuid::Uuid::new_v4().as_u128());
    let span_id = format!("{:016x}", rand::random::<u64>());
    let traceparent = format!("00-{}-{}-01", trace_id, span_id);

    let client_id = format!("stream-client-{}", uuid::Uuid::new_v4());
    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", client_id.parse().unwrap());
    headers.insert("x-client-group", "basic".parse().unwrap());
    headers.insert("traceparent", traceparent.parse().unwrap());

    // Make a streaming chat completion request
    let response = test_server
        .client
        .request(reqwest::Method::POST, "/llm/v1/chat/completions")
        .headers(headers)
        .json(&json!({
            "model": "test_openai/gpt-3.5-turbo",
            "messages": [
                {"role": "user", "content": "Count from 1 to 5"}
            ],
            "stream": true,
            "temperature": 0.5,
            "max_tokens": 50
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    // Consume the stream
    let body = response.text().await.unwrap();
    assert!(body.contains("data:"));
    assert!(body.contains("[DONE]"));

    let clickhouse = create_clickhouse_client().await;

    // Query for streaming LLM spans
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
            AND SpanName = 'llm:chat_completion_stream'
        ORDER BY Timestamp DESC
        LIMIT 10
    "#};

    let spans = wait_for_metrics_matching::<TraceSpanRow, _>(&clickhouse, &query, |rows| {
        rows.iter().any(|r| r.span_name == "llm:chat_completion_stream")
    })
    .await
    .expect("Failed to get streaming LLM trace spans");

    // Filter to get streaming-specific attributes
    let mut stream_spans: Vec<_> = spans
        .into_iter()
        .filter(|s| s.span_name == "llm:chat_completion_stream")
        .collect();

    // Clean up dynamic attributes for snapshot
    for span in &mut stream_spans {
        span.span_attributes.retain(|(k, _)| {
            k.starts_with("gen_ai.") || k.starts_with("client.") || k == "llm.auth_forwarded" || k == "llm.stream"
        });
        // Sort attributes for consistent snapshots
        span.span_attributes.sort_by(|a, b| a.0.cmp(&b.0));
    }

    insta::assert_json_snapshot!(stream_spans, {
        "[].TraceId" => "[TRACE_ID]",
        "[].SpanId" => "[SPAN_ID]",
        "[].ParentSpanId" => "[PARENT_SPAN_ID]",
        "[].ServiceName" => "[SERVICE_NAME]",
        "[].SpanAttributes[0][1]" => "[CLIENT_GROUP]",
        "[].SpanAttributes[1][1]" => "[CLIENT_ID]"
    }, @r#"
    [
      {
        "TraceId": "[TRACE_ID]",
        "SpanId": "[SPAN_ID]",
        "ParentSpanId": "[PARENT_SPAN_ID]",
        "SpanName": "llm:chat_completion_stream",
        "ServiceName": "[SERVICE_NAME]",
        "SpanAttributes": [
          [
            "client.group",
            "[CLIENT_GROUP]"
          ],
          [
            "client.id",
            "[CLIENT_ID]"
          ],
          [
            "gen_ai.request.max_tokens",
            "50"
          ],
          [
            "gen_ai.request.model",
            "test_openai/gpt-3.5-turbo"
          ],
          [
            "gen_ai.request.temperature",
            "0.5"
          ],
          [
            "gen_ai.response.finish_reason",
            "stop"
          ],
          [
            "gen_ai.response.model",
            "test_openai/gpt-3.5-turbo"
          ],
          [
            "gen_ai.usage.input_tokens",
            "10"
          ],
          [
            "gen_ai.usage.output_tokens",
            "15"
          ],
          [
            "gen_ai.usage.total_tokens",
            "25"
          ],
          [
            "llm.auth_forwarded",
            "false"
          ],
          [
            "llm.stream",
            "true"
          ]
        ],
        "StatusCode": "Unset"
      }
    ]
    "#);
}

#[tokio::test]
async fn llm_with_tools_adds_tool_attributes() {
    let service_name = unique_service_name("llm-trace-tools");
    let config = create_llm_tracing_config(&service_name);

    // Setup mock LLM provider
    let mut builder = TestServer::builder();
    builder.spawn_llm(OpenAIMock::new("test_openai")).await;
    let test_server = builder.build(&config).await;

    // Generate trace context
    let trace_id = format!("{:032x}", uuid::Uuid::new_v4().as_u128());
    let span_id = format!("{:016x}", rand::random::<u64>());
    let traceparent = format!("00-{}-{}-01", trace_id, span_id);

    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", "tools-test-client".parse().unwrap());
    headers.insert("traceparent", traceparent.parse().unwrap());

    // Make a chat completion request with tools
    let response = test_server
        .client
        .request(reqwest::Method::POST, "/llm/v1/chat/completions")
        .headers(headers)
        .json(&json!({
            "model": "test_openai/gpt-4",
            "messages": [
                {"role": "user", "content": "What's the weather like?"}
            ],
            "tools": [
                {
                    "type": "function",
                    "function": {
                        "name": "get_weather",
                        "description": "Get weather for a location",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "location": {"type": "string"}
                            }
                        }
                    }
                },
                {
                    "type": "function",
                    "function": {
                        "name": "get_news",
                        "description": "Get latest news",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "topic": {"type": "string"}
                            }
                        }
                    }
                }
            ],
            "tool_choice": "auto"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let clickhouse = create_clickhouse_client().await;

    // Query for LLM spans with tools
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
            AND SpanName = 'llm:chat_completion'
        ORDER BY Timestamp DESC
        LIMIT 10
    "#};

    let spans = wait_for_metrics_matching::<TraceSpanRow, _>(&clickhouse, &query, |rows| {
        rows.iter().any(|r| r.span_name == "llm:chat_completion")
    })
    .await
    .expect("Failed to get LLM trace spans with tools");

    // Filter to get tool-related attributes
    let mut tool_spans: Vec<_> = spans
        .into_iter()
        .filter(|s| s.span_name == "llm:chat_completion")
        .collect();

    // Clean up dynamic attributes for snapshot - only keep tool-related ones
    for span in &mut tool_spans {
        span.span_attributes.retain(|(k, _)| {
            k == "gen_ai.request.has_tools" || k == "gen_ai.request.tool_count" || k == "gen_ai.request.model"
        });
        span.span_attributes.sort_by(|a, b| a.0.cmp(&b.0));
    }

    insta::assert_json_snapshot!(tool_spans, {
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
        "SpanName": "llm:chat_completion",
        "ServiceName": "[SERVICE_NAME]",
        "SpanAttributes": [
          [
            "gen_ai.request.has_tools",
            "true"
          ],
          [
            "gen_ai.request.model",
            "test_openai/gpt-4"
          ],
          [
            "gen_ai.request.tool_count",
            "2"
          ]
        ],
        "StatusCode": "Unset"
      }
    ]
    "#);
}

#[tokio::test]
async fn llm_error_creates_span_with_error_attributes() {
    let service_name = unique_service_name("llm-trace-error");
    let config = create_llm_tracing_config(&service_name);

    // Setup mock LLM provider that will fail
    let mut builder = TestServer::builder();
    let mock = OpenAIMock::new("test_openai").with_auth_error("Invalid API key");
    builder.spawn_llm(mock).await;
    let test_server = builder.build(&config).await;

    // Generate trace context
    let trace_id = format!("{:032x}", uuid::Uuid::new_v4().as_u128());
    let span_id = format!("{:016x}", rand::random::<u64>());
    let traceparent = format!("00-{}-{}-01", trace_id, span_id);

    let mut headers = HeaderMap::new();
    headers.insert("x-client-id", "error-test-client".parse().unwrap());
    headers.insert("traceparent", traceparent.parse().unwrap());

    // Make a chat completion request that will fail
    let response = test_server
        .client
        .request(reqwest::Method::POST, "/llm/v1/chat/completions")
        .headers(headers)
        .json(&json!({
            "model": "test_openai/gpt-4",
            "messages": [
                {"role": "user", "content": "This will fail"}
            ]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 401);

    let clickhouse = create_clickhouse_client().await;

    // Query for error spans
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
            AND SpanName = 'llm:chat_completion'
        ORDER BY Timestamp DESC
        LIMIT 10
    "#};

    let spans = wait_for_metrics_matching::<TraceSpanRow, _>(&clickhouse, &query, |rows| {
        rows.iter().any(|r| r.span_name == "llm:chat_completion")
    })
    .await
    .expect("Failed to get error LLM trace spans");

    // Filter to get error-related attributes
    let mut error_spans: Vec<_> = spans
        .into_iter()
        .filter(|s| s.span_name == "llm:chat_completion")
        .collect();

    // Clean up dynamic attributes for snapshot - only keep error-related ones
    for span in &mut error_spans {
        span.span_attributes
            .retain(|(k, _)| k == "error" || k == "error.type" || k == "gen_ai.request.model");
        span.span_attributes.sort_by(|a, b| a.0.cmp(&b.0));
    }

    insta::assert_json_snapshot!(error_spans, {
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
        "SpanName": "llm:chat_completion",
        "ServiceName": "[SERVICE_NAME]",
        "SpanAttributes": [
          [
            "error",
            "true"
          ],
          [
            "error.type",
            "authentication_error"
          ],
          [
            "gen_ai.request.model",
            "test_openai/gpt-4"
          ]
        ],
        "StatusCode": "Unset"
      }
    ]
    "#);
}
