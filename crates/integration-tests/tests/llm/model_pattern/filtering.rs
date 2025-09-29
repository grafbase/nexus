//! Tests for model listing with pattern filtering and static models

use indoc::indoc;
use insta::assert_json_snapshot;
use integration_tests::{
    TestServer,
    llms::{ModelConfig, OpenAIMock},
};

#[tokio::test]
async fn models_filtered_by_pattern_and_static_models_included() {
    // Mock OpenAI API that returns various models
    let mock = OpenAIMock::new("openai")
        .with_models(vec![
            "gpt-4".to_string(),
            "gpt-4-turbo".to_string(),
            "gpt-3.5-turbo".to_string(),
            "dall-e-3".to_string(),
            "text-embedding-ada-002".to_string(),
            "whisper-1".to_string(),
        ])
        .with_model_pattern("^gpt-.*")
        .with_model_configs(vec![ModelConfig::new("custom-gpt"), ModelConfig::new("legacy-model")]);

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;
    let server = builder.build("").await;

    let models = server.openai_list_models().await;

    // Should include:
    // - Pattern-matched models from API: gpt-4, gpt-4-turbo, gpt-3.5-turbo
    // - Static models: custom-gpt, legacy-model
    // Should NOT include: dall-e-3, text-embedding-ada-002, whisper-1

    assert_json_snapshot!(models, {
        ".data[].created" => "[created]",
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
          "id": "openai/custom-gpt",
          "object": "model",
          "created": "[created]",
          "owned_by": "openai"
        },
        {
          "id": "openai/legacy-model",
          "object": "model",
          "created": "[created]",
          "owned_by": "openai"
        }
      ]
    }
    "#);
}

#[tokio::test]
async fn multiple_providers_with_patterns_and_static_models() {
    use integration_tests::llms::{AnthropicMock, OpenAIMock};

    // Set up multiple mocks
    let openai_mock = OpenAIMock::new("openai")
        .with_models(vec![
            "gpt-4".to_string(),
            "gpt-3.5-turbo".to_string(),
            "dall-e-3".to_string(),
        ])
        .with_model_pattern("^gpt-.*")
        .with_model_configs(vec![ModelConfig::new("custom-openai")]);

    let anthropic_mock = AnthropicMock::new("anthropic")
        .with_models(vec![
            "claude-3-opus".to_string(),
            "claude-2.1".to_string(),
            "claude-instant".to_string(),
        ])
        .with_model_pattern("^claude-3-.*")
        .with_model_configs(vec![ModelConfig::new("custom-anthropic")]);

    let mut builder = TestServer::builder();
    builder.spawn_llm(openai_mock).await;
    builder.spawn_llm(anthropic_mock).await;
    let server = builder.build("").await;

    let models = server.openai_list_models().await;

    // Should include:
    // - From OpenAI: gpt-4, gpt-3.5-turbo (matched by pattern), custom-openai (static)
    // - From Anthropic: claude-3-opus (matched by pattern), custom-anthropic (static)
    // Should NOT include: dall-e-3, claude-2.1, claude-instant

    assert_json_snapshot!(models, {
        ".data[].created" => "[created]",
    }, @r#"
    {
      "object": "list",
      "data": [
        {
          "id": "claude-3-opus",
          "object": "model",
          "created": "[created]",
          "owned_by": "anthropic"
        },
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
          "id": "anthropic/custom-anthropic",
          "object": "model",
          "created": "[created]",
          "owned_by": "anthropic"
        },
        {
          "id": "openai/custom-openai",
          "object": "model",
          "created": "[created]",
          "owned_by": "openai"
        }
      ]
    }
    "#);
}

#[tokio::test]
async fn no_pattern_only_static_models() {
    let config = indoc! {r#"
        [server]
        listen_address = "127.0.0.1:0"

        [llm]
        enabled = true

        [llm.protocols.openai]
        enabled = true
        path = "/llm"

        # Provider with no pattern - only static models
        [llm.providers.custom]
        type = "openai"
        api_key = "test-key"
        base_url = "http://custom.test.com"
        # No model_pattern specified

        [llm.providers.custom.models."model-a"]
        [llm.providers.custom.models."model-b"]
        [llm.providers.custom.models."model-c"]
    "#};

    let server = TestServer::builder().build(config).await;

    let models = server.openai_list_models().await;

    // Should only include static models with provider prefix
    assert_json_snapshot!(models, {
        ".data[].created" => "[created]",
    }, @r#"
    {
      "object": "list",
      "data": [
        {
          "id": "custom/model-a",
          "object": "model",
          "created": "[created]",
          "owned_by": "openai"
        },
        {
          "id": "custom/model-b",
          "object": "model",
          "created": "[created]",
          "owned_by": "openai"
        },
        {
          "id": "custom/model-c",
          "object": "model",
          "created": "[created]",
          "owned_by": "openai"
        }
      ]
    }
    "#);
}

#[tokio::test]
async fn cached_models_returned_when_provider_errors() {
    use integration_tests::llms::OpenAIMock;

    let mock = OpenAIMock::new("openai")
        .with_models(vec!["gpt-4".to_string(), "gpt-4o-mini".to_string()])
        .with_model_pattern("^gpt-4.*");

    let controller = mock.controller();

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;
    let server = builder.build("").await;

    let first = server.openai_list_models().await;

    fn model_ids(value: &serde_json::Value) -> Vec<String> {
        value["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|model| model["id"].as_str().unwrap().to_string())
            .collect()
    }

    let expected_ids = vec![
        "gpt-4".to_string(),
        "gpt-4o-mini".to_string(),
        "openai/gpt-3.5-turbo".to_string(),
        "openai/gpt-4".to_string(),
    ];

    assert_eq!(model_ids(&first), expected_ids);

    controller.set_service_unavailable("maintenance");

    let second = server.openai_list_models().await;
    assert_eq!(
        first, second,
        "expected cached model list to be reused when upstream errors"
    );
    assert_eq!(model_ids(&second), expected_ids);
}
