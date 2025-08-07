use serde::Serialize;

use crate::messages::{ChatCompletionRequest, ChatMessage, ChatRole};

/// Request body for Anthropic Messages API.
///
/// This struct represents the request format for creating messages with Claude models
/// as documented in the [Anthropic API Reference](https://docs.anthropic.com/en/api/messages).
#[derive(Debug, Serialize)]
pub(super) struct AnthropicRequest {
    /// The model that will complete your prompt.
    /// See [models](https://docs.anthropic.com/en/docs/models-overview) for additional details.
    /// Examples: "claude-3-opus-20240229", "claude-3-sonnet-20240229", "claude-3-haiku-20240307"
    pub model: String,

    /// Input messages.
    ///
    /// Our models are trained to operate on alternating user and assistant conversational turns.
    /// Messages must alternate between user and assistant roles.
    pub messages: Vec<AnthropicMessage>,

    /// System prompt.
    ///
    /// A system prompt is a way of providing context and instructions to Claude,
    /// separate from the user's direct input.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,

    /// The maximum number of tokens to generate before stopping.
    ///
    /// Different models have different maximum values.
    /// Refer to [models](https://docs.anthropic.com/en/docs/models-overview) for details.
    pub max_tokens: u32,

    /// Amount of randomness injected into the response.
    ///
    /// Defaults to 1.0. Ranges from 0.0 to 1.0. Use temperature closer to 0.0
    /// for analytical / multiple choice, and closer to 1.0 for creative and generative tasks.
    ///
    /// Note that even with temperature of 0.0, the results will not be fully deterministic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,

    /// Use nucleus sampling.
    ///
    /// In nucleus sampling, we compute the cumulative distribution over all the options
    /// for each subsequent token in decreasing probability order and cut it off once it
    /// exceeds the value of top_p. You should either alter temperature or top_p, but not both.
    ///
    /// Recommended for advanced use cases only. You usually only need to use temperature.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,

    /// Only sample from the top K options for each subsequent token.
    ///
    /// Used to remove "long tail" low probability responses.
    /// [Learn more technical details here](https://towardsdatascience.com/how-to-sample-from-language-models-682bceb97277).
    ///
    /// Recommended for advanced use cases only. You usually only need to use temperature.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,

    /// Custom text sequences that will cause the model to stop generating.
    ///
    /// Our models will normally stop when they have naturally completed their turn,
    /// which will result in a response stop_reason of "end_turn".
    ///
    /// If you want the model to stop generating when it encounters custom strings of text,
    /// you can use the stop_sequences parameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,

    /// Whether to stream the response using server-sent events.
    ///
    /// When true, the response will be streamed incrementally as it's generated.
    /// Default is false for non-streaming responses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

/// Represents a message in the conversation with Claude.
///
/// Messages must alternate between user and assistant roles.
#[derive(Debug, Serialize)]
pub(super) struct AnthropicMessage {
    /// The role of the message sender.
    /// Must be either "user" or "assistant".
    pub role: ChatRole,

    /// The content of the message.
    /// For the Messages API, this is always a string.
    /// Multi-modal content (images) would use a different structure.
    pub content: String,
}

impl From<ChatMessage> for AnthropicMessage {
    fn from(msg: ChatMessage) -> Self {
        Self {
            role: msg.role,
            content: msg.content,
        }
    }
}

impl From<ChatCompletionRequest> for AnthropicRequest {
    fn from(request: ChatCompletionRequest) -> Self {
        let mut system_message = None;
        let mut anthropic_messages = Vec::new();

        for msg in request.messages {
            match &msg.role {
                ChatRole::System => {
                    system_message = Some(msg.content);
                }
                ChatRole::Assistant | ChatRole::User => {
                    anthropic_messages.push(AnthropicMessage::from(msg));
                }
                ChatRole::Other(role) => {
                    log::warn!("Unknown chat role from request: {role}, treating as user");
                    anthropic_messages.push(AnthropicMessage {
                        role: ChatRole::User,
                        content: msg.content,
                    });
                }
            }
        }

        AnthropicRequest {
            model: request.model,
            messages: anthropic_messages,
            system: system_message,
            max_tokens: request.max_tokens.unwrap_or(4096),
            temperature: request.temperature,
            top_p: request.top_p,
            top_k: None,
            stop_sequences: request.stop,
            stream: None,
        }
    }
}
