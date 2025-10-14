use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::protocol::{
    anthropic::{error::Error, messages::ResponseContent},
    unknown_fields::UnknownFields,
};

use super::{CacheCreation, Container, ResponseContextManagement, Role, ServerToolUsage, StopReason, UsageServiceTier};

/// Server-sent event surface emitted by Anthropic's Messages streaming API.
///
/// Each serialized value maps to a concrete SSE `event:` name (for example
/// `message_start`, `content_block_delta`, or `ping`). Streams always begin with
/// a [`StreamEvent::MessageStart`], emit one or more content block lifecycles
/// (`content_block_start` → `content_block_delta*` → `content_block_stop`), may
/// include top-level [`StreamEvent::MessageDelta`] updates, and finish with a
/// terminal [`StreamEvent::MessageStop`].
///
/// Anthropic may append additional event types over time. Unknown payloads are
/// preserved through the [`StreamEvent::Unknown`] variant so callers can handle
/// them gracefully without losing information.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    /// `message_start` — announces the streamed message and includes metadata
    /// such as the message `id`, `model`, and an empty `content` array.
    MessageStart(Box<StreamMessageStart>),
    /// `content_block_start` — signals the start of a content block. The same
    /// `index` will be used in subsequent delta and stop events.
    ContentBlockStart { index: u32, content_block: ResponseContent },
    /// `content_block_delta` — incremental updates for the referenced block.
    /// The concrete shape of `delta` depends on the block type and may represent
    /// streaming text (`text_delta`), partial JSON for tool inputs
    /// (`input_json_delta`), or extended-thinking data (`thinking_delta` and its
    /// associated `signature_delta`).
    ContentBlockDelta { index: u32, delta: Value },
    /// `content_block_stop` — marks the end of updates for the indexed block.
    ContentBlockStop { index: u32 },
    /// `message_delta` — carries top-level message changes such as stop reasons
    /// and cumulative usage counters.
    MessageDelta(Box<MessageDelta>),
    /// `message_stop` — indicates no further events will be emitted for the
    /// stream.
    MessageStop,
    /// `ping` — optional heartbeat events that may appear at any point.
    Ping,
    /// `error` — surfacing recoverable API errors without tearing down the HTTP
    /// connection (for example, `overloaded_error`).
    Error { error: Error },
    /// Future or currently undocumented event variants forwarded to callers so
    /// they can implement graceful fallback handling.
    #[serde(untagged)]
    Unknown(Value),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageDelta {
    /// Top-level changes applied to the in-flight message (for example updated
    /// `stop_reason` or `stop_sequence`).
    pub delta: Delta,
    #[serde(default)]
    pub usage: Option<StreamUsage>,

    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Information provided alongside the initial `message_start` event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamMessageStart {
    /// Identifier of the streamed message.
    pub id: String,

    /// Role associated with the partial response.
    pub role: Role,
    /// Content blocks observed at the start of the stream.
    pub content: Vec<ResponseContent>,
    /// Model emitting the streamed response.
    pub model: String,
    /// Usage snapshot captured at stream start. Usage counters for the overall
    /// message will continue to accumulate in later [`StreamEvent::MessageDelta`]
    /// events.
    pub usage: StreamUsage,

    /// Stop reason if known at stream start.
    #[serde(default)]
    pub stop_reason: Option<StopReason>,

    /// Stop sequence if known at stream start.
    #[serde(default)]
    pub stop_sequence: Option<String>,

    /// Context management information available during streaming.
    #[serde(default)]
    pub context_management: Option<ResponseContextManagement>,

    /// Container information included in streaming responses.
    #[serde(default)]
    pub container: Option<Container>,

    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Usage metrics surfaced in streaming events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamUsage {
    /// Total input tokens observed so far. Values in `message_delta` events are
    /// cumulative.
    #[serde(default)]
    pub input_tokens: Option<u32>,
    /// Total output tokens observed so far. Values in `message_delta` events are
    /// cumulative.
    #[serde(default)]
    pub output_tokens: Option<u32>,

    /// Breakdown of cached token usage when provided.
    #[serde(default)]
    pub cache_creation: Option<CacheCreation>,

    /// Input tokens spent creating cache entries.
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u32>,

    /// Input tokens served from cache.
    #[serde(default)]
    pub cache_read_input_tokens: Option<u32>,

    /// Usage details for Anthropic managed server tools.
    #[serde(default)]
    pub server_tool_use: Option<ServerToolUsage>,

    /// Service tier for the current request.
    #[serde(default)]
    pub service_tier: Option<UsageServiceTier>,

    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Partial updates applied to the message when a `message_delta` event is
/// emitted. Additional fields may appear over time and are captured via
/// [`UnknownFields`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delta {
    /// Stop reason emitted partway through streaming.
    #[serde(default)]
    pub stop_reason: Option<StopReason>,

    /// Stop sequence emitted partway through streaming.
    #[serde(default)]
    pub stop_sequence: Option<String>,

    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}
