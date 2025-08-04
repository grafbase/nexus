#![allow(clippy::panic)]

use std::sync::Arc;

use indoc::indoc;
use integration_tests::{TestServer, TestService, tools::AdderTool};
use serde_json::json;

#[tokio::test]
async fn global_rate_limit_basic() {
    let config = indoc! {r#"
        [server.rate_limits]
        enabled = true

        [server.rate_limits.global]
        limit = 3
        duration = "10s"
    "#};

    let server = TestServer::builder().build(config).await;

    // Make multiple rapid requests to trigger global rate limit
    let mut results = Vec::new();
    for i in 1..=6 {
        let response = server
            .client
            .request(reqwest::Method::POST, "/mcp")
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .json(&json!({
                "jsonrpc": "2.0",
                "method": "tools/list",
                "id": i
            }))
            .send()
            .await
            .unwrap();
        results.push(json!({
            "request": i,
            "status": response.status().as_u16(),
            "retry_after": response.headers().get("retry-after").map(|h| h.to_str().unwrap_or("invalid"))
        }));
    }

    // Verify we got some successful requests and then hit rate limit
    let success_count = results.iter().filter(|r| r["status"] == 200).count();
    let rate_limited_count = results.iter().filter(|r| r["status"] == 429).count();

    assert!(
        success_count >= 2,
        "Should have at least 2 successful requests, got {success_count}"
    );
    assert!(
        rate_limited_count >= 2,
        "Should have at least 2 rate-limited requests, got {rate_limited_count}"
    );
    assert_eq!(
        success_count + rate_limited_count,
        6,
        "All requests should either succeed or be rate-limited"
    );
}

#[tokio::test]
async fn per_ip_rate_limit_basic() {
    let config = indoc! {r#"
        [server.rate_limits]
        enabled = true

        [server.rate_limits.per_ip]
        limit = 2
        duration = "10s"

        [mcp]
        enabled = true
        path = "/mcp"
    "#};

    let server = TestServer::builder().build(config).await;

    // Test requests from different IPs using X-Forwarded-For header
    let mut results = Vec::new();

    // IP 1: Should hit limit after 2 requests
    for i in 1..=4 {
        let response = server
            .client
            .request(reqwest::Method::POST, "/mcp")
            .header("X-Forwarded-For", "192.168.1.1")
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .json(&json!({
                "jsonrpc": "2.0",
                "method": "tools/list",
                "id": i
            }))
            .send()
            .await
            .unwrap();

        results.push(json!({
            "ip": "192.168.1.1",
            "request": i,
            "status": response.status().as_u16(),
            "retry_after": response.headers().get("retry-after").map(|h| h.to_str().unwrap_or("invalid"))
        }));
    }

    // IP 2: Should have independent limit
    for i in 1..=3 {
        let response = server
            .client
            .request(reqwest::Method::POST, "/mcp")
            .header("X-Forwarded-For", "192.168.1.2")
            .header("Content-Type", "application/json")
            .json(&json!({
                "jsonrpc": "2.0",
                "method": "tools/list",
                "id": i
            }))
            .send()
            .await
            .unwrap();

        results.push(json!({
            "ip": "192.168.1.2",
            "request": i,
            "status": response.status().as_u16(),
            "retry_after": response.headers().get("retry-after").map(|h| h.to_str().unwrap_or("invalid"))
        }));
    }

    insta::assert_json_snapshot!(results, @r#"
    [
      {
        "ip": "192.168.1.1",
        "request": 1,
        "status": 200,
        "retry_after": null
      },
      {
        "ip": "192.168.1.1",
        "request": 2,
        "status": 200,
        "retry_after": null
      },
      {
        "ip": "192.168.1.1",
        "request": 3,
        "status": 429,
        "retry_after": "0"
      },
      {
        "ip": "192.168.1.1",
        "request": 4,
        "status": 429,
        "retry_after": "0"
      },
      {
        "ip": "192.168.1.2",
        "request": 1,
        "status": 406,
        "retry_after": null
      },
      {
        "ip": "192.168.1.2",
        "request": 2,
        "status": 406,
        "retry_after": null
      },
      {
        "ip": "192.168.1.2",
        "request": 3,
        "status": 429,
        "retry_after": "0"
      }
    ]
    "#);
}

#[tokio::test]
async fn mcp_server_rate_limit() {
    let mut builder = TestServer::builder();

    // Create a test service with tools
    let mut service = TestService::streamable_http("test_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let config = indoc! {r#"
        [server.rate_limits]
        enabled = true

        [mcp.servers.test_server.rate_limits]
        limit = 2
        duration = "10s"
    "#};

    let server = builder.build(config).await;
    let mcp_client = server.mcp_client("/mcp").await;

    // Test MCP server-level rate limits
    let mut results = Vec::new();

    // First 2 requests should succeed
    for i in 1..=2 {
        let call_result = mcp_client.execute("test_server__adder", json!({"a": 1, "b": 2})).await;
        results.push(json!({
            "request": i,
            "success": true,
            "content_count": call_result.content.len(),
            "result": call_result.content[0].raw.as_text().unwrap().text
        }));
    }

    // Next 2 requests should be rate limited
    for i in 3..=4 {
        let error = mcp_client
            .execute_expect_error("test_server__adder", json!({"a": 1, "b": 2}))
            .await;
        results.push(json!({
            "request": i,
            "success": false,
            "error": format!("{error:?}")
        }));
    }

    insta::assert_json_snapshot!(results, @r#"
    [
      {
        "request": 1,
        "success": true,
        "content_count": 1,
        "result": "1 + 2 = 3"
      },
      {
        "request": 2,
        "success": true,
        "content_count": 1,
        "result": "1 + 2 = 3"
      },
      {
        "request": 3,
        "success": false,
        "error": "McpError(ErrorData { code: ErrorCode(-32603), message: \"Rate limit exceeded\", data: None })"
      },
      {
        "request": 4,
        "success": false,
        "error": "McpError(ErrorData { code: ErrorCode(-32603), message: \"Rate limit exceeded\", data: None })"
      }
    ]
    "#);
}

#[tokio::test]
async fn mcp_tool_specific_rate_limit() {
    let mut builder = TestServer::builder();

    // Create a test service with tools
    let mut service = TestService::streamable_http("test_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let config = indoc! {r#"
        [server.rate_limits]
        enabled = true

        [mcp.servers.test_server.rate_limits]
        limit = 10
        duration = "10s"

        [mcp.servers.test_server.rate_limits.tools]
        adder = { limit = 2, duration = "10s" }
    "#};

    let server = builder.build(config).await;
    let mcp_client = server.mcp_client("/mcp").await;

    // Test tool-specific rate limits (should override server limit)
    let mut results = Vec::new();

    // First 2 requests should succeed
    for i in 1..=2 {
        let call_result = mcp_client.execute("test_server__adder", json!({"a": i, "b": 10})).await;
        results.push(json!({
            "request": i,
            "success": true,
            "result": call_result.content[0].raw.as_text().unwrap().text
        }));
    }

    // Next 2 requests should be rate limited
    for i in 3..=4 {
        let error = mcp_client
            .execute_expect_error("test_server__adder", json!({"a": i, "b": 10}))
            .await;
        results.push(json!({
            "request": i,
            "success": false,
            "error": format!("{error:?}")
        }));
    }

    insta::assert_json_snapshot!(results, @r#"
    [
      {
        "request": 1,
        "success": true,
        "result": "1 + 10 = 11"
      },
      {
        "request": 2,
        "success": true,
        "result": "2 + 10 = 12"
      },
      {
        "request": 3,
        "success": false,
        "error": "McpError(ErrorData { code: ErrorCode(-32603), message: \"Rate limit exceeded\", data: None })"
      },
      {
        "request": 4,
        "success": false,
        "error": "McpError(ErrorData { code: ErrorCode(-32603), message: \"Rate limit exceeded\", data: None })"
      }
    ]
    "#);
}

#[tokio::test]
async fn mcp_only_rate_limits_no_http_middleware() {
    let mut builder = TestServer::builder();

    // Create a test service with tools
    let mut service = TestService::streamable_http("test_server".to_string());
    service.add_tool(AdderTool);
    builder.spawn_service(service).await;

    let config = indoc! {r#"
        [server.rate_limits]
        enabled = true

        [mcp.servers.test_server.rate_limits]
        limit = 1
        duration = "10s"
    "#};

    let server = builder.build(config).await;

    // HTTP endpoints should NOT be rate limited (no HTTP middleware applied)
    let mut http_success_count = 0;

    for _ in 1..=10 {
        let response = server
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

        if response.status() == 200 {
            http_success_count += 1;
        }
    }

    // MCP endpoints SHOULD be rate limited after 1 request
    let mcp_client = server.mcp_client("/mcp").await;
    let first_result = mcp_client.execute("test_server__adder", json!({"a": 1, "b": 2})).await;

    let second_error = mcp_client
        .execute_expect_error("test_server__adder", json!({"a": 1, "b": 2}))
        .await;

    insta::assert_json_snapshot!(json!({
        "http_requests_successful": http_success_count,
        "http_all_succeeded": http_success_count == 10,
        "mcp_first_request_success": true,
        "mcp_first_result": first_result.content[0].raw.as_text().unwrap().text,
        "mcp_second_request_failed": true,
        "mcp_second_error": format!("{second_error:?}")
    }), @r#"
    {
      "http_requests_successful": 10,
      "http_all_succeeded": true,
      "mcp_first_request_success": true,
      "mcp_first_result": "1 + 2 = 3",
      "mcp_second_request_failed": true,
      "mcp_second_error": "McpError(ErrorData { code: ErrorCode(-32603), message: \"Rate limit exceeded\", data: None })"
    }
    "#);
}

#[tokio::test]
async fn rate_limiting_disabled() {
    let config = indoc! {r#"
        [server.rate_limits]
        enabled = false
    "#};

    let server = TestServer::builder().build(config).await;

    // Make many requests - all should succeed when rate limiting is disabled
    let mut success_count = 0;
    for _ in 1..=20 {
        let response = server
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
        if response.status() == 200 {
            success_count += 1;
        }
    }

    insta::assert_json_snapshot!(json!({
        "total_requests": 20,
        "successful_requests": success_count,
        "all_succeeded": success_count == 20
    }), @r#"
    {
      "total_requests": 20,
      "successful_requests": 20,
      "all_succeeded": true
    }
    "#);
}

#[tokio::test]
async fn mixed_rate_limits() {
    let config = indoc! {r#"
        [server.rate_limits]
        enabled = true

        [server.rate_limits.storage]
        type = "memory"

        [server.rate_limits.global]
        limit = 10
        duration = "10s"

        [server.rate_limits.per_ip]
        limit = 5
        duration = "10s"
    "#};

    let server = TestServer::builder().build(config).await;

    // Test that per-IP limit kicks in before global limit
    let mut results = Vec::new();
    for i in 1..=8 {
        let response = server
            .client
            .request(reqwest::Method::POST, "/mcp")
            .header("X-Forwarded-For", "10.0.0.1")
            .json(&json!({
                "jsonrpc": "2.0",
                "method": "tools/list",
                "id": i
            }))
            .send()
            .await
            .unwrap();

        results.push(json!({
            "request": i,
            "status": response.status().as_u16(),
            "limit_type": if response.status() == 429 { "rate_limited" } else { "allowed" }
        }));
    }

    insta::assert_json_snapshot!(results, @r#"
    [
      {
        "request": 1,
        "status": 406,
        "limit_type": "allowed"
      },
      {
        "request": 2,
        "status": 406,
        "limit_type": "allowed"
      },
      {
        "request": 3,
        "status": 406,
        "limit_type": "allowed"
      },
      {
        "request": 4,
        "status": 406,
        "limit_type": "allowed"
      },
      {
        "request": 5,
        "status": 406,
        "limit_type": "allowed"
      },
      {
        "request": 6,
        "status": 429,
        "limit_type": "rate_limited"
      },
      {
        "request": 7,
        "status": 429,
        "limit_type": "rate_limited"
      },
      {
        "request": 8,
        "status": 429,
        "limit_type": "rate_limited"
      }
    ]
    "#);
}

#[tokio::test]
async fn concurrent_memory_rate_limiting() {
    let config = indoc! {r#"
        [server.rate_limits]
        enabled = true

        [server.rate_limits.storage]
        type = "memory"

        [server.rate_limits.global]
        limit = 10
        duration = "5s"
    "#};

    let server = Arc::new(TestServer::builder().build(config).await);

    // Launch 20 concurrent requests when limit is 10
    let mut handles = vec![];

    for _ in 0..20 {
        let server_clone = Arc::clone(&server);
        let handle = tokio::spawn(async move {
            server_clone
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
                .unwrap()
                .status()
        });
        handles.push(handle);
    }

    // Wait for all requests to complete
    let mut success_count = 0;
    let mut rate_limited_count = 0;

    for handle in handles {
        match handle.await.unwrap().as_u16() {
            200 => success_count += 1,
            429 => rate_limited_count += 1,
            status => panic!("Unexpected status code: {status}"),
        }
    }

    // With memory storage (governor crate), which uses per-second quotas,
    // the behavior is different from Redis. With 10 limit over 5 seconds,
    // it allows 2 per second, so concurrent requests might get less through
    assert!(
        (2..=10).contains(&success_count),
        "Expected 2-10 successful requests with memory storage, got {success_count}"
    );
    assert_eq!(success_count + rate_limited_count, 20, "Total should be 20");
}
