use anyhow::Result;
use config::LlmConfig;

use crate::messages::{ChatChoice, ChatCompletionRequest, ChatCompletionResponse, ChatMessage, ModelsResponse, Usage};

pub(crate) struct LlmServer;

impl LlmServer {
    pub async fn new(_: LlmConfig) -> Result<Self> {
        Ok(Self)
    }

    /// Process a chat completion request.
    pub async fn completions(&self, request: ChatCompletionRequest) -> Result<ChatCompletionResponse> {
        let mut choices = Vec::new();

        for (index, message) in request.messages.into_iter().enumerate() {
            let choice = ChatChoice {
                index: index as u32,
                message,
                finish_reason: "stop".to_string(),
            };

            choices.push(choice);
        }

        let response = ChatChoice {
            index: 1,
            message: ChatMessage {
                role: "assistant".to_string(),
                content: "Hello, world!".to_string(),
            },
            finish_reason: "stop".to_string(),
        };

        choices.push(response);

        Ok(ChatCompletionResponse {
            id: "chatcmpl-123".to_string(),
            object: "chat.completion".to_string(),
            created: 1677651200,
            model: request.model.clone(),
            choices,
            usage: Usage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            },
        })
    }

    /// List available models.
    pub async fn list_models(&self) -> Result<ModelsResponse> {
        Ok(ModelsResponse {
            object: "list".to_string(),
            data: Vec::new(),
        })
    }
}
