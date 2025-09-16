use std::{
    pin::Pin,
    task::{Context, Poll},
};

use fastrace::prelude::LocalSpan;
use futures::Stream;

use crate::{messages::openai::ChatCompletionChunk, provider::ChatCompletionStream};

/// Stream wrapper that adds tracing attributes for streaming responses
pub(super) struct TracingStream {
    inner: ChatCompletionStream,
    model_recorded: bool,
    finish_reason_recorded: bool,
    usage_recorded: bool,
}

impl TracingStream {
    pub(super) fn new(inner: ChatCompletionStream) -> Self {
        Self {
            inner,
            model_recorded: false,
            finish_reason_recorded: false,
            usage_recorded: false,
        }
    }
}

impl Stream for TracingStream {
    type Item = crate::Result<ChatCompletionChunk>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let poll_result = self.inner.as_mut().poll_next(cx);

        match &poll_result {
            Poll::Ready(Some(Ok(chunk))) => {
                // Record model from chunk if not already recorded
                if !self.model_recorded {
                    LocalSpan::add_property(|| ("gen_ai.response.model", chunk.model.clone()));
                    self.model_recorded = true;
                }

                // Record finish reason if available and not already recorded
                if !self.finish_reason_recorded
                    && let Some(choice) = chunk.choices.first()
                    && let Some(ref finish_reason) = choice.finish_reason
                {
                    LocalSpan::add_property(|| ("gen_ai.response.finish_reason", finish_reason.to_string()));
                    self.finish_reason_recorded = true;
                }

                // Record usage metrics from chunks that contain usage info
                if !self.usage_recorded
                    && let Some(usage) = &chunk.usage
                {
                    LocalSpan::add_property(|| ("gen_ai.usage.input_tokens", usage.prompt_tokens.to_string()));
                    LocalSpan::add_property(|| ("gen_ai.usage.output_tokens", usage.completion_tokens.to_string()));
                    LocalSpan::add_property(|| ("gen_ai.usage.total_tokens", usage.total_tokens.to_string()));
                    self.usage_recorded = true;
                }
            }
            Poll::Ready(Some(Err(e))) => {
                // Record error if stream fails
                LocalSpan::add_property(|| ("error", "true"));
                LocalSpan::add_property(|| ("error.type", e.error_type().to_string()));
            }
            Poll::Ready(None) => {
                // Stream ended - nothing more to record
            }
            Poll::Pending => {}
        }

        poll_result
    }
}
