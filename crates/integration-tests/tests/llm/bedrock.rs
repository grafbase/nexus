mod live;

use integration_tests::{
    TestServer,
    llms::{BedrockMock, ModelConfig},
};

#[tokio::test]
async fn bedrock_list_models() {
    let mock = BedrockMock::new("bedrock")
        .with_models(vec![
            "anthropic.claude-3-sonnet-20240229-v1:0".to_string(),
            "amazon.titan-text-express-v1".to_string(),
            "deepseek.r1-v1:0".to_string(),
        ])
        .with_model_configs(vec![
            ModelConfig::new("claude-3-sonnet").with_rename("anthropic.claude-3-sonnet-20240229-v1:0"),
            ModelConfig::new("titan-express").with_rename("amazon.titan-text-express-v1"),
            ModelConfig::new("r1").with_rename("deepseek.r1-v1:0"),
        ]);

    let mut builder = TestServer::builder();
    builder.spawn_llm(mock).await;

    let server = builder.build("").await;
    let body = server.openai_list_models().await;

    insta::assert_json_snapshot!(body, {
        ".data[].created" => "[created]"
    }, @r#"
    {
      "object": "list",
      "data": [
        {
          "id": "bedrock/claude-3-sonnet",
          "object": "model",
          "created": "[created]",
          "owned_by": "bedrock"
        },
        {
          "id": "bedrock/r1",
          "object": "model",
          "created": "[created]",
          "owned_by": "bedrock"
        },
        {
          "id": "bedrock/titan-express",
          "object": "model",
          "created": "[created]",
          "owned_by": "bedrock"
        }
      ]
    }
    "#);
}
