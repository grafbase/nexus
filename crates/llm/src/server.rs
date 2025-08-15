use std::sync::Arc;

use config::LlmConfig;
use futures::stream::StreamExt;
use itertools::Itertools;

use crate::{
    error::LlmError,
    messages::{ChatCompletionRequest, ChatCompletionResponse, Model, ModelsResponse, ObjectType},
    provider::{
        ChatCompletionStream, Provider, anthropic::AnthropicProvider, bedrock::BedrockProvider, google::GoogleProvider,
        openai::OpenAIProvider,
    },
    request::RequestContext,
};

#[derive(Clone)]
pub(crate) struct LlmServer {
    shared: Arc<LlmServerInner>,
}

struct LlmServerInner {
    providers: Vec<Box<dyn Provider>>,
}

impl LlmServer {
    pub async fn new(config: LlmConfig) -> crate::Result<Self> {
        log::debug!("Initializing LLM server with {} providers", config.providers.len());
        let mut providers = Vec::with_capacity(config.providers.len());

        for (name, provider_config) in config.providers.into_iter() {
            log::debug!("Initializing provider: {name}");

            match provider_config.provider_type() {
                config::ProviderType::Openai => {
                    let provider = Box::new(OpenAIProvider::new(name.clone(), provider_config)?);
                    providers.push(provider as Box<dyn Provider>)
                }
                config::ProviderType::Anthropic => {
                    let provider = Box::new(AnthropicProvider::new(name.clone(), provider_config)?);
                    providers.push(provider as Box<dyn Provider>)
                }
                config::ProviderType::Google => {
                    let provider = Box::new(GoogleProvider::new(name.clone(), provider_config)?);
                    providers.push(provider as Box<dyn Provider>)
                }
                config::ProviderType::Bedrock => {
                    let provider = Box::new(BedrockProvider::new(name.clone(), provider_config).await?);
                    providers.push(provider as Box<dyn Provider>)
                }
            }
        }

        // Check if any providers were successfully initialized
        if providers.is_empty() {
            return Err(LlmError::InternalError(Some(
                "Failed to initialize any LLM providers.".to_string(),
            )));
        } else {
            log::debug!("LLM server initialized with {} active provider(s)", providers.len());
        }

        Ok(Self {
            shared: Arc::new(LlmServerInner { providers }),
        })
    }

    /// Process a chat completion request.
    pub async fn completions(
        &self,
        mut request: ChatCompletionRequest,
        context: &RequestContext,
    ) -> crate::Result<ChatCompletionResponse> {
        // Note: Streaming is handled by completions_stream(), this method is for non-streaming only

        // Extract provider name from the model string (format: "provider/model")
        let Some((provider_name, model_name)) = request.model.split_once('/') else {
            return Err(LlmError::InvalidModelFormat(request.model.clone()));
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
        request.model = model_name.to_string();

        let mut response = provider.chat_completion(request, context).await?;

        // Restore the full model name with provider prefix in the response
        response.model = original_model;

        Ok(response)
    }

    /// Process a streaming chat completion request.
    ///
    /// Returns a stream of completion chunks that are sent incrementally as the
    /// model generates the response. The stream is prefixed with the provider name
    /// to maintain consistency with the non-streaming API.
    pub async fn completions_stream(
        &self,
        mut request: ChatCompletionRequest,
        context: &RequestContext,
    ) -> crate::Result<ChatCompletionStream> {
        // Extract provider name from the model string (format: "provider/model")
        let Some((provider_name, model_name)) = request.model.split_once('/') else {
            return Err(LlmError::InvalidModelFormat(request.model.clone()));
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

        // Strip the provider prefix from the model name for the provider
        request.model = model_name.to_string();

        // Get the stream from the provider
        let stream = provider.chat_completion_stream(request, context).await?;

        // Transform the stream to restore the full model name with prefix
        let transformed_stream = stream.map(move |chunk_result| {
            chunk_result.map(|mut chunk| {
                // Restore the full model name with provider prefix
                chunk.model = original_model.clone();
                chunk
            })
        });

        Ok(Box::pin(transformed_stream))
    }

    /// List available models.
    pub fn list_models(&self) -> ModelsResponse {
        let mut data = Vec::new();

        for provider in &self.shared.providers {
            for model in provider.list_models() {
                data.push(Model {
                    id: format!("{}/{}", provider.name(), model.id),
                    object: model.object,
                    created: model.created,
                    owned_by: model.owned_by,
                })
            }
        }

        ModelsResponse {
            object: ObjectType::List,
            data,
        }
    }

    fn get_provider(&self, name: &str) -> Option<&dyn Provider> {
        self.shared.providers.iter().find(|p| p.name() == name).map(|v| &**v)
    }
}
