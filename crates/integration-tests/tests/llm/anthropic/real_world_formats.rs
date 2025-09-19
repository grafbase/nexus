use indoc::indoc;
use integration_tests::{TestServer, llms::AnthropicMock};
use serde_json::json;

/// Test the exact message format that Claude Code sends when using multiple tools
/// This replicates the issue that was causing "tool_use ids must be unique" errors
#[tokio::test]
async fn claude_code_multiple_tool_scenario() {
    let mut builder = TestServer::builder();

    builder
        .spawn_llm(AnthropicMock::new("anthropic").with_models(vec!["claude-3-5-haiku-latest".to_string()]))
        .await;

    let config = indoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.anthropic]
        enabled = true
        path = "/llm/anthropic"
    "#};

    let server = builder.build(config).await;

    // This is the exact format Claude Code was sending that caused problems
    let request = json!({
        "model": "anthropic/claude-3-5-haiku-latest",
        "messages": [
            {
                "role": "user",
                "content": "I want to understand the structure of this codebase. Can you help me explore it?"
            },
            {
                "role": "assistant",
                "content": [
                    {
                        "type": "text",
                        "text": "I'll help you explore the codebase structure. Let me start by looking at the overall directory structure and then examine some key files."
                    },
                    {
                        "type": "tool_use",
                        "id": "toolu_01A6FvQKjQZXmG8LhH7hZgv1",
                        "name": "Bash",
                        "input": {
                            "command": "find . -type f -name '*.rs' | head -20",
                            "description": "Find Rust source files to understand the codebase structure"
                        }
                    }
                ]
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_01A6FvQKjQZXmG8LhH7hZgv1",
                        "content": [
                            {
                                "type": "text",
                                "text": "./nexus/src/main.rs\n./nexus/src/logger.rs\n./nexus/src/args.rs\n./crates/server/src/lib.rs\n./crates/config/src/lib.rs"
                            }
                        ]
                    }
                ]
            },
            {
                "role": "assistant",
                "content": [
                    {
                        "type": "text",
                        "text": "Great! I can see this is a Rust project with a binary in `nexus/` and multiple crates. Let me get more details about the project structure."
                    },
                    {
                        "type": "tool_use",
                        "id": "toolu_01B7GwRLkQAYnH9MiI8iAhw2",
                        "name": "Read",
                        "input": {
                            "file_path": "./Cargo.toml"
                        }
                    },
                    {
                        "type": "tool_use",
                        "id": "toolu_01C8HxSMlRBZoI0NjJ9jBix3",
                        "name": "Glob",
                        "input": {
                            "pattern": "crates/*/Cargo.toml"
                        }
                    }
                ]
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_01B7GwRLkQAYnH9MiI8iAhw2",
                        "content": [
                            {
                                "type": "text",
                                "text": "[workspace]\nmembers = [\n    \"nexus\",\n    \"crates/config\",\n    \"crates/server\"\n]"
                            }
                        ]
                    },
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_01C8HxSMlRBZoI0NjJ9jBix3",
                        "content": [
                            {
                                "type": "text",
                                "text": "crates/config/Cargo.toml\ncrates/server/Cargo.toml\ncrates/llm/Cargo.toml"
                            }
                        ]
                    }
                ]
            }
        ],
        "max_tokens": 1024
    });

    // This should work without any "tool_use ids must be unique" errors
    let response = server.anthropic_completions(request).send().await;

    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".model" => "[model]",
        ".usage" => "[usage]"
    }, @r###"
    {
      "id": "[id]",
      "type": "message",
      "role": "assistant",
      "content": [
        {
          "type": "text",
          "text": "Test response to: I want to understand the structure of this codebase. Can you help me explore it?"
        }
      ],
      "model": "[model]",
      "stop_reason": null,
      "stop_sequence": null,
      "usage": "[usage]"
    }
    "###);
}

/// Test the mixed content format that Claude Code uses - strings and arrays
/// This was causing deserialization issues
#[tokio::test]
async fn claude_code_mixed_content_conversation() {
    let mut builder = TestServer::builder();

    builder
        .spawn_llm(AnthropicMock::new("anthropic").with_models(vec!["claude-3-5-sonnet-20241022".to_string()]))
        .await;

    let config = indoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.anthropic]
        enabled = true
        path = "/llm/anthropic"
    "#};

    let server = builder.build(config).await;

    // Real conversation format from Claude Code with mixed content types
    let request = json!({
        "model": "anthropic/claude-3-5-sonnet-20241022",
        "messages": [
            {
                "role": "user",
                "content": "Create a simple HTTP server in Rust using Axum"
            },
            {
                "role": "assistant",
                "content": [
                    {
                        "type": "text",
                        "text": "I'll help you create a simple HTTP server using Axum. Let me create the necessary files and code structure."
                    },
                    {
                        "type": "tool_use",
                        "id": "toolu_create_cargo",
                        "name": "Write",
                        "input": {
                            "file_path": "./Cargo.toml",
                            "content": "[package]\nname = \"axum-server\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\naxum = \"0.7\"\ntokio = { version = \"1.0\", features = [\"full\"] }\n"
                        }
                    }
                ]
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_create_cargo",
                        "content": [
                            {
                                "type": "text",
                                "text": "File created successfully."
                            }
                        ]
                    }
                ]
            },
            {
                "role": "assistant",
                "content": "Now let me create the main server file:"  // String format
            },
            {
                "role": "user",
                "content": "Great! What comes next?"  // String format
            },
            {
                "role": "assistant",
                "content": [  // Array format
                    {
                        "type": "text",
                        "text": "I'll create the main.rs file with a basic Axum server setup."
                    },
                    {
                        "type": "tool_use",
                        "id": "toolu_create_main",
                        "name": "Write",
                        "input": {
                            "file_path": "./src/main.rs",
                            "content": "use axum::{response::Html, routing::get, Router};\n\n#[tokio::main]\nasync fn main() {\n    let app = Router::new().route(\"/\", get(hello));\n    \n    println!(\"Server running on http://localhost:3000\");\n    \n    let listener = tokio::net::TcpListener::bind(\"0.0.0.0:3000\")\n        .await\n        .unwrap();\n    \n    axum::serve(listener, app).await.unwrap();\n}\n\nasync fn hello() -> Html<&'static str> {\n    Html(\"<h1>Hello, World!</h1>\")\n}\n"
                        }
                    }
                ]
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_create_main",
                        "content": [
                            {
                                "type": "text",
                                "text": "main.rs created successfully."
                            }
                        ]
                    }
                ]
            }
        ],
        "max_tokens": 1024
    });

    let response = server.anthropic_completions(request).send().await;

    // Should handle mixed content formats without errors
    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".model" => "[model]",
        ".usage" => "[usage]"
    }, @r###"
    {
      "id": "[id]",
      "type": "message",
      "role": "assistant",
      "content": [
        {
          "type": "text",
          "text": "Test response to: Create a simple HTTP server in Rust using Axum"
        }
      ],
      "model": "[model]",
      "stop_reason": null,
      "stop_sequence": null,
      "usage": "[usage]"
    }
    "###);
}

/// Test the scenario where Claude Code sends tool arguments as JSON values vs strings
/// This covers argument parsing edge cases
#[tokio::test]
async fn claude_code_tool_argument_variations() {
    let mut builder = TestServer::builder();

    builder
        .spawn_llm(AnthropicMock::new("anthropic").with_models(vec!["claude-3-5-sonnet-20241022".to_string()]))
        .await;

    let config = indoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.anthropic]
        enabled = true
        path = "/llm/anthropic"
    "#};

    let server = builder.build(config).await;

    // Real message showing various argument formats
    let request = json!({
        "model": "anthropic/claude-3-5-sonnet-20241022",
        "messages": [
            {
                "role": "user",
                "content": "Help me analyze this project"
            },
            {
                "role": "assistant",
                "content": [
                    {
                        "type": "text",
                        "text": "I'll analyze the project using different tools with various argument formats."
                    },
                    {
                        "type": "tool_use",
                        "id": "toolu_glob_001",
                        "name": "Glob",
                        "input": {
                            "pattern": "**/*.rs",
                            "path": null  // null value
                        }
                    },
                    {
                        "type": "tool_use",
                        "id": "toolu_bash_002",
                        "name": "Bash",
                        "input": {
                            "command": "wc -l src/*.rs",
                            "timeout": 5000,
                            "run_in_background": false
                        }
                    },
                    {
                        "type": "tool_use",
                        "id": "toolu_read_003",
                        "name": "Read",
                        "input": {
                            "file_path": "./README.md",
                            "limit": 100
                        }
                    }
                ]
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_glob_001",
                        "content": [
                            {
                                "type": "text",
                                "text": "src/main.rs\nsrc/lib.rs\nsrc/utils.rs"
                            }
                        ]
                    },
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_bash_002",
                        "content": [
                            {
                                "type": "text",
                                "text": "  245 src/main.rs\n  189 src/lib.rs\n   67 src/utils.rs\n  501 total"
                            }
                        ]
                    },
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_read_003",
                        "content": [
                            {
                                "type": "text",
                                "text": "# My Project\n\nThis is a Rust project that does amazing things."
                            }
                        ]
                    }
                ]
            }
        ],
        "max_tokens": 500
    });

    let response = server.anthropic_completions(request).send().await;

    // Should handle various argument types (null, numbers, booleans, strings)
    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".model" => "[model]",
        ".usage" => "[usage]"
    }, @r###"
    {
      "id": "[id]",
      "type": "message",
      "role": "assistant",
      "content": [
        {
          "type": "text",
          "text": "Test response to: Help me analyze this project"
        }
      ],
      "model": "[model]",
      "stop_reason": null,
      "stop_sequence": null,
      "usage": "[usage]"
    }
    "###);
}

/// Test the OpenAI format conversion to Anthropic when tool calls are present
/// This tests the specific conversion logic that was causing duplicate IDs
#[tokio::test]
async fn openai_to_anthropic_tool_conversion() {
    let mut builder = TestServer::builder();

    builder
        .spawn_llm(AnthropicMock::new("anthropic").with_models(vec!["claude-3-5-sonnet-20241022".to_string()]))
        .await;

    let config = indoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.openai]
        enabled = true
        path = "/llm"
    "#};

    let server = builder.build(config).await;

    // OpenAI format that gets converted to Anthropic internally
    let request = json!({
        "model": "anthropic/claude-3-5-sonnet-20241022",
        "messages": [
            {
                "role": "user",
                "content": "Help me with file operations"
            },
            {
                "role": "assistant",
                "content": "I'll help you with file operations.",
                "tool_calls": [
                    {
                        "id": "call_file_read_001",
                        "type": "function",
                        "function": {
                            "name": "Read",
                            "arguments": "{\"file_path\": \"./config.toml\"}"
                        }
                    },
                    {
                        "id": "call_file_write_002",
                        "type": "function",
                        "function": {
                            "name": "Write",
                            "arguments": "{\"file_path\": \"./output.txt\", \"content\": \"Hello World\"}"
                        }
                    }
                ]
            },
            {
                "role": "tool",
                "content": "# Configuration\nport = 8080",
                "tool_call_id": "call_file_read_001"
            },
            {
                "role": "tool",
                "content": "File written successfully",
                "tool_call_id": "call_file_write_002"
            },
            {
                "role": "user",
                "content": "Great! Now what's in the config?"
            }
        ],
        "tools": [
            {
                "type": "function",
                "function": {
                    "name": "Read",
                    "description": "Read file contents",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "file_path": {"type": "string"}
                        }
                    }
                }
            },
            {
                "type": "function",
                "function": {
                    "name": "Write",
                    "description": "Write file contents",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "file_path": {"type": "string"},
                            "content": {"type": "string"}
                        }
                    }
                }
            }
        ]
    });

    let response = server.openai_completions(request).send().await;

    // Should convert correctly without duplicate tool_use IDs
    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".created" => "[timestamp]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[timestamp]",
      "model": "anthropic/claude-3-5-sonnet-20241022",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "Test response to: Help me with file operations"
          },
          "finish_reason": "stop"
        }
      ],
      "usage": {
        "prompt_tokens": 10,
        "completion_tokens": 15,
        "total_tokens": 25
      }
    }
    "#);
}

/// Test the scenario that was causing role mapping issues
/// This covers user messages with tool_result content vs tool role messages
#[tokio::test]
async fn role_mapping_tool_results() {
    let mut builder = TestServer::builder();

    builder
        .spawn_llm(AnthropicMock::new("anthropic").with_models(vec!["claude-3-5-sonnet-20241022".to_string()]))
        .await;

    let config = indoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.anthropic]
        enabled = true
        path = "/llm/anthropic"
    "#};

    let server = builder.build(config).await;

    // This tests the specific case where tool results come in user messages
    // vs. dedicated tool role messages in OpenAI format
    let request = json!({
        "model": "anthropic/claude-3-5-sonnet-20241022",
        "messages": [
            {
                "role": "user",
                "content": "Check the system status"
            },
            {
                "role": "assistant",
                "content": [
                    {
                        "type": "text",
                        "text": "I'll check the system status for you."
                    },
                    {
                        "type": "tool_use",
                        "id": "toolu_status_check",
                        "name": "Bash",
                        "input": {
                            "command": "systemctl status nginx",
                            "description": "Check nginx service status"
                        }
                    }
                ]
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_status_check",
                        "content": [
                            {
                                "type": "text",
                                "text": "● nginx.service - A high performance web server\n   Loaded: loaded\n   Active: active (running)"
                            }
                        ]
                    },
                    {
                        "type": "text",
                        "text": "Also, can you check disk space?"
                    }
                ]
            },
            {
                "role": "assistant",
                "content": [
                    {
                        "type": "text",
                        "text": "The nginx service is running well. Let me check the disk space."
                    },
                    {
                        "type": "tool_use",
                        "id": "toolu_disk_check",
                        "name": "Bash",
                        "input": {
                            "command": "df -h",
                            "description": "Check disk space usage"
                        }
                    }
                ]
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_disk_check",
                        "content": [
                            {
                                "type": "text",
                                "text": "Filesystem      Size  Used Avail Use%\n/dev/sda1        50G   30G   18G  63%"
                            }
                        ]
                    }
                ]
            }
        ],
        "max_tokens": 300
    });

    let response = server.anthropic_completions(request).send().await;

    // Should handle mixed user content (tool_result + text) correctly
    insta::assert_json_snapshot!(response, {
        ".id" => "[id]",
        ".model" => "[model]",
        ".usage" => "[usage]"
    }, @r###"
    {
      "id": "[id]",
      "type": "message",
      "role": "assistant",
      "content": [
        {
          "type": "text",
          "text": "Test response to: Check the system status"
        }
      ],
      "model": "[model]",
      "stop_reason": null,
      "stop_sequence": null,
      "usage": "[usage]"
    }
    "###);
}

/// Test that duplicate tool IDs are correctly rejected
/// This ensures we don't silently accept invalid requests
#[tokio::test]
async fn duplicate_tool_ids_correctly_rejected() {
    let mut builder = TestServer::builder();

    builder
        .spawn_llm(AnthropicMock::new("anthropic").with_models(vec!["claude-3-5-sonnet-20241022".to_string()]))
        .await;

    let config = indoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.anthropic]
        enabled = true
        path = "/llm/anthropic"
    "#};

    let server = builder.build(config).await;

    // This message simulates the case where the same tool_use ID appears multiple times
    // This should be rejected by the API
    let request = json!({
        "model": "anthropic/claude-3-5-sonnet-20241022",
        "messages": [
            {
                "role": "user",
                "content": "Process multiple files"
            },
            {
                "role": "assistant",
                "content": [
                    {
                        "type": "text",
                        "text": "I'll process the files for you."
                    },
                    {
                        "type": "tool_use",
                        "id": "toolu_process_001",
                        "name": "Read",
                        "input": {"file_path": "file1.txt"}
                    },
                    {
                        "type": "tool_use",
                        "id": "toolu_process_001",  // Duplicate ID - should cause error
                        "name": "Read",
                        "input": {"file_path": "file2.txt"}
                    },
                    {
                        "type": "tool_use",
                        "id": "toolu_process_002",  // Unique ID
                        "name": "Write",
                        "input": {"file_path": "output.txt", "content": "processed"}
                    }
                ]
            }
        ],
        "max_tokens": 200
    });

    // This should fail with "tool_use ids must be unique" error
    // We do not silently deduplicate - it's the client's responsibility to fix
    let (status, body) = server.anthropic_completions(request).send_raw().await;

    assert_eq!(status, 502);
    insta::assert_json_snapshot!(body, @r###"
    {
      "type": "error",
      "error": {
        "type": "invalid_request_error",
        "message": "messages: `tool_use` ids must be unique (duplicate id: toolu_process_001)"
      }
    }
    "###);
}
