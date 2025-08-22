pub(crate) mod anthropic;
pub mod bedrock;
pub(crate) mod google;
mod model_manager;
pub(crate) mod openai;
mod token;

pub(crate) use model_manager::ModelManager;

use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;

use crate::{
    messages::{ChatCompletionChunk, ChatCompletionRequest, ChatCompletionResponse, Model},
    request::RequestContext,
};
use config::{HeaderRule, ModelConfig};
use reqwest::{Client, Method, RequestBuilder};

/// Type alias for a stream of chat completion chunks.
///
/// This represents an asynchronous stream of completion chunks that are sent
/// incrementally during a streaming response. The stream is pinned and boxed
/// to allow for dynamic dispatch across different provider implementations.
pub(crate) type ChatCompletionStream = Pin<Box<dyn Stream<Item = crate::Result<ChatCompletionChunk>> + Send>>;

/// Trait for LLM provider implementations.
///
/// Note for async_trait: We need this trait to be dyn-compatible, so we can't just use the
/// Rust async trait functions without Box/Pin.
#[async_trait]
pub(crate) trait Provider: Send + Sync {
    /// Process a chat completion request.
    async fn chat_completion(
        &self,
        request: ChatCompletionRequest,
        context: &RequestContext,
    ) -> crate::Result<ChatCompletionResponse>;

    /// Process a streaming chat completion request.
    ///
    /// Returns a stream of completion chunks that are sent incrementally as the
    /// model generates the response. Each chunk contains a delta that should be
    /// concatenated to build the complete message.
    ///
    /// # Errors
    ///
    /// Returns `LlmError::StreamingNotSupported` if the provider doesn't support
    /// streaming or if streaming is disabled in configuration.
    async fn chat_completion_stream(
        &self,
        _request: ChatCompletionRequest,
        _context: &RequestContext,
    ) -> crate::Result<ChatCompletionStream> {
        // Default implementation returns an error for providers that don't support streaming
        Err(crate::error::LlmError::StreamingNotSupported)
    }

    /// Check if this provider supports streaming completions.
    ///
    /// Returns `true` if the provider has implemented streaming support,
    /// `false` otherwise. This allows the server to validate streaming
    /// requests before attempting to process them.
    fn supports_streaming(&self) -> bool {
        false
    }

    /// List available models for this provider.
    fn list_models(&self) -> Vec<Model>;

    /// Get the provider name.
    fn name(&self) -> &str;
}

/// Trait for HTTP-based LLM providers.
///
/// This trait extends Provider and adds HTTP-specific functionality for providers
/// that use HTTP APIs (OpenAI, Anthropic, Google). Providers that implement this
/// trait MUST also implement Provider.
///
/// Bedrock doesn't implement this trait since it uses AWS SDK instead of HTTP.
pub(crate) trait HttpProvider: Provider {
    /// Get the provider's header rules configuration.
    ///
    /// This must return the header rules from the provider's configuration.
    fn get_provider_headers(&self) -> &[HeaderRule];

    /// Get the HTTP client for this provider.
    fn get_http_client(&self) -> &Client;

    /// Create a POST request with header rules automatically applied.
    ///
    /// This method ensures that header rules are always applied when making requests.
    /// It combines provider-level and model-level headers according to the hierarchy.
    fn request_builder(
        &self,
        method: Method,
        url: &str,
        context: &RequestContext,
        model_config: Option<&ModelConfig>,
    ) -> RequestBuilder {
        let client = self.get_http_client();

        // Apply header rules
        let provider_headers = self.get_provider_headers();
        let model_headers = model_config.map(|c| c.headers()).unwrap_or(&[]);

        // Combine provider and model headers (model overrides provider)
        let mut all_rules = Vec::with_capacity(provider_headers.len() + model_headers.len());
        all_rules.extend_from_slice(provider_headers);
        all_rules.extend_from_slice(model_headers);

        let headers = header_rules::apply(&context.headers, &all_rules);
        client.request(method, url).headers(headers)
    }
}
