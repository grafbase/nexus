#![allow(clippy::panic)]

use indoc::indoc;
use integration_tests::{TestServer, TestService, tools};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn basic_redis_rate_limiting() {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();

    let config = indoc::formatdoc! {r#"
        [server.rate_limit]
        enabled = true

        [server.rate_limit.storage]
        type = "redis"
        url = "redis://localhost:6379/0"
        key_prefix = "test_basic_redis_{timestamp}:"

        [server.rate_limit.global]
        limit = 5
        duration = "60s"
    "#};

    let server = TestServer::builder().build(&config).await;

    // Make requests up to the limit
    let mut success_count = 0;
    let mut rate_limited = false;

    for _ in 0..7 {
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
        } else if response.status() == 429 {
            rate_limited = true;
            assert!(response.headers().contains_key("retry-after"));
            break;
        }
    }

    assert!(rate_limited, "Rate limit should have been hit");
    assert!(
        success_count >= 4,
        "At least 4 requests should have succeeded, got {success_count}"
    );
}

#[tokio::test]
async fn redis_per_server_rate_limiting() {
    let mut builder = TestServer::builder();

    // Create test services
    let mut limited_service = TestService::streamable_http("limited_server".to_string());
    limited_service.add_tool(tools::AdderTool);
    builder.spawn_service(limited_service).await;

    let mut unlimited_service = TestService::streamable_http("unlimited_server".to_string());
    unlimited_service.add_tool(tools::AdderTool);
    builder.spawn_service(unlimited_service).await;

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();

    let config = indoc::formatdoc! {r#"
        [server.rate_limit]
        enabled = true

        [server.rate_limit.storage]
        type = "redis"
        url = "redis://localhost:6379/1"
        key_prefix = "nexus_test_{timestamp}:"

        [mcp.servers.limited_server.rate_limit]
        limit = 2
        duration = "30s"
    "#};

    let server = builder.build(&config).await;
    let mcp_client = server.mcp_client("/mcp").await;

    // Test server-specific rate limit
    for _ in 0..2 {
        let response = mcp_client
            .execute("limited_server__adder", json!({"a": 1, "b": 2}))
            .await;
        assert_eq!(response.content[0].raw.as_text().unwrap().text, "1 + 2 = 3");
    }

    // 3rd request to limited-server should fail
    let error = mcp_client
        .execute_expect_error("limited_server__adder", json!({"a": 1, "b": 2}))
        .await;

    assert!(error.to_string().contains("Rate limit exceeded"));

    // But unlimited-server should still work
    let response = mcp_client
        .execute("unlimited_server__adder", json!({"a": 1, "b": 2}))
        .await;
    assert_eq!(response.content[0].raw.as_text().unwrap().text, "1 + 2 = 3");
}

#[tokio::test]
async fn redis_tls_rate_limiting() {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();

    let config = indoc::formatdoc! {r#"
        [server.rate_limit]
        enabled = true

        [server.rate_limit.storage]
        type = "redis"
        url = "rediss://localhost:6380/0"
        key_prefix = "test_tls_{timestamp}:"

        [server.rate_limit.storage.tls]
        enabled = true
        insecure = true
        ca_cert_path = "./docker/redis/tls/ca.crt"

        [server.rate_limit.global]
        limit = 5
        duration = "60s"
    "#};

    let server = TestServer::builder().build(&config).await;

    // Make requests up to the limit
    let mut success_count = 0;
    let mut rate_limited = false;

    for _ in 0..7 {
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
        } else if response.status() == 429 {
            rate_limited = true;
            break;
        }
    }

    assert!(rate_limited, "Rate limit should have been hit");
    assert!(
        success_count >= 4,
        "At least 4 requests should have succeeded, got {success_count}"
    );
}

#[tokio::test]
async fn redis_pool_configuration() {
    let config = indoc! {r#"
        [server.rate_limit]
        enabled = true

        [server.rate_limit.storage]
        type = "redis"
        url = "redis://localhost:6379/2"
        key_prefix = "nexus_pool_test:"
        response_timeout = "1s"
        connection_timeout = "5s"
        
        [server.rate_limit.storage.pool]
        max_size = 10
        min_idle = 2
        timeout_create = "5s"
        timeout_wait = "2s"
        timeout_recycle = "300s"

        [server.rate_limit.per_ip]
        limit = 2
        duration = "30s"
    "#};

    let server = TestServer::builder().build(config).await;

    // Test that the server starts successfully with pool config
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
    assert_eq!(response.status(), 200);
}

#[tokio::test]
#[should_panic(expected = "Failed to get Redis connection from pool")]
async fn redis_connection_failure() {
    let config = indoc! {r#"
        [server.rate_limit]
        enabled = true

        [server.rate_limit.storage]
        type = "redis"
        url = "redis://localhost:9999/0"  # Non-existent Redis server
        connection_timeout = "1s"

        [server.rate_limit.global]
        limit = 5
        duration = "60s"
    "#};

    // Should panic due to Redis connection error
    let _server = TestServer::builder().build(config).await;
}

#[tokio::test]
async fn redis_window_expiry() {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();

    let config = indoc::formatdoc! {r#"
        [server.rate_limit]
        enabled = true

        [server.rate_limit.storage]
        type = "redis"
        url = "redis://localhost:6379/3"
        key_prefix = "test_window_expiry_{timestamp}:"

        [server.rate_limit.global]
        limit = 5
        duration = "2s"  # Short duration for testing
    "#};

    let server = TestServer::builder().build(&config).await;

    // Wait a bit to ensure we're in a clean window
    sleep(Duration::from_millis(100)).await;

    // Use up the rate limit
    let mut success_count = 0;
    for _ in 0..6 {
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
    assert!(
        success_count >= 4,
        "At least 4 requests should succeed, got {success_count}"
    );

    // Next request should fail
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

    assert_eq!(response.status(), 429);

    // Wait for window to expire (with some buffer)
    sleep(Duration::from_secs(3)).await;

    // Should be able to make requests again (may need a few attempts due to averaging)
    let mut success = false;
    for _ in 0..3 {
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
            success = true;
            break;
        }

        sleep(Duration::from_millis(500)).await;
    }

    assert!(success, "Should be able to make requests after window expiry");
}

#[tokio::test]
async fn redis_tls_with_client_certs() {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();

    let config = indoc::formatdoc! {r#"
        [server.rate_limit]
        enabled = true

        [server.rate_limit.storage]
        type = "redis"
        url = "rediss://localhost:6381/0"
        key_prefix = "test_mtls_{timestamp}:"

        [server.rate_limit.storage.tls]
        enabled = true
        ca_cert_path = "./docker/redis/tls/ca.crt"
        client_cert_path = "./docker/redis/tls/client.crt"
        client_key_path = "./docker/redis/tls/client.key"

        [server.rate_limit.global]
        limit = 3
        duration = "60s"
    "#};

    let server = TestServer::builder().build(&config).await;

    // Test that mutual TLS works properly
    let mut success_count = 0;
    let mut rate_limited = false;

    for _ in 0..5 {
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
        } else if response.status() == 429 {
            rate_limited = true;
            break;
        }
    }

    assert!(rate_limited, "Rate limit should have been hit");
    assert!(
        success_count >= 2,
        "At least 2 requests should have succeeded with mutual TLS, got {success_count}"
    );
}

#[tokio::test]
async fn concurrent_redis_rate_limiting() {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();

    let config = indoc::formatdoc! {r#"
       [server.rate_limit]
       enabled = true

       [server.rate_limit.storage]
       type = "redis"
       url = "redis://localhost:6379/5"
       key_prefix = "test_concurrent_{timestamp}:"

       [server.rate_limit.global]
       limit = 10
       duration = "5s"
    "#};

    let server = Arc::new(TestServer::builder().build(&config).await);

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

    // With atomic operations and averaging window, we should have close to 10 successes
    assert!(
        (9..=11).contains(&success_count),
        "Expected around 10 successful requests, got {success_count}"
    );
    assert!(
        (9..=11).contains(&rate_limited_count),
        "Expected around 10 rate-limited requests, got {rate_limited_count}"
    );
    assert_eq!(success_count + rate_limited_count, 20, "Total should be 20");
}
