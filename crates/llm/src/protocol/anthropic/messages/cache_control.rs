use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::protocol::unknown_fields::UnknownFields;

/// Cache-control hints for tool execution.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CacheControl {
    Ephemeral {
        /// Optional TTL defining how long the cached segment should live.
        #[serde(default)]
        ttl: Option<CacheControlTtl>,

        /// Unspecified cache-control properties retained verbatim.
        #[serde(flatten)]
        unknown_fields: UnknownFields,
    },
    #[serde(untagged)]
    Unknown(Value),
}

/// Supported TTL values for ephemeral cache control.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub enum CacheControlTtl {
    #[serde(rename = "5m")]
    FiveMinutes,
    #[serde(rename = "1h")]
    OneHour,
    #[serde(untagged)]
    Unknown(String),
}
