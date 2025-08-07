use bytes::Bytes;
use futures::{stream, Stream, StreamExt};
use std::pin::Pin;

use crate::error::LlmError;

/// A parsed SSE event from a stream.
#[derive(Debug, Clone)]
pub(crate) struct SseEvent {
    /// The event data, if present
    pub data: Option<String>,
    /// The event type/name, if specified
    pub event: Option<String>,
    /// The event ID for resuming connections
    pub id: Option<String>,
    /// Retry timeout in milliseconds
    pub retry: Option<u64>,
}

impl SseEvent {
    /// Check if this is a data event with content.
    pub fn has_data(&self) -> bool {
        self.data.as_ref().map_or(false, |d| !d.is_empty())
    }

    /// Check if this is the special [DONE] marker.
    pub fn is_done(&self) -> bool {
        self.data.as_ref().map_or(false, |d| d.trim() == "[DONE]")
    }
}

/// Parse a byte stream into SSE events.
///
/// This function processes a stream of bytes (typically from an HTTP response)
/// and parses them according to the Server-Sent Events specification.
/// It handles partial chunks, buffering, and multi-line events correctly.
///
/// # Arguments
///
/// * `stream` - A stream of byte chunks from an HTTP response
///
/// # Returns
///
/// A stream of parsed SSE events or errors
pub(crate) fn parse_sse_stream<S>(
    stream: S,
) -> Pin<Box<dyn Stream<Item = Result<SseEvent, LlmError>> + Send>>
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Send + Unpin + 'static,
{
    // Use unfold to maintain state across async operations without lifetime issues
    let parsed_stream = stream::unfold(
        (stream, String::new(), None::<SseEvent>),
        |(mut stream, mut buffer, mut current_event)| async move {
            // Get next chunk from stream
            let chunk_result = stream.next().await?;

            match chunk_result {
                Ok(bytes) => {
                    // Convert bytes to string and append to buffer
                    match std::str::from_utf8(&bytes) {
                        Ok(text) => {
                            buffer.push_str(text);
                        }
                        Err(e) => {
                            log::error!("Invalid UTF-8 in SSE stream: {e}");
                            return Some((
                                vec![Err(LlmError::ConnectionError(format!(
                                    "Invalid UTF-8 in stream: {e}"
                                )))],
                                (stream, buffer, current_event),
                            ));
                        }
                    }

                    let mut events = Vec::new();

                    // Process complete lines from the buffer
                    while let Some(newline_pos) = buffer.find('\n') {
                        let line = buffer.drain(..=newline_pos).collect::<String>();
                        let line = line.trim_end_matches(&['\r', '\n']);

                        if line.is_empty() {
                            // Empty line signals end of an event
                            if let Some(event) = current_event.take() {
                                if event.has_data() || event.event.is_some() {
                                    events.push(Ok(event));
                                }
                            }
                        } else if let Some(event) = current_event.as_mut() {
                            // Parse field into current event
                            parse_sse_field(line, event);
                        } else {
                            // Start a new event
                            let mut event = SseEvent {
                                data: None,
                                event: None,
                                id: None,
                                retry: None,
                            };
                            parse_sse_field(line, &mut event);
                            current_event = Some(event);
                        }
                    }

                    Some((events, (stream, buffer, current_event)))
                }
                Err(e) => {
                    log::error!("Stream read error: {e}");
                    Some((
                        vec![Err(LlmError::ConnectionError(format!("Stream error: {e}")))],
                        (stream, buffer, current_event),
                    ))
                }
            }
        },
    );

    // Flatten the vector of events into individual events
    let flattened = parsed_stream.flat_map(|events| stream::iter(events));

    Box::pin(flattened)
}

/// Parse a single SSE field line and update the event.
///
/// SSE fields have the format "field: value" where field can be:
/// - data: Event data (can appear multiple times)
/// - event: Event type
/// - id: Event ID for resuming
/// - retry: Reconnection timeout
/// - Lines starting with ':' are comments and ignored
fn parse_sse_field(line: &str, event: &mut SseEvent) {
    if line.starts_with(':') {
        // Comment line, ignore
        return;
    }

    if let Some(colon_pos) = line.find(':') {
        let field = &line[..colon_pos];
        let value = line[colon_pos + 1..].trim_start();

        match field {
            "data" => {
                // Data fields are concatenated with newlines
                if let Some(ref mut data) = event.data {
                    data.push('\n');
                    data.push_str(value);
                } else {
                    event.data = Some(value.to_string());
                }
            }
            "event" => {
                event.event = Some(value.to_string());
            }
            "id" => {
                event.id = Some(value.to_string());
            }
            "retry" => {
                if let Ok(retry) = value.parse::<u64>() {
                    event.retry = Some(retry);
                }
            }
            _ => {
                // Unknown field, ignore per SSE spec
                log::debug!("Ignoring unknown SSE field: {field}");
            }
        }
    } else if !line.is_empty() {
        // Line without colon - treat entire line as field name with empty value
        match line {
            "data" => {
                if event.data.is_none() {
                    event.data = Some(String::new());
                }
            }
            "event" => {
                event.event = Some(String::new());
            }
            "id" => {
                event.id = Some(String::new());
            }
            _ => {
                log::debug!("Ignoring SSE line without colon: {line}");
            }
        }
    }
}

/// Extract JSON data from a stream of SSE events.
///
/// This function filters SSE events to only data events and attempts
/// to extract the JSON content. It handles the special [DONE] marker
/// by ending the stream.
///
/// # Arguments
///
/// * `events` - A stream of SSE events
///
/// # Returns
///
/// A stream of JSON strings from data events
pub(crate) fn extract_json_from_events<S>(
    events: S,
) -> Pin<Box<dyn Stream<Item = Result<String, LlmError>> + Send>>
where
    S: Stream<Item = Result<SseEvent, LlmError>> + Send + Unpin + 'static,
{
    let json_stream = events.filter_map(|event_result| async move {
        match event_result {
            Ok(event) => {
                if event.is_done() {
                    // End the stream on [DONE]
                    None
                } else if let Some(data) = event.data {
                    if !data.is_empty() {
                        Some(Ok(data))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            Err(e) => Some(Err(e)),
        }
    });

    Box::pin(json_stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;

    #[tokio::test]
    async fn test_parse_simple_event() {
        let data = b"data: {\"test\": \"value\"}\n\n";
        let stream = stream::once(async { Ok(Bytes::from(&data[..])) });

        let mut events: Vec<_> = parse_sse_stream(Box::pin(stream)).collect().await;

        assert_eq!(events.len(), 1);
        let event = events.pop().unwrap().unwrap();
        assert_eq!(event.data, Some(r#"{"test": "value"}"#.to_string()));
    }

    #[tokio::test]
    async fn test_parse_multiline_data() {
        let data = b"data: line1\ndata: line2\n\n";
        let stream = stream::once(async { Ok(Bytes::from(&data[..])) });

        let mut events: Vec<_> = parse_sse_stream(Box::pin(stream)).collect().await;

        assert_eq!(events.len(), 1);
        let event = events.pop().unwrap().unwrap();
        assert_eq!(event.data, Some("line1\nline2".to_string()));
    }

    #[tokio::test]
    async fn test_parse_done_marker() {
        let data = b"data: [DONE]\n\n";
        let stream = stream::once(async { Ok(Bytes::from(&data[..])) });

        let mut events: Vec<_> = parse_sse_stream(Box::pin(stream)).collect().await;

        assert_eq!(events.len(), 1);
        let event = events.pop().unwrap().unwrap();
        assert!(event.is_done());
    }

    #[tokio::test]
    async fn test_parse_with_event_type() {
        let data = b"event: message\ndata: test\n\n";
        let stream = stream::once(async { Ok(Bytes::from(&data[..])) });

        let mut events: Vec<_> = parse_sse_stream(Box::pin(stream)).collect().await;

        assert_eq!(events.len(), 1);
        let event = events.pop().unwrap().unwrap();
        assert_eq!(event.event, Some("message".to_string()));
        assert_eq!(event.data, Some("test".to_string()));
    }

    #[tokio::test]
    async fn test_parse_split_chunks() {
        // Simulate data split across multiple chunks
        let chunk1 = b"data: {\"test\"";
        let chunk2 = b": \"value\"}\n\n";
        let stream = stream::iter(vec![
            Ok(Bytes::from(&chunk1[..])),
            Ok(Bytes::from(&chunk2[..])),
        ]);

        let mut events: Vec<_> = parse_sse_stream(Box::pin(stream)).collect().await;

        assert_eq!(events.len(), 1);
        let event = events.pop().unwrap().unwrap();
        assert_eq!(event.data, Some(r#"{"test": "value"}"#.to_string()));
    }

    #[tokio::test]
    async fn test_ignore_comments() {
        let data = b":comment\ndata: test\n\n";
        let stream = stream::once(async { Ok(Bytes::from(&data[..])) });

        let mut events: Vec<_> = parse_sse_stream(Box::pin(stream)).collect().await;

        assert_eq!(events.len(), 1);
        let event = events.pop().unwrap().unwrap();
        assert_eq!(event.data, Some("test".to_string()));
    }
}