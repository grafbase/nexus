use integration_tests::{
    TestServer,
    llms::{BedrockMock, ModelConfig},
};

#[tokio::test]
async fn bedrock_list_models() {
    let mut builder = TestServer::builder();
    builder
        .spawn_llm(
            BedrockMock::new("bedrock")
                .with_models(vec![
                    "anthropic.claude-3-sonnet-20240229-v1:0".to_string(),
                    "amazon.titan-text-express-v1".to_string(),
                ])
                .with_model_configs(vec![
                    ModelConfig::new("claude-3-sonnet").with_rename("anthropic.claude-3-sonnet-20240229-v1:0"),
                    ModelConfig::new("titan-express").with_rename("amazon.titan-text-express-v1"),
                ]),
        )
        .await;

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
          "id": "bedrock/claude-3-sonnet",
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

// Note: Other Bedrock integration tests are not possible due to AWS SDK limitations.
// The AWS SDK performs validation and authentication that cannot be easily mocked.
// Comprehensive test coverage is provided by:
// - Unit tests in crates/llm/src/provider/bedrock/transform.rs (all model family transforms)
// - Configuration tests in crates/config/src/llm.rs (all credential scenarios)
// - This integration test proves the basic AWS SDK integration works
