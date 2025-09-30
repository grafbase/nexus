mod tools;

use indoc::indoc;
use integration_tests::{
    TestServer,
    llms::{OpenAIMock, openai::ModelConfig},
};
use serde_json::json;

// Helper function to extract content from streaming chunks
fn extract_content_from_chunks(chunks: &[serde_json::Value]) -> String {
    let mut content = String::new();
    for chunk in chunks {
        if let Some(delta) = chunk["choices"][0]["delta"]["content"].as_str() {
            content.push_str(delta);
        }
    }
    content
}

#[tokio::test]
async fn list_models() {
    let mut builder = TestServer::builder();
    builder.spawn_llm(OpenAIMock::new("test_openai")).await;

    let server = builder.build("").await;
    let body = server.openai_list_models().await;

    insta::assert_json_snapshot!(body, {
        ".data[].created" => "[created]"
    }, @r#"
    {
      "object": "list",
      "data": [
        {
          "id": "gpt-3.5-turbo",
          "object": "model",
          "created": "[created]",
          "owned_by": "openai"
        },
        {
          "id": "gpt-4",
          "object": "model",
          "created": "[created]",
          "owned_by": "openai"
        },
        {
          "id": "gpt-4-turbo",
          "object": "model",
          "created": "[created]",
          "owned_by": "openai"
        },
        {
          "id": "test_openai/gpt-3.5-turbo",
          "object": "model",
          "created": "[created]",
          "owned_by": "openai"
        },
        {
          "id": "test_openai/gpt-4",
          "object": "model",
          "created": "[created]",
          "owned_by": "openai"
        }
      ]
    }
    "#);
}

#[tokio::test]
async fn custom_path() {
    let config = indoc! {r#"
        [llm]

        [llm.protocols.openai]
enabled = true
path = "/custom"
    "#};

    let mut builder = TestServer::builder();
    builder.spawn_llm(OpenAIMock::new("test_openai")).await;

    let server = builder.build(config).await;
    let body = server.openai_list_models().await;

    insta::assert_json_snapshot!(body, {
        ".data[].created" => "[created]"
    }, @r#"
    {
      "object": "list",
      "data": [
        {
          "id": "gpt-3.5-turbo",
          "object": "model",
          "created": "[created]",
          "owned_by": "openai"
        },
        {
          "id": "gpt-4",
          "object": "model",
          "created": "[created]",
          "owned_by": "openai"
        },
        {
          "id": "gpt-4-turbo",
          "object": "model",
          "created": "[created]",
          "owned_by": "openai"
        },
        {
          "id": "test_openai/gpt-3.5-turbo",
          "object": "model",
          "created": "[created]",
          "owned_by": "openai"
        },
        {
          "id": "test_openai/gpt-4",
          "object": "model",
          "created": "[created]",
          "owned_by": "openai"
        }
      ]
    }
    "#);
}

#[tokio::test]
async fn chat_completions() {
    let mut builder = TestServer::builder();
    builder.spawn_llm(OpenAIMock::new("test_openai")).await;

    let server = builder.build("").await;

    let request = json!({
        "model": "test_openai/gpt-3.5-turbo",
        "messages": [
            {
                "role": "user",
                "content": "Hello!"
            }
        ]
    });

    let body = server.openai_completions(request).send().await;

    insta::assert_json_snapshot!(body, {
        ".id" => "chatcmpl-test-[uuid]"
    }, @r#"
    {
      "id": "chatcmpl-test-[uuid]",
      "object": "chat.completion",
      "created": 1677651200,
      "model": "test_openai/gpt-3.5-turbo",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "Hello! I'm a test LLM assistant. How can I help you today?"
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
async fn chat_completions_simple() {
    let mut builder = TestServer::builder();
    builder.spawn_llm(OpenAIMock::new("test_openai")).await;

    let server = builder.build("").await;
    let request = json!({
        "model": "test_openai/gpt-3.5-turbo",
        "messages": [
            {
                "role": "user",
                "content": "Hello!"
            }
        ]
    });

    let body = server.openai_completions(request).send().await;

    insta::assert_json_snapshot!(body, {
        ".id" => "chatcmpl-test-[uuid]"
    }, @r#"
    {
      "id": "chatcmpl-test-[uuid]",
      "object": "chat.completion",
      "created": 1677651200,
      "model": "test_openai/gpt-3.5-turbo",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "Hello! I'm a test LLM assistant. How can I help you today?"
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
async fn chat_completions_with_parameters() {
    let mut builder = TestServer::builder();
    builder.spawn_llm(OpenAIMock::new("test_openai")).await;

    let server = builder.build("").await;

    let request = json!({
        "model": "test_openai/gpt-3.5-turbo",
        "messages": [
            {
                "role": "user",
                "content": "Test message"
            }
        ],
        "temperature": 1.8,
        "max_tokens": 100,
        "top_p": 0.9,
        "frequency_penalty": 0.5,
        "presence_penalty": 0.3,
        "stop": ["\\n", "END"]
    });

    let body = server.openai_completions(request).send().await;

    insta::assert_json_snapshot!(body, {
        ".id" => "chatcmpl-test-[uuid]"
    }, @r#"
    {
      "id": "chatcmpl-test-[uuid]",
      "object": "chat.completion",
      "created": 1677651200,
      "model": "test_openai/gpt-3.5-turbo",
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
async fn streaming_with_multiple_chunks() {
    let mut builder = TestServer::builder();
    builder
        .spawn_llm(OpenAIMock::new("openai").with_streaming().with_streaming_chunks(vec![
            "Hello", " there", "!", " How", " can", " I", " help", " you", " today", "?",
        ]))
        .await;

    let server = builder.build("").await;

    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Hi"}],
        "stream": true
    });

    let chunks = server.openai_completions_stream(request).send().await;
    let content = extract_content_from_chunks(&chunks);
    insta::assert_snapshot!(content, @"Hello there! How can I help you today?");
}

#[tokio::test]
async fn streaming_includes_usage_in_final_chunk() {
    let mut builder = TestServer::builder();
    builder.spawn_llm(OpenAIMock::new("openai").with_streaming()).await;

    let server = builder.build("").await;

    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Hi"}],
        "stream": true
    });

    let chunks = server.openai_completions_stream(request).send().await;

    // Last chunk should have usage data
    let last_chunk = chunks.last().unwrap();
    insta::assert_json_snapshot!(last_chunk, {
        ".id" => "[id]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion.chunk",
      "created": 1677651200,
      "model": "openai/gpt-4",
      "choices": [
        {
          "index": 0,
          "delta": {},
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
        "This is a test.\n\nThis text has paragraph breaks.\n\nThe streaming should handle escape sequences correctly.";

    let mock = OpenAIMock::new("openai").with_streaming_text_with_newlines(text_with_newlines);
    builder.spawn_llm(mock).await;

    let server = builder.build("").await;

    let request = json!({
        "model": "openai/gpt-3.5-turbo",
        "messages": [{"role": "user", "content": "Test"}],
        "stream": true
    });

    let chunks = server.openai_completions_stream(request).send().await;
    let full_content = extract_content_from_chunks(&chunks);

    // Verify we got the complete text including the newlines
    insta::assert_snapshot!(full_content, @r"
    This is a test.

    This text has paragraph breaks.

    The streaming should handle escape sequences correctly.
    ");
}

#[tokio::test]
async fn handles_fragmented_chunks() {
    let mut builder = TestServer::builder();
    builder
        .spawn_llm(OpenAIMock::new("openai").with_streaming().with_streaming_chunks(vec![
            "Solid",
            " Snake",
            " is",
            " a",
            " character",
            " from",
            " the",
            " Metal",
            " Gear",
            " series",
        ]))
        .await;

    let config = indoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.openai]
enabled = true
path = "/llm"
    "#};

    let server = builder.build(config).await;

    let request = json!({
        "model": "openai/gpt-4",
        "messages": [{"role": "user", "content": "Who is Solid Snake?"}],
        "stream": true
    });

    let chunks = server.openai_completions_stream(request).send().await;
    let full_content = extract_content_from_chunks(&chunks);
    insta::assert_snapshot!(full_content, @"Solid Snake is a character from the Metal Gear series");
}

#[tokio::test]
async fn streaming_with_json_values() {
    let mut builder = TestServer::builder();
    builder.spawn_llm(OpenAIMock::new("openai").with_streaming()).await;

    let config = indoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.openai]
enabled = true
path = "/llm"
    "#};

    let server = builder.build(config).await;

    let request = json!({
        "model": "openai/gpt-3.5-turbo",
        "messages": [{"role": "user", "content": "Tell me a joke"}],
        "stream": true
    });

    let chunks = server.openai_completions_stream(request).send().await;

    // First chunk should have the expected structure
    insta::assert_json_snapshot!(chunks[0], {
        ".id" => "[id]",
        ".created" => "[created]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion.chunk",
      "created": "[created]",
      "model": "openai/gpt-3.5-turbo",
      "choices": [
        {
          "index": 0,
          "delta": {
            "role": "assistant",
            "content": "Why don't scientists trust atoms? "
          }
        }
      ]
    }
    "#);

    // Last chunk should have finish reason
    let last_chunk = chunks.last().unwrap();
    insta::assert_snapshot!(last_chunk["choices"][0]["finish_reason"].is_string(), @"true");
}

#[tokio::test]
async fn collect_streaming_content() {
    let mut builder = TestServer::builder();
    builder
        .spawn_llm(
            OpenAIMock::new("openai")
                .with_streaming()
                .with_streaming_chunks(vec!["Hello", " world", "!", " How", " are", " you", "?"]),
        )
        .await;

    let config = indoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.openai]
enabled = true
path = "/llm"
    "#};

    let server = builder.build(config).await;

    let request = json!({
        "model": "openai/gpt-3.5-turbo",
        "messages": [{"role": "user", "content": "Hi"}],
        "stream": true
    });

    let chunks = server.openai_completions_stream(request).send().await;
    let content = extract_content_from_chunks(&chunks);
    insta::assert_snapshot!(content, @"Hello world! How are you?");
}

#[tokio::test]
async fn model_filter_routing_respects_config_order() {
    let mut builder = TestServer::builder();

    builder
        .spawn_llm(
            OpenAIMock::new("alpha")
                .with_models(vec!["gpt-4-super".to_string()])
                .with_model_filter("gpt-4.*")
                .with_model_configs(vec![ModelConfig::new("gpt-4-super").with_rename("gpt-4")])
                .with_response("route probe", "alpha handled"),
        )
        .await;

    builder
        .spawn_llm(
            OpenAIMock::new("omega")
                .with_models(vec!["gpt-4-super".to_string()])
                .with_model_filter("gpt-4-super.*")
                .with_model_configs(vec![ModelConfig::new("gpt-4-super").with_rename("gpt-4")])
                .with_response("route probe", "omega handled"),
        )
        .await;

    let server = builder.build("").await;

    let request = json!({
        "model": "gpt-4-super",
        "messages": [{"role": "user", "content": "route probe"}]
    });

    let body = server.openai_completions(request).send().await;

    insta::assert_json_snapshot!(body, {
        ".id" => "chatcmpl-test-[uuid]",
    }, @r#"
    {
      "id": "chatcmpl-test-[uuid]",
      "object": "chat.completion",
      "created": 1677651200,
      "model": "gpt-4-super",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "alpha handled"
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
