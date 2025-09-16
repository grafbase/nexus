//! LLM service trait for middleware composition

use crate::{
    messages::openai::{ChatCompletionRequest, ChatCompletionResponse, ModelsResponse},
    provider::ChatCompletionStream,
    request::RequestContext,
};

/// Trait for LLM service operations that can be composed with middleware
pub(crate) trait LlmService: Send + Sync {
    /// List all available models from all providers.
    fn models(&self) -> ModelsResponse;

    /// Process a chat completion request.
    fn completions(
        &self,
        request: ChatCompletionRequest,
        context: &RequestContext,
    ) -> impl std::future::Future<Output = crate::Result<ChatCompletionResponse>> + Send;

    /// Process a streaming chat completion request.
    fn completions_stream(
        &self,
        request: ChatCompletionRequest,
        context: &RequestContext,
    ) -> impl std::future::Future<Output = crate::Result<ChatCompletionStream>> + Send;
}
