//! Tests for model listing with regex-based filtering and static models

use indoc::formatdoc;
use insta::assert_json_snapshot;
use integration_tests::{
    TestServer,
    llms::{ModelConfig, OpenAIMock, TestLlmProvider, generate_config_for_type},
};

async fn spawn_provider_config<P>(provider: P) -> String
where
    P: TestLlmProvider,
{
    let provider = Box::new(provider);
    let model_configs = provider.model_configs();
    let mut config = provider.spawn().await.expect("failed to spawn test provider");
    config.model_configs = model_configs;
    let snippet = generate_config_for_type(config.provider_type, &config);

    format!("\n{}", snippet.trim_start())
}

#[tokio::test]
async fn models_filtered_by_regex_and_static_models_included() {
    // Mock OpenAI API that returns various models
    let provider_config = spawn_provider_config(
        OpenAIMock::new("openai")
            .with_models(vec![
                "gpt-4".to_string(),
                "gpt-4-turbo".to_string(),
                "gpt-3.5-turbo".to_string(),
                "dall-e-3".to_string(),
                "text-embedding-ada-002".to_string(),
                "whisper-1".to_string(),
            ])
            .with_model_filter("^gpt-")
            .with_model_configs(vec![ModelConfig::new("custom-gpt"), ModelConfig::new("legacy-model")]),
    )
    .await;

    let config = formatdoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.openai]
        enabled = true
        path = "/llm/openai"{provider_config}
    "#};

    let server = TestServer::builder().build(&config).await;

    let models = server.openai_list_models().await;

    // Should include:
    // - Filter-matched models from API: gpt-4, gpt-4-turbo, gpt-3.5-turbo
    // - Static models: custom-gpt, legacy-model
    // Should NOT include: dall-e-3, text-embedding-ada-002, whisper-1

    assert_json_snapshot!(models, {
        ".data[].created" => "[created]",
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
          "id": "openai/custom-gpt",
          "object": "model",
          "owned_by": "openai"
        },
        {
          "created": "[created]",
          "id": "openai/legacy-model",
          "object": "model",
          "owned_by": "openai"
        }
      ],
      "object": "list"
    }
    "#);
}

#[tokio::test]
async fn multiple_providers_with_filters_and_static_models() {
    use integration_tests::llms::AnthropicMock;

    let openai_config = spawn_provider_config(
        OpenAIMock::new("openai")
            .with_models(vec![
                "gpt-4".to_string(),
                "gpt-3.5-turbo".to_string(),
                "dall-e-3".to_string(),
            ])
            .with_model_filter("^gpt-")
            .with_model_configs(vec![ModelConfig::new("custom-openai")]),
    )
    .await;

    let anthropic_config = spawn_provider_config(
        AnthropicMock::new("anthropic")
            .with_models(vec![
                "claude-3-opus".to_string(),
                "claude-2.1".to_string(),
                "claude-instant".to_string(),
            ])
            .with_model_filter("^claude-3-")
            .with_model_configs(vec![ModelConfig::new("custom-anthropic")]),
    )
    .await;

    let config = formatdoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.openai]
        enabled = true
        path = "/llm/openai"{openai_config}{anthropic_config}
    "#};

    let server = TestServer::builder().build(&config).await;

    let models = server.openai_list_models().await;

    // Should include:
    // - From OpenAI: gpt-4, gpt-3.5-turbo (matched by filter), custom-openai (static)
    // - From Anthropic: claude-3-opus (matched by filter), custom-anthropic (static)
    // Should NOT include: dall-e-3, claude-2.1, claude-instant

    assert_json_snapshot!(models, {
        ".data[].created" => "[created]",
    }, @r#"
    {
      "data": [
        {
          "created": "[created]",
          "id": "claude-3-opus",
          "object": "model",
          "owned_by": "anthropic"
        },
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
          "id": "anthropic/custom-anthropic",
          "object": "model",
          "owned_by": "anthropic"
        },
        {
          "created": "[created]",
          "id": "openai/custom-openai",
          "object": "model",
          "owned_by": "openai"
        }
      ],
      "object": "list"
    }
    "#);
}

#[tokio::test]
async fn no_filter_only_static_models() {
    let provider_config =
        spawn_provider_config(OpenAIMock::new("custom").with_models(vec![]).with_model_configs(vec![
            ModelConfig::new("model-a"),
            ModelConfig::new("model-b"),
            ModelConfig::new("model-c"),
        ]))
        .await;

    let config = formatdoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.openai]
        enabled = true
        path = "/llm/openai"{provider_config}
    "#};

    let server = TestServer::builder().build(&config).await;

    let models = server.openai_list_models().await;

    // Should only include static models with provider prefix
    assert_json_snapshot!(models, {
        ".data[].created" => "[created]",
    }, @r#"
    {
      "data": [
        {
          "created": "[created]",
          "id": "custom/model-a",
          "object": "model",
          "owned_by": "openai"
        },
        {
          "created": "[created]",
          "id": "custom/model-b",
          "object": "model",
          "owned_by": "openai"
        },
        {
          "created": "[created]",
          "id": "custom/model-c",
          "object": "model",
          "owned_by": "openai"
        }
      ],
      "object": "list"
    }
    "#);
}

#[tokio::test]
async fn cached_models_returned_when_provider_errors() {
    use integration_tests::llms::OpenAIMock;

    let mock = OpenAIMock::new("openai")
        .with_models(vec!["gpt-4".to_string(), "gpt-4o-mini".to_string()])
        .with_model_filter("^gpt-4");

    let controller = mock.controller();

    let provider_config = spawn_provider_config(mock).await;

    let config = formatdoc! {r#"
        [llm]
        enabled = true

        [llm.protocols.openai]
        enabled = true
        path = "/llm/openai"{provider_config}
    "#};

    let server = TestServer::builder().build(&config).await;

    let first = server.openai_list_models().await;

    fn model_ids(value: &serde_json::Value) -> Vec<String> {
        value["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|model| model["id"].as_str().unwrap().to_string())
            .collect()
    }

    let first_ids = serde_json::json!({
        "models": model_ids(&first),
    });

    insta::assert_json_snapshot!(first_ids, @r###"
    {
      "models": [
        "gpt-4",
        "gpt-4o-mini",
        "openai/gpt-3.5-turbo",
        "openai/gpt-4"
      ]
    }
    "###);

    controller.set_service_unavailable("maintenance");

    let second = server.openai_list_models().await;

    let second_ids = serde_json::json!({
        "models": model_ids(&second),
    });

    insta::assert_json_snapshot!(second_ids, @r###"
    {
      "models": [
        "gpt-4",
        "gpt-4o-mini",
        "openai/gpt-3.5-turbo",
        "openai/gpt-4"
      ]
    }
    "###);
}
