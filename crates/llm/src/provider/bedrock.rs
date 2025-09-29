//! AWS Bedrock provider using the unified Converse API.
//!
//! This module provides integration with AWS Bedrock foundation models through
//! the Converse API, which provides a unified interface across all model families.

mod input;
mod output;

use async_trait::async_trait;
use aws_config::Region;
use aws_credential_types::Credentials;
use aws_sdk_bedrock::Client as BedrockClient;
use aws_sdk_bedrockruntime::{
    Client as BedrockRuntimeClient, error::ProvideErrorMetadata, operation::converse_stream::ConverseStreamInput,
};
use aws_smithy_runtime_api::client::result::SdkError;
use futures::stream;
use secrecy::ExposeSecret;
use std::borrow::Cow;

use crate::{
    error::LlmError,
    messages::{
        openai::{Model, ObjectType},
        unified::{UnifiedChunk, UnifiedRequest, UnifiedResponse},
    },
    provider::{ChatCompletionStream, ModelManager, Provider, resolve_model},
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
    /// AWS Bedrock client for listing models
    bedrock_client: BedrockClient,
    /// AWS region for this provider instance
    #[allow(dead_code)] // Might be used for diagnostics or logging
    region: String,
    /// Provider instance name
    name: String,
    /// Provider configuration
    config: BedrockProviderConfig,
    /// Model manager for resolving and validating configured models
    model_manager: ModelManager,
}

impl BedrockProvider {
    /// Create a new Bedrock Converse provider instance.
    pub async fn new(name: String, config: BedrockProviderConfig) -> crate::Result<Self> {
        let sdk_config = create_aws_config(&config).await?;
        let client = BedrockRuntimeClient::new(&sdk_config);
        let bedrock_client = BedrockClient::new(&sdk_config);
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
            bedrock_client,
            region: config.region.clone(),
            name,
            config,
            model_manager,
        })
    }
}

#[async_trait]
impl Provider for BedrockProvider {
    async fn chat_completion(&self, mut request: UnifiedRequest, _: &RequestContext) -> crate::Result<UnifiedResponse> {
        log::debug!("Processing Bedrock chat completion for model: {}", request.model);

        let original_model = request.model.clone();

        // Resolve model if needed (pattern doesn't match or no pattern)
        resolve_model(&mut request, self.config.model_pattern.as_ref(), &self.model_manager)?;

        // Convert request to Bedrock format - moves request
        let converse_input = aws_sdk_bedrockruntime::operation::converse::ConverseInput::from(request);

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
        let mut response = UnifiedResponse::from(output);
        response.model = original_model;

        Ok(response)
    }

    async fn chat_completion_stream(
        &self,
        mut request: UnifiedRequest,
        _: &RequestContext,
    ) -> crate::Result<ChatCompletionStream> {
        log::debug!("Processing Bedrock streaming for model: {}", request.model);

        let original_model = request.model.clone();

        // Resolve model if needed (pattern doesn't match or no pattern)
        resolve_model(&mut request, self.config.model_pattern.as_ref(), &self.model_manager)?;

        // Convert request to Bedrock streaming format - moves request
        let converse_input = ConverseStreamInput::from(request);

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
        let stream = stream::unfold(
            (stream_output.stream, original_model),
            move |(mut event_receiver, model)| async move {
                loop {
                    match event_receiver.recv().await {
                        Ok(Some(event)) => {
                            if let Ok(mut chunk) = UnifiedChunk::try_from(event) {
                                chunk.model = Cow::Owned(model.clone());
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
        );

        Ok(Box::pin(stream))
    }

    async fn list_models(&self) -> anyhow::Result<Vec<Model>> {
        let mut models = Vec::new();

        // If model_pattern is configured, use ListInferenceProfiles API for newer models
        // that require inference profiles (like Claude 3.7)
        if let Some(ref pattern) = self.config.model_pattern {
            // Try inference profiles first (for newer models)
            match self.bedrock_client.list_inference_profiles().send().await {
                Ok(response) => {
                    let inference_profiles = response.inference_profile_summaries();

                    let filtered_models = inference_profiles
                        .iter()
                        .filter(|p| {
                            // Filter by inference profile ID
                            pattern.is_match(p.inference_profile_id())
                        })
                        .map(|p| {
                            // Extract provider name from model ARN if available
                            let owned_by = p
                                .models()
                                .first()
                                .and_then(|m| m.model_arn())
                                .and_then(|arn| {
                                    // ARN format: arn:aws:bedrock:region::foundation-model/provider.model
                                    arn.rsplit('/').next()
                                })
                                .and_then(|model_id| model_id.split('.').next())
                                .unwrap_or(&self.name)
                                .to_string();

                            Model {
                                id: p.inference_profile_id().to_string(),
                                object: ObjectType::Model,
                                created: 0,
                                owned_by,
                            }
                        });

                    models.extend(filtered_models);
                }
                Err(e) => {
                    log::debug!("Failed to fetch Bedrock inference profiles: {e}");

                    // Fall back to foundation models if inference profiles not available
                    match self.bedrock_client.list_foundation_models().send().await {
                        Ok(response) => {
                            let model_summaries = response.model_summaries();

                            let filtered_models = model_summaries
                                .iter()
                                .filter(|m| pattern.is_match(m.model_id()))
                                .map(|m| Model {
                                    id: m.model_id().to_string(),
                                    object: ObjectType::Model,
                                    created: 0,
                                    owned_by: m.provider_name().unwrap_or(&self.name).to_string(),
                                });

                            models.extend(filtered_models);
                        }
                        Err(e) => {
                            log::debug!("Failed to fetch Bedrock foundation models: {e}");
                        }
                    }
                }
            }
        }

        let configured_models = self.model_manager.get_configured_models().into_iter().map(|mut model| {
            model.id = format!("{}/{}", self.name, model.id);
            model
        });

        models.extend(configured_models);

        Ok(models)
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
