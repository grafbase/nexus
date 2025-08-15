//! AWS Bedrock LLM provider implementation.
//!
//! This module provides integration with AWS Bedrock foundation models through
//! the Nexus LLM interface. It supports multiple model families (Anthropic, Amazon,
//! Meta, Mistral, Cohere, AI21) with automatic request/response format transformation.

mod families;
mod streaming;
mod transform;

use async_trait::async_trait;
use aws_config::Region;
use aws_credential_types::Credentials;
use aws_sdk_bedrockruntime::{Client as BedrockRuntimeClient, error::ProvideErrorMetadata};
use secrecy::ExposeSecret;
use std::collections::HashMap;

use crate::{
    error::LlmError,
    messages::{ChatCompletionRequest, ChatCompletionResponse, Model},
    provider::{ChatCompletionStream, ModelManager, Provider},
    request::RequestContext,
};

use self::transform::{transform_request, transform_response};
use config::LlmProviderConfig;

pub use families::ModelFamily;

/// AWS Bedrock provider implementation.
///
/// This provider supports multiple model families available through AWS Bedrock:
/// - Anthropic (Claude models)
/// - Amazon (Titan models)  
/// - Meta (Llama models)
/// - Mistral (Mistral models)
/// - Cohere (Command models)
/// - AI21 (Jurassic/Jamba models)
///
/// Each model family has its own request/response format that is automatically
/// transformed to maintain compatibility with the unified Nexus interface.
pub(crate) struct BedrockProvider {
    /// AWS Bedrock Runtime client for making API calls
    client: BedrockRuntimeClient,
    /// AWS region for this provider instance
    #[allow(dead_code)]
    region: String,
    /// Provider instance name
    name: String,
    /// Model manager for resolving and validating configured models
    model_manager: ModelManager,
    /// Cache of model family mappings for quick lookup
    model_families: HashMap<String, ModelFamily>,
}

impl BedrockProvider {
    /// Create a new Bedrock provider instance.
    ///
    /// # Arguments
    /// * `name` - Unique name for this provider instance
    /// * `config` - Configuration containing AWS credentials, region, and models
    ///
    /// # Errors
    /// Returns an error if:
    /// - Required region is not specified
    /// - AWS credentials cannot be resolved
    /// - No models are configured
    /// - Model family cannot be determined from model IDs
    pub async fn new(name: String, config: LlmProviderConfig) -> crate::Result<Self> {
        // Extract Bedrock-specific configuration
        let aws_config = config
            .aws_config()
            .ok_or_else(|| LlmError::InvalidRequest("Invalid provider type for Bedrock provider".to_string()))?;

        // Create AWS SDK configuration
        let sdk_config = create_aws_config(&aws_config, config.base_url()).await?;
        let client = BedrockRuntimeClient::new(&sdk_config);

        // Initialize model manager
        let model_manager = ModelManager::new(&config, &name);

        // Pre-compute model family mappings for performance
        let mut model_families = HashMap::new();
        for model_id in config.models().keys() {
            let actual_model_id = model_manager
                .resolve_model(model_id)
                .ok_or_else(|| LlmError::InvalidModelFormat(format!("Model '{}' not found", model_id)))?;

            let family = ModelFamily::from_model_id(&actual_model_id).map_err(|e| {
                LlmError::InvalidModelFormat(format!(
                    "Unable to determine model family for '{}': {}",
                    actual_model_id, e
                ))
            })?;

            model_families.insert(model_id.clone(), family);
        }

        Ok(Self {
            client,
            region: aws_config.region.to_string(),
            name,
            model_manager,
            model_families,
        })
    }
}

#[async_trait]
impl Provider for BedrockProvider {
    async fn chat_completion(
        &self,
        request: ChatCompletionRequest,
        _context: &RequestContext,
    ) -> crate::Result<ChatCompletionResponse> {
        log::debug!("Processing Bedrock chat completion for model: {}", request.model);

        // Resolve the configured model to the actual Bedrock model ID
        let actual_model_id = self
            .model_manager
            .resolve_model(&request.model)
            .ok_or_else(|| LlmError::ModelNotFound(format!("Model '{}' is not configured", request.model)))?;

        log::debug!(
            "Resolved model '{}' to Bedrock model ID: {}",
            request.model,
            actual_model_id
        );

        // Get the model family for this model
        let family = self.model_families.get(&request.model).ok_or_else(|| {
            LlmError::InternalError(Some(format!("Model family not found for model: {}", request.model)))
        })?;

        log::debug!("Using model family: {:?} for model: {}", family, request.model);

        // Transform the request to the appropriate vendor format
        let request_body = transform_request(&request, *family, &actual_model_id).map_err(|e| {
            log::error!("Failed to transform request for model {}: {}", request.model, e);
            LlmError::InternalError(None)
        })?;

        log::debug!("Transformed request body size: {} bytes", request_body.as_ref().len());

        // Make the API call to Bedrock
        let invoke_result = self
            .client
            .invoke_model()
            .model_id(&actual_model_id)
            .body(request_body)
            .send()
            .await
            .map_err(|e| {
                log::error!("Failed to invoke Bedrock model {}: {}", actual_model_id, e);
                // Check if it's a service error with status code
                match e.meta().code() {
                    Some("ValidationException") => LlmError::InvalidRequest(format!("Invalid request: {}", e)),
                    Some("ResourceNotFoundException") => {
                        LlmError::ModelNotFound(format!("Model not found: {}", actual_model_id))
                    }
                    Some("AccessDeniedException") => LlmError::AuthenticationFailed(format!("Access denied: {}", e)),
                    Some("ThrottlingException") => LlmError::RateLimitExceeded(format!("Rate limit exceeded: {}", e)),
                    Some("ServiceQuotaExceededException") => {
                        LlmError::InsufficientQuota(format!("Service quota exceeded: {}", e))
                    }
                    Some("InternalServerException") => {
                        LlmError::InternalError(Some(format!("Bedrock internal error: {}", e)))
                    }
                    _ => LlmError::ConnectionError(format!("Bedrock API error: {}", e)),
                }
            })?;

        log::debug!(
            "Received response from Bedrock, body size: {} bytes",
            invoke_result.body.as_ref().len()
        );

        // Transform the response back to the unified format
        let response = transform_response(
            invoke_result.body.as_ref(),
            *family,
            &format!("{}/{}", self.name, request.model),
        )
        .map_err(|e| {
            log::error!("Failed to transform response for model {}: {}", request.model, e);
            LlmError::InternalError(None)
        })?;

        log::debug!("Successfully processed chat completion for model: {}", request.model);
        Ok(response)
    }

    async fn chat_completion_stream(
        &self,
        request: ChatCompletionRequest,
        _context: &RequestContext,
    ) -> crate::Result<ChatCompletionStream> {
        log::debug!(
            "Processing Bedrock streaming chat completion for model: {}",
            request.model
        );

        // Resolve the configured model to the actual Bedrock model ID
        let actual_model_id = self
            .model_manager
            .resolve_model(&request.model)
            .ok_or_else(|| LlmError::ModelNotFound(format!("Model '{}' is not configured", request.model)))?;

        log::debug!(
            "Resolved model '{}' to Bedrock model ID: {}",
            request.model,
            actual_model_id
        );

        // Get the model family for this model
        let family = self.model_families.get(&request.model).ok_or_else(|| {
            LlmError::InternalError(Some(format!("Model family not found for model: {}", request.model)))
        })?;

        // Check if the model family supports streaming
        if !family.supports_streaming() {
            return Err(LlmError::StreamingNotSupported);
        }

        log::debug!(
            "Using model family: {:?} for streaming model: {}",
            family,
            request.model
        );

        // Create the streaming request
        let stream_output = streaming::create_streaming_request(&self.client, &request, *family, &actual_model_id)
            .await
            .map_err(|e| {
                log::error!("Failed to create streaming request for model {}: {}", request.model, e);
                LlmError::InternalError(None)
            })?;

        // Convert EventStream to SSE stream
        let stream = streaming::convert_event_stream_to_sse(
            stream_output.body,
            *family,
            request.model.clone(),
            self.name.clone(),
        );

        log::debug!("Successfully created streaming response for model: {}", request.model);
        Ok(stream)
    }

    fn supports_streaming(&self) -> bool {
        // Most Bedrock models support streaming
        true
    }

    fn list_models(&self) -> Vec<Model> {
        self.model_manager.get_configured_models()
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// Create AWS configuration from Bedrock-specific config.
///
/// This handles the AWS credential chain resolution:
/// 1. Explicit credentials from config (access_key_id + secret_access_key)
/// 2. AWS profile specified in config
/// 3. Default AWS credential chain (env vars, files, IAM, etc.)
/// 4. Custom endpoint URL for testing with mock servers
async fn create_aws_config(
    config: &config::BedrockAwsConfig<'_>,
    base_url: Option<&str>,
) -> crate::Result<aws_config::SdkConfig> {
    let mut aws_config_builder = aws_config::from_env();

    // Set the region
    aws_config_builder = aws_config_builder.region(Region::new(config.region.to_owned()));

    // Set custom endpoint for testing/development if provided
    if let Some(endpoint_url) = base_url {
        log::debug!("Using custom Bedrock endpoint: {endpoint_url}");
        aws_config_builder = aws_config_builder.endpoint_url(endpoint_url);
    }

    // Handle explicit credentials if provided
    if let (Some(access_key_id), Some(secret_access_key)) = (config.access_key_id, config.secret_access_key) {
        let creds = Credentials::new(
            access_key_id.expose_secret(),
            secret_access_key.expose_secret(),
            config.session_token.map(|t| t.expose_secret().to_string()),
            None,
            "nexus-bedrock",
        );
        aws_config_builder = aws_config_builder.credentials_provider(creds);
    }
    // Handle profile if specified (only if no explicit credentials)
    else if let Some(profile) = config.profile {
        aws_config_builder = aws_config_builder.profile_name(profile);
    }

    // Load the AWS configuration
    let aws_config = aws_config_builder.load().await;

    Ok(aws_config)
}
