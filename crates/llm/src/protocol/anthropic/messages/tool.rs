use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{messages::openai, protocol::unknown_fields::UnknownFields};

use super::cache_control::CacheControl;

/// Anthropic tool definition.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Tool {
    /// Unique tool name surfaced to the model and in tool_use blocks.
    pub name: String,

    /// Optional natural-language description of the tool's purpose.
    #[serde(default)]
    pub description: Option<String>,

    /// Tool category reported to Anthropic (defaults to custom if omitted).
    #[serde(rename = "type")]
    pub kind: Option<ToolKind>,

    /// JSON Schema describing the tool's expected input payload.
    pub input_schema: Box<openai::JsonSchema>,

    /// Cache-control hints describing how Anthropic may reuse tool inputs.
    #[serde(default)]
    pub cache_control: Option<CacheControl>,

    /// Additional tool fields forwarded to Anthropic unchanged.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Tool kinds supported by Anthropic.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    Custom,
    #[serde(untagged)]
    Unknown(String),
}

/// Controls how the model may interact with tools.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolChoice {
    Auto {
        /// When true, limit the model to at most one tool use.
        #[serde(default)]
        disable_parallel_tool_use: Option<bool>,
        /// Extra tool-choice fields preserved from the request.
        #[serde(flatten)]
        unknown_fields: UnknownFields,
    },
    Any {
        /// When true, limit the model to a single tool use.
        #[serde(default)]
        disable_parallel_tool_use: Option<bool>,
        /// Additional any-choice fields carried through untouched.
        #[serde(flatten)]
        unknown_fields: UnknownFields,
    },
    Tool {
        /// Name of the required tool.
        name: String,
        /// When true, force the model to emit exactly one tool use.
        #[serde(default)]
        disable_parallel_tool_use: Option<bool>,
        /// Additional specific-choice settings.
        #[serde(flatten)]
        unknown_fields: UnknownFields,
    },
    None {
        /// Unknown none-mode attributes left intact.
        #[serde(flatten)]
        unknown_fields: UnknownFields,
    },
    #[serde(untagged)]
    Unknown(Value),
}
