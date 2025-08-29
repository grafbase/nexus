//! Middleware for recording LLM server metrics

mod stream;

use crate::{
    error::LlmError,
    messages::{ChatCompletionRequest, ChatCompletionResponse, ModelsResponse},
    provider::ChatCompletionStream,
    request::RequestContext,
    server::LlmServer,
};
use stream::MetricsStream;
use telemetry::metrics::{GEN_AI_CLIENT_OPERATION_DURATION, GEN_AI_CLIENT_TIME_TO_FIRST_TOKEN, Recorder};

/// Wrapper that adds metrics recording to the LLM server
#[derive(Clone)]
pub struct LlmServerWithMetrics {
    inner: LlmServer,
}

impl LlmServerWithMetrics {
    /// Create a new metrics middleware wrapping the given server
    pub fn new(inner: LlmServer) -> Self {
        Self { inner }
    }

    /// List all available models from all providers.
    pub fn models(&self) -> ModelsResponse {
        // No metrics for model listing
        self.inner.models()
    }

    /// Check token rate limits for a request.
    pub async fn check_token_rate_limit(
        &self,
        request: &ChatCompletionRequest,
        context: &RequestContext,
    ) -> Option<std::time::Duration> {
        // No metrics for rate limit checks
        self.inner.check_token_rate_limit(request, context).await
    }

    /// Process a chat completion request with metrics.
    pub async fn completions(
        &self,
        request: ChatCompletionRequest,
        context: &RequestContext,
    ) -> crate::Result<ChatCompletionResponse> {
        let mut recorder = create_recorder(GEN_AI_CLIENT_OPERATION_DURATION, &request.model, context);
        let result = self.inner.completions(request, context).await;

        if let Err(ref e) = result {
            recorder.push_attribute("error.type", error_type(e));
        }

        recorder.record();

        result
    }

    /// Process a streaming chat completion request with metrics.
    pub async fn completions_stream(
        &self,
        request: ChatCompletionRequest,
        context: &RequestContext,
    ) -> crate::Result<ChatCompletionStream> {
        let operation_recorder = create_recorder(GEN_AI_CLIENT_OPERATION_DURATION, &request.model, context);
        let ttft_recorder = create_recorder(GEN_AI_CLIENT_TIME_TO_FIRST_TOKEN, &request.model, context);

        let stream = self.inner.completions_stream(request, context).await?;
        let metrics_stream = MetricsStream::new(stream, operation_recorder, ttft_recorder);

        Ok(Box::pin(metrics_stream))
    }
}

/// Create a recorder with common LLM attributes
fn create_recorder(metric_name: &'static str, model: &str, context: &RequestContext) -> Recorder {
    let mut recorder = Recorder::new(metric_name);

    recorder.push_attribute("gen_ai.system", "nexus.llm");
    recorder.push_attribute("gen_ai.operation.name", "chat.completions");
    recorder.push_attribute("gen_ai.request.model", model.to_string());

    // Add client identity if available
    if let Some(ref client_id) = context.client_id {
        recorder.push_attribute("client.id", client_id.clone());
    }

    if let Some(ref group) = context.group {
        recorder.push_attribute("client.group", group.clone());
    }

    recorder
}

/// Map LLM errors to standardized error types for metrics
fn error_type(error: &LlmError) -> &'static str {
    match error {
        LlmError::InvalidRequest(_) => "invalid_request",
        LlmError::AuthenticationFailed(_) => "authentication_failed",
        LlmError::InsufficientQuota(_) => "insufficient_quota",
        LlmError::ModelNotFound(_) => "model_not_found",
        LlmError::RateLimitExceeded { .. } => "rate_limit_exceeded",
        LlmError::StreamingNotSupported => "streaming_not_supported",
        LlmError::InvalidModelFormat(_) => "invalid_model_format",
        LlmError::ProviderNotFound(_) => "provider_not_found",
        LlmError::InternalError(_) => "internal_error",
        LlmError::ProviderApiError { .. } => "provider_api_error",
        LlmError::ConnectionError(_) => "connection_error",
    }
}
