//! Tests covering startup behaviour for model discovery failures

use std::any::Any;

use integration_tests::TestServer;

#[tokio::test]
async fn startup_fails_when_initial_model_discovery_fails() {
    use integration_tests::llms::OpenAIMock;

    let mut builder = TestServer::builder();
    builder
        .spawn_llm(OpenAIMock::new("openai").with_list_models_auth_error("Invalid API key"))
        .await;

    let join_handle = tokio::spawn(async move {
        builder.build("").await;
    });

    let join_error = join_handle
        .await
        .expect_err("expected server startup to panic when model discovery fails");

    assert!(join_error.is_panic(), "expected panic from failed startup");

    fn panic_message(payload: Box<dyn Any + Send>) -> String {
        match payload.downcast::<String>() {
            Ok(message) => *message,
            Err(payload) => match payload.downcast::<&str>() {
                Ok(message) => (*message).to_string(),
                Err(_) => "<non-string panic payload>".to_string(),
            },
        }
    }

    let message = panic_message(join_error.into_panic());

    insta::assert_snapshot!(message, @"Server failed to start: Failed to initialize LLM router: Failed to initialize LLM server: Internal server error");
}
