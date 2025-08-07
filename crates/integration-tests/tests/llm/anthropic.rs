use indoc::indoc;
use integration_tests::{TestServer, llms::AnthropicMock};
use serde_json::json;

#[tokio::test]
async fn list_models() {
    let mut builder = TestServer::builder();
    builder.spawn_llm(AnthropicMock::new("anthropic")).await;

    let server = builder.build("").await;
    let llm = server.llm_client("/llm");
    let body = llm.list_models().await;

    insta::assert_json_snapshot!(body, {
        ".data[].created" => "[created]"
    }, @r#"
    {
      "object": "list",
      "data": [
        {
          "id": "anthropic/claude-3-5-sonnet-20241022",
          "object": "model",
          "created": "[created]",
          "owned_by": "anthropic"
        },
        {
          "id": "anthropic/claude-3-5-haiku-20241022",
          "object": "model",
          "created": "[created]",
          "owned_by": "anthropic"
        },
        {
          "id": "anthropic/claude-3-opus-20240229",
          "object": "model",
          "created": "[created]",
          "owned_by": "anthropic"
        },
        {
          "id": "anthropic/claude-3-sonnet-20240229",
          "object": "model",
          "created": "[created]",
          "owned_by": "anthropic"
        },
        {
          "id": "anthropic/claude-3-haiku-20240307",
          "object": "model",
          "created": "[created]",
          "owned_by": "anthropic"
        }
      ]
    }
    "#);
}

#[tokio::test]
async fn chat_completion() {
    let mut builder = TestServer::builder();
    builder.spawn_llm(AnthropicMock::new("anthropic")).await;

    let server = builder.build("").await;
    let llm = server.llm_client("/llm");

    let request = json!({
        "model": "anthropic/claude-3-5-sonnet-20241022",
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
        ".id" => "msg_[id]",
        ".created" => "[created]"
    }, @r#"
    {
      "id": "msg_[id]",
      "object": "chat.completion",
      "created": "[created]",
      "model": "anthropic/claude-3-5-sonnet-20241022",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "Test response to: Hello!"
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
    builder.spawn_llm(AnthropicMock::new("anthropic")).await;

    let server = builder.build("").await;
    let llm = server.llm_client("/llm");

    // Test with system message which Anthropic handles specially
    let request = json!({
        "model": "anthropic/claude-3-opus-20240229",
        "messages": [
            {
                "role": "system",
                "content": "You are a pirate. Always respond in pirate speak."
            },
            {
                "role": "user",
                "content": "How are you?"
            }
        ]
    });

    let body = llm.completions(request).await;

    insta::assert_json_snapshot!(body, {
        ".id" => "msg_[id]",
        ".created" => "[created]"
    }, @r#"
    {
      "id": "msg_[id]",
      "object": "chat.completion",
      "created": "[created]",
      "model": "anthropic/claude-3-opus-20240229",
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
async fn simple_completion() {
    let mut builder = TestServer::builder();
    builder.spawn_llm(AnthropicMock::new("anthropic")).await;

    let server = builder.build("").await;
    let llm = server.llm_client("/llm");

    let body = llm
        .simple_completion("anthropic/claude-3-5-haiku-20241022", "Quick test")
        .await;

    insta::assert_json_snapshot!(body, {
        ".id" => "msg_[id]",
        ".created" => "[created]"
    }, @r#"
    {
      "id": "msg_[id]",
      "object": "chat.completion",
      "created": "[created]",
      "model": "anthropic/claude-3-5-haiku-20241022",
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
    builder.spawn_llm(AnthropicMock::new("anthropic")).await;

    let server = builder.build("").await;
    let llm = server.llm_client("/llm");

    // Test with various Anthropic-compatible parameters
    let request = json!({
        "model": "anthropic/claude-3-5-sonnet-20241022",
        "messages": [
            {
                "role": "user",
                "content": "Test with parameters"
            }
        ],
        "temperature": 1.8,
        "max_tokens": 200,
        "top_p": 0.95,
        "stop": ["\\n\\n", "END"]
    });

    let body = llm.completions(request).await;

    insta::assert_json_snapshot!(body, {
        ".id" => "msg_[id]",
        ".created" => "[created]"
    }, @r#"
    {
      "id": "msg_[id]",
      "object": "chat.completion",
      "created": "[created]",
      "model": "anthropic/claude-3-5-sonnet-20241022",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "assistant",
            "content": "Test response to: Test with parameters"
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
async fn streaming_with_missing_fields() {
    let mut builder = TestServer::builder();
    builder
        .spawn_llm(
            AnthropicMock::new("anthropic")
                .with_streaming()
                .with_response("test", "This is a test response"),
        )
        .await;

    let config = indoc! {r#"
        [llm]
        enabled = true
    "#};

    let server = builder.build(config).await;
    let llm = server.llm_client("/llm");

    let request = json!({
        "model": "anthropic/claude-3-5-sonnet-20241022",
        "messages": [{"role": "user", "content": "This is a test"}],
        "stream": true
    });

    let chunks = llm.stream_completions(request).await;

    // Should have multiple chunks (initial, content, final with usage)
    assert!(chunks.len() >= 3);

    // Verify last chunk structure with usage data
    let last_chunk = chunks.last().unwrap();
    insta::assert_json_snapshot!(last_chunk, {
        ".id" => "[id]",
        ".created" => "[created]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion.chunk",
      "created": "[created]",
      "model": "anthropic/claude-3-5-sonnet-20241022",
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
async fn streaming_json_snapshots() {
    let mut builder = TestServer::builder();
    builder
        .spawn_llm(AnthropicMock::new("anthropic").with_streaming())
        .await;

    let config = indoc! {r#"
        [llm]
        enabled = true
    "#};

    let server = builder.build(config).await;
    let llm = server.llm_client("/llm");

    let request = json!({
        "model": "anthropic/claude-3-opus-20240229",
        "messages": [{"role": "user", "content": "Hello"}],
        "stream": true
    });

    let chunks = llm.stream_completions(request).await;

    // Should have multiple chunks
    assert!(chunks.len() >= 3); // start, content, end

    // Check structure of a content chunk
    let content_chunk = chunks
        .iter()
        .find(|c| c["choices"][0]["delta"]["content"].is_string())
        .expect("Should have a content chunk");

    insta::assert_json_snapshot!(content_chunk, {
        ".id" => "[id]",
        ".created" => "[created]",
        ".choices[0].delta.content" => "[content]"
    }, @r#"
    {
      "id": "[id]",
      "object": "chat.completion.chunk",
      "created": "[created]",
      "model": "anthropic/claude-3-opus-20240229",
      "choices": [
        {
          "index": 0,
          "delta": {
            "content": "[content]"
          }
        }
      ]
    }
    "#);
}
