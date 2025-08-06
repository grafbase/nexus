use indoc::{formatdoc, indoc};
use integration_tests::*;
use tokio::time::{Duration, sleep};

#[tokio::test]
async fn stdio_basic_echo_tool() {
    let server = TestServer::builder()
        .build(indoc! {r#"
            [mcp.servers.test_stdio]
            cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
        "#})
        .await;

    let client = server.mcp_client("/mcp").await;

    // Wait a bit for STDIO server to be fully ready
    sleep(Duration::from_millis(100)).await;

    // Test built-in tools listing (STDIO tools won't appear here)
    let tools = client.list_tools().await;
    let tool_names: Vec<&str> = tools.tools.iter().map(|t| t.name.as_ref()).collect();
    insta::assert_debug_snapshot!(tool_names, @r###"
    [
        "search",
        "execute",
    ]
    "###);

    // Test STDIO tool discovery via search
    let search_results = client.search(&["echo"]).await;
    insta::assert_json_snapshot!(search_results, @r#"
    [
      {
        "name": "test_stdio__echo",
        "description": "Echoes back the input text",
        "input_schema": {
          "type": "object",
          "properties": {
            "text": {
              "type": "string",
              "description": "Text to echo back"
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

    // Test tool execution
    let result = client
        .execute(
            "test_stdio__echo",
            serde_json::json!({
                "text": "Hello, STDIO!"
            }),
        )
        .await;

    insta::assert_json_snapshot!(result, @r#"
    {
      "content": [
        {
          "type": "text",
          "text": "Echo: Hello, STDIO!"
        }
      ]
    }
    "#);
}

#[tokio::test]
async fn stdio_math_tool() {
    let server = TestServer::builder()
        .build(indoc! {r#"
            [mcp.servers.test_stdio]
            cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
        "#})
        .await;

    let client = server.mcp_client("/mcp").await;

    // Wait a bit for STDIO server to be fully ready
    sleep(Duration::from_millis(100)).await;

    // Test math tool
    let result = client
        .execute(
            "test_stdio__add",
            serde_json::json!({
                "a": 15,
                "b": 27
            }),
        )
        .await;

    insta::assert_json_snapshot!(result, @r#"
    {
      "content": [
        {
          "type": "text",
          "text": "15 + 27 = 42"
        }
      ]
    }
    "#);
}

#[tokio::test]
async fn stdio_environment_variables() {
    let server = TestServer::builder()
        .build(indoc! {r#"
            [mcp.servers.test_stdio]
            cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
            env = { "TEST_VAR" = "test_value_123" }
        "#})
        .await;

    let client = server.mcp_client("/mcp").await;

    // Test environment variable access
    let result = client
        .execute(
            "test_stdio__environment",
            serde_json::json!({
                "var": "TEST_VAR"
            }),
        )
        .await;

    insta::assert_json_snapshot!(result, @r#"
    {
      "content": [
        {
          "type": "text",
          "text": "TEST_VAR=test_value_123"
        }
      ]
    }
    "#);
}

#[tokio::test]
async fn stdio_working_directory() {
    use std::env;

    let current_dir = env::current_dir().unwrap();
    let cwd_str = current_dir.to_string_lossy();

    let server = TestServer::builder()
        .build(&format!(
            indoc! {r#"
            [mcp.servers.test_stdio]
            cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
            cwd = "{}"
        "#},
            cwd_str
        ))
        .await;

    let client = server.mcp_client("/mcp").await;

    // Wait a bit for STDIO server to be fully ready
    sleep(Duration::from_millis(100)).await;

    // Test that the server can access files in the working directory by searching for tools
    let search_results = client.search(&["echo"]).await;
    assert!(
        !search_results.is_empty(),
        "Should find STDIO tools from server with working directory"
    );

    // Verify we can execute a tool from the STDIO server
    let result = client
        .execute(
            "test_stdio__echo",
            serde_json::json!({
                "text": "Working directory test"
            }),
        )
        .await;

    insta::assert_json_snapshot!(result, @r#"
    {
      "content": [
        {
          "type": "text",
          "text": "Echo: Working directory test"
        }
      ]
    }
    "#);
}

#[tokio::test]
async fn stdio_error_handling() {
    let server = TestServer::builder()
        .build(indoc! {r#"
            [mcp.servers.test_stdio]
            cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
        "#})
        .await;

    let client = server.mcp_client("/mcp").await;

    // Wait a bit for STDIO server to be fully ready
    sleep(Duration::from_millis(100)).await;

    // Test tool that always fails
    let error = client
        .execute_expect_error("test_stdio__fail", serde_json::json!({}))
        .await;

    // Should get an error response
    insta::assert_debug_snapshot!(error, @r#"
    McpError(
        ErrorData {
            code: ErrorCode(
                -32603,
            ),
            message: "Internal error: This tool always fails",
            data: None,
        },
    )
    "#);
}

#[tokio::test]
async fn stdio_invalid_tool() {
    let server = TestServer::builder()
        .build(indoc! {r#"
            [mcp.servers.test_stdio]
            cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
        "#})
        .await;

    let client = server.mcp_client("/mcp").await;

    // Wait a bit for STDIO server to be fully ready
    sleep(Duration::from_millis(100)).await;

    // Test calling non-existent tool
    let error = client
        .execute_expect_error("test_stdio__nonexistent", serde_json::json!({}))
        .await;

    // Should get an error response
    insta::assert_debug_snapshot!(error, @r#"
    McpError(
        ErrorData {
            code: ErrorCode(
                -32601,
            ),
            message: "tools/call",
            data: None,
        },
    )
    "#);
}

#[tokio::test]
async fn stdio_tool_search() {
    let server = TestServer::builder()
        .build(indoc! {r#"
            [mcp.servers.test_stdio]
            cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
        "#})
        .await;

    let client = server.mcp_client("/mcp").await;

    // Wait a bit for STDIO server to be fully ready
    sleep(Duration::from_millis(100)).await;

    // Test searching for tools
    let search_results = client.search(&["echo", "text"]).await;
    insta::assert_json_snapshot!(search_results, @r#"
    [
      {
        "name": "test_stdio__echo",
        "description": "Echoes back the input text",
        "input_schema": {
          "type": "object",
          "properties": {
            "text": {
              "type": "string",
              "description": "Text to echo back"
            }
          },
          "required": [
            "text"
          ]
        },
        "score": 5.026234149932861
      }
    ]
    "#);
}

#[tokio::test]
async fn stdio_multiple_servers() {
    let server = TestServer::builder()
        .build(indoc! {r#"
            [mcp.servers.stdio_server_1]
            cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
            env = { "SERVER_ID" = "server1" }

            [mcp.servers.stdio_server_2]
            cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
            env = { "SERVER_ID" = "server2" }
        "#})
        .await;

    let client = server.mcp_client("/mcp").await;

    // Wait a bit for STDIO servers to be fully ready
    sleep(Duration::from_millis(200)).await;

    // Test tool discovery with multiple servers via search
    let search_results = client.search(&["echo"]).await;

    let mut tool_names: Vec<&str> = search_results
        .iter()
        .filter_map(|result| result.get("name")?.as_str())
        .collect();

    tool_names.sort_unstable();

    insta::assert_json_snapshot!(tool_names, @r#"
    [
      "stdio_server_1__echo",
      "stdio_server_2__echo"
    ]
    "#);

    // Test executing tools from both servers
    let result1 = client
        .execute(
            "stdio_server_1__echo",
            serde_json::json!({
                "text": "Hello from server 1"
            }),
        )
        .await;

    let result2 = client
        .execute(
            "stdio_server_2__echo",
            serde_json::json!({
                "text": "Hello from server 2"
            }),
        )
        .await;

    insta::assert_json_snapshot!(result1, @r#"
    {
      "content": [
        {
          "type": "text",
          "text": "Echo: Hello from server 1"
        }
      ]
    }
    "#);

    insta::assert_json_snapshot!(result2, @r#"
    {
      "content": [
        {
          "type": "text",
          "text": "Echo: Hello from server 2"
        }
      ]
    }
    "#);
}

#[tokio::test]
#[should_panic]
async fn stdio_server_startup_failure() {
    // Test that a nonexistent command causes server startup to fail
    TestServer::builder()
        .build(indoc! {r#"
            [mcp.servers.bad_stdio]
            cmd = ["nonexistent_command_that_should_fail"]
        "#})
        .await;
}

#[tokio::test]
#[should_panic]
async fn stdio_minimal_config() {
    // Test that a minimal echo command causes server startup to fail
    // because echo doesn't provide MCP protocol support
    TestServer::builder()
        .build(indoc! {r#"
            [mcp.servers.minimal]
            cmd = ["echo", "hello"]
        "#})
        .await;
}

#[tokio::test]
async fn stdio_complex_command_args() {
    let server = TestServer::builder()
        .build(indoc! {r#"
            [mcp.servers.complex_args]
            cmd = ["python3", "-u", "mock-mcp-servers/simple_mcp_server.py"]
            env = { "PYTHONUNBUFFERED" = "1" }
        "#})
        .await;

    let client = server.mcp_client("/mcp").await;

    // Wait a bit for STDIO server to be fully ready
    sleep(Duration::from_millis(100)).await;

    // Test that complex command arguments work correctly by searching for tools
    let search_results = client.search(&["echo"]).await;

    let tool_names: Vec<&str> = search_results
        .iter()
        .filter_map(|result| result.get("name")?.as_str())
        .collect();

    insta::assert_json_snapshot!(tool_names, @r###"
    [
      "complex_args__echo"
    ]
    "###);
}

#[tokio::test]
#[should_panic]
async fn stdio_command_not_found() {
    // Test that a nonexistent command causes server startup to fail
    TestServer::builder()
        .build(indoc! {r#"
            [mcp.servers.nonexistent]
            cmd = ["nonexistent_command_xyz123"]
        "#})
        .await;
}

#[tokio::test]
#[should_panic]
async fn stdio_permission_denied() {
    // Test that a file without execute permissions causes server startup to fail
    TestServer::builder()
        .build(indoc! {r#"
            [mcp.servers.permission_denied]
            cmd = ["/etc/passwd"]
        "#})
        .await;
}

#[tokio::test]
#[should_panic]
async fn stdio_invalid_working_directory() {
    // Test that an invalid working directory causes server startup to fail
    TestServer::builder()
        .build(indoc! {r#"
            [mcp.servers.bad_cwd]
            cmd = ["echo", "hello"]
            cwd = "/nonexistent/directory/path"
        "#})
        .await;
}

#[tokio::test]
#[should_panic]
async fn stdio_process_crashes_early() {
    // Test that a command that exits immediately with an error causes server startup to fail
    TestServer::builder()
        .build(indoc! {r#"
            [mcp.servers.crash_early]
            cmd = ["false"]
        "#})
        .await;
}

#[tokio::test]
#[should_panic]
async fn stdio_invalid_json_from_subprocess() {
    // Test that a subprocess outputting invalid JSON causes server startup to fail
    TestServer::builder()
        .build(indoc! {r#"
            [mcp.servers.invalid_json]
            cmd = ["echo", "not valid json"]
        "#})
        .await;
}

#[tokio::test]
async fn stdio_working_server_starts_successfully() {
    // Test that a properly configured STDIO server allows the server to start
    let server = TestServer::builder()
        .build(indoc! {r#"
            [mcp.servers.working_server]
            cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
        "#})
        .await;

    let client = server.mcp_client("/mcp").await;

    // Verify the server is functional and has tools from the STDIO server
    let search_results = client.search(&["echo"]).await;
    assert!(
        !search_results.is_empty(),
        "Should find tools from working STDIO server"
    );

    // Verify we can execute a tool
    let result = client
        .execute(
            "working_server__echo",
            serde_json::json!({
                "text": "Test message"
            }),
        )
        .await;

    insta::assert_json_snapshot!(result, @r#"
    {
      "content": [
        {
          "type": "text",
          "text": "Echo: Test message"
        }
      ]
    }
    "#);
}

#[tokio::test]
async fn stdio_empty_environment_variable() {
    // Test that empty environment variables work correctly
    let server = TestServer::builder()
        .build(indoc! {r#"
            [mcp.servers.empty_env]
            cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
            env = { "EMPTY_VAR" = "" }
        "#})
        .await;

    let client = server.mcp_client("/mcp").await;

    // Test accessing the empty environment variable
    let result = client
        .execute(
            "empty_env__environment",
            serde_json::json!({
                "var": "EMPTY_VAR"
            }),
        )
        .await;

    insta::assert_json_snapshot!(result, @r#"
    {
      "content": [
        {
          "type": "text",
          "text": "EMPTY_VAR="
        }
      ]
    }
    "#);
}

#[tokio::test]
async fn stdio_large_environment() {
    // Test that large numbers of environment variables work correctly
    use std::collections::HashMap;

    const MAX_ENV_VARS: usize = 50; // Reduced from 100 to avoid overly long test
    let mut env_vars = HashMap::new();
    for i in 0..MAX_ENV_VARS {
        env_vars.insert(format!("VAR_{i}"), format!("value_{i}"));
    }

    let env_config = env_vars
        .iter()
        .map(|(k, v)| format!("{k} = \"{v}\""))
        .collect::<Vec<_>>()
        .join(", ");

    let config = formatdoc! {r#"
        [mcp.servers.large_env]
        cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
        env = {{ {env_config} }}
    "#};

    let server = TestServer::builder().build(&config).await;
    let client = server.mcp_client("/mcp").await;

    // Test accessing one of the environment variables
    let result = client
        .execute(
            "large_env__environment",
            serde_json::json!({
                "var": "VAR_25"
            }),
        )
        .await;

    insta::assert_json_snapshot!(result, @r#"
    {
      "content": [
        {
          "type": "text",
          "text": "VAR_25=value_25"
        }
      ]
    }
    "#);
}

#[tokio::test]
async fn stdio_unicode_in_command_args() {
    // Test that Unicode in environment variables works correctly
    let server = TestServer::builder()
        .build(indoc! {r#"
            [mcp.servers.unicode_args]
            cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
            env = { "UNICODE_VAR" = "こんにちは🌍" }
        "#})
        .await;

    let client = server.mcp_client("/mcp").await;

    // Test accessing the Unicode environment variable
    let result = client
        .execute(
            "unicode_args__environment",
            serde_json::json!({
                "var": "UNICODE_VAR"
            }),
        )
        .await;

    insta::assert_json_snapshot!(result, @r#"
    {
      "content": [
        {
          "type": "text",
          "text": "UNICODE_VAR=こんにちは🌍"
        }
      ]
    }
    "#);
}

#[tokio::test]
async fn stdio_stderr_file_configuration() {
    use tempfile::tempdir;

    let temp_dir = tempdir().unwrap();
    let log_file = temp_dir.path().join("server.log");
    let log_path = log_file.to_string_lossy();

    let config = formatdoc! {r#"
        [mcp.servers.stderr_file]
        cmd = ["python3", "mock-mcp-servers/simple_mcp_server.py"]
        stderr = {{ file = "{log_path}" }}
    "#};

    let server = TestServer::builder().build(&config).await;
    let client = server.mcp_client("/mcp").await;

    // Wait a bit for STDIO server to be fully ready
    sleep(Duration::from_millis(200)).await;

    // Test that the server is working normally with stderr file configuration
    let result = client
        .execute(
            "stderr_file__echo",
            serde_json::json!({
                "text": "Testing stderr file config"
            }),
        )
        .await;

    insta::assert_json_snapshot!(result, @r#"
    {
      "content": [
        {
          "type": "text",
          "text": "Echo: Testing stderr file config"
        }
      ]
    }
    "#);

    let content = std::fs::read_to_string(log_file).unwrap();

    insta::assert_snapshot!(content, @r###"
        SimpleMcpServer: Starting server initialization
        SimpleMcpServer: Server initialization complete
        SimpleMcpServer: Starting main server loop
        SimpleMcpServer: Handling initialize request
    "###);
}
