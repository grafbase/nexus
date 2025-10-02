mod stream;

use fastrace::{future::FutureExt, prelude::LocalSpan};
use fastrace_futures::StreamExt as FastraceStreamExt;
use fastrace_utils::FutureExt as _;
use telemetry::tracing;

use crate::{
    messages::{
        anthropic::CountTokensResponse,
        openai::ModelsResponse,
        unified::{UnifiedRequest, UnifiedResponse},
    },
    provider::ChatCompletionStream,
    request::RequestContext,
    server::LlmService,
};

use self::stream::TracingStream;

/// Wrapper that adds tracing to LLM service operations
#[derive(Clone)]
pub struct LlmServerWithTracing<S> {
    inner: S,
}

impl<S> LlmServerWithTracing<S> {
    pub fn new(inner: S) -> Self {
        Self { inner }
    }
}

impl<S> LlmService for LlmServerWithTracing<S>
where
    S: LlmService,
{
    async fn models(&self) -> ModelsResponse {
        let span = tracing::create_child_span_if_sampled("llm:list_models");

        let fut = async move { self.inner.models().await };

        fut.in_span(span).await
    }

    async fn completions(&self, request: UnifiedRequest, context: &RequestContext) -> crate::Result<UnifiedResponse> {
        let span = tracing::create_child_span_if_sampled("llm:chat_completion");

        // Add request attributes
        span.add_property(|| ("gen_ai.request.model", request.model.clone()));
        if let Some(max_tokens) = request.max_tokens {
            span.add_property(|| ("gen_ai.request.max_tokens", max_tokens.to_string()));
        }
        if let Some(temperature) = request.temperature {
            span.add_property(|| ("gen_ai.request.temperature", temperature.to_string()));
        }
        if let Some(tools) = &request.tools {
            span.add_property(|| ("gen_ai.request.has_tools", "true"));
            span.add_property(|| ("gen_ai.request.tool_count", tools.len().to_string()));
        }

        // Add client identification
        if let Some(ref client_identity) = context.client_identity {
            span.add_property(|| ("client.id", client_identity.client_id.clone()));

            if let Some(ref group) = client_identity.group {
                span.add_property(|| ("client.group", group.clone()));
            }
        }

        // Track auth forwarding (boolean only for privacy)
        let auth_forwarded = context.api_key.is_some();
        span.add_property(|| ("llm.auth_forwarded", auth_forwarded.to_string()));

        let fut = async move {
            let result = self.inner.completions(request, context).await;

            // Add response attributes
            match &result {
                Ok(response) => {
                    LocalSpan::add_property(|| ("gen_ai.response.model", response.model.clone()));
                    LocalSpan::add_property(|| ("gen_ai.usage.input_tokens", response.usage.prompt_tokens.to_string()));
                    LocalSpan::add_property(|| {
                        (
                            "gen_ai.usage.output_tokens",
                            response.usage.completion_tokens.to_string(),
                        )
                    });
                    LocalSpan::add_property(|| ("gen_ai.usage.total_tokens", response.usage.total_tokens.to_string()));
                    if let Some(choice) = response.choices.first()
                        && let Some(finish_reason) = &choice.finish_reason
                    {
                        LocalSpan::add_property(|| ("gen_ai.response.finish_reason", finish_reason.to_string()));
                    }
                }
                Err(e) => {
                    LocalSpan::add_property(|| ("error", "true"));
                    let error_type = e.error_type().to_string();
                    LocalSpan::add_property(|| ("error.type", error_type));
                }
            }

            result
        };

        fut.in_span(span).await
    }

    async fn completions_stream(
        &self,
        request: UnifiedRequest,
        context: &RequestContext,
    ) -> crate::Result<ChatCompletionStream> {
        // Create span for the stream with all request attributes
        let span = tracing::create_child_span_if_sampled("llm:chat_completion_stream");

        // Add request attributes
        span.add_property(|| ("gen_ai.request.model", request.model.clone()));
        if let Some(max_tokens) = request.max_tokens {
            span.add_property(|| ("gen_ai.request.max_tokens", max_tokens.to_string()));
        }
        if let Some(temperature) = request.temperature {
            span.add_property(|| ("gen_ai.request.temperature", temperature.to_string()));
        }
        if let Some(tools) = &request.tools {
            span.add_property(|| ("gen_ai.request.has_tools", "true"));
            span.add_property(|| ("gen_ai.request.tool_count", tools.len().to_string()));
        }

        // Add client identification
        if let Some(ref client_identity) = context.client_identity {
            span.add_property(|| ("client.id", client_identity.client_id.clone()));

            if let Some(ref group) = client_identity.group {
                span.add_property(|| ("client.group", group.clone()));
            }
        }

        // Track auth forwarding and stream flag
        let auth_forwarded = context.api_key.is_some();
        span.add_property(|| ("llm.auth_forwarded", auth_forwarded.to_string()));
        span.add_property(|| ("llm.stream", "true"));

        let (result, span) = self
            .inner
            .completions_stream(request, context)
            .in_span_and_out(span)
            .await;

        match result {
            Ok(stream) => {
                // Wrap the stream with tracing instrumentation
                // The TracingStream will add response attributes as chunks flow through
                let tracing_stream = TracingStream::new(stream);

                // Use fastrace_futures::StreamExt to attach the span to the stream
                // All poll operations will happen within this span context
                let instrumented_stream = tracing_stream.in_span(span);

                Ok(Box::pin(instrumented_stream) as ChatCompletionStream)
            }
            Err(e) => {
                span.add_property(|| ("error", "true"));
                span.add_property(|| ("error.type", e.error_type().to_string()));

                // Record the error span immediately
                drop(span);

                Err(e)
            }
        }
    }

    async fn count_tokens(
        &self,
        request: UnifiedRequest,
        context: &RequestContext,
    ) -> crate::Result<CountTokensResponse> {
        let span = tracing::create_child_span_if_sampled("llm:count_tokens");

        span.add_property(|| ("gen_ai.request.model", request.model.clone()));

        if let Some(ref client_identity) = context.client_identity {
            span.add_property(|| ("client.id", client_identity.client_id.clone()));

            if let Some(ref group) = client_identity.group {
                span.add_property(|| ("client.group", group.clone()));
            }
        }

        let auth_forwarded = context.api_key.is_some();
        span.add_property(|| ("llm.auth_forwarded", auth_forwarded.to_string()));

        let fut = async move {
            let result = self.inner.count_tokens(request, context).await;

            match &result {
                Ok(response) => {
                    LocalSpan::add_property(|| ("llm.count_tokens.input", response.input_tokens.to_string()));
                    LocalSpan::add_property(|| {
                        (
                            "llm.count_tokens.cache_creation",
                            response.cache_creation_input_tokens.to_string(),
                        )
                    });
                    LocalSpan::add_property(|| {
                        (
                            "llm.count_tokens.cache_read",
                            response.cache_read_input_tokens.to_string(),
                        )
                    });
                }
                Err(error) => {
                    LocalSpan::add_property(|| ("error", "true"));
                    LocalSpan::add_property(|| ("error.type", error.error_type().to_string()));
                }
            }

            result
        };

        fut.in_span(span).await
    }
}
