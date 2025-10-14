use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Instant,
};

use futures::{Stream, future::BoxFuture, stream::BoxStream};
use opentelemetry::{Key, Value};
use pin_project::pin_project;
use telemetry::{
    Histogram, KeyValue,
    attributes::{
        GEN_AI_OPERATION_NAME, GEN_AI_PROVIDER_NAME, GEN_AI_REQUEST_MODEL, GEN_AI_RESPONSE_MODEL, GEN_AI_TOKEN_TYPE,
    },
    metrics::{GEN_AI_CLIENT_OPERATION_DURATION, GEN_AI_CLIENT_TOKEN_USAGE},
};
use tower::Service;

use super::*;
use crate::telemetry::Error;

pub struct Metrics;

impl<S> tower::Layer<S> for Metrics {
    type Service = MetricsService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        MetricsService(inner)
    }
}

#[derive(Clone, Debug)]
pub struct MetricsService<S>(S);

impl<S, Req, Resp> Service<Req> for MetricsService<S>
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

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.0.poll_ready(cx)
    }

    fn call(&mut self, req: Req) -> Self::Future {
        let mut recorder = Recorder::new();

        recorder.push_attribute(GEN_AI_OPERATION_NAME, OPERATION_NAME);
        recorder.push_attribute(GEN_AI_PROVIDER_NAME, req.provider_name());
        recorder.push_attribute(GEN_AI_REQUEST_MODEL, req.model().to_owned());

        let future = self.0.call(req);

        Box::pin(async move {
            let mut result = future.await;

            match &mut result {
                Ok(response) => match response.as_message_or_stream_mut() {
                    Ok(message) => {
                        if let Some(error_type) = message.error_type() {
                            recorder.push_attribute("error.type", error_type.to_owned());
                        }
                        if let Some(model) = message.model() {
                            recorder.push_attribute(GEN_AI_RESPONSE_MODEL, model.to_owned());
                        }
                        drop(recorder);
                    }
                    Err(stream) => {
                        recorder.error_type = stream.error_type().map(ToString::to_string);
                        stream.wrap_event_stream(|inner| {
                            Box::pin(StreamWrapper {
                                inner,
                                recorder: Some(recorder),
                            })
                        });
                    }
                },
                Err(error) => {
                    recorder.push_attribute("error.type", Error::error_type(error).to_owned());
                    drop(recorder);
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
    recorder: Option<Recorder>,
}

// Similar to telemetry::Recorder but with additional state and most importantly that relies on Drop.
struct Recorder {
    start: Instant,
    duration_histogram: Histogram<f64>,
    token_usage_histogram: Histogram<u64>,
    attributes: Vec<KeyValue>,
    error_type: Option<String>,
    input_tokens: u64,
    output_tokens: u64,
    response_model: Option<String>,
}

impl Recorder {
    fn new() -> Self {
        Self {
            start: Instant::now(),
            duration_histogram: telemetry::metrics::meter()
                .f64_histogram(GEN_AI_CLIENT_OPERATION_DURATION)
                .with_unit("s")
                .build(),
            token_usage_histogram: telemetry::metrics::meter()
                .u64_histogram(GEN_AI_CLIENT_TOKEN_USAGE)
                .build(),
            attributes: Vec::new(),
            error_type: None,
            input_tokens: 0,
            output_tokens: 0,
            response_model: None,
        }
    }

    pub fn push_attribute<K, V>(&mut self, key: K, value: V)
    where
        K: Into<Key>,
        V: Into<Value>,
    {
        self.attributes.push(KeyValue::new(key, value));
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        if let Some(error_type) = self.error_type.take() {
            self.push_attribute("error.type", error_type);
        }
        if let Some(model) = self.response_model.take() {
            self.push_attribute(GEN_AI_RESPONSE_MODEL, model);
        }
        self.duration_histogram
            .record(self.start.elapsed().as_secs_f64(), &self.attributes);
        if self.input_tokens > 0 {
            self.push_attribute(GEN_AI_TOKEN_TYPE, "input");
            self.token_usage_histogram.record(self.input_tokens, &self.attributes);
            self.attributes.pop();
        }
        if self.output_tokens > 0 {
            self.push_attribute(GEN_AI_TOKEN_TYPE, "output");
            self.token_usage_histogram.record(self.output_tokens, &self.attributes);
        }
    }
}

impl<Item: Message> Stream for StreamWrapper<Item> {
    type Item = Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();

        match this.inner.poll_next(cx) {
            Poll::Ready(Some(message)) => {
                if let Some(recorder) = this.recorder.as_mut() {
                    if let Some(error_type) = message.error_type() {
                        recorder.error_type.get_or_insert_with(|| error_type.to_owned());
                    }

                    if let Some(tokens) = message.tokens() {
                        recorder.input_tokens += tokens.input as u64;
                        recorder.output_tokens += tokens.output as u64;
                    }

                    if let Some(model) = message.model() {
                        recorder.error_type.get_or_insert_with(|| model.to_owned());
                    }
                }

                Poll::Ready(Some(message))
            }
            Poll::Ready(None) => {
                drop(this.recorder.take());
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}
