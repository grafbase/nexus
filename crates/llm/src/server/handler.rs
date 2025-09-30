//! LLM handler that conditionally applies metrics and tracing

use crate::{
    messages::{
        anthropic::CountTokensResponse,
        openai::ModelsResponse,
        unified::{UnifiedRequest, UnifiedResponse},
    },
    provider::ChatCompletionStream,
    request::RequestContext,
    server::{LlmServer, LlmService, metrics::LlmServerWithMetrics, tracing::LlmServerWithTracing},
};

/// LLM handler that optionally applies metrics and tracing based on configuration
#[derive(Clone)]
pub(crate) enum LlmHandler {
    /// Server with both metrics and tracing
    WithMetricsAndTracing(LlmServerWithTracing<LlmServerWithMetrics<LlmServer>>),
    /// Server with metrics only
    WithMetrics(LlmServerWithMetrics<LlmServer>),
    /// Server with tracing only
    WithTracing(LlmServerWithTracing<LlmServer>),
    /// Server without metrics or tracing (direct calls)
    Direct(LlmServer),
}

impl LlmHandler {
    /// List all available models from all providers.
    pub async fn models(&self) -> ModelsResponse {
        match self {
            LlmHandler::WithMetricsAndTracing(server) => server.models().await,
            LlmHandler::WithMetrics(server) => server.models().await,
            LlmHandler::WithTracing(server) => server.models().await,
            LlmHandler::Direct(server) => server.models().await,
        }
    }

    /// Process a unified chat completion request (protocol-agnostic).
    pub async fn completions(
        &self,
        request: UnifiedRequest,
        context: &RequestContext,
    ) -> crate::Result<UnifiedResponse> {
        match self {
            LlmHandler::WithMetricsAndTracing(server) => server.completions(request, context).await,
            LlmHandler::WithMetrics(server) => server.completions(request, context).await,
            LlmHandler::WithTracing(server) => server.completions(request, context).await,
            LlmHandler::Direct(server) => server.completions(request, context).await,
        }
    }

    /// Process a unified streaming chat completion request (protocol-agnostic).
    pub async fn completions_stream(
        &self,
        request: UnifiedRequest,
        context: &RequestContext,
    ) -> crate::Result<ChatCompletionStream> {
        match self {
            LlmHandler::WithMetricsAndTracing(server) => server.completions_stream(request, context).await,
            LlmHandler::WithMetrics(server) => server.completions_stream(request, context).await,
            LlmHandler::WithTracing(server) => server.completions_stream(request, context).await,
            LlmHandler::Direct(server) => server.completions_stream(request, context).await,
        }
    }

    /// Forward an Anthropic count tokens request to the appropriate provider.
    pub async fn count_tokens(
        &self,
        request: UnifiedRequest,
        context: &RequestContext,
    ) -> crate::Result<CountTokensResponse> {
        match self {
            LlmHandler::WithMetricsAndTracing(server) => server.count_tokens(request, context).await,
            LlmHandler::WithMetrics(server) => server.count_tokens(request, context).await,
            LlmHandler::WithTracing(server) => server.count_tokens(request, context).await,
            LlmHandler::Direct(server) => server.count_tokens(request, context).await,
        }
    }
}
