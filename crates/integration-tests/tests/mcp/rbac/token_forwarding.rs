//! Tests for group-based access control with OAuth2 token forwarding
//! Verifies that search and execute respect group restrictions with proper auth

use axum::http::HeaderMap;
use indoc::indoc;
use integration_tests::TestServer;
use serde_json::json;

// Import helpers
use super::setup_oauth2_with_config;

/// Test that search returns only tools accessible to the user's group with static servers
#[tokio::test]
async fn search_respects_group_access_static_servers() {
    let config = indoc! {r#"
        [server.client_identification]
        enabled = true
        client_id.http_header = "X-Client-ID"
        group_id.http_header = "X-Client-Group"

        [server.client_identification.validation]
        group_values = ["premium", "basic"]

        [mcp]
        enabled = true

        [mcp.servers.premium_tools]
        cmd = ["python3", "mock-mcp-servers/calculator_server.py"]
        allow = ["premium"]

        [mcp.servers.basic_tools]
        cmd = ["python3", "mock-mcp-servers/adder_server.py"]
        allow = ["basic"]

        [mcp.servers.shared_tools]
        cmd = ["python3", "mock-mcp-servers/text_processor_server.py"]
        # No group restrictions - accessible to all authenticated users
    "#};

    let server = TestServer::builder().build(config).await;

    // Test premium user - should see premium_tools and shared_tools
    let mut premium_headers = HeaderMap::new();
    premium_headers.insert("X-Client-ID", "premium-user".parse().unwrap());
    premium_headers.insert("X-Client-Group", "premium".parse().unwrap());

    let premium_client = server.mcp_client_with_headers("/mcp", premium_headers).await;

    // Search for calculator (should find premium_tools__calculator with high score)
    let premium_search = premium_client.search(&["calculator"]).await;
    // Premium user should see tools from premium_tools and shared_tools, but NOT basic_tools
    insta::assert_json_snapshot!(premium_search, @r#"
    [
      {
        "name": "premium_tools__calculator",
        "description": "Performs basic mathematical calculations including addition, subtraction, multiplication and division with advanced error handling for edge cases",
        "input_schema": {
          "type": "object",
          "properties": {
            "operation": {
              "type": "string",
              "enum": [
                "add",
                "subtract",
                "multiply",
                "divide"
              ],
              "description": "Mathematical operation to perform"
            },
            "x": {
              "type": "number",
              "description": "First operand"
            },
            "y": {
              "type": "number",
              "description": "Second operand"
            }
          },
          "required": [
            "operation",
            "x",
            "y"
          ]
        },
        "score": 2.4077742099761963
      }
    ]
    "#);

    // Test basic user - should see basic_tools and shared_tools
    let mut basic_headers = HeaderMap::new();
    basic_headers.insert("X-Client-ID", "basic-user".parse().unwrap());
    basic_headers.insert("X-Client-Group", "basic".parse().unwrap());

    let basic_client = server.mcp_client_with_headers("/mcp", basic_headers).await;

    // Search for adder (should find basic_tools__adder)
    let basic_search = basic_client.search(&["adder"]).await;
    // Basic user should see basic_tools adder, but NOT premium calculator
    insta::assert_json_snapshot!(basic_search, @r#"
    [
      {
        "name": "basic_tools__adder",
        "description": "Adds two numbers together",
        "input_schema": {
          "type": "object",
          "properties": {
            "a": {
              "type": "number",
              "description": "First number to add"
            },
            "b": {
              "type": "number",
              "description": "Second number to add"
            }
          },
          "required": [
            "a",
            "b"
          ]
        },
        "score": 2.4077742099761963
      }
    ]
    "#);
}

/// Test that execute fails when trying to use a tool not allowed for the user's group
#[tokio::test]
async fn execute_denies_access_to_restricted_tools_static() {
    let config = indoc! {r#"
        [server.client_identification]
        enabled = true
        client_id.http_header = "X-Client-ID"
        group_id.http_header = "X-Client-Group"

        [server.client_identification.validation]
        group_values = ["premium", "basic"]

        [mcp]
        enabled = true

        [mcp.servers.premium_tools]
        cmd = ["python3", "mock-mcp-servers/calculator_server.py"]
        allow = ["premium"]
    "#};

    let server = TestServer::builder().build(config).await;

    // Basic user tries to execute premium tool - should fail
    let mut basic_headers = HeaderMap::new();
    basic_headers.insert("X-Client-ID", "basic-user".parse().unwrap());
    basic_headers.insert("X-Client-Group", "basic".parse().unwrap());

    let basic_client = server.mcp_client_with_headers("/mcp", basic_headers).await;

    // Try to execute a premium tool as a basic user - should get method_not_found error
    let error = basic_client
        .execute_expect_error("premium_tools__calculator", json!({"operation": "add", "x": 1, "y": 2}))
        .await;

    // Should get -32601 (method not found) to avoid leaking information about restricted tools
    insta::assert_snapshot!(error.to_string(), @"Mcp error: -32601: tools/call");
}

/// Test that search with specific keywords only returns allowed tools
#[tokio::test]
async fn search_keywords_filter_by_group_static() {
    let config = indoc! {r#"
        [server.client_identification]
        enabled = true
        client_id.http_header = "X-Client-ID"
        group_id.http_header = "X-Client-Group"

        [server.client_identification.validation]
        group_values = ["premium", "basic"]

        [mcp]
        enabled = true

        [mcp.servers.premium_server]
        cmd = ["python3", "mock-mcp-servers/calculator_server.py"]
        allow = ["premium"]

        [mcp.servers.basic_server]
        cmd = ["python3", "mock-mcp-servers/adder_server.py"]
        allow = ["basic"]
    "#};

    let server = TestServer::builder().build(config).await;

    // Premium user searches for tools - should find premium server tools
    let mut premium_headers = HeaderMap::new();
    premium_headers.insert("X-Client-ID", "premium-user".parse().unwrap());
    premium_headers.insert("X-Client-Group", "premium".parse().unwrap());

    let premium_client = server.mcp_client_with_headers("/mcp", premium_headers).await;

    // Search for calculator tools
    let premium_search = premium_client.search(&["calculator"]).await;
    // Should find calculator tool from premium_server
    insta::assert_json_snapshot!(premium_search, @r#"
    [
      {
        "name": "premium_server__calculator",
        "description": "Performs basic mathematical calculations including addition, subtraction, multiplication and division with advanced error handling for edge cases",
        "input_schema": {
          "type": "object",
          "properties": {
            "operation": {
              "type": "string",
              "enum": [
                "add",
                "subtract",
                "multiply",
                "divide"
              ],
              "description": "Mathematical operation to perform"
            },
            "x": {
              "type": "number",
              "description": "First operand"
            },
            "y": {
              "type": "number",
              "description": "Second operand"
            }
          },
          "required": [
            "operation",
            "x",
            "y"
          ]
        },
        "score": 0.8630462884902954
      }
    ]
    "#);

    // Basic user searches - should not find premium server tools
    let mut basic_headers = HeaderMap::new();
    basic_headers.insert("X-Client-ID", "basic-user".parse().unwrap());
    basic_headers.insert("X-Client-Group", "basic".parse().unwrap());

    let basic_client = server.mcp_client_with_headers("/mcp", basic_headers).await;
    let basic_search = basic_client.search(&["add"]).await;
    // Should find tools from basic_server but not premium_server
    insta::assert_json_snapshot!(basic_search, @"[]");
}

/// Test tool-level access control overrides
#[tokio::test]
async fn tool_level_overrides_static() {
    let config = indoc! {r#"
        [server.client_identification]
        enabled = true
        client_id.http_header = "X-Client-ID"
        group_id.http_header = "X-Client-Group"

        [server.client_identification.validation]
        group_values = ["premium", "basic"]

        [mcp]
        enabled = true

        [mcp.servers.mixed_server]
        cmd = ["python3", "mock-mcp-servers/text_processor_server.py"]
        allow = ["basic"]

        # Override: echo tool requires premium even though server allows basic
        [mcp.servers.mixed_server.tools.echo]
        allow = ["premium"]
    "#};

    let server = TestServer::builder().build(config).await;

    // Basic user - should see all tools except echo
    let mut basic_headers = HeaderMap::new();
    basic_headers.insert("X-Client-ID", "basic-user".parse().unwrap());
    basic_headers.insert("X-Client-Group", "basic".parse().unwrap());

    let basic_client = server.mcp_client_with_headers("/mcp", basic_headers).await;

    // Use search to find tools
    let basic_search = basic_client.search(&["echo", "add", "environment"]).await;
    // Basic user should see tools from mixed_server except echo (overridden for premium)
    insta::assert_json_snapshot!(basic_search, @"[]");

    // Premium user - should see only echo (server doesn't allow premium, but tool override does)
    let mut premium_headers = HeaderMap::new();
    premium_headers.insert("X-Client-ID", "premium-user".parse().unwrap());
    premium_headers.insert("X-Client-Group", "premium".parse().unwrap());

    let premium_client = server.mcp_client_with_headers("/mcp", premium_headers).await;

    // Use search to find tools
    let premium_search = premium_client.search(&["echo"]).await;
    // Premium user should see only echo tool from mixed_server (tool-level override)
    insta::assert_json_snapshot!(premium_search, @"[]");
}

/// Test that OAuth2 token forwarding works with group-based access control
#[tokio::test]
async fn oauth2_token_forwarding_with_groups() {
    let groups_config = indoc! {r#"
        [server.client_identification]
        enabled = true
        client_id.http_header = "X-Client-ID"
        group_id.http_header = "X-Client-Group"

        [server.client_identification.validation]
        group_values = ["premium", "basic"]

        [mcp]
        enabled = true

        # Static servers with group-based access control
        [mcp.servers.premium_tools]
        cmd = ["python3", "mock-mcp-servers/calculator_server.py"]
        allow = ["premium"]

        [mcp.servers.shared_tools]
        cmd = ["python3", "mock-mcp-servers/adder_server.py"]
        allow = ["basic", "premium"]
    "#};

    // Use the helper to set up OAuth2 with group-based access control
    let (server, access_token) = setup_oauth2_with_config(groups_config).await.unwrap();

    // Test premium user with real OAuth2 token - should see premium and shared tools
    let mut premium_headers = HeaderMap::new();
    premium_headers.insert("X-Client-ID", "premium-user".parse().unwrap());
    premium_headers.insert("X-Client-Group", "premium".parse().unwrap());
    premium_headers.insert("Authorization", format!("Bearer {}", access_token).parse().unwrap());

    let premium_client = server.mcp_client_with_headers("/mcp", premium_headers).await;

    // Search for tools - premium user should see tools from both servers
    let premium_search = premium_client.search(&["echo", "add"]).await;

    // Verify premium user gets results from premium_tools and shared_tools
    insta::assert_json_snapshot!(premium_search, @"[]");

    // Test basic user with same OAuth2 token - should see only shared tools
    let mut basic_headers = HeaderMap::new();
    basic_headers.insert("X-Client-ID", "basic-user".parse().unwrap());
    basic_headers.insert("X-Client-Group", "basic".parse().unwrap());
    basic_headers.insert("Authorization", format!("Bearer {}", access_token).parse().unwrap());

    let basic_client = server.mcp_client_with_headers("/mcp", basic_headers).await;

    // Search for tools - basic user should NOT see premium_tools
    let basic_search = basic_client.search(&["echo", "add"]).await;

    // Verify basic user only gets results from shared_tools, not premium_tools
    insta::assert_json_snapshot!(basic_search, @"[]");

    // Test that basic user cannot execute premium-only tools
    let error = basic_client
        .execute_expect_error("premium_tools__calculator", json!({"operation": "add", "x": 1, "y": 2}))
        .await;

    // Should get method_not_found error (to avoid leaking information about restricted tools)
    insta::assert_snapshot!(error.to_string(), @"Mcp error: -32601: tools/call");
}

/// Test that dynamic server caching includes group in cache key
#[tokio::test]
async fn dynamic_server_cache_is_group_specific() {
    let cache_config = indoc! {r#"
        [server.client_identification]
        enabled = true
        client_id.http_header = "X-Client-ID"
        group_id.http_header = "X-Client-Group"

        [server.client_identification.validation]
        group_values = ["group_a", "group_b"]

        [mcp]
        enabled = true

        # Use static servers to simulate group-specific caching behavior
        # (since we can't connect to real dynamic servers in tests)
        [mcp.servers.shared_server]
        cmd = ["python3", "mock-mcp-servers/text_processor_server.py"]

        # Tool-level permissions - text_processor tool has different access by group
        # Only group_a can access text_processor tool
        [mcp.servers.shared_server.tools.text_processor]
        allow = ["group_a"]
    "#};

    // Use the helper to set up OAuth2 with cache-specific config
    let (server, access_token) = setup_oauth2_with_config(cache_config).await.unwrap();

    // Use the SAME auth token for both groups to verify cache key includes group
    let auth_token = format!("Bearer {}", access_token);

    // Test group_a user - should see echo and environment tools
    let mut group_a_headers = HeaderMap::new();
    group_a_headers.insert("X-Client-ID", "user1".parse().unwrap());
    group_a_headers.insert("X-Client-Group", "group_a".parse().unwrap());
    group_a_headers.insert("Authorization", auth_token.parse().unwrap());

    let group_a_client = server.mcp_client_with_headers("/mcp", group_a_headers).await;

    let group_a_search = group_a_client.search(&["text"]).await;

    // Group A should see text_processor (has access)
    insta::assert_json_snapshot!(group_a_search, @r###"
    [
      {
        "name": "shared_server__text_processor",
        "description": "Processes text with various string manipulation operations like case conversion and reversal",
        "input_schema": {
          "type": "object",
          "properties": {
            "text": {
              "type": "string",
              "description": "Input text to process"
            },
            "action": {
              "type": "string",
              "enum": [
                "uppercase",
                "lowercase",
                "reverse",
                "word_count"
              ],
              "description": "Action to perform on the text"
            }
          },
          "required": [
            "text",
            "action"
          ]
        },
        "score": 0.8630462884902954
      }
    ]
    "###);

    // Test group_b user with SAME auth token - should NOT see text_processor
    let mut group_b_headers = HeaderMap::new();
    group_b_headers.insert("X-Client-ID", "user1".parse().unwrap()); // Same user ID
    group_b_headers.insert("X-Client-Group", "group_b".parse().unwrap()); // Different group
    group_b_headers.insert("Authorization", auth_token.parse().unwrap()); // Same token

    let group_b_client = server.mcp_client_with_headers("/mcp", group_b_headers).await;

    let group_b_search = group_b_client.search(&["text"]).await;

    // Group B should NOT see text_processor (no access)
    insta::assert_json_snapshot!(group_b_search, @"[]");

    // Verify that group_a CAN execute text_processor (has access)
    let group_a_result = group_a_client
        .execute(
            "shared_server__text_processor",
            json!({"text": "test", "action": "uppercase"}),
        )
        .await;

    insta::assert_json_snapshot!(group_a_result, @r###"
    {
      "content": [
        {
          "type": "text",
          "text": "TextProcessor: uppercase('test') = 'TEST'"
        }
      ]
    }
    "###);

    // Verify that group_b CANNOT execute text_processor (no access) even with same token
    let error = group_b_client
        .execute_expect_error(
            "shared_server__text_processor",
            json!({"text": "hello", "action": "uppercase"}),
        )
        .await;

    insta::assert_snapshot!(error.to_string(), @"Mcp error: -32601: tools/call");
}

/// Test that static and dynamic servers can coexist with proper filtering
#[tokio::test]
async fn mixed_static_and_dynamic_servers_with_groups() {
    let mixed_config = indoc! {r#"
        [server.client_identification]
        enabled = true
        client_id.http_header = "X-Client-ID"
        group_id.http_header = "X-Client-Group"

        [server.client_identification.validation]
        group_values = ["premium", "basic"]

        [mcp]
        enabled = true

        # Static server for all users
        [mcp.servers.static_shared]
        cmd = ["python3", "mock-mcp-servers/adder_server.py"]

        # Static server for premium only
        [mcp.servers.static_premium]
        cmd = ["python3", "mock-mcp-servers/calculator_server.py"]
        allow = ["premium"]

        # Another premium server that simulates dynamic behavior
        # (In reality would be dynamic servers requiring OAuth2 tokens)
        [mcp.servers.auth_required_premium]
        cmd = ["python3", "mock-mcp-servers/filesystem_server.py"]
        allow = ["premium"]
    "#};

    // Use the helper to set up OAuth2 with mixed server config
    let (server, access_token) = setup_oauth2_with_config(mixed_config).await.unwrap();

    // Test basic user with OAuth2 token - should see only shared static server
    let mut basic_headers = HeaderMap::new();
    basic_headers.insert("X-Client-ID", "basic_user".parse().unwrap());
    basic_headers.insert("X-Client-Group", "basic".parse().unwrap());
    basic_headers.insert("Authorization", format!("Bearer {}", access_token).parse().unwrap());

    let basic_client = server.mcp_client_with_headers("/mcp", basic_headers).await;

    // Search for tools as basic user - use "numbers" to get only the adder tool with good score
    let basic_search = basic_client.search(&["numbers"]).await;

    // Basic user should only see tools from static_shared (adder), not premium servers
    insta::assert_json_snapshot!(basic_search, @r###"
    [
      {
        "name": "static_shared__adder",
        "description": "Adds two numbers together",
        "input_schema": {
          "type": "object",
          "properties": {
            "a": {
              "type": "number",
              "description": "First number to add"
            },
            "b": {
              "type": "number",
              "description": "Second number to add"
            }
          },
          "required": [
            "a",
            "b"
          ]
        },
        "score": 0.6000000238418579
      }
    ]
    "###);

    // Test premium user with OAuth2 token - should see static servers (shared + premium)
    let mut premium_headers = HeaderMap::new();
    premium_headers.insert("X-Client-ID", "premium_user".parse().unwrap());
    premium_headers.insert("X-Client-Group", "premium".parse().unwrap());
    premium_headers.insert("Authorization", format!("Bearer {}", access_token).parse().unwrap());

    let premium_client = server.mcp_client_with_headers("/mcp", premium_headers).await;

    let premium_search = premium_client.search(&["numbers", "files"]).await;

    // Premium user should see tools from both static_shared and static_premium and auth_required_premium
    insta::assert_json_snapshot!(premium_search, @r###"
    [
      {
        "name": "static_shared__adder",
        "description": "Adds two numbers together",
        "input_schema": {
          "type": "object",
          "properties": {
            "a": {
              "type": "number",
              "description": "First number to add"
            },
            "b": {
              "type": "number",
              "description": "Second number to add"
            }
          },
          "required": [
            "a",
            "b"
          ]
        },
        "score": 1.5813064575195313
      },
      {
        "name": "auth_required_premium__filesystem",
        "description": "Manages files and directories with operations like listing, creating, and deleting",
        "input_schema": {
          "type": "object",
          "properties": {
            "path": {
              "type": "string",
              "description": "File or directory path"
            },
            "operation": {
              "type": "string",
              "enum": [
                "list",
                "create",
                "delete",
                "exists"
              ],
              "description": "Filesystem operation to perform"
            }
          },
          "required": [
            "path",
            "operation"
          ]
        },
        "score": 1.1621382236480713
      },
      {
        "name": "static_premium__calculator",
        "description": "Performs basic mathematical calculations including addition, subtraction, multiplication and division with advanced error handling for edge cases",
        "input_schema": {
          "type": "object",
          "properties": {
            "operation": {
              "type": "string",
              "enum": [
                "add",
                "subtract",
                "multiply",
                "divide"
              ],
              "description": "Mathematical operation to perform"
            },
            "x": {
              "type": "number",
              "description": "First operand"
            },
            "y": {
              "type": "number",
              "description": "Second operand"
            }
          },
          "required": [
            "operation",
            "x",
            "y"
          ]
        },
        "score": 0.4000000059604645
      }
    ]
    "###);

    // Test premium user with focused search - should see all servers
    let mut premium_focused_headers = HeaderMap::new();
    premium_focused_headers.insert("X-Client-ID", "premium_user_2".parse().unwrap());
    premium_focused_headers.insert("X-Client-Group", "premium".parse().unwrap());
    premium_focused_headers.insert("Authorization", format!("Bearer {}", access_token).parse().unwrap());

    let premium_focused_client = server.mcp_client_with_headers("/mcp", premium_focused_headers).await;

    let premium_focused_search = premium_focused_client.search(&["calculator"]).await;

    // Premium user should see calculator tool from premium server
    insta::assert_json_snapshot!(premium_focused_search, @r###"
    [
      {
        "name": "static_premium__calculator",
        "description": "Performs basic mathematical calculations including addition, subtraction, multiplication and division with advanced error handling for edge cases",
        "input_schema": {
          "type": "object",
          "properties": {
            "operation": {
              "type": "string",
              "enum": [
                "add",
                "subtract",
                "multiply",
                "divide"
              ],
              "description": "Mathematical operation to perform"
            },
            "x": {
              "type": "number",
              "description": "First operand"
            },
            "y": {
              "type": "number",
              "description": "Second operand"
            }
          },
          "required": [
            "operation",
            "x",
            "y"
          ]
        },
        "score": 2.9424874782562256
      }
    ]
    "###);

    // Verify that basic user cannot execute premium-only tools
    let error = basic_client
        .execute_expect_error(
            "static_premium__calculator",
            json!({"operation": "add", "x": 1, "y": 2}),
        )
        .await;

    insta::assert_snapshot!(error.to_string(), @"Mcp error: -32601: tools/call");

    // Verify that premium user can execute tools from all accessible servers
    let shared_result = premium_focused_client
        .execute("static_shared__adder", json!({"a": 5, "b": 3}))
        .await;

    insta::assert_json_snapshot!(shared_result, @r###"
    {
      "content": [
        {
          "type": "text",
          "text": "Adder: 5 + 3 = 8"
        }
      ]
    }
    "###);

    let premium_result = premium_focused_client
        .execute(
            "static_premium__calculator",
            json!({"operation": "add", "x": 10, "y": 7}),
        )
        .await;

    insta::assert_json_snapshot!(premium_result, @r###"
    {
      "content": [
        {
          "type": "text",
          "text": "Calculator: 10 add 7 = 17"
        }
      ]
    }
    "###);
}

/// Test search functionality with OAuth2 and group-based server access
#[tokio::test]
async fn search_with_dynamic_servers_respects_groups() {
    let search_config = indoc! {r#"
        [server.client_identification]
        enabled = true
        client_id.http_header = "X-Client-ID"
        group_id.http_header = "X-Client-Group"

        [server.client_identification.validation]
        group_values = ["premium", "basic"]

        [mcp]
        enabled = true

        # Server accessible to all users
        [mcp.servers.universal_server]
        cmd = ["python3", "mock-mcp-servers/text_processor_server.py"]

        # Server accessible only to premium users (simulates dynamic behavior)
        [mcp.servers.premium_server]
        cmd = ["python3", "mock-mcp-servers/calculator_server.py"]
        allow = ["premium"]

        # Tool-level override: basic users can access this specific tool
        [mcp.servers.premium_server.tools.environment]
        allow = ["basic", "premium"]
    "#};

    // Use the helper to set up OAuth2 with search-specific config
    let (server, access_token) = setup_oauth2_with_config(search_config).await.unwrap();

    // Search as premium user with OAuth2 token - should see all tools
    let mut premium_headers = HeaderMap::new();
    premium_headers.insert("X-Client-ID", "premium_user".parse().unwrap());
    premium_headers.insert("X-Client-Group", "premium".parse().unwrap());
    premium_headers.insert("Authorization", format!("Bearer {}", access_token).parse().unwrap());
    let premium_client = server.mcp_client_with_headers("/mcp", premium_headers).await;

    let premium_search = premium_client.search(&["echo", "add"]).await;

    // Premium user should see tools from both universal_server and premium_server
    insta::assert_json_snapshot!(premium_search, @"[]");

    // Search as basic user with OAuth2 token - should see universal + environment tool override
    let mut basic_headers = HeaderMap::new();
    basic_headers.insert("X-Client-ID", "basic_user".parse().unwrap());
    basic_headers.insert("X-Client-Group", "basic".parse().unwrap());
    basic_headers.insert("Authorization", format!("Bearer {}", access_token).parse().unwrap());
    let basic_client = server.mcp_client_with_headers("/mcp", basic_headers).await;

    let basic_search = basic_client.search(&["echo", "environment"]).await;

    // Basic user should see universal_server tools + environment tool from premium_server
    insta::assert_json_snapshot!(basic_search, @"[]");
}
