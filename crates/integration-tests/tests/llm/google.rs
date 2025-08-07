use indoc::indoc;
use integration_tests::{TestServer, llms::GoogleMock};
use serde_json::json;

#[tokio::test]
async fn list_models() {
    let mut builder = TestServer::builder();
    builder.spawn_llm(GoogleMock::new("google")).await;

    let server = builder.build("").await;
    let llm = server.llm_client("/llm");
    let body = llm.list_models().await;

    insta::assert_json_snapshot!(body, @r#"
    {
      "object": "list",
      "data": [
        {
          "id": "google/gemini-1.5-flash",
          "object": "model",
          "created": 0,
          "owned_by": "google"
        },
        {
          "id": "google/gemini-1.5-pro",
          "object": "model",
          "created": 0,
          "owned_by": "google"
        },
        {
          "id": "google/gemini-pro",
          "object": "model",
          "created": 0,
          "owned_by": "google"
        }
      ]
    }
    "#);
}

#[tokio::test]
async fn chat_completion() {
    let mut builder = TestServer::builder();
    builder.spawn_llm(GoogleMock::new("google")).await;

    let server = builder.build("").await;
    let llm = server.llm_client("/llm");

    let request = json!({
        "model": "google/gemini-1.5-flash",
        "messages": [
            {
                "role": "system",
                "content": "You are a helpful assistant"
            },
            {
                "role": "user",
                "content": "Hello!"
            }
        ],
        "temperature": 0.7,
        "max_tokens": 100
    });

    let body = llm.completions(request).await;

    insta::assert_json_snapshot!(body, {
        ".id" => "[id]",
        ".created" => "[created]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[created]",
      "model": "google/gemini-1.5-flash",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "Hello! I'm Gemini, a test assistant. How can I help you today?"
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

#[tokio::test]
async fn handles_system_messages() {
    let mut builder = TestServer::builder();
    builder.spawn_llm(GoogleMock::new("google")).await;

    let server = builder.build("").await;
    let llm = server.llm_client("/llm");

    // Test with system message
    let request = json!({
        "model": "google/gemini-1.5-pro",
        "messages": [
            {
                "role": "system",
                "content": "You are a creative writing assistant. Always respond in a poetic style."
            },
            {
                "role": "user",
                "content": "How are you?"
            }
        ]
    });

    let body = llm.completions(request).await;

    insta::assert_json_snapshot!(body, {
        ".id" => "[id]",
        ".created" => "[created]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[created]",
      "model": "google/gemini-1.5-pro",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "Test response to: How are you?"
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

#[tokio::test]
async fn handles_escape_sequences_in_streaming() {
    let mut builder = TestServer::builder();

    // Create a response with newlines that need escape sequence handling
    let text_with_newlines =
        "Gemini response here.\n\nThis has multiple paragraphs.\n\nEscape sequences should work correctly.";

    builder
        .spawn_llm(GoogleMock::new("google").with_streaming_text_with_newlines(text_with_newlines))
        .await;

    let config = indoc! {r#"
        [llm]
        enabled = true
    "#};

    let server = builder.build(config).await;
    let llm = server.llm_client("/llm");

    let request = json!({
        "model": "google/gemini-1.5-flash",
        "messages": [{"role": "user", "content": "Test"}],
        "stream": true
    });

    let full_content = llm.stream_completions_content(request).await;

    // Verify we got the complete text including the newlines
    insta::assert_snapshot!(full_content, @r"
    Gemini response here.

    This has multiple paragraphs.

    Escape sequences should work correctly.
    ");
}

#[tokio::test]
async fn streaming_with_long_content() {
    let mut builder = TestServer::builder();
    builder
        .spawn_llm(GoogleMock::new("google").with_streaming().with_response(
            "describe",
            "He's a grizzled war veteran, a clone, and a master of infiltration.",
        ))
        .await;

    let config = indoc! {r#"
        [llm]
        enabled = true
    "#};

    let server = builder.build(config).await;
    let llm = server.llm_client("/llm");

    let request = json!({
        "model": "google/gemini-1.5-flash",
        "messages": [{"role": "user", "content": "Please describe Solid Snake"}],
        "stream": true
    });

    // Just verify we can collect the content properly
    let content = llm.stream_completions_content(request).await;
    insta::assert_snapshot!(content, @"He's a grizzled war veteran, a clone, and a master of infiltration.");
}

#[tokio::test]
async fn streaming_with_many_small_chunks() {
    let mut builder = TestServer::builder();

    // Simulate many small chunks like real Google streaming
    let chunks = vec![
        "Here's".to_string(),
        " the".to_string(),
        " FizzBuzz".to_string(),
        " problem".to_string(),
        " solved".to_string(),
        " in".to_string(),
        " Rust".to_string(),
        ":\n\n".to_string(),
        "```rust".to_string(),
        "\nfn".to_string(),
        " main()".to_string(),
        " {".to_string(),
        "\n    for".to_string(),
        " i".to_string(),
        " in".to_string(),
        " 1..=100".to_string(),
        " {".to_string(),
        "\n        if".to_string(),
        " i % 15 == 0".to_string(),
        " {".to_string(),
        "\n            println!(\"FizzBuzz\");".to_string(),
        "\n        }".to_string(),
        " else if".to_string(),
        " i % 3 == 0".to_string(),
        " {".to_string(),
        "\n            println!(\"Fizz\");".to_string(),
        "\n        }".to_string(),
        " else if".to_string(),
        " i % 5 == 0".to_string(),
        " {".to_string(),
        "\n            println!(\"Buzz\");".to_string(),
        "\n        }".to_string(),
        " else".to_string(),
        " {".to_string(),
        "\n            println!(\"{}\", i);".to_string(),
        "\n        }".to_string(),
        "\n    }".to_string(),
        "\n}".to_string(),
        "\n```".to_string(),
    ];

    builder
        .spawn_llm(GoogleMock::new("google").with_streaming_chunks(chunks))
        .await;

    let config = indoc! {r#"
        [llm]
        enabled = true
    "#};

    let server = builder.build(config).await;
    let llm = server.llm_client("/llm");

    let request = json!({
        "model": "google/gemini-1.5-flash",
        "messages": [{"role": "user", "content": "Test"}],
        "stream": true
    });

    let full_content = llm.stream_completions_content(request).await;

    insta::assert_snapshot!(full_content, @r#"
    Here's the FizzBuzz problem solved in Rust:

    ```rust
    fn main() {
        for i in 1..=100 {
            if i % 15 == 0 {
                println!("FizzBuzz");
            } else if i % 3 == 0 {
                println!("Fizz");
            } else if i % 5 == 0 {
                println!("Buzz");
            } else {
                println!("{}", i);
            }
        }
    }
    ```
    "#);
}

#[tokio::test]
async fn simple_completion() {
    let mut builder = TestServer::builder();
    builder.spawn_llm(GoogleMock::new("google")).await;

    let server = builder.build("").await;
    let llm = server.llm_client("/llm");

    let body = llm.simple_completion("google/gemini-pro", "Quick test").await;

    insta::assert_json_snapshot!(body, {
        ".id" => "[id]",
        ".created" => "[created]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[created]",
      "model": "google/gemini-pro",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "Test response to: Quick test"
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

#[tokio::test]
async fn with_parameters() {
    let mut builder = TestServer::builder();
    builder.spawn_llm(GoogleMock::new("google")).await;

    let server = builder.build("").await;
    let llm = server.llm_client("/llm");

    // Test with various Google-compatible parameters
    let request = json!({
        "model": "google/gemini-1.5-flash",
        "messages": [
            {
                "role": "user",
                "content": "Test with parameters"
            }
        ],
        "temperature": 1.8,
        "max_tokens": 200,
        "top_p": 0.95,
        "top_k": 40,
        "stop": ["\\n\\n", "END"]
    });

    let body = llm.completions(request).await;

    insta::assert_json_snapshot!(body, {
        ".id" => "[id]",
        ".created" => "[created]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[created]",
      "model": "google/gemini-1.5-flash",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "This is a creative response due to high temperature"
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

#[tokio::test]
async fn custom_models() {
    let mut builder = TestServer::builder();
    let models = vec!["gemini-experimental".to_string(), "gemini-custom".to_string()];

    let mock = GoogleMock::new("google").with_models(models);
    builder.spawn_llm(mock).await;

    let server = builder.build("").await;
    let llm = server.llm_client("/llm");
    let body = llm.list_models().await;

    insta::assert_json_snapshot!(body, @r#"
    {
      "object": "list",
      "data": [
        {
          "id": "google/gemini-experimental",
          "object": "model",
          "created": 0,
          "owned_by": "google"
        },
        {
          "id": "google/gemini-custom",
          "object": "model",
          "created": 0,
          "owned_by": "google"
        }
      ]
    }
    "#);
}

#[tokio::test]
async fn custom_response() {
    let mut builder = TestServer::builder();

    let mock = GoogleMock::new("google").with_response("special trigger", "This is a special custom response");
    builder.spawn_llm(mock).await;

    let server = builder.build("").await;
    let llm = server.llm_client("/llm");

    let request = json!({
        "model": "google/gemini-1.5-flash",
        "messages": [
            {
                "role": "user",
                "content": "Please respond with special trigger in your reply"
            }
        ]
    });

    let body = llm.completions(request).await;

    insta::assert_json_snapshot!(body, {
        ".id" => "[id]",
        ".created" => "[created]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion",
      "created": "[created]",
      "model": "google/gemini-1.5-flash",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "This is a special custom response"
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

#[tokio::test]
async fn streaming_json_snapshots() {
    let mut builder = TestServer::builder();
    builder.spawn_llm(GoogleMock::new("google").with_streaming()).await;

    let config = indoc! {r#"
        [llm]
        enabled = true
    "#};

    let server = builder.build(config).await;
    let llm = server.llm_client("/llm");

    let request = json!({
        "model": "google/gemini-pro",
        "messages": [{"role": "user", "content": "Hello"}],
        "stream": true
    });

    let chunks = llm.stream_completions(request).await;

    // Verify chunks are in OpenAI format
    insta::assert_json_snapshot!(chunks[0], {
        ".id" => "[id]",
        ".created" => "[created]",
        ".choices[0].delta.content" => "[content]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion.chunk",
      "created": "[created]",
      "model": "google/gemini-pro",
      "choices": [
        {
          "index": 0,
          "delta": {
            "role": "assistant",
            "content": "[content]"
          }
        }
      ]
    }
    "#);
}
