use std::{
    pin::Pin,
    task::{Context, Poll},
};

use futures::Stream;
use telemetry::metrics::Recorder;

use crate::{messages::ChatCompletionChunk, provider::ChatCompletionStream};

/// Stream wrapper that records metrics for streaming responses
pub(super) struct MetricsStream {
    inner: ChatCompletionStream,
    operation_recorder: Option<Recorder>,
    ttft_recorder: Option<Recorder>,
}

impl MetricsStream {
    pub(super) fn new(inner: ChatCompletionStream, operation_recorder: Recorder, ttft_recorder: Recorder) -> Self {
        Self {
            inner,
            operation_recorder: Some(operation_recorder),
            ttft_recorder: Some(ttft_recorder),
        }
    }
}

impl Stream for MetricsStream {
    type Item = crate::Result<ChatCompletionChunk>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let poll_result = self.inner.as_mut().poll_next(cx);

        match &poll_result {
            Poll::Ready(Some(Ok(chunk))) => {
                // Record time to first token if this is the first chunk with content
                if let Some(ttft_recorder) = self.ttft_recorder.take() {
                    if let Some(choice) = chunk.choices.first()
                        && choice.delta.content.is_some()
                    {
                        // Record the time to first token - the recorder already has the start time
                        ttft_recorder.record();
                    } else {
                        // Not a content chunk yet, keep the recorder for the next chunk
                        self.ttft_recorder = Some(ttft_recorder);
                    }
                }

                // Check if this is the final chunk (has finish_reason)
                if let Some(choice) = chunk.choices.first()
                    && choice.finish_reason.is_some()
                {
                    // Record operation duration for the complete stream
                    if let Some(recorder) = self.operation_recorder.take() {
                        recorder.record();
                    }
                }
            }
            Poll::Ready(Some(Err(e))) => {
                // Record error metrics
                if let Some(mut recorder) = self.operation_recorder.take() {
                    recorder.push_attribute("error.type", super::error_type(e));
                    recorder.record();
                }
            }
            Poll::Ready(None) => {
                // Stream ended without a final chunk with finish_reason
                // Still record the operation duration
                if let Some(recorder) = self.operation_recorder.take() {
                    recorder.record();
                }
            }
            Poll::Pending => {}
        }

        poll_result
    }
}
