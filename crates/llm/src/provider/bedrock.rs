//! AWS Bedrock provider using the unified Converse API.
//!
//! This module provides integration with AWS Bedrock foundation models through
//! the Converse API, which provides a unified interface across all model families.

mod input;
mod output;

use async_trait::async_trait;
use aws_config::Region;
use aws_credential_types::Credentials;
use aws_sdk_bedrockruntime::{
    Client as BedrockRuntimeClient, error::ProvideErrorMetadata, operation::converse_stream::ConverseStreamInput,
};
use aws_smithy_runtime_api::client::result::SdkError;
use futures::stream;
use secrecy::ExposeSecret;

use crate::{
    error::LlmError,
    messages::{ChatCompletionChunk, ChatCompletionRequest, ChatCompletionResponse, Model, ObjectType},
    provider::{ChatCompletionStream, ModelManager, Provider},
    request::RequestContext,
};

use config::BedrockProviderConfig;

/// AWS Bedrock provider using the Converse API.
///
/// This provider uses AWS's unified Converse API which handles all model families
/// (Anthropic, Amazon, Meta, Mistral, Cohere, AI21) with a single interface.
pub(crate) struct BedrockProvider {
    /// AWS Bedrock Runtime client for making API calls
    client: BedrockRuntimeClient,
    /// AWS region for this provider instance
    #[allow(dead_code)] // Might be used for diagnostics or logging
    region: String,
    /// Provider instance name
    name: String,
    /// Model manager for resolving and validating configured models
    model_manager: ModelManager,
}

impl BedrockProvider {
    /// Create a new Bedrock Converse provider instance.
    pub async fn new(name: String, config: BedrockProviderConfig) -> crate::Result<Self> {
        let sdk_config = create_aws_config(&config).await?;
        let client = BedrockRuntimeClient::new(&sdk_config);
        // Convert BedrockModelConfig to unified ModelConfig for ModelManager
        let models = config
            .models
            .clone()
            .into_iter()
            .map(|(k, v)| (k, config::ModelConfig::Bedrock(v)))
            .collect();
        let model_manager = ModelManager::new(models, &name);

        Ok(Self {
            client,
            region: config.region.clone(),
            name,
            model_manager,
        })
    }
}

#[async_trait]
impl Provider for BedrockProvider {
    async fn chat_completion(
        &self,
        request: ChatCompletionRequest,
        _: &RequestContext,
    ) -> crate::Result<ChatCompletionResponse> {
        log::debug!("Processing Bedrock chat completion for model: {}", request.model);

        // Resolve the configured model to the actual Bedrock model ID
        let actual_model_id = self
            .model_manager
            .resolve_model(&request.model)
            .ok_or_else(|| LlmError::ModelNotFound(format!("Model '{}' is not configured", request.model)))?;

        let original_model = request.model.clone();

        // Convert request to Bedrock format - moves request
        let mut converse_input = aws_sdk_bedrockruntime::operation::converse::ConverseInput::from(request);
        converse_input.model_id = Some(actual_model_id);

        let output = self
            .client
            .converse()
            .set_model_id(converse_input.model_id)
            .set_messages(converse_input.messages)
            .set_system(converse_input.system)
            .set_inference_config(converse_input.inference_config)
            .set_tool_config(converse_input.tool_config)
            .send()
            .await
            .map_err(|e| {
                log::error!("Failed to invoke Converse API: {e:?}");
                handle_bedrock_error(e)
            })?;

        // Convert response using From trait
        let mut response = ChatCompletionResponse::from(output);
        response.model = original_model;

        Ok(response)
    }

    async fn chat_completion_stream(
        &self,
        request: ChatCompletionRequest,
        _: &RequestContext,
    ) -> crate::Result<ChatCompletionStream> {
        log::debug!("Processing Bedrock streaming for model: {}", request.model);

        // Resolve the configured model to the actual Bedrock model ID
        let actual_model_id = self
            .model_manager
            .resolve_model(&request.model)
            .ok_or_else(|| LlmError::ModelNotFound(format!("Model '{}' is not configured", request.model)))?;

        let original_model = request.model.clone();

        // Convert request to Bedrock streaming format - moves request
        let mut converse_input = ConverseStreamInput::from(request);
        converse_input.model_id = Some(actual_model_id);

        let stream_output = self
            .client
            .converse_stream()
            .set_model_id(converse_input.model_id)
            .set_messages(converse_input.messages)
            .set_system(converse_input.system)
            .set_inference_config(converse_input.inference_config)
            .set_tool_config(converse_input.tool_config)
            .send()
            .await
            .map_err(|e| {
                log::error!("Failed to invoke Converse stream API: {e:?}");
                handle_bedrock_error(e)
            })?;

        // Simple stream conversion like other providers
        let stream = Box::pin(stream::unfold(
            (stream_output.stream, original_model),
            move |(mut event_receiver, model)| async move {
                loop {
                    match event_receiver.recv().await {
                        Ok(Some(event)) => {
                            if let Ok(mut chunk) = ChatCompletionChunk::try_from(event) {
                                chunk.model = model.clone(); // Set model like other providers
                                return Some((Ok(chunk), (event_receiver, model)));
                            }
                        }
                        Ok(None) => return None, // Stream ended
                        Err(e) => {
                            log::error!("Stream error: {e:?}");
                            return Some((
                                Err(LlmError::ConnectionError(format!("Stream error: {e:?}"))),
                                (event_receiver, model),
                            ));
                        }
                    }
                }
            },
        ));

        Ok(stream)
    }

    fn list_models(&self) -> Vec<Model> {
        // Bedrock doesn't have a convenient list models API that returns available models
        // We return the configured models instead
        self.model_manager
            .get_configured_models()
            .into_iter()
            .map(|model| Model {
                id: model.id,
                object: ObjectType::Model,
                created: 0,
                owned_by: "aws-bedrock".to_string(),
            })
            .collect()
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn supports_streaming(&self) -> bool {
        true
    }
}

/// Create AWS SDK configuration from provider config.
async fn create_aws_config(config: &BedrockProviderConfig) -> crate::Result<aws_config::SdkConfig> {
    let region = Region::new(config.region.clone());

    let mut config_loader = aws_config::from_env().region(region);

    // Use explicit credentials if provided
    if let (Some(access_key), Some(secret_key)) = (&config.access_key_id, &config.secret_access_key) {
        config_loader = config_loader.credentials_provider(Credentials::new(
            access_key.expose_secret(),
            secret_key.expose_secret(),
            config.session_token.as_ref().map(|t| t.expose_secret().to_string()),
            None,
            "bedrock_provider",
        ));
    }

    // Use profile if specified
    if let Some(profile) = &config.profile {
        config_loader = config_loader.profile_name(profile);
    }

    // Load the configuration
    let mut sdk_config = config_loader.load().await;

    // Apply custom endpoint if specified (for testing)
    if let Some(base_url) = &config.base_url {
        log::debug!("Using custom Bedrock endpoint: {}", base_url);
        sdk_config = sdk_config.into_builder().endpoint_url(base_url).build();
    }

    Ok(sdk_config)
}

/// Handle Bedrock SDK errors and convert to LlmError.
fn handle_bedrock_error<E, R>(error: SdkError<E, R>) -> LlmError
where
    E: ProvideErrorMetadata + std::fmt::Debug,
    R: std::fmt::Debug,
{
    match &error {
        SdkError::ServiceError(service_error) => {
            let err = service_error.err();
            let message = err.message().unwrap_or("Unknown error").to_string();

            match err.code() {
                Some("AccessDeniedException") => LlmError::AuthenticationFailed(message),
                Some("ResourceNotFoundException") => LlmError::ModelNotFound(message),
                Some("ThrottlingException") => LlmError::RateLimitExceeded { message },
                Some("ValidationException") => LlmError::InvalidRequest(message),
                Some("ModelTimeoutException") => LlmError::ProviderApiError { status: 504, message },
                Some("ServiceUnavailableException") => LlmError::ProviderApiError { status: 503, message },
                Some("InternalServerException") => LlmError::InternalError(Some(message)),
                _ => LlmError::ProviderApiError { status: 500, message },
            }
        }
        _ => LlmError::ConnectionError(format!("{:?}", error)),
    }
}
