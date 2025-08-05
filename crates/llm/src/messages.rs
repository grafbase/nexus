use serde::{Deserialize, Serialize};

/// OpenAI-compatible chat completion request.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ChatCompletionRequest {
    pub(crate) messages: Vec<ChatMessage>,
    pub(crate) model: String,
}

/// Chat message in OpenAI format.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct ChatMessage {
    pub(crate) role: String,
    pub(crate) content: String,
}

/// OpenAI-compatible chat completion response.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ChatCompletionResponse {
    pub(crate) id: String,
    pub(crate) object: String,
    pub(crate) created: u64,
    pub(crate) model: String,
    pub(crate) choices: Vec<ChatChoice>,
    pub(crate) usage: Usage,
}

/// Chat completion choice.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ChatChoice {
    pub(crate) index: u32,
    pub(crate) message: ChatMessage,
    pub(crate) finish_reason: String,
}

/// Token usage information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Usage {
    pub(crate) prompt_tokens: u32,
    pub(crate) completion_tokens: u32,
    pub(crate) total_tokens: u32,
}

/// Model information.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Model {
    pub(crate) id: String,
    pub(crate) object: String,
    pub(crate) created: u64,
    pub(crate) owned_by: String,
}

/// Models list response.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ModelsResponse {
    pub(crate) object: String,
    pub(crate) data: Vec<Model>,
}
