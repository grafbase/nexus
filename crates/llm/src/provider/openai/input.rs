use serde::Serialize;

use crate::messages::{openai, unified};

/// Request body for OpenAI Chat Completions API.
///
/// This struct represents the request format for the `/v1/chat/completions` endpoint
/// as documented in the [OpenAI API Reference](https://platform.openai.com/docs/api-reference/chat/create).
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct OpenAIRequest {
    /// ID of the model to use.
    /// See the [model endpoint compatibility](https://platform.openai.com/docs/models/model-endpoint-compatibility)
    /// table for details on which models work with the Chat API.
    pub model: String,

    /// A list of messages comprising the conversation so far.
    /// Each message has a role (system, user, or assistant) and content.
    pub messages: Vec<openai::ChatMessage>,

    /// What sampling temperature to use, between 0 and 2.
    /// Higher values like 0.8 will make the output more random, while lower values like 0.2
    /// will make it more focused and deterministic.
    ///
    /// We generally recommend altering this or `top_p` but not both.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,

    /// The maximum number of tokens that can be generated in the chat completion.
    ///
    /// The total length of input tokens and generated tokens is limited by the model's context length.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_completion_tokens: Option<u32>,

    /// An alternative to sampling with temperature, called nucleus sampling.
    /// The model considers the results of the tokens with top_p probability mass.
    /// So 0.1 means only the tokens comprising the top 10% probability mass are considered.
    ///
    /// We generally recommend altering this or `temperature` but not both.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,

    /// Number between -2.0 and 2.0.
    /// Positive values penalize new tokens based on their existing frequency in the text so far,
    /// decreasing the model's likelihood to repeat the same line verbatim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f32>,

    /// Number between -2.0 and 2.0.
    /// Positive values penalize new tokens based on whether they appear in the text so far,
    /// increasing the model's likelihood to talk about new topics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f32>,

    /// Up to 4 sequences where the API will stop generating further tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,

    /// If set, partial message deltas will be sent, like in ChatGPT.
    /// Tokens will be sent as data-only server-sent events as they become available,
    /// with the stream terminated by a `data: [DONE]` message.
    pub stream: bool,

    /// A list of tools the model may call. Currently, only functions are supported as a tool.
    /// Use this to provide a list of functions the model may generate JSON inputs for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<openai::Tool>>,

    /// Controls which (if any) tool is called by the model.
    /// none means the model will not call any tool and instead generates a message.
    /// auto means the model can pick between generating a message or calling one or more tools.
    /// required means the model must call one or more tools.
    /// Specifying a particular tool via {"type": "function", "function": {"name": "my_function"}}
    /// forces the model to call that tool.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<openai::ToolChoice>,

    /// Whether to enable parallel function calling during tool use.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,
}

impl From<unified::UnifiedRequest> for OpenAIRequest {
    fn from(request: unified::UnifiedRequest) -> Self {
        // First convert unified to OpenAI format
        let openai_request = openai::ChatCompletionRequest::from(request);

        let openai::ChatCompletionRequest {
            model,
            messages,
            temperature,
            max_tokens,
            top_p,
            frequency_penalty,
            presence_penalty,
            stop,
            stream,
            tools,
            tool_choice,
            parallel_tool_calls,
        } = openai_request;

        Self {
            model,
            messages,
            temperature,
            max_completion_tokens: max_tokens,
            top_p,
            frequency_penalty,
            presence_penalty,
            stop,
            stream: stream.unwrap_or(false),
            tools,
            tool_choice,
            parallel_tool_calls,
        }
    }
}
