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
pub enum Server {
    /// Server with both metrics and tracing
    WithMetricsAndTracing(LlmServerWithTracing<LlmServerWithMetrics<LlmServer>>),
    /// Server with metrics only
    WithMetrics(LlmServerWithMetrics<LlmServer>),
    /// Server with tracing only
    WithTracing(LlmServerWithTracing<LlmServer>),
    /// Server without metrics or tracing (direct calls)
    Direct(LlmServer),
}

impl Server {
    /// List all available models from all providers.
    pub(crate) async fn models(&self) -> ModelsResponse {
        match self {
            Server::WithMetricsAndTracing(server) => server.models().await,
            Server::WithMetrics(server) => server.models().await,
            Server::WithTracing(server) => server.models().await,
            Server::Direct(server) => server.models().await,
        }
    }

    /// Process a unified chat completion request (protocol-agnostic).
    pub(crate) async fn completions(
        &self,
        request: UnifiedRequest,
        context: &RequestContext,
    ) -> crate::Result<UnifiedResponse> {
        match self {
            Server::WithMetricsAndTracing(server) => server.completions(request, context).await,
            Server::WithMetrics(server) => server.completions(request, context).await,
            Server::WithTracing(server) => server.completions(request, context).await,
            Server::Direct(server) => server.completions(request, context).await,
        }
    }

    /// Process a unified streaming chat completion request (protocol-agnostic).
    pub(crate) async fn completions_stream(
        &self,
        request: UnifiedRequest,
        context: &RequestContext,
    ) -> crate::Result<ChatCompletionStream> {
        match self {
            Server::WithMetricsAndTracing(server) => server.completions_stream(request, context).await,
            Server::WithMetrics(server) => server.completions_stream(request, context).await,
            Server::WithTracing(server) => server.completions_stream(request, context).await,
            Server::Direct(server) => server.completions_stream(request, context).await,
        }
    }

    /// Forward an Anthropic count tokens request to the appropriate provider.
    pub(crate) async fn count_tokens(
        &self,
        request: UnifiedRequest,
        context: &RequestContext,
    ) -> crate::Result<CountTokensResponse> {
        match self {
            Server::WithMetricsAndTracing(server) => server.count_tokens(request, context).await,
            Server::WithMetrics(server) => server.count_tokens(request, context).await,
            Server::WithTracing(server) => server.count_tokens(request, context).await,
            Server::Direct(server) => server.count_tokens(request, context).await,
        }
    }
}
