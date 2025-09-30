//! Comprehensive LLM tracing tests with inline snapshots

use clickhouse::Row;
use indoc::formatdoc;
use integration_tests::{
    TestServer,
    llms::{AnthropicMock, OpenAIMock},
    telemetry::*,
};
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

        [llm.protocols.anthropic]
        enabled = true
        path = "/llm/anthropic"

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
    let (status, _body) = test_server
        .openai_completions(json!({
            "model": "test_openai/gpt-4",
            "messages": [
                {"role": "system", "content": "You are a helpful assistant."},
                {"role": "user", "content": "What is 2+2?"}
            ],
            "temperature": 0.7,
            "max_tokens": 150
        }))
        .header("x-client-id", &client_id)
        .header("x-client-group", "premium")
        .header("traceparent", &traceparent)
        .send_raw()
        .await;

    assert_eq!(status, 200);

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
    let chunks = test_server
        .openai_completions_stream(json!({
            "model": "test_openai/gpt-3.5-turbo",
            "messages": [
                {"role": "user", "content": "Count from 1 to 5"}
            ],
            "stream": true,
            "temperature": 0.5,
            "max_tokens": 50
        }))
        .header("x-client-id", &client_id)
        .header("x-client-group", "basic")
        .header("traceparent", &traceparent)
        .send()
        .await;

    // Verify we got streaming chunks (the [DONE] marker is filtered out by the streaming parser)
    assert!(!chunks.is_empty(), "Should receive streaming chunks");

    // Check that we got actual content chunks (the [DONE] marker is automatically filtered out)
    let has_content_chunk = chunks.iter().any(|chunk| {
        chunk
            .get("choices")
            .and_then(|choices| choices.as_array())
            .is_some_and(|choices| !choices.is_empty())
    });
    assert!(has_content_chunk, "Should contain streaming content chunks");

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
async fn count_tokens_creates_span() {
    let service_name = unique_service_name("anthropic-trace-count");
    let config = create_llm_tracing_config(&service_name);

    let mut builder = TestServer::builder();
    builder.spawn_llm(AnthropicMock::new("anthropic")).await;
    let test_server = builder.build(&config).await;

    let trace_id = format!("{:032x}", uuid::Uuid::new_v4().as_u128());
    let span_id = format!("{:016x}", rand::random::<u64>());
    let traceparent = format!("00-{}-{}-01", trace_id, span_id);

    let client_id = format!("anthropic-client-{}", uuid::Uuid::new_v4());

    let request = json!({
        "model": "anthropic/claude-3-sonnet-20240229",
        "messages": [
            {"role": "user", "content": "Hello"}
        ],
        "max_tokens": 256
    });

    let (status, _body) = test_server
        .count_tokens(request)
        .header("traceparent", &traceparent)
        .header("x-client-id", &client_id)
        .header("x-client-group", "premium")
        .send_raw()
        .await;

    assert_eq!(status, 200);

    let clickhouse = create_clickhouse_client().await;

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
            AND SpanName = 'llm:count_tokens'
        ORDER BY Timestamp DESC
        LIMIT 10
    "#};

    let spans = wait_for_metrics_matching::<TraceSpanRow, _>(&clickhouse, &query, |rows| {
        rows.iter().any(|row| row.span_name == "llm:count_tokens")
    })
    .await
    .expect("Failed to fetch anthropic count tokens span");

    let mut count_spans: Vec<_> = spans
        .into_iter()
        .filter(|span| span.span_name == "llm:count_tokens")
        .collect();

    for span in &mut count_spans {
        span.span_attributes.retain(|(k, _)| {
            k.starts_with("gen_ai.")
                || k.starts_with("client.")
                || k == "llm.auth_forwarded"
                || k.starts_with("llm.count_tokens")
        });
        span.span_attributes.sort_by(|a, b| a.0.cmp(&b.0));
        for attribute in &mut span.span_attributes {
            match attribute.0.as_str() {
                "client.group" => attribute.1 = "[CLIENT_GROUP]".to_string(),
                "client.id" => attribute.1 = "[CLIENT_ID]".to_string(),
                "llm.count_tokens.cache_creation" => attribute.1 = "[CACHE_CREATION]".to_string(),
                "llm.count_tokens.cache_read" => attribute.1 = "[CACHE_READ]".to_string(),
                "llm.count_tokens.input" => attribute.1 = "[INPUT_TOKENS]".to_string(),
                _ => {}
            }
        }
    }

    for span in &mut count_spans {
        span.trace_id = "[TRACE_ID]".to_string();
        span.span_id = "[SPAN_ID]".to_string();
        span.parent_span_id = "[PARENT_SPAN_ID]".to_string();
        span.service_name = "[SERVICE_NAME]".to_string();
    }

    insta::assert_json_snapshot!(count_spans, @r#"
    [
      {
        "TraceId": "[TRACE_ID]",
        "SpanId": "[SPAN_ID]",
        "ParentSpanId": "[PARENT_SPAN_ID]",
        "SpanName": "llm:count_tokens",
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
            "gen_ai.request.model",
            "anthropic/claude-3-sonnet-20240229"
          ],
          [
            "llm.auth_forwarded",
            "false"
          ],
          [
            "llm.count_tokens.cache_creation",
            "[CACHE_CREATION]"
          ],
          [
            "llm.count_tokens.cache_read",
            "[CACHE_READ]"
          ],
          [
            "llm.count_tokens.input",
            "[INPUT_TOKENS]"
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
    let (status, _body) = test_server
        .openai_completions(json!({
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
        .header("x-client-id", "tools-test-client")
        .header("traceparent", &traceparent)
        .send_raw()
        .await;

    assert_eq!(status, 200);

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
async fn llm_span_has_http_parent() {
    let service_name = unique_service_name("llm-trace-hierarchy");
    let config = create_llm_tracing_config(&service_name);

    // Setup mock LLM provider
    let mut builder = TestServer::builder();
    builder.spawn_llm(OpenAIMock::new("test_openai")).await;
    let test_server = builder.build(&config).await;

    // Generate trace context
    let trace_id = format!("{:032x}", uuid::Uuid::new_v4().as_u128());
    let parent_span_id = format!("{:016x}", rand::random::<u64>());
    let traceparent = format!("00-{}-{}-01", trace_id, parent_span_id);

    // Make a chat completion request
    let (status, _body) = test_server
        .openai_completions(json!({
            "model": "test_openai/gpt-4",
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        }))
        .header("traceparent", &traceparent)
        .header("x-client-id", "test-client")
        .send_raw()
        .await;

    assert_eq!(status, 200);

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
        // We need at least 2 spans: HTTP and LLM
        rows.len() >= 2
    })
    .await
    .expect("Failed to get trace spans");

    // Find the HTTP span and LLM span
    let http_span = spans
        .iter()
        .find(|s| s.span_name.starts_with("POST "))
        .expect("HTTP span not found");
    let llm_span = spans
        .iter()
        .find(|s| s.span_name == "llm:chat_completion")
        .expect("LLM span not found");

    // Verify trace hierarchy:
    // 1. Both spans should have the same trace ID
    assert_eq!(http_span.trace_id, trace_id);
    assert_eq!(llm_span.trace_id, trace_id);

    // 2. HTTP span should have the external parent
    assert_eq!(http_span.parent_span_id, parent_span_id);

    // 3. LLM span should have HTTP span as parent
    assert_eq!(
        llm_span.parent_span_id, http_span.span_id,
        "LLM span should be a child of the HTTP span"
    );
}

#[tokio::test]
async fn llm_span_has_http_parent_stream() {
    let service_name = unique_service_name("llm-trace-hierarchy-stream");
    let config = create_llm_tracing_config(&service_name);

    // Setup mock LLM provider with streaming support
    let mut builder = TestServer::builder();
    let mock = OpenAIMock::new("test_openai").with_streaming();
    builder.spawn_llm(mock).await;
    let test_server = builder.build(&config).await;

    // Generate trace context
    let trace_id = format!("{:032x}", uuid::Uuid::new_v4().as_u128());
    let parent_span_id = format!("{:016x}", rand::random::<u64>());
    let traceparent = format!("00-{}-{}-01", trace_id, parent_span_id);

    // Make a streaming chat completion request
    let chunks = test_server
        .openai_completions_stream(json!({
            "model": "test_openai/gpt-3.5-turbo",
            "messages": [
                {"role": "user", "content": "Hello"}
            ],
            "stream": true
        }))
        .header("traceparent", &traceparent)
        .header("x-client-id", "test-client")
        .send()
        .await;

    assert!(!chunks.is_empty(), "Should receive streaming chunks");

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
        // We need at least 2 spans: HTTP and LLM
        rows.len() >= 2
    })
    .await
    .expect("Failed to get trace spans");

    // Find the HTTP span and LLM streaming span
    let http_span = spans
        .iter()
        .find(|s| s.span_name.starts_with("POST "))
        .expect("HTTP span not found");
    let llm_span = spans
        .iter()
        .find(|s| s.span_name == "llm:chat_completion_stream")
        .expect("LLM streaming span not found");

    // Verify trace hierarchy:
    // 1. Both spans should have the same trace ID
    assert_eq!(http_span.trace_id, trace_id);
    assert_eq!(llm_span.trace_id, trace_id);

    // 2. HTTP span should have the external parent
    assert_eq!(http_span.parent_span_id, parent_span_id);

    // 3. LLM streaming span should have HTTP span as parent
    assert_eq!(
        llm_span.parent_span_id, http_span.span_id,
        "LLM streaming span should be a child of the HTTP span"
    );
}

#[tokio::test]
async fn count_tokens_span_has_http_parent() {
    let service_name = unique_service_name("anthropic-trace-hierarchy");
    let config = create_llm_tracing_config(&service_name);

    let mut builder = TestServer::builder();
    builder.spawn_llm(AnthropicMock::new("anthropic")).await;
    let test_server = builder.build(&config).await;

    let trace_id = format!("{:032x}", uuid::Uuid::new_v4().as_u128());
    let parent_span_id = format!("{:016x}", rand::random::<u64>());
    let traceparent = format!("00-{}-{}-01", trace_id, parent_span_id);

    let request = json!({
        "model": "anthropic/claude-3-sonnet-20240229",
        "messages": [
            {"role": "user", "content": "Trace hierarchy"}
        ],
        "max_tokens": 128
    });

    let (status, _body) = test_server
        .count_tokens(request)
        .header("traceparent", &traceparent)
        .header("x-client-id", "hierarchy-client")
        .header("x-client-group", "premium")
        .send_raw()
        .await;

    assert_eq!(status, 200);

    let clickhouse = create_clickhouse_client().await;

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
    }

    let query = formatdoc! {r#"
        SELECT
            TraceId,
            SpanId,
            ParentSpanId,
            SpanName
        FROM otel_traces
        WHERE
            ServiceName = '{service_name}'
            AND TraceId = '{trace_id}'
        ORDER BY Timestamp ASC
        LIMIT 10
    "#};

    let spans = wait_for_metrics_matching::<SimpleSpanRow, _>(&clickhouse, &query, |rows| rows.len() >= 2)
        .await
        .expect("Failed to fetch hierarchy spans");

    let http_span = spans
        .iter()
        .find(|span| span.span_name.starts_with("POST "))
        .expect("HTTP span not found");
    let count_span = spans
        .iter()
        .find(|span| span.span_name == "llm:count_tokens")
        .expect("Count tokens span not found");

    assert_eq!(http_span.trace_id, trace_id);
    assert_eq!(count_span.trace_id, trace_id);
    assert_eq!(http_span.parent_span_id, parent_span_id);
    assert_eq!(
        count_span.parent_span_id, http_span.span_id,
        "Count tokens span should be child of HTTP span"
    );
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
    let (status, _body) = test_server
        .openai_completions(json!({
            "model": "test_openai/gpt-4",
            "messages": [
                {"role": "user", "content": "This will fail"}
            ]
        }))
        .header("x-client-id", "error-test-client")
        .header("traceparent", &traceparent)
        .send_raw()
        .await;

    assert_eq!(status, 401);

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
