use super::{
    MockJwksServer, RequestBuilderExt, oauth_config_no_poll_interval, oauth_config_with_jwks_url, setup_hydra_test,
};
use integration_tests::TestServer;
use std::time::Duration;

#[tokio::test]
async fn mock_server_basic_functionality() {
    // First, test that the mock server itself works
    let mock_server = MockJwksServer::start().await;

    // Create a properly formatted but invalid JWT to trigger JWKS fetching
    // This JWT has valid structure but will fail signature validation
    let invalid_but_formatted_jwt = super::create_test_jwt_unsigned(Some("read write"));

    // Test the server configuration
    let config = oauth_config_with_jwks_url(&mock_server.jwks_url(), "5m");
    let server = TestServer::builder().build(&config).await;

    // Make request with formatted JWT - this should trigger JWKS fetch
    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&invalid_but_formatted_jwt)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "ping", "id": 1}"#)
        .send()
        .await
        .unwrap();

    // Should fail auth but fetch JWKS
    assert_eq!(response.status(), 401);
    assert_eq!(
        mock_server.get_request_count(),
        1,
        "Formatted JWT should trigger JWKS fetch"
    );
}

#[tokio::test]
async fn cache_miss_then_hit() {
    let mock_server = MockJwksServer::start().await;
    let config = oauth_config_with_jwks_url(&mock_server.jwks_url(), "5m");
    let server = TestServer::builder().build(&config).await;

    // Create a properly formatted but unsigned JWT to trigger JWKS fetching
    let invalid_token = super::create_test_jwt_unsigned(Some("read write"));

    // First request should be a cache miss (fetches JWKS)
    let response1 = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&invalid_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "ping", "id": 1}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(response1.status(), 401); // Should fail auth but fetch JWKS
    assert_eq!(mock_server.get_request_count(), 1, "First request should fetch JWKS");

    // Second request should be a cache hit (no additional fetch)
    let response2 = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&invalid_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "ping", "id": 2}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(response2.status(), 401); // Should still fail auth

    assert_eq!(
        mock_server.get_request_count(),
        1,
        "Second request should use cached JWKS"
    );

    // Third request should also be a cache hit
    let response3 = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&invalid_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "ping", "id": 3}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(response3.status(), 401); // Should still fail auth

    assert_eq!(
        mock_server.get_request_count(),
        1,
        "Third request should use cached JWKS"
    );
}

#[tokio::test]
async fn cache_expiration_and_refresh() {
    let mock_server = MockJwksServer::start().await;

    // Use very short TTL for fast testing
    let config = oauth_config_with_jwks_url(&mock_server.jwks_url(), "1s");
    let server = TestServer::builder().build(&config).await;

    let invalid_token = super::create_test_jwt_unsigned(Some("read write"));

    // First request - cache miss
    let response1 = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&invalid_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "ping", "id": 1}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(response1.status(), 401);
    assert_eq!(mock_server.get_request_count(), 1, "Initial request should fetch JWKS");

    // Second request within TTL - cache hit
    let response2 = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&invalid_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "ping", "id": 2}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(response2.status(), 401);

    assert_eq!(
        mock_server.get_request_count(),
        1,
        "Request within TTL should use cache"
    );

    // Wait for cache to expire
    tokio::time::sleep(Duration::from_millis(1100)).await;

    // Third request after expiration - should refresh cache
    let response3 = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&invalid_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "ping", "id": 3}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(response3.status(), 401);

    assert_eq!(
        mock_server.get_request_count(),
        2,
        "Request after TTL should refresh JWKS"
    );

    // Fourth request - should use newly cached JWKS
    let response4 = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&invalid_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "ping", "id": 4}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(response4.status(), 401);
    assert_eq!(mock_server.get_request_count(), 2, "Request should use refreshed cache");
}

#[tokio::test]
async fn cache_concurrent_requests() {
    let mock_server = MockJwksServer::start().await;
    let config = oauth_config_with_jwks_url(&mock_server.jwks_url(), "5m");
    let server = TestServer::builder().build(&config).await;

    // Make multiple concurrent requests during cold cache
    let server = std::sync::Arc::new(server);
    let mut handles = Vec::new();

    for i in 0..5 {
        let server_clone = server.clone();

        let handle = tokio::spawn(async move {
            let response = server_clone
                .client
                .request(reqwest::Method::POST, "/mcp")
                .authorization(&super::create_test_jwt_unsigned(Some("read write")))
                .mcp_json(r#"{"jsonrpc": "2.0", "method": "ping", "id": 1}"#)
                .send()
                .await
                .unwrap();
            (i, response.status())
        });

        handles.push(handle);
    }

    // Wait for all requests to complete
    let results: Vec<_> = futures_util::future::join_all(handles)
        .await
        .into_iter()
        .map(|r| r.unwrap())
        .collect();

    // All requests should fail auth (401) but succeed in making the request
    for (i, status) in results {
        assert_eq!(status, 401, "Request {i} should fail auth but complete");
    }

    // Despite 5 concurrent requests, only 1 JWKS fetch should occur due to refresh lock
    assert_eq!(
        mock_server.get_request_count(),
        1,
        "Concurrent requests should result in only one JWKS fetch due to refresh lock"
    );
}

#[tokio::test]
async fn cache_no_ttl_never_expires() {
    let mock_server = MockJwksServer::start().await;

    // No poll_interval means cache never expires
    let config = oauth_config_no_poll_interval(&mock_server.jwks_url());
    let server = TestServer::builder().build(&config).await;

    let invalid_token = super::create_test_jwt_unsigned(Some("read write"));

    // First request - cache miss
    let response1 = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&invalid_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "ping", "id": 1}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(response1.status(), 401);
    assert_eq!(mock_server.get_request_count(), 1, "Initial request should fetch JWKS");

    // Wait longer than typical TTL would be
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Multiple subsequent requests - should all be cache hits
    for i in 2..=5 {
        let response = server
            .client
            .request(reqwest::Method::POST, "/mcp")
            .authorization(&invalid_token)
            .mcp_json(&format!(r#"{{"jsonrpc": "2.0", "method": "ping", "id": {i}}}"#))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), 401);

        assert_eq!(
            mock_server.get_request_count(),
            1,
            "Request {i} should use cache (no TTL configured)"
        );
    }
}

#[tokio::test]
async fn cache_with_authentication_flow() {
    // This test uses real Hydra tokens to test JWKS caching during actual authentication
    let (server, access_token) = setup_hydra_test("jwks-cache-test", "read write").await.unwrap();

    // First authenticated request - may trigger JWKS fetch
    let response1 = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&access_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "ping", "id": 1}"#)
        .send()
        .await
        .unwrap();

    assert_ne!(response1.status(), 401, "First authenticated request should succeed");

    // Second authenticated request - should use cached JWKS
    let response2 = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&access_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "ping", "id": 2}"#)
        .send()
        .await
        .unwrap();

    assert_ne!(response2.status(), 401, "Second authenticated request should succeed");

    // Both requests should have the same success status (either both work or both fail due to method issues)
    assert_eq!(
        response1.status(),
        response2.status(),
        "Both requests should have consistent behavior due to JWKS caching"
    );
}

#[tokio::test]
async fn cache_different_ttl_configurations() {
    // Test various TTL configurations
    let test_cases = vec![
        ("1s", Duration::from_millis(1100)),
        ("2s", Duration::from_millis(2100)),
        ("30s", Duration::from_millis(500)), // Don't wait 30s, just verify it doesn't expire early
    ];

    for (ttl_config, wait_duration) in test_cases {
        let mock_server = MockJwksServer::start().await;
        let config = oauth_config_with_jwks_url(&mock_server.jwks_url(), ttl_config);
        let server = TestServer::builder().build(&config).await;

        let invalid_token = super::create_test_jwt_unsigned(Some("read write"));

        // First request
        let response1 = server
            .client
            .request(reqwest::Method::POST, "/mcp")
            .authorization(&invalid_token)
            .mcp_json(r#"{"jsonrpc": "2.0", "method": "ping", "id": 1}"#)
            .send()
            .await
            .unwrap();

        assert_eq!(response1.status(), 401);

        assert_eq!(
            mock_server.get_request_count(),
            1,
            "Initial request for TTL {ttl_config}"
        );

        // Wait based on TTL
        tokio::time::sleep(wait_duration).await;

        // Second request
        let response2 = server
            .client
            .request(reqwest::Method::POST, "/mcp")
            .authorization(&invalid_token)
            .mcp_json(r#"{"jsonrpc": "2.0", "method": "ping", "id": 2}"#)
            .send()
            .await
            .unwrap();

        assert_eq!(response2.status(), 401);
        let expected_count = if ttl_config == "30s" { 1 } else { 2 };

        assert_eq!(
            mock_server.get_request_count(),
            expected_count,
            "TTL {ttl_config} behavior after {wait_duration:?} wait"
        );
    }
}

#[tokio::test]
async fn cache_handles_server_errors() {
    use axum::{Router, http::StatusCode, routing::get};
    use tokio::net::TcpListener;

    // Create a server that returns 500 errors
    let failing_server = {
        async fn handle_error() -> StatusCode {
            StatusCode::INTERNAL_SERVER_ERROR
        }

        let app = Router::new().route("/.well-known/jwks.json", get(handle_error));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        addr
    };

    let jwks_url = format!("http://127.0.0.1:{}/.well-known/jwks.json", failing_server.port());
    let config = oauth_config_with_jwks_url(&jwks_url, "5m");
    let server = TestServer::builder().build(&config).await;

    // Test that protected endpoints handle JWKS fetch errors gracefully
    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&super::create_test_jwt_unsigned(Some("read write")))
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "ping", "id": 1}"#)
        .send()
        .await
        .unwrap();

    // The request should complete (not hang) even if JWKS fetch fails
    // The exact status depends on how JWKS errors are handled
    assert!(
        response.status().is_client_error() || response.status().is_server_error(),
        "Protected endpoint should return error when JWKS fetch fails"
    );
}

#[tokio::test]
async fn cache_refresh_race_condition() {
    let mock_server = MockJwksServer::start().await;

    // Use short TTL to trigger refresh
    let config = oauth_config_with_jwks_url(&mock_server.jwks_url(), "500ms");
    let server = TestServer::builder().build(&config).await;

    let invalid_token = super::create_test_jwt_unsigned(Some("read write"));

    // Initial request to populate cache
    let response = server
        .client
        .request(reqwest::Method::POST, "/mcp")
        .authorization(&invalid_token)
        .mcp_json(r#"{"jsonrpc": "2.0", "method": "ping", "id": 1}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 401);
    assert_eq!(mock_server.get_request_count(), 1);

    // Wait for cache to expire
    tokio::time::sleep(Duration::from_millis(600)).await;

    // Make multiple concurrent requests after expiration
    let server = std::sync::Arc::new(server);
    let mut handles = Vec::new();

    for i in 0..10 {
        let server_clone = server.clone();

        let handle = tokio::spawn(async move {
            let response = server_clone
                .client
                .request(reqwest::Method::POST, "/mcp")
                .authorization(&super::create_test_jwt_unsigned(Some("read write")))
                .mcp_json(r#"{"jsonrpc": "2.0", "method": "ping", "id": 1}"#)
                .send()
                .await
                .unwrap();

            (i, response.status())
        });

        handles.push(handle);
    }

    // Wait for all requests
    let results: Vec<_> = futures_util::future::join_all(handles)
        .await
        .into_iter()
        .map(|r| r.unwrap())
        .collect();

    // All should fail auth but complete the request
    for (i, status) in results {
        assert_eq!(
            status, 401,
            "Concurrent refresh request {i} should fail auth but complete"
        );
    }

    // Should have exactly 2 JWKS fetches: initial + one refresh (despite 10 concurrent requests)
    assert_eq!(
        mock_server.get_request_count(),
        2,
        "Should have exactly 2 JWKS fetches despite concurrent refresh requests"
    );
}
