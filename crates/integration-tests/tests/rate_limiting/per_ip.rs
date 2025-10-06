#![allow(clippy::panic)]

use indoc::indoc;
use integration_tests::TestServer;
use serde_json::json;

#[tokio::test]
async fn x_real_ip() {
    let config = indoc! {r#"
        [server.rate_limits]
        enabled = true

        [server.rate_limits.per_ip]
        limit = 2
        interval = "10s"

        [server.client_ip]
        x_real_ip = true

        [mcp]
        enabled = true
        path = "/mcp"

        # Dummy server to satisfy validation
        [mcp.servers.dummy]
        cmd = ["echo", "dummy"]
    "#};

    let server = TestServer::builder().build(config).await;

    // Test requests from different IPs using X-Forwarded-For header
    let mut results = Vec::new();

    // IP 1: Should hit limit after 2 requests
    for i in 1..=4 {
        let response = server
            .client
            .request(reqwest::Method::POST, "/mcp")
            .header("X-Real-Ip", "192.168.1.1")
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
            .header("X-Real-Ip", "192.168.1.2")
            .header("X-Forwarded-For", "192.168.1.1")
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
        "retry_after": null,
        "status": 200
      },
      {
        "ip": "192.168.1.1",
        "request": 2,
        "retry_after": null,
        "status": 200
      },
      {
        "ip": "192.168.1.1",
        "request": 3,
        "retry_after": null,
        "status": 429
      },
      {
        "ip": "192.168.1.1",
        "request": 4,
        "retry_after": null,
        "status": 429
      },
      {
        "ip": "192.168.1.2",
        "request": 1,
        "retry_after": null,
        "status": 406
      },
      {
        "ip": "192.168.1.2",
        "request": 2,
        "retry_after": null,
        "status": 406
      },
      {
        "ip": "192.168.1.2",
        "request": 3,
        "retry_after": null,
        "status": 429
      }
    ]
    "#);
}

#[tokio::test]
async fn x_forwarded_for_ip() {
    let config = indoc! {r#"
        [server.rate_limits]
        enabled = true

        [server.rate_limits.per_ip]
        limit = 2
        interval = "10s"

        [server.client_ip]
        x_forwarded_for_trusted_hops = 1

        [mcp]
        enabled = true
        path = "/mcp"

        # Dummy server to satisfy validation
        [mcp.servers.dummy]
        cmd = ["echo", "dummy"]
    "#};

    let server = TestServer::builder().build(config).await;

    let mut results = Vec::new();

    // Test with X-Forwarded-For header (trusted hop count = 1)
    // IP from rightmost position should be used (192.168.1.100)
    for i in 1..=4 {
        let response = server
            .client
            .request(reqwest::Method::POST, "/mcp")
            .header("X-Forwarded-For", "10.0.0.1, 192.168.1.100")
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
            "request": i,
            "ip": "10.0.0.1",
            "status": response.status().as_u16(),
            "retry_after": response.headers().get("retry-after").map(|h| h.to_str().unwrap_or("invalid"))
        }));
    }
    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .header("X-Forwarded-For", "10.0.0.2, 10.0.0.1, 192.168.1.100")
        .header("X-Real-Ip", "10.0.0.2")
        .header("Content-Type", "application/json")
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "tools/list",
            "id": 5
        }))
        .send()
        .await
        .unwrap();

    results.push(json!({
        "request": 5,
        "ip": "10.0.0.1",
        "status": response.status().as_u16(),
        "retry_after": response.headers().get("retry-after").map(|h| h.to_str().unwrap_or("invalid"))
    }));

    // Different IP in X-Forwarded-For should have independent limit
    // It should not blidnly trust the first IP in the list, but instead rely on the trusted hops
    // count.
    for i in 1..=3 {
        let response = server
            .client
            .request(reqwest::Method::POST, "/mcp")
            .header("X-Forwarded-For", "10.0.0.1, 10.0.0.2, 192.168.1.100")
            .header("X-Real-Ip", "10.0.0.1")
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
            "request": i,
            "ip": "10.0.0.2",
            "status": response.status().as_u16(),
            "retry_after": response.headers().get("retry-after").map(|h| h.to_str().unwrap_or("invalid"))
        }));
    }

    insta::assert_json_snapshot!(results, @r#"
    [
      {
        "ip": "10.0.0.1",
        "request": 1,
        "retry_after": null,
        "status": 406
      },
      {
        "ip": "10.0.0.1",
        "request": 2,
        "retry_after": null,
        "status": 406
      },
      {
        "ip": "10.0.0.1",
        "request": 3,
        "retry_after": null,
        "status": 429
      },
      {
        "ip": "10.0.0.1",
        "request": 4,
        "retry_after": null,
        "status": 429
      },
      {
        "ip": "10.0.0.1",
        "request": 5,
        "retry_after": null,
        "status": 429
      },
      {
        "ip": "10.0.0.2",
        "request": 1,
        "retry_after": null,
        "status": 406
      },
      {
        "ip": "10.0.0.2",
        "request": 2,
        "retry_after": null,
        "status": 406
      },
      {
        "ip": "10.0.0.2",
        "request": 3,
        "retry_after": null,
        "status": 429
      }
    ]
    "#);
}

#[tokio::test]
async fn default_connection_based_ip() {
    let config = indoc! {r#"
        [server.rate_limits]
        enabled = true

        [server.rate_limits.per_ip]
        limit = 2
        interval = "10s"

        # No client_ip configuration - should use connection address

        [mcp]
        enabled = true
        path = "/mcp"

        # Dummy server to satisfy validation
        [mcp.servers.dummy]
        cmd = ["echo", "dummy"]
    "#};

    let server = TestServer::builder().build(config).await;

    let mut results = Vec::new();

    // Without any client_ip config, all requests from the test client
    // will appear from the same IP (127.0.0.1 or similar)
    for i in 1..=4 {
        let response = server
            .client
            .request(reqwest::Method::POST, "/mcp")
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
            "request": i,
            "status": response.status().as_u16(),
            "retry_after": response.headers().get("retry-after").map(|h| h.to_str().unwrap_or("invalid"))
        }));
    }

    // Try with headers that should be ignored without config
    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .header("X-Forwarded-For", "192.168.1.50")
        .header("X-Real-Ip", "192.168.1.60")
        .header("Content-Type", "application/json")
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "tools/list",
            "id": 5
        }))
        .send()
        .await
        .unwrap();

    results.push(json!({
        "request": 5,
        "status": response.status().as_u16(),
        "retry_after": response.headers().get("retry-after").map(|h| h.to_str().unwrap_or("invalid"))
    }));

    insta::assert_json_snapshot!(results, @r#"
    [
      {
        "request": 1,
        "retry_after": null,
        "status": 406
      },
      {
        "request": 2,
        "retry_after": null,
        "status": 406
      },
      {
        "request": 3,
        "retry_after": null,
        "status": 429
      },
      {
        "request": 4,
        "retry_after": null,
        "status": 429
      },
      {
        "request": 5,
        "retry_after": null,
        "status": 429
      }
    ]
    "#);
}
