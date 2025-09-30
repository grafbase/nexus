//! LLM service trait for middleware composition

use crate::{
    messages::{
        anthropic::CountTokensResponse,
        openai::ModelsResponse,
        unified::{UnifiedRequest, UnifiedResponse},
    },
    provider::ChatCompletionStream,
    request::RequestContext,
};

/// Trait for LLM service operations that can be composed with middleware
pub(crate) trait LlmService: Send + Sync {
    /// List all available models from all providers.
    fn models(&self) -> impl std::future::Future<Output = ModelsResponse> + Send;

    /// Process a unified chat completion request (protocol-agnostic).
    fn completions(
        &self,
        request: UnifiedRequest,
        context: &RequestContext,
    ) -> impl std::future::Future<Output = crate::Result<UnifiedResponse>> + Send;

    /// Process a unified streaming chat completion request (protocol-agnostic).
    fn completions_stream(
        &self,
        request: UnifiedRequest,
        context: &RequestContext,
    ) -> impl std::future::Future<Output = crate::Result<ChatCompletionStream>> + Send;

    /// Forward an Anthropic count tokens request to the appropriate provider.
    fn count_tokens(
        &self,
        request: UnifiedRequest,
        context: &RequestContext,
    ) -> impl std::future::Future<Output = crate::Result<CountTokensResponse>> + Send;
}
