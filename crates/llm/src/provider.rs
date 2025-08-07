pub(crate) mod anthropic;
pub(crate) mod google;
pub(crate) mod openai;

use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;

use crate::messages::{ChatCompletionChunk, ChatCompletionRequest, ChatCompletionResponse, Model};

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
    async fn chat_completion(&self, request: ChatCompletionRequest) -> crate::Result<ChatCompletionResponse>;

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
    async fn chat_completion_stream(&self, _request: ChatCompletionRequest) -> crate::Result<ChatCompletionStream> {
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
    async fn list_models(&self) -> crate::Result<Vec<Model>>;

    /// Get the provider name.
    fn name(&self) -> &str;
}
