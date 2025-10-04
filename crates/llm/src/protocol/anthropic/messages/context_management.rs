use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::protocol::unknown_fields::UnknownFields;

/// Context management configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ContextManagementConfig {
    /// Ordered list of context edits to apply before running the request.
    #[serde(default)]
    pub edits: Vec<ClearToolUses20250919>,

    /// Supplemental context management fields.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Clear tool uses edit (2025-09-19 release).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClearToolUses20250919 {
    /// Minimum tokens that must be removed for the edit to take effect.
    #[serde(default)]
    pub clear_at_least: Option<InputTokensClearAtLeast>,

    /// Indicates which tool inputs should be cleared.
    #[serde(default)]
    pub clear_tool_inputs: Option<ClearToolInputs>,

    /// Tool names that must be preserved when clearing history.
    #[serde(default)]
    pub exclude_tools: Option<Vec<String>>,

    /// Number of recent tool uses to keep in the conversation.
    #[serde(default)]
    pub keep: Option<ToolUsesKeep>,

    /// Condition that triggers this clear operation.
    #[serde(default)]
    pub trigger: Option<ContextManagementTrigger>,

    /// Context management edit type identifier expected by Anthropic.
    #[serde(rename = "type", default)]
    pub kind: ClearToolUsesType,

    /// Additional clear-tool-use fields passed through as-is.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Context management details returned in the response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseContextManagement {
    /// Context management edits Anthropic applied while serving the request.
    pub applied_edits: Vec<ResponseClearToolUses20250919Edit>,

    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Applied clear tool uses edit in the response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseClearToolUses20250919Edit {
    /// Number of input tokens removed by the edit.
    pub cleared_input_tokens: u32,
    /// Number of tool uses cleared by the edit.
    pub cleared_tool_uses: u32,

    /// Context management edit type identifier.
    #[serde(rename = "type", default)]
    pub kind: ClearToolUsesType,

    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Enum representing known clear tool use edit kinds.
#[derive(Default, Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClearToolUsesType {
    #[default]
    ClearToolUses20250919,
    #[serde(untagged)]
    Unknown(String),
}

/// Clear tool inputs may be a flag or list of tool names.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ClearToolInputs {
    Flag(bool),
    Tools(Vec<String>),
    #[serde(untagged)]
    Unknown(Value),
}

/// Minimum tokens that must be cleared.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InputTokensClearAtLeast {
    /// Amount of input tokens that must be cleared.
    pub value: u32,

    /// Input-token discriminator required by the API schema.
    #[serde(default, rename = "type")]
    pub kind: InputTokensKind,

    /// Extra fields retained for forward compatibility.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Marker for input-token based metrics in context management.
#[derive(Default, Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InputTokensKind {
    #[default]
    InputTokens,
    #[serde(untagged)]
    Unknown(String),
}

/// Number of tool uses to retain.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToolUsesKeep {
    /// Number of tool uses to retain in the transcript.
    pub value: u32,

    /// Tool-uses discriminator required by the API schema.
    #[serde(default, rename = "type")]
    pub kind: ToolUsesKind,

    /// Additional bookkeeping fields forwarded unchanged.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Marker for tool-use based metrics in context management.
#[derive(Default, Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolUsesKind {
    #[default]
    ToolUses,
    #[serde(untagged)]
    Unknown(String),
}

/// Triggers for context management edits.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContextManagementTrigger {
    InputTokens {
        /// Trigger value expressed in input tokens.
        value: u32,
        /// Extra trigger metadata retained verbatim.
        #[serde(flatten)]
        unknown_fields: UnknownFields,
    },
    ToolUses {
        /// Trigger value expressed in tool uses.
        value: u32,
        /// Extra trigger metadata retained verbatim.
        #[serde(flatten)]
        unknown_fields: UnknownFields,
    },
    #[serde(untagged)]
    Unknown(Value),
}
