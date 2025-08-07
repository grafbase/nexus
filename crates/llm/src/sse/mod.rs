pub(crate) mod parser;

use axum::response::sse::{Event, KeepAlive, Sse};
use futures::{Stream, StreamExt};
use std::convert::Infallible;
use std::time::Duration;

use crate::{error::LlmError, provider::ChatCompletionStream};

/// Convert a stream of chat completion chunks to Server-Sent Events.
///
/// This function transforms a stream of completion chunks from an LLM provider
/// into an SSE stream that can be sent to clients. Each chunk is serialized
/// to JSON and sent as a data event. The stream concludes with a special
/// `[DONE]` message to signal completion.
///
/// # Arguments
///
/// * `stream` - The stream of completion chunks from a provider
/// * `model_prefix` - The provider name prefix to prepend to model names
///
/// # Returns
///
/// An SSE response that can be returned from an Axum handler
pub(crate) fn chunks_to_sse(
    stream: ChatCompletionStream,
    model_prefix: String,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // Transform each chunk into an SSE event
    let event_stream = stream.map(move |chunk_result| {
        match chunk_result {
            Ok(mut chunk) => {
                // Restore full model name with provider prefix
                chunk.model = format!("{}/{}", model_prefix, chunk.model);

                // Serialize chunk to JSON
                match serde_json::to_string(&chunk) {
                    Ok(data) => {
                        // Create SSE data event with the JSON chunk
                        Ok(Event::default().data(data))
                    }
                    Err(e) => {
                        log::error!("Failed to serialize streaming chunk: {e}");
                        // Send error as a data event
                        let error_msg = format!(r#"{{"error":"Failed to serialize response"}}"#);
                        Ok(Event::default().data(error_msg))
                    }
                }
            }
            Err(e) => {
                log::error!("Stream error: {e}");
                // Convert error to SSE event
                // We send errors as data events rather than SSE comments to ensure
                // clients receive them as part of the stream
                let error_data = format!(
                    r#"{{"error":"{}","type":"{}"}}"#,
                    e.to_string().replace('"', r#"\""#),
                    e.error_type()
                );
                Ok(Event::default().data(error_data))
            }
        }
    });

    // Add the final [DONE] event to signal stream completion
    let done_stream = futures::stream::once(async { Ok(Event::default().data("[DONE]")) });

    // Combine the chunk stream with the done marker
    let combined_stream = event_stream.chain(done_stream);

    // Configure SSE with keep-alive to prevent connection timeouts
    Sse::new(combined_stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(30))
            .text(":\n\n"), // Standard SSE comment for keep-alive
    )
}

/// Parse an SSE data line and extract the JSON content.
///
/// SSE data lines have the format "data: {json content}".
/// This function extracts and returns the JSON portion.
///
/// # Arguments
///
/// * `line` - A line from an SSE stream
///
/// # Returns
///
/// The extracted data content if the line is a data event, None otherwise
pub(crate) fn parse_sse_data(line: &str) -> Option<&str> {
    if let Some(data) = line.strip_prefix("data: ") {
        let trimmed = data.trim();
        // Skip empty data lines and the [DONE] marker
        if !trimmed.is_empty() && trimmed != "[DONE]" {
            return Some(trimmed);
        }
    }
    None
}

/// Check if an SSE line indicates the end of the stream.
///
/// # Arguments
///
/// * `line` - A line from an SSE stream
///
/// # Returns
///
/// True if this line signals the end of the stream
pub(crate) fn is_sse_done(line: &str) -> bool {
    line.trim() == "data: [DONE]"
}

/// Create an error event for SSE streams.
///
/// This creates a properly formatted error event that can be sent
/// to clients when stream processing fails.
///
/// # Arguments
///
/// * `error` - The error to convert to an SSE event
///
/// # Returns
///
/// An SSE event containing the error information
pub(crate) fn create_error_event(error: &LlmError) -> Event {
    let error_data = format!(
        r#"{{"error":"{}","type":"{}","code":{}}}"#,
        error.to_string().replace('"', r#"\""#),
        error.error_type(),
        error.status_code().as_u16()
    );
    Event::default().data(error_data)
}