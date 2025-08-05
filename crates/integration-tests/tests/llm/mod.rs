use indoc::indoc;
use integration_tests::TestServer;
use serde_json::json;

#[tokio::test]
async fn list_models() {
    let config = indoc! {r#"
        [llm.providers.test_openai]
        type = "openai"
        api_key = "test-key"
    "#};

    let server = TestServer::builder().build(config).await;
    let llm = server.llm_client("/llm");

    let body = llm.list_models().await;

    // Since created is a timestamp, let's normalize it
    let mut normalized_body = body.clone();
    if let Some(data) = normalized_body.get_mut("data").and_then(|d| d.as_array_mut()) {
        for item in data {
            if let Some(obj) = item.as_object_mut() {
                obj.insert("created".to_string(), serde_json::json!("[created]"));
            }
        }
    }

    insta::assert_json_snapshot!(normalized_body, @r#"
    {
      "object": "list",
      "data": []
    }
    "#);
}

#[tokio::test]
async fn custom_path() {
    let config = indoc! {r#"
        [llm]
        path = "/custom"

        [llm.providers.test_openai]
        type = "openai"
        api_key = "test-key"
    "#};

    let server = TestServer::builder().build(config).await;
    let llm = server.llm_client("/custom");

    let body = llm.list_models().await;

    // Since created is a timestamp, let's normalize it
    let mut normalized_body = body.clone();
    if let Some(data) = normalized_body.get_mut("data").and_then(|d| d.as_array_mut()) {
        for item in data {
            if let Some(obj) = item.as_object_mut() {
                obj.insert("created".to_string(), serde_json::json!("[created]"));
            }
        }
    }

    insta::assert_json_snapshot!(normalized_body, @r#"
    {
      "object": "list",
      "data": []
    }
    "#);
}

#[tokio::test]
async fn chat_completions() {
    let config = indoc! {r#"
        [llm.providers.test_openai]
        type = "openai"
        api_key = "test-key"
    "#};

    let server = TestServer::builder().build(config).await;
    let llm = server.llm_client("/llm");

    let request = json!({
        "model": "gpt-3.5-turbo",
        "messages": [
            {
                "role": "user",
                "content": "Hello!"
            }
        ]
    });

    let body = llm.completions(request).await;

    insta::assert_json_snapshot!(body, @r#"
    {
      "id": "chatcmpl-123",
      "object": "chat.completion",
      "created": 1677651200,
      "model": "gpt-3.5-turbo",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "user",
            "content": "Hello!"
          },
          "finish_reason": "stop"
        },
        {
          "index": 1,
          "message": {
            "role": "assistant",
            "content": "Hello, world!"
          },
          "finish_reason": "stop"
        }
      ],
      "usage": {
        "prompt_tokens": 10,
        "completion_tokens": 5,
        "total_tokens": 15
      }
    }
    "#);
}

#[tokio::test]
async fn chat_completions_simple() {
    let config = indoc! {r#"
        [llm.providers.test_openai]
        type = "openai"
        api_key = "test-key"
    "#};

    let server = TestServer::builder().build(config).await;
    let llm = server.llm_client("/llm");

    let body = llm.simple_completion("gpt-3.5-turbo", "Hello!").await;

    insta::assert_json_snapshot!(body, @r#"
    {
      "id": "chatcmpl-123",
      "object": "chat.completion",
      "created": 1677651200,
      "model": "gpt-3.5-turbo",
      "choices": [
        {
          "index": 0,
          "message": {
            "role": "user",
            "content": "Hello!"
          },
          "finish_reason": "stop"
        },
        {
          "index": 1,
          "message": {
            "role": "assistant",
            "content": "Hello, world!"
          },
          "finish_reason": "stop"
        }
      ],
      "usage": {
        "prompt_tokens": 10,
        "completion_tokens": 5,
        "total_tokens": 15
      }
    }
    "#);
}
