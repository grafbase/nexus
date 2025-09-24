use axum::http::HeaderMap;
use indoc::indoc;
use integration_tests::TestServer;

/// Test that search returns only tools accessible to the user's group
#[tokio::test]
async fn search_returns_group_specific_tools() {
    let config = indoc! {r#"
        [server.client_identification]
        enabled = true
        client_id.http_header = "X-Client-ID"
        group_id.http_header = "X-Client-Group"

        [server.client_identification.validation]
        group_values = ["premium", "basic"]

        [mcp]
        enabled = true

        [mcp.servers.premium_only]
        cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
        allow = ["premium"]

        [mcp.servers.basic_only]
        cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
        allow = ["basic"]

        [mcp.servers.unrestricted]
        cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
    "#};

    let server = TestServer::builder().build(config).await;

    // Premium user searches - should see premium_only and unrestricted tools
    let mut premium_headers = HeaderMap::new();
    premium_headers.insert("X-Client-ID", "premium_user".parse().unwrap());
    premium_headers.insert("X-Client-Group", "premium".parse().unwrap());
    let premium_client = server.mcp_client_with_headers("/mcp", premium_headers).await;

    let premium_search = premium_client.search(&["echo"]).await;

    // Verify premium user sees tools from premium_only and unrestricted servers
    insta::assert_json_snapshot!(premium_search, @r#"
    [
      {
        "name": "premium_only__echo",
        "description": "Echoes back the input text message verbatim for testing and debugging purposes",
        "input_schema": {
          "type": "object",
          "properties": {
            "text": {
              "type": "string",
              "description": "Text message to echo back verbatim"
            }
          },
          "required": [
            "text"
          ]
        },
        "score": 3.842801570892334
      },
      {
        "name": "unrestricted__echo",
        "description": "Echoes back the input text message verbatim for testing and debugging purposes",
        "input_schema": {
          "type": "object",
          "properties": {
            "text": {
              "type": "string",
              "description": "Text message to echo back verbatim"
            }
          },
          "required": [
            "text"
          ]
        },
        "score": 3.842801570892334
      }
    ]
    "#);

    // Basic user searches - should see basic_only and unrestricted tools
    let mut basic_headers = HeaderMap::new();
    basic_headers.insert("X-Client-ID", "basic_user".parse().unwrap());
    basic_headers.insert("X-Client-Group", "basic".parse().unwrap());
    let basic_client = server.mcp_client_with_headers("/mcp", basic_headers).await;

    let basic_search = basic_client.search(&["echo"]).await;

    // Verify basic user sees tools from basic_only and unrestricted servers
    insta::assert_json_snapshot!(basic_search, @r#"
    [
      {
        "name": "basic_only__echo",
        "description": "Echoes back the input text message verbatim for testing and debugging purposes",
        "input_schema": {
          "type": "object",
          "properties": {
            "text": {
              "type": "string",
              "description": "Text message to echo back verbatim"
            }
          },
          "required": [
            "text"
          ]
        },
        "score": 3.842801570892334
      },
      {
        "name": "unrestricted__echo",
        "description": "Echoes back the input text message verbatim for testing and debugging purposes",
        "input_schema": {
          "type": "object",
          "properties": {
            "text": {
              "type": "string",
              "description": "Text message to echo back verbatim"
            }
          },
          "required": [
            "text"
          ]
        },
        "score": 3.842801570892334
      }
    ]
    "#);

    // User without group - when groups are configured, should see no tools
    // (In production, they would be rejected by middleware, but tests bypass middleware)
    let mut no_group_headers = HeaderMap::new();
    no_group_headers.insert("X-Client-ID", "no_group_user".parse().unwrap());
    let no_group_client = server.mcp_client_with_headers("/mcp", no_group_headers).await;

    let no_group_search = no_group_client.search(&["echo"]).await;

    // Verify user without group sees no tools (groups are configured, so no-group is invalid)
    insta::assert_json_snapshot!(no_group_search, @"[]");
}

/// Test that search with tool-level overrides returns correct results
#[tokio::test]
async fn search_respects_tool_level_overrides() {
    let config = indoc! {r#"
        [server.client_identification]
        enabled = true
        client_id.http_header = "X-Client-ID"
        group_id.http_header = "X-Client-Group"

        [server.client_identification.validation]
        group_values = ["basic", "premium"]

        [mcp]
        enabled = true

        # Use calculator server - basic group can access all tools
        [mcp.servers.calc]
        cmd = ["python3", "mock-mcp-servers/calculator_server.py"]
        allow = ["basic"]

        # Override: calculator tool is premium only
        [mcp.servers.calc.tools.calculator]
        allow = ["premium"]

        # Use text processor - premium group can access all tools
        [mcp.servers.text]
        cmd = ["python3", "mock-mcp-servers/text_processor_server.py"]
        allow = ["premium"]

        # Override: text_processor tool is basic only
        [mcp.servers.text.tools.text_processor]
        allow = ["basic"]
    "#};

    let server = TestServer::builder().build(config).await;

    // Basic user searches for "process" - should see only text_processor (overridden)
    let mut basic_headers = HeaderMap::new();
    basic_headers.insert("X-Client-ID", "basic_user".parse().unwrap());
    basic_headers.insert("X-Client-Group", "basic".parse().unwrap());
    let basic_client = server.mcp_client_with_headers("/mcp", basic_headers).await;

    let basic_search = basic_client.search(&["process"]).await;

    // Verify basic user sees only text_processor (overridden to allow basic)
    // NOT calculator (server allowed basic, but tool overridden to premium only)
    insta::assert_json_snapshot!(basic_search, @r#"
    [
      {
        "name": "text__text_processor",
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
        "score": 0.4000000059604645
      }
    ]
    "#);

    // Premium user searches for "calculator" - should see only calculator (overridden)
    let mut premium_headers = HeaderMap::new();
    premium_headers.insert("X-Client-ID", "premium_user".parse().unwrap());
    premium_headers.insert("X-Client-Group", "premium".parse().unwrap());
    let premium_client = server.mcp_client_with_headers("/mcp", premium_headers).await;

    let premium_search = premium_client.search(&["calculator"]).await;

    // Verify premium user sees only calculator (overridden to allow premium)
    // NOT text_processor (server allowed premium, but tool overridden to basic only)
    insta::assert_json_snapshot!(premium_search, @r#"
    [
      {
        "name": "calc__calculator",
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
}

/// Test that empty search still respects access control
#[tokio::test]
async fn empty_search_respects_access_control() {
    let config = indoc! {r#"
        [server.client_identification]
        enabled = true
        client_id.http_header = "X-Client-ID"
        group_id.http_header = "X-Client-Group"

        [server.client_identification.validation]
        group_values = ["premium", "suspended"]

        [mcp]
        enabled = true

        [mcp.servers.no_suspended]
        cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
        deny = ["suspended"]
    "#};

    let server = TestServer::builder().build(config).await;

    // Premium user searches for all tools (using common prefix) - should see tools
    let mut premium_headers = HeaderMap::new();
    premium_headers.insert("X-Client-ID", "premium_user".parse().unwrap());
    premium_headers.insert("X-Client-Group", "premium".parse().unwrap());
    let premium_client = server.mcp_client_with_headers("/mcp", premium_headers).await;

    let premium_search = premium_client.search(&["no_suspended"]).await;

    // Verify premium user sees tools (should find no_suspended server tools)
    insta::assert_json_snapshot!(premium_search, @r#"
    [
      {
        "name": "no_suspended__add",
        "description": "Performs mathematical addition operation on two numerical values and returns the sum",
        "input_schema": {
          "type": "object",
          "properties": {
            "a": {
              "type": "number",
              "description": "First numerical value for addition"
            },
            "b": {
              "type": "number",
              "description": "Second numerical value for addition"
            }
          },
          "required": [
            "a",
            "b"
          ]
        },
        "score": 0.30000001192092896
      },
      {
        "name": "no_suspended__echo",
        "description": "Echoes back the input text message verbatim for testing and debugging purposes",
        "input_schema": {
          "type": "object",
          "properties": {
            "text": {
              "type": "string",
              "description": "Text message to echo back verbatim"
            }
          },
          "required": [
            "text"
          ]
        },
        "score": 0.30000001192092896
      },
      {
        "name": "no_suspended__environment",
        "description": "Retrieves system environment variable values from the operating system configuration",
        "input_schema": {
          "type": "object",
          "properties": {
            "var": {
              "type": "string",
              "description": "System environment variable name to retrieve"
            }
          },
          "required": [
            "var"
          ]
        },
        "score": 0.30000001192092896
      },
      {
        "name": "no_suspended__fail",
        "description": "Always fails for testing error handling",
        "input_schema": {
          "type": "object",
          "properties": {}
        },
        "score": 0.30000001192092896
      }
    ]
    "#);

    // Suspended user with same search - should see no tools
    let mut suspended_headers = HeaderMap::new();
    suspended_headers.insert("X-Client-ID", "suspended_user".parse().unwrap());
    suspended_headers.insert("X-Client-Group", "suspended".parse().unwrap());
    let suspended_client = server.mcp_client_with_headers("/mcp", suspended_headers).await;

    let suspended_search = suspended_client.search(&["no_suspended"]).await;

    // Verify suspended user sees no tools (denied by group)
    insta::assert_json_snapshot!(suspended_search, @"[]");
}

/// Test that search respects complex allow/deny combinations
#[tokio::test]
async fn search_complex_allow_deny_rules() {
    let config = indoc! {r#"
        [server.client_identification]
        enabled = true
        client_id.http_header = "X-Client-ID"
        group_id.http_header = "X-Client-Group"

        [server.client_identification.validation]
        group_values = ["premium", "basic", "trial", "suspended"]

        [mcp]
        enabled = true

        # Server 1: Premium only
        [mcp.servers.premium_features]
        cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
        allow = ["premium"]

        # Server 2: All except suspended
        [mcp.servers.general_features]
        cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
        deny = ["suspended"]

        # Server 3: Basic and trial, but not suspended
        [mcp.servers.limited_features]
        cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
        allow = ["basic", "trial"]
        deny = ["suspended"]
    "#};

    let server = TestServer::builder().build(config).await;

    // Test premium user - should see premium_features and general_features
    let mut premium_headers = HeaderMap::new();
    premium_headers.insert("X-Client-ID", "premium_user".parse().unwrap());
    premium_headers.insert("X-Client-Group", "premium".parse().unwrap());
    let premium_client = server.mcp_client_with_headers("/mcp", premium_headers).await;

    let premium_search = premium_client.search(&["echo"]).await;

    // Premium user should see premium_features and general_features tools
    insta::assert_json_snapshot!(premium_search, @r#"
    [
      {
        "name": "general_features__echo",
        "description": "Echoes back the input text message verbatim for testing and debugging purposes",
        "input_schema": {
          "type": "object",
          "properties": {
            "text": {
              "type": "string",
              "description": "Text message to echo back verbatim"
            }
          },
          "required": [
            "text"
          ]
        },
        "score": 3.842801570892334
      },
      {
        "name": "premium_features__echo",
        "description": "Echoes back the input text message verbatim for testing and debugging purposes",
        "input_schema": {
          "type": "object",
          "properties": {
            "text": {
              "type": "string",
              "description": "Text message to echo back verbatim"
            }
          },
          "required": [
            "text"
          ]
        },
        "score": 3.842801570892334
      }
    ]
    "#);

    // Test basic user - should see general_features and limited_features
    let mut basic_headers = HeaderMap::new();
    basic_headers.insert("X-Client-ID", "basic_user".parse().unwrap());
    basic_headers.insert("X-Client-Group", "basic".parse().unwrap());
    let basic_client = server.mcp_client_with_headers("/mcp", basic_headers).await;

    let basic_search = basic_client.search(&["echo"]).await;

    // Basic user should see general_features and limited_features tools
    insta::assert_json_snapshot!(basic_search, @r#"
    [
      {
        "name": "general_features__echo",
        "description": "Echoes back the input text message verbatim for testing and debugging purposes",
        "input_schema": {
          "type": "object",
          "properties": {
            "text": {
              "type": "string",
              "description": "Text message to echo back verbatim"
            }
          },
          "required": [
            "text"
          ]
        },
        "score": 3.842801570892334
      },
      {
        "name": "limited_features__echo",
        "description": "Echoes back the input text message verbatim for testing and debugging purposes",
        "input_schema": {
          "type": "object",
          "properties": {
            "text": {
              "type": "string",
              "description": "Text message to echo back verbatim"
            }
          },
          "required": [
            "text"
          ]
        },
        "score": 3.842801570892334
      }
    ]
    "#);

    // Test suspended user - should see nothing
    let mut suspended_headers = HeaderMap::new();
    suspended_headers.insert("X-Client-ID", "suspended_user".parse().unwrap());
    suspended_headers.insert("X-Client-Group", "suspended".parse().unwrap());
    let suspended_client = server.mcp_client_with_headers("/mcp", suspended_headers).await;

    let suspended_search = suspended_client.search(&["echo"]).await;

    // Suspended user should see no tools
    insta::assert_json_snapshot!(suspended_search, @"[]");
}

/// Test that per-group indexes are isolated (no cross-contamination)
#[tokio::test]
async fn search_group_isolation_verified() {
    let config = indoc! {r#"
        [server.client_identification]
        enabled = true
        client_id.http_header = "X-Client-ID"
        group_id.http_header = "X-Client-Group"

        [server.client_identification.validation]
        group_values = ["group_a", "group_b"]

        [mcp]
        enabled = true

        [mcp.servers.group_a_only]
        cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
        allow = ["group_a"]

        [mcp.servers.group_b_only]
        cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
        allow = ["group_b"]
    "#};

    let server = TestServer::builder().build(config).await;

    // Group A search
    let mut group_a_headers = HeaderMap::new();
    group_a_headers.insert("X-Client-ID", "user_a".parse().unwrap());
    group_a_headers.insert("X-Client-Group", "group_a".parse().unwrap());
    let group_a_client = server.mcp_client_with_headers("/mcp", group_a_headers).await;

    let group_a_search = group_a_client.search(&["add"]).await;

    // Group A should only see group_a_only tools
    insta::assert_json_snapshot!(group_a_search, @r#"
    [
      {
        "name": "group_a_only__add",
        "description": "Performs mathematical addition operation on two numerical values and returns the sum",
        "input_schema": {
          "type": "object",
          "properties": {
            "a": {
              "type": "number",
              "description": "First numerical value for addition"
            },
            "b": {
              "type": "number",
              "description": "Second numerical value for addition"
            }
          },
          "required": [
            "a",
            "b"
          ]
        },
        "score": 3.611918449401855
      }
    ]
    "#);

    // Group B search
    let mut group_b_headers = HeaderMap::new();
    group_b_headers.insert("X-Client-ID", "user_b".parse().unwrap());
    group_b_headers.insert("X-Client-Group", "group_b".parse().unwrap());
    let group_b_client = server.mcp_client_with_headers("/mcp", group_b_headers).await;

    let group_b_search = group_b_client.search(&["add"]).await;

    // Group B should only see group_b_only tools
    insta::assert_json_snapshot!(group_b_search, @r#"
    [
      {
        "name": "group_b_only__add",
        "description": "Performs mathematical addition operation on two numerical values and returns the sum",
        "input_schema": {
          "type": "object",
          "properties": {
            "a": {
              "type": "number",
              "description": "First numerical value for addition"
            },
            "b": {
              "type": "number",
              "description": "Second numerical value for addition"
            }
          },
          "required": [
            "a",
            "b"
          ]
        },
        "score": 3.611918449401855
      }
    ]
    "#);
}

/// Test search with mixed tool-level and server-level permissions
#[tokio::test]
async fn search_mixed_permission_levels() {
    let config = indoc! {r#"
        [server.client_identification]
        enabled = true
        client_id.http_header = "X-Client-ID"
        group_id.http_header = "X-Client-Group"

        [server.client_identification.validation]
        group_values = ["basic", "premium", "admin"]

        [mcp]
        enabled = true

        # Use calculator server for basic functionality
        [mcp.servers.calc]
        cmd = ["python3", "mock-mcp-servers/calculator_server.py"]
        allow = ["basic", "premium", "admin"]

        # Use text processor for premium features
        [mcp.servers.text]
        cmd = ["python3", "mock-mcp-servers/text_processor_server.py"]
        allow = ["premium", "admin"]

        # Use filesystem for admin only
        [mcp.servers.fs]
        cmd = ["python3", "mock-mcp-servers/filesystem_server.py"]
        allow = ["admin"]
    "#};

    let server = TestServer::builder().build(config).await;

    // Basic user - should see only calculator tool
    let mut basic_headers = HeaderMap::new();
    basic_headers.insert("X-Client-ID", "basic_user".parse().unwrap());
    basic_headers.insert("X-Client-Group", "basic".parse().unwrap());
    let basic_client = server.mcp_client_with_headers("/mcp", basic_headers).await;

    let basic_search = basic_client.search(&["calculator"]).await;

    // Basic user should see only calculator tool
    insta::assert_json_snapshot!(basic_search, @r#"
    [
      {
        "name": "calc__calculator",
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

    // Premium user - should see calculator and text_processor but not filesystem
    let mut premium_headers = HeaderMap::new();
    premium_headers.insert("X-Client-ID", "premium_user".parse().unwrap());
    premium_headers.insert("X-Client-Group", "premium".parse().unwrap());
    let premium_client = server.mcp_client_with_headers("/mcp", premium_headers).await;

    let premium_search = premium_client.search(&["process"]).await;

    // Premium user should see text_processor (score will be higher than calculator)
    insta::assert_json_snapshot!(premium_search, @r#"
    [
      {
        "name": "text__text_processor",
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
        "score": 0.4000000059604645
      }
    ]
    "#);

    // Admin user - should see everything
    let mut admin_headers = HeaderMap::new();
    admin_headers.insert("X-Client-ID", "admin_user".parse().unwrap());
    admin_headers.insert("X-Client-Group", "admin".parse().unwrap());
    let admin_client = server.mcp_client_with_headers("/mcp", admin_headers).await;

    // Search for "filesystem" to get filesystem tool with high score
    let admin_search = admin_client.search(&["filesystem"]).await;

    // Admin user should see filesystem tool (and possibly others matching "file")
    insta::assert_json_snapshot!(admin_search, @r#"
    [
      {
        "name": "fs__filesystem",
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
        "score": 3.277707576751709
      }
    ]
    "#);
}

/// Test that configuration without groups still works (backwards compatibility)
#[tokio::test]
async fn search_no_groups_configured() {
    let config = indoc! {r#"
        [mcp]
        enabled = true

        [mcp.servers.tools]
        cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
    "#};

    let server = TestServer::builder().build(config).await;
    let client = server.mcp_client("/mcp").await;

    // Search should work without any group configuration
    let search_result = client.search(&["echo"]).await;

    // Should find the echo tool when no groups are configured
    insta::assert_json_snapshot!(search_result, @r#"
    [
      {
        "name": "tools__echo",
        "description": "Echoes back the input text message verbatim for testing and debugging purposes",
        "input_schema": {
          "type": "object",
          "properties": {
            "text": {
              "type": "string",
              "description": "Text message to echo back verbatim"
            }
          },
          "required": [
            "text"
          ]
        },
        "score": 3.611918449401855
      }
    ]
    "#);
}
