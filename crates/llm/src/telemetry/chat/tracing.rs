use std::{
    borrow::Cow,
    pin::Pin,
    task::{Context, Poll},
};

use fastrace::Span;
use fastrace_utils::FutureExt;
use futures::{Stream, future::BoxFuture};
use pin_project::pin_project;
use tower::Service;

use telemetry::attributes::{
    GEN_AI_PROVIDER_NAME, GEN_AI_REQUEST_MAX_TOKENS, GEN_AI_REQUEST_MODEL, GEN_AI_REQUEST_TEMPERATURE,
    GEN_AI_RESPONSE_FINISH_REASONS, GEN_AI_RESPONSE_ID, GEN_AI_RESPONSE_MODEL, GEN_AI_USAGE_INPUT_TOKENS,
    GEN_AI_USAGE_OUTPUT_TOKENS,
};

use super::*;
use crate::telemetry::Error;

pub struct Tracing;

impl<S> tower::Layer<S> for Tracing {
    type Service = TracingService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        TracingService(inner)
    }
}

#[derive(Clone, Debug)]
pub struct TracingService<S>(S);

impl<S, Req, Resp> Service<Req> for TracingService<S>
where
    S: Service<Req, Response = Resp>,
    S::Future: Send + 'static,
    Req: Request,
    Resp: Response,
    S::Error: Error,
{
    type Response = Resp;

    type Error = S::Error;

    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), Self::Error>> {
        self.0.poll_ready(cx)
    }

    fn call(&mut self, req: Req) -> Self::Future {
        // https://opentelemetry.io/docs/specs/semconv/registry/attributes/gen-ai/

        let mut span = Span::enter_with_local_parent(format!("{} {}", OPERATION_NAME, req.model()));

        span = span.with_properties(|| {
            [
                (GEN_AI_REQUEST_MODEL, Cow::Owned(req.model().to_owned())),
                (GEN_AI_PROVIDER_NAME, req.provider_name()),
            ]
        });
        if let Some(max_tokens) = req.max_tokens() {
            span = span.with_property(|| (GEN_AI_REQUEST_MAX_TOKENS, max_tokens.to_string()));
        }
        if let Some(temperature) = req.temperature() {
            span = span.with_property(|| (GEN_AI_REQUEST_TEMPERATURE, temperature.to_string()));
        }
        if let Some(identity) = req.ctx().client_identity.as_ref() {
            span = span.with_property(|| ("nexus.client.id", identity.client_id.clone()));

            if let Some(group) = identity.group.clone() {
                span = span.with_property(|| ("nexus.client.group", group));
            }
        }

        let future = self.0.call(req);

        Box::pin(async move {
            let (mut result, mut span) = future.in_span_and_out(span).await;

            match &mut result {
                Ok(response) => match response.as_message_or_stream_mut() {
                    Ok(message) => {
                        if let Some(error_type) = message.error_type() {
                            span = span.with_properties(|| {
                                [
                                    ("error", Cow::Borrowed("true")),
                                    ("error.type", Cow::Owned(error_type.into())),
                                ]
                            });
                        }
                        if let Some(id) = message.id() {
                            span = span.with_property(|| (GEN_AI_RESPONSE_ID, id.to_owned()));
                        }
                        if let Some(model) = message.model() {
                            span = span.with_property(|| (GEN_AI_RESPONSE_MODEL, model.to_owned()));
                        }
                        if let Some(tokens) = message.tokens() {
                            span = span.with_property(|| (GEN_AI_USAGE_INPUT_TOKENS, tokens.input.to_string()));
                            span = span.with_property(|| (GEN_AI_USAGE_OUTPUT_TOKENS, tokens.output.to_string()));
                        }
                        if let Some(finish_reasons) = message.finish_reasons() {
                            span = span.with_property(|| (GEN_AI_RESPONSE_FINISH_REASONS, finish_reasons));
                        }
                        drop(span);
                    }
                    Err(stream) => {
                        let error_type = stream.error_type().map(ToString::to_string);
                        stream.wrap_event_stream(|inner| {
                            Box::pin(StreamWrapper {
                                inner,
                                state: Some(SpanState::new(span, error_type)),
                            })
                        });
                    }
                },
                Err(error) => {
                    span = span.with_properties(|| {
                        [
                            ("error", Cow::Borrowed("true")),
                            ("error.type", Cow::Owned(Error::error_type(error).into())),
                        ]
                    });
                    drop(span);
                }
            }

            result
        })
    }
}

#[pin_project]
struct StreamWrapper<Item> {
    #[pin]
    inner: BoxStream<'static, Item>,
    state: Option<SpanState>,
}

struct SpanState {
    span: Option<Span>,
    error_type: Option<String>,
    input_tokens: u64,
    output_tokens: u64,
    response_model: Option<String>,
    response_id: Option<String>,
    finish_reasons: Option<String>,
}

impl SpanState {
    fn new(span: Span, error_type: Option<String>) -> Self {
        Self {
            span: Some(span),
            error_type,
            input_tokens: 0,
            output_tokens: 0,
            response_model: None,
            response_id: None,
            finish_reasons: None,
        }
    }
}

impl Drop for SpanState {
    fn drop(&mut self) {
        let Some(mut span) = self.span.take() else {
            return;
        };
        if let Some(error_type) = self.error_type.take() {
            span = span.with_properties(|| [("error", Cow::Borrowed("true")), ("error.type", error_type.into())]);
        }
        if self.input_tokens > 0 {
            span = span.with_property(|| (GEN_AI_USAGE_INPUT_TOKENS, self.input_tokens.to_string()));
        }
        if self.output_tokens > 0 {
            span = span.with_property(|| (GEN_AI_USAGE_OUTPUT_TOKENS, self.output_tokens.to_string()));
        }
        if let Some(model) = self.response_model.take() {
            span = span.with_property(|| (GEN_AI_RESPONSE_MODEL, model));
        }
        if let Some(id) = self.response_id.take() {
            span = span.with_property(|| (GEN_AI_RESPONSE_ID, id));
        }
        if let Some(finish_reasons) = self.finish_reasons.take() {
            span = span.with_property(|| (GEN_AI_RESPONSE_FINISH_REASONS, finish_reasons));
        }
        drop(span);
    }
}

impl<Item: Message> Stream for StreamWrapper<Item> {
    type Item = Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();

        let res = {
            let _guard = this
                .state
                .as_ref()
                .and_then(|s| s.span.as_ref())
                .map(|span| span.set_local_parent());
            this.inner.poll_next(cx)
        };

        match res {
            Poll::Ready(Some(message)) => {
                if let Some(state) = this.state.as_mut() {
                    if let Some(error_type) = message.error_type() {
                        state.error_type.get_or_insert_with(|| error_type.to_owned());
                    }

                    if let Some(tokens) = message.tokens() {
                        state.input_tokens += tokens.input as u64;
                        state.output_tokens += tokens.output as u64;
                    }

                    if let Some(model) = message.model() {
                        state.error_type.get_or_insert_with(|| model.to_owned());
                    }
                    if let Some(id) = message.id() {
                        state.response_id.get_or_insert_with(|| id.to_owned());
                    }
                    if let Some(finish_reasons) = message.finish_reasons() {
                        state.finish_reasons.get_or_insert(finish_reasons);
                    }
                }
                Poll::Ready(Some(message))
            }
            Poll::Ready(None) => {
                drop(this.state.take());
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}
