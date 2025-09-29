//! Middleware for recording LLM server metrics

mod stream;

use crate::{
    error::LlmError,
    messages::{
        openai::ModelsResponse,
        unified::{UnifiedRequest, UnifiedResponse},
    },
    provider::ChatCompletionStream,
    request::RequestContext,
    server::LlmService,
};
use opentelemetry::metrics::Counter;
use stream::MetricsStream;
use telemetry::metrics::{
    GEN_AI_CLIENT_INPUT_TOKEN_USAGE, GEN_AI_CLIENT_OPERATION_DURATION, GEN_AI_CLIENT_OUTPUT_TOKEN_USAGE,
    GEN_AI_CLIENT_TIME_TO_FIRST_TOKEN, GEN_AI_CLIENT_TOTAL_TOKEN_USAGE, Recorder,
};

/// Wrapper that adds metrics recording to the LLM server
#[derive(Clone)]
pub struct LlmServerWithMetrics<S> {
    inner: S,
    input_token_counter: Counter<u64>,
    output_token_counter: Counter<u64>,
    total_token_counter: Counter<u64>,
}

impl<S> LlmServerWithMetrics<S> {
    /// Create a new metrics middleware wrapping the given server
    pub fn new(inner: S) -> Self {
        let meter = telemetry::metrics::meter();

        Self {
            inner,
            input_token_counter: meter.u64_counter(GEN_AI_CLIENT_INPUT_TOKEN_USAGE).build(),
            output_token_counter: meter.u64_counter(GEN_AI_CLIENT_OUTPUT_TOKEN_USAGE).build(),
            total_token_counter: meter.u64_counter(GEN_AI_CLIENT_TOTAL_TOKEN_USAGE).build(),
        }
    }
}

impl<S> LlmService for LlmServerWithMetrics<S>
where
    S: LlmService + Clone + Send + Sync,
{
    /// List all available models from all providers.
    async fn models(&self) -> ModelsResponse {
        // No metrics for model listing
        self.inner.models().await
    }

    /// Process a chat completion request with metrics.
    async fn completions(&self, request: UnifiedRequest, context: &RequestContext) -> crate::Result<UnifiedResponse> {
        let mut recorder = create_recorder(GEN_AI_CLIENT_OPERATION_DURATION, &request.model, context);

        let result = self.inner.completions(request.clone(), context).await;

        if let Err(ref e) = result {
            let error_type_str = error_type(e);
            recorder.push_attribute("error.type", error_type_str);
        }

        // Add finish reason to operation duration if successful
        // Note: We record only the first choice's finish reason for simplicity.
        // In practice, most requests use n=1 (single choice), and when n>1,
        // finish reasons are typically consistent across choices.
        if let Ok(ref response) = result
            && let Some(choice) = response.choices.first()
            && let Some(finish_reason) = &choice.finish_reason
        {
            recorder.push_attribute("gen_ai.response.finish_reason", finish_reason.to_string());
        }

        recorder.record();

        // Record token metrics if the request was successful using actual counts from the LLM
        if let Ok(ref response) = result {
            let attributes = create_base_attributes(&request.model, context);

            // Use actual token counts from the LLM response for accuracy
            // These are the authoritative counts that should be used for billing
            let input_tokens = response.usage.prompt_tokens as u64;
            let output_tokens = response.usage.completion_tokens as u64;
            let total_tokens = input_tokens + output_tokens;

            self.input_token_counter.add(input_tokens, &attributes);
            self.output_token_counter.add(output_tokens, &attributes);
            self.total_token_counter.add(total_tokens, &attributes);
        }

        result
    }

    /// Process a streaming chat completion request with metrics.
    async fn completions_stream(
        &self,
        request: UnifiedRequest,
        context: &RequestContext,
    ) -> crate::Result<ChatCompletionStream> {
        let operation_recorder = create_recorder(GEN_AI_CLIENT_OPERATION_DURATION, &request.model, context);
        let ttft_recorder = create_recorder(GEN_AI_CLIENT_TIME_TO_FIRST_TOKEN, &request.model, context);

        let stream = self.inner.completions_stream(request.clone(), context).await?;

        let token_config = stream::TokenMetricsConfig {
            input_token_counter: self.input_token_counter.clone(),
            output_token_counter: self.output_token_counter.clone(),
            total_token_counter: self.total_token_counter.clone(),
            model: request.model.clone(),
            client_identity: context.client_identity.clone(),
        };

        let metrics_stream = MetricsStream::new(stream, operation_recorder, ttft_recorder, token_config);

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
    if let Some(ref client_identity) = context.client_identity {
        recorder.push_attribute("client.id", client_identity.client_id.clone());

        if let Some(ref group) = client_identity.group {
            recorder.push_attribute("client.group", group.clone());
        }
    }

    recorder
}

/// Create base attributes for metrics
fn create_base_attributes(model: &str, context: &RequestContext) -> Vec<opentelemetry::KeyValue> {
    use opentelemetry::{Key, KeyValue, Value};

    let mut attributes = vec![
        KeyValue::new(Key::from("gen_ai.system"), Value::from("nexus.llm")),
        KeyValue::new(Key::from("gen_ai.request.model"), Value::from(model.to_string())),
    ];

    if let Some(ref client_identity) = context.client_identity {
        attributes.push(KeyValue::new(
            Key::from("client.id"),
            Value::from(client_identity.client_id.clone()),
        ));

        if let Some(ref group) = client_identity.group {
            attributes.push(KeyValue::new(Key::from("client.group"), Value::from(group.clone())));
        }
    }

    attributes
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
