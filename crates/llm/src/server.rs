mod builder;
mod handler;
mod metrics;
mod service;
mod tracing;

pub(crate) use builder::LlmServerBuilder;
pub(crate) use handler::LlmHandler;
pub(crate) use service::LlmService;

use std::sync::Arc;

use config::LlmConfig;
use futures::stream::StreamExt;
use itertools::Itertools;
use rate_limit::{TokenRateLimitManager, TokenRateLimitRequest};

use crate::{
    error::LlmError,
    messages::{
        openai::{ChatCompletionRequest, Model, ModelsResponse, ObjectType},
        unified::{UnifiedRequest, UnifiedResponse},
    },
    provider::{ChatCompletionStream, Provider},
    request::RequestContext,
};

#[derive(Clone)]
pub(crate) struct LlmServer {
    shared: Arc<LlmServerInner>,
}

pub(crate) struct LlmServerInner {
    pub(crate) providers: Vec<Box<dyn Provider>>,
    pub(crate) config: LlmConfig,
    pub(crate) token_rate_limiter: Option<TokenRateLimitManager>,
}

impl LlmServer {
    /// Get a provider by name.
    fn get_provider(&self, name: &str) -> Option<&dyn Provider> {
        self.shared.providers.iter().find(|p| p.name() == name).map(|v| &**v)
    }

    /// Check rate limits and return an error if exceeded.
    async fn check_and_enforce_rate_limit(
        &self,
        request: &UnifiedRequest,
        context: &RequestContext,
    ) -> crate::Result<()> {
        if let Some(wait_duration) = self.check_token_rate_limit(request, context).await {
            // Duration::MAX is used as a sentinel value to indicate the request can never succeed
            // (requires more tokens than the rate limit allows)
            if wait_duration == std::time::Duration::MAX {
                log::debug!("Request requires more tokens than rate limit allows - cannot be fulfilled");

                return Err(LlmError::RateLimitExceeded {
                    message: "Token rate limit exceeded. Request requires more tokens than the configured limit allows and cannot be fulfilled.".to_string(),
                });
            } else {
                log::debug!("Request rate limited, need to wait {wait_duration:?}");

                return Err(LlmError::RateLimitExceeded {
                    message: "Token rate limit exceeded. Please try again later.".to_string(),
                });
            }
        }
        Ok(())
    }

    /// List all available models from all providers.
    pub fn models(&self) -> ModelsResponse {
        let models: Vec<Model> = self
            .shared
            .providers
            .iter()
            .flat_map(|provider| {
                provider.list_models().into_iter().map(|mut model| {
                    // Prefix model ID with provider name
                    model.id = format!("{}/{}", provider.name(), model.id);
                    model
                })
            })
            .collect();

        ModelsResponse {
            object: ObjectType::List,
            data: models,
        }
    }

    /// Check token rate limits for a request.
    ///
    /// Returns the duration to wait before retrying if rate limited, or None if the request can proceed.
    pub async fn check_token_rate_limit(
        &self,
        request: &UnifiedRequest,
        context: &RequestContext,
    ) -> Option<std::time::Duration> {
        // Check if client identification is available
        let Some(ref client_identity) = context.client_identity else {
            log::debug!(
                "No client_id found in request context. \
                Token rate limiting requires client identification to be enabled and a client_id to be present."
            );
            return None;
        };

        log::debug!(
            "Checking token rate limit for client_id={}, group={:?}, model={}",
            client_identity.client_id,
            client_identity.group,
            request.model
        );

        // Extract provider and model from the request
        let (provider_name, model_name) = request.model.split_once('/')?;
        log::debug!("Parsed model: provider={}, model={}", provider_name, model_name);

        // Get provider config
        let provider_config = self.shared.config.providers.get(provider_name)?;

        // Get model config if it exists
        let models = provider_config.models();
        let model_config = models.get(model_name);

        // Check rate limit if token rate limiter is configured
        let Some(ref token_rate_limiter) = self.shared.token_rate_limiter else {
            log::debug!(
                "Token rate limiter not initialized - no providers have token rate limits configured. \
                Allowing request without token rate limiting."
            );
            return None;
        };

        // Gather provider and model rate limit configurations
        let (provider_limits, model_limits) = (
            provider_config.rate_limits(),
            model_config.and_then(|m| m.rate_limits()),
        );

        // Count request tokens (input only, no output buffering)
        // Convert to OpenAI format for token counting since the counter uses OpenAI-specific logic
        let openai_request = ChatCompletionRequest::from(request.clone());
        let input_tokens = crate::token_counter::count_input_tokens(&openai_request);

        log::debug!("Token accounting: input={input_tokens} (output tokens not counted for rate limiting)",);

        // Create token rate limit request
        let token_request = TokenRateLimitRequest {
            client_id: client_identity.client_id.clone(),
            group: client_identity.group.clone(),
            provider: provider_name.to_string(),
            model: Some(model_name.to_string()),
            input_tokens,
        };

        match token_rate_limiter
            .check_request(&token_request, provider_limits, model_limits)
            .await
        {
            Ok(duration) => duration,
            Err(e) => {
                log::error!("Error checking token rate limit: {e}");
                None
            }
        }
    }

    /// Process a unified chat completion request (protocol-agnostic).
    pub async fn unified_completions(
        &self,
        request: UnifiedRequest,
        context: &RequestContext,
    ) -> crate::Result<UnifiedResponse> {
        // Check token rate limits first
        self.check_and_enforce_rate_limit(&request, context).await?;

        // Extract provider name from the model string (format: "provider/model")
        let model_string = request.model.clone();
        let Some((provider_name, model_name)) = model_string.split_once('/') else {
            return Err(LlmError::InvalidModelFormat(model_string));
        };

        let Some(provider) = self.get_provider(provider_name) else {
            log::error!(
                "Provider '{provider_name}' not found. Available providers: [{providers}]",
                providers = self.shared.providers.iter().map(|p| p.name()).join(", ")
            );

            return Err(LlmError::ProviderNotFound(provider_name.to_string()));
        };

        // Store the original model name before stripping the prefix
        let original_model = request.model.clone();

        // Create a modified request with the stripped model name
        let mut modified_request = request;
        modified_request.model = model_name.to_string();

        // Call provider with unified types directly
        let unified_response = provider.chat_completion(modified_request, context).await?;

        // Restore the full model name with provider prefix in the response
        let mut final_response = unified_response;
        final_response.model = original_model;

        Ok(final_response)
    }

    /// Process a unified streaming chat completion request (protocol-agnostic).
    pub async fn unified_completions_stream(
        &self,
        request: UnifiedRequest,
        context: &RequestContext,
    ) -> crate::Result<ChatCompletionStream> {
        // Check token rate limits first
        self.check_and_enforce_rate_limit(&request, context).await?;

        // Extract provider name from the model string (format: "provider/model")
        let model_string = request.model.clone();
        let Some((provider_name, model_name)) = model_string.split_once('/') else {
            return Err(LlmError::InvalidModelFormat(model_string));
        };

        let Some(provider) = self.get_provider(provider_name) else {
            log::error!(
                "Provider '{provider_name}' not found. Available providers: [{providers}]",
                providers = self.shared.providers.iter().map(|p| p.name()).join(", ")
            );

            return Err(LlmError::ProviderNotFound(provider_name.to_string()));
        };

        // Check if provider supports streaming
        if !provider.supports_streaming() {
            log::debug!("Provider '{provider_name}' does not support streaming");
            return Err(LlmError::StreamingNotSupported);
        }

        // Store the original model name for later
        let original_model = request.model.clone();

        // Create a modified request with the stripped model name
        let mut modified_request = request;
        modified_request.model = model_name.to_string();

        // Get the stream from the provider
        let stream = provider.chat_completion_stream(modified_request, context).await?;

        // Transform the stream to restore the full model name with prefix
        let transformed_stream = stream.map(move |chunk_result| {
            chunk_result.map(|mut chunk| {
                // Restore the full model name with provider prefix
                chunk.model = original_model.clone().into();
                chunk
            })
        });

        Ok(Box::pin(transformed_stream))
    }
}

impl LlmService for LlmServer {
    fn models(&self) -> ModelsResponse {
        self.models()
    }

    async fn completions(&self, request: UnifiedRequest, context: &RequestContext) -> crate::Result<UnifiedResponse> {
        self.unified_completions(request, context).await
    }

    async fn completions_stream(
        &self,
        request: UnifiedRequest,
        context: &RequestContext,
    ) -> crate::Result<ChatCompletionStream> {
        self.unified_completions_stream(request, context).await
    }
}
