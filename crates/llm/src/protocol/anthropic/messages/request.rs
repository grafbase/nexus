use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::protocol::unknown_fields::UnknownFields;

use super::*;

/// Request body for the Anthropic Messages API.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Request {
    /// The model that will complete the prompt.
    pub model: String,
    /// Conversation turns supplied to the Messages API.
    pub messages: Vec<InputMessage>,
    /// Maximum output tokens Anthropic may generate.
    pub max_tokens: u32,

    /// System prompt providing global instructions for the assistant.
    #[serde(default)]
    pub system: Option<SystemPrompt>,

    /// Sampling temperature controlling randomness (0.0–1.0).
    #[serde(default)]
    pub temperature: Option<f32>,

    /// Probability mass cutoff used for nucleus sampling.
    #[serde(default)]
    pub top_p: Option<f32>,

    /// Top-K sampling limit that constrains candidate tokens.
    #[serde(default)]
    pub top_k: Option<u32>,

    /// Custom strings that cause generation to stop when produced.
    #[serde(default)]
    pub stop_sequences: Option<Vec<String>>,

    /// When true, deliver a Server-Sent Events stream instead of a single body.
    #[serde(default)]
    pub stream: Option<bool>,

    /// Optional metadata describing the end user for abuse detection.
    #[serde(default)]
    pub metadata: Option<Metadata>,

    /// Tool specifications the model may call during this request.
    #[serde(default)]
    pub tools: Option<Vec<Tool>>,

    /// Directive controlling if and how the model must use tools.
    #[serde(default)]
    pub tool_choice: Option<ToolChoice>,

    /// Optional container identifier reused for code execution workflows.
    #[serde(default)]
    pub container: Option<String>,

    /// Context management edits applied before processing the request.
    #[serde(default)]
    pub context_management: Option<ContextManagementConfig>,

    /// MCP servers available to the model when handling this request.
    #[serde(default)]
    pub mcp_servers: Option<Vec<McpServerURLDefinition>>,

    /// Desired service tier (auto, standard-only, etc.).
    #[serde(default)]
    pub service_tier: Option<ServiceTier>,

    /// Configuration enabling Claude's extended thinking mode.
    #[serde(default)]
    pub thinking: Option<ThinkingConfig>,

    /// Additional undocumented fields preserved for forward compatibility.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// System prompt payload accepted by the Anthropic Messages API.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum SystemPrompt {
    /// Plain-text system prompt.
    Text(String),
    /// Structured system prompt comprised of content blocks.
    Blocks(Vec<SystemInputMessage>),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SystemInputMessage {
    Text(RequestTextBlock),
    #[serde(untagged)]
    Unknown(Value),
}

/// Optional metadata forwarded to Anthropic.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Metadata {
    /// External identifier for the end user associated with this request.
    #[serde(default)]
    pub user_id: Option<String>,

    /// Additional metadata keys retained for compatibility.
    #[serde(flatten)]
    pub unknown_fields: UnknownFields,
}

/// Allowed service tiers for Anthropic.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceTier {
    Auto,
    StandardOnly,
    #[serde(untagged)]
    Unknown(String),
}

/// Configuration for Anthropic thinking mode.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ThinkingConfig {
    Enabled {
        budget_tokens: u32,
        #[serde(flatten)]
        unknown_fields: UnknownFields,
    },
    Disabled {
        #[serde(flatten)]
        unknown_fields: UnknownFields,
    },
    #[serde(untagged)]
    Unknown(Value),
}
