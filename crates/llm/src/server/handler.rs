//! LLM handler that conditionally applies metrics

use crate::{
    messages::{ChatCompletionRequest, ChatCompletionResponse, ModelsResponse},
    provider::ChatCompletionStream,
    request::RequestContext,
    server::{LlmServer, metrics::LlmServerWithMetrics},
};

/// LLM handler that optionally applies metrics based on configuration
#[derive(Clone)]
pub(crate) enum LlmHandler {
    /// Server with metrics recording enabled
    WithMetrics(LlmServerWithMetrics),
    /// Server without metrics (direct calls)
    WithoutMetrics(LlmServer),
}

impl LlmHandler {
    /// List all available models from all providers.
    pub fn models(&self) -> ModelsResponse {
        match self {
            LlmHandler::WithMetrics(server) => server.models(),
            LlmHandler::WithoutMetrics(server) => server.models(),
        }
    }

    /// Check token rate limits for a request.
    pub async fn check_token_rate_limit(
        &self,
        request: &ChatCompletionRequest,
        context: &RequestContext,
    ) -> Option<std::time::Duration> {
        match self {
            LlmHandler::WithMetrics(server) => server.check_token_rate_limit(request, context).await,
            LlmHandler::WithoutMetrics(server) => server.check_token_rate_limit(request, context).await,
        }
    }

    /// Process a chat completion request.
    pub async fn completions(
        &self,
        request: ChatCompletionRequest,
        context: &RequestContext,
    ) -> crate::Result<ChatCompletionResponse> {
        match self {
            LlmHandler::WithMetrics(server) => server.completions(request, context).await,
            LlmHandler::WithoutMetrics(server) => server.completions(request, context).await,
        }
    }

    /// Process a streaming chat completion request.
    pub async fn completions_stream(
        &self,
        request: ChatCompletionRequest,
        context: &RequestContext,
    ) -> crate::Result<ChatCompletionStream> {
        match self {
            LlmHandler::WithMetrics(server) => server.completions_stream(request, context).await,
            LlmHandler::WithoutMetrics(server) => server.completions_stream(request, context).await,
        }
    }
}