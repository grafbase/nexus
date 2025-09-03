use std::{
    pin::Pin,
    task::{Context, Poll},
};

use futures::Stream;
use opentelemetry::metrics::Counter;
use telemetry::metrics::Recorder;

use crate::{messages::ChatCompletionChunk, provider::ChatCompletionStream};

/// Configuration for token metrics in streaming responses
pub(super) struct TokenMetricsConfig {
    pub input_token_counter: Counter<u64>,
    pub output_token_counter: Counter<u64>,
    pub total_token_counter: Counter<u64>,
    pub model: String,
    pub client_id: Option<String>,
    pub group: Option<String>,
}

/// Stream wrapper that records metrics for streaming responses
pub(super) struct MetricsStream {
    inner: ChatCompletionStream,
    operation_recorder: Option<Recorder>,
    ttft_recorder: Option<Recorder>,
    token_config: TokenMetricsConfig,
    input_tokens_from_usage: Option<u32>,
    output_tokens_from_usage: Option<u32>,
    tokens_recorded: bool,
}

impl MetricsStream {
    pub(super) fn new(
        inner: ChatCompletionStream,
        operation_recorder: Recorder,
        ttft_recorder: Recorder,
        token_config: TokenMetricsConfig,
    ) -> Self {
        Self {
            inner,
            operation_recorder: Some(operation_recorder),
            ttft_recorder: Some(ttft_recorder),
            token_config,
            input_tokens_from_usage: None,
            output_tokens_from_usage: None,
            tokens_recorded: false,
        }
    }
}

impl MetricsStream {
    fn record_token_metrics(&mut self) {
        // Prevent double recording
        if self.tokens_recorded {
            return;
        }

        use opentelemetry::{Key, KeyValue, Value};

        let mut attributes = vec![
            KeyValue::new(Key::from("gen_ai.system"), Value::from("nexus.llm")),
            KeyValue::new(
                Key::from("gen_ai.request.model"),
                Value::from(self.token_config.model.clone()),
            ),
        ];

        if let Some(ref client_id) = self.token_config.client_id {
            attributes.push(KeyValue::new(Key::from("client.id"), Value::from(client_id.clone())));
        }

        if let Some(ref group) = self.token_config.group {
            attributes.push(KeyValue::new(Key::from("client.group"), Value::from(group.clone())));
        }

        // Use actual token counts from the LLM when available
        // These are the authoritative counts that should be used for billing
        if let (Some(input_tokens), Some(output_tokens)) = (self.input_tokens_from_usage, self.output_tokens_from_usage)
        {
            let input = input_tokens as u64;
            let output = output_tokens as u64;
            let total = input + output;

            self.token_config.input_token_counter.add(input, &attributes);
            self.token_config.output_token_counter.add(output, &attributes);
            self.token_config.total_token_counter.add(total, &attributes);

            // Mark as recorded only if we actually recorded metrics
            self.tokens_recorded = true;
        }
        // If we somehow didn't get usage data, don't record metrics
        // Better to have no data than incorrect data for billing purposes
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
                    if let Some(choice) = chunk.choices.first() {
                        let has_content = choice.delta.content.is_some();
                        let has_tool_calls = choice.delta.tool_calls.is_some();
                        
                        // Record TTFT for either text content or tool calls (first model output)
                        if has_content || has_tool_calls {
                            // Record the time to first token/decision - the recorder already has the start time
                            ttft_recorder.record();
                        } else {
                            // Not a content chunk yet, keep the recorder for the next chunk
                            self.ttft_recorder = Some(ttft_recorder);
                        }
                    } else {
                        self.ttft_recorder = Some(ttft_recorder);
                    }
                }

                // Capture actual token counts from chunks that contain usage info
                // The final chunk typically contains the complete usage data
                if let Some(usage) = &chunk.usage {
                    self.input_tokens_from_usage = Some(usage.prompt_tokens);
                    self.output_tokens_from_usage = Some(usage.completion_tokens);
                }

                // Check if this is the final chunk (has finish_reason)
                if let Some(choice) = chunk.choices.first()
                    && let Some(ref finish_reason) = choice.finish_reason
                {
                    // Record operation duration for the complete stream with finish reason
                    if let Some(mut recorder) = self.operation_recorder.take() {
                        recorder.push_attribute("gen_ai.response.finish_reason", finish_reason.to_string());
                        recorder.record();
                    }

                    // Record token metrics when stream completes successfully
                    self.record_token_metrics();
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

                // Record token metrics if we have usage data
                self.record_token_metrics();
            }
            Poll::Pending => {}
        }

        poll_result
    }
}
