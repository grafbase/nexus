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
      "data": [
        {
          "created": "[created]",
          "id": "gpt-3.5-turbo",
          "object": "model",
          "owned_by": "openai"
        },
        {
          "created": "[created]",
          "id": "gpt-4",
          "object": "model",
          "owned_by": "openai"
        },
        {
          "created": "[created]",
          "id": "gpt-4-turbo",
          "object": "model",
          "owned_by": "openai"
        },
        {
          "created": "[created]",
          "id": "test_openai/gpt-3.5-turbo",
          "object": "model",
          "owned_by": "openai"
        },
        {
          "created": "[created]",
          "id": "test_openai/gpt-4",
          "object": "model",
          "owned_by": "openai"
        }
      ],
      "object": "list"
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
      "data": [
        {
          "created": "[created]",
          "id": "gpt-3.5-turbo",
          "object": "model",
          "owned_by": "openai"
        },
        {
          "created": "[created]",
          "id": "gpt-4",
          "object": "model",
          "owned_by": "openai"
        },
        {
          "created": "[created]",
          "id": "gpt-4-turbo",
          "object": "model",
          "owned_by": "openai"
        },
        {
          "created": "[created]",
          "id": "test_openai/gpt-3.5-turbo",
          "object": "model",
          "owned_by": "openai"
        },
        {
          "created": "[created]",
          "id": "test_openai/gpt-4",
          "object": "model",
          "owned_by": "openai"
        }
      ],
      "object": "list"
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
      "choices": [
        {
          "finish_reason": "stop",
          "index": 0,
          "message": {
            "content": "Hello! I'm a test LLM assistant. How can I help you today?",
            "role": "assistant"
          }
        }
      ],
      "created": 1677651200,
      "id": "chatcmpl-test-[uuid]",
      "model": "test_openai/gpt-3.5-turbo",
      "object": "chat.completion",
      "usage": {
        "completion_tokens": 15,
        "prompt_tokens": 10,
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
      "choices": [
        {
          "finish_reason": "stop",
          "index": 0,
          "message": {
            "content": "Hello! I'm a test LLM assistant. How can I help you today?",
            "role": "assistant"
          }
        }
      ],
      "created": 1677651200,
      "id": "chatcmpl-test-[uuid]",
      "model": "test_openai/gpt-3.5-turbo",
      "object": "chat.completion",
      "usage": {
        "completion_tokens": 15,
        "prompt_tokens": 10,
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
      "choices": [
        {
          "finish_reason": "stop",
          "index": 0,
          "message": {
            "content": "This is a creative response due to high temperature",
            "role": "assistant"
          }
        }
      ],
      "created": 1677651200,
      "id": "chatcmpl-test-[uuid]",
      "model": "test_openai/gpt-3.5-turbo",
      "object": "chat.completion",
      "usage": {
        "completion_tokens": 15,
        "prompt_tokens": 10,
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
      "choices": [
        {
          "delta": {},
          "finish_reason": "stop",
          "index": 0
        }
      ],
      "created": 1677651200,
      "id": "[id]",
      "model": "openai/gpt-4",
      "object": "chat.completion.chunk",
      "usage": {
        "completion_tokens": 15,
        "prompt_tokens": 10,
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
      "choices": [
        {
          "delta": {
            "content": "Why don't scientists trust atoms? ",
            "role": "assistant"
          },
          "index": 0
        }
      ],
      "created": "[created]",
      "id": "[id]",
      "model": "openai/gpt-3.5-turbo",
      "object": "chat.completion.chunk"
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
      "choices": [
        {
          "finish_reason": "stop",
          "index": 0,
          "message": {
            "content": "alpha handled",
            "role": "assistant"
          }
        }
      ],
      "created": 1677651200,
      "id": "chatcmpl-test-[uuid]",
      "model": "gpt-4-super",
      "object": "chat.completion",
      "usage": {
        "completion_tokens": 15,
        "prompt_tokens": 10,
        "total_tokens": 25
      }
    }
    "#);
}
