//! Token counting for rate limiting.

use std::sync::OnceLock;

use tiktoken_rs::{CoreBPE, cl100k_base};

use crate::messages::openai::{ChatCompletionRequest, ChatMessage};

/// Global tokenizer instance using cl100k_base encoding.
static TOKENIZER: OnceLock<CoreBPE> = OnceLock::new();

/// Get or initialize the tokenizer.
fn get_tokenizer() -> &'static CoreBPE {
    TOKENIZER.get_or_init(|| cl100k_base().expect("Failed to initialize cl100k_base tokenizer"))
}

/// Count tokens in a chat completion request.
///
/// This uses the cl100k_base encoding which is used by GPT-4 and GPT-3.5-turbo models.
/// While not all providers use the same tokenization, this provides a reasonable
/// approximation for rate limiting purposes.
///
/// # Token Counting Methodology
///
/// The total token count includes three components:
///
/// 1. **Message Content**: The actual tokens in each message's role and content
/// 2. **Formatting Overhead**: ~3 tokens per message for OpenAI's internal message structure
///    - These include special tokens like `<|im_start|>`, role markers, and `<|im_end|>`
///    - This overhead is part of how OpenAI formats messages in their chat completion API
/// 3. **Assistant Reply Priming**: 3 tokens reserved for starting the assistant's response
///    - Before generating any actual content, the model consumes tokens like `<|im_start|>assistant`
///    - These tokens are always consumed even if the response is empty
///
/// This methodology follows OpenAI's official token counting guidelines to ensure
/// accurate rate limiting based on actual API consumption.
pub(crate) fn count_input_tokens(request: &ChatCompletionRequest) -> usize {
    // Get the cl100k_base tokenizer used by GPT-4 and GPT-3.5-turbo
    let tokenizer = get_tokenizer();
    let mut total = 0;

    // Count the actual content tokens in each message (role + content)
    for message in &request.messages {
        total += count_message_tokens(tokenizer, message);
    }

    // Add formatting overhead: OpenAI uses ~3 tokens per message for structural markers
    // These tokens wrap each message in the conversation format but aren't visible in the API
    total += request.messages.len() * 3;

    // Reserve tokens for assistant response initialization
    // The model always consumes ~3 tokens to begin its response before any actual content
    total += 3;

    total
}

/// Count tokens in a single message.
fn count_message_tokens(tokenizer: &CoreBPE, message: &ChatMessage) -> usize {
    let mut tokens = 0;

    // Count role tokens
    let role_str = message.role.as_ref();
    tokens += tokenizer.encode_ordinary(role_str).len();

    // Count content tokens
    if let Some(content) = &message.content {
        tokens += tokenizer.encode_ordinary(content).len();
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::openai::ChatRole;

    #[test]
    fn count_simple_message() {
        let request = ChatCompletionRequest {
            model: "gpt-4".to_string(),
            messages: vec![ChatMessage {
                role: ChatRole::User,
                content: Some("Hello, how are you?".to_string()),
                tool_calls: None,
                tool_call_id: None,
            }],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            stop: None,
            frequency_penalty: None,
            presence_penalty: None,
            tools: None,
            tool_choice: None,
            parallel_tool_calls: None,
        };

        let tokens = count_input_tokens(&request);
        // "Hello, how are you?" is approximately 6 tokens
        // Plus role ("user" = 1 token) and message overhead (3 tokens)
        // Plus assistant reply priming (3 tokens)
        // Total should be around 13 tokens
        assert!(tokens > 0);
        assert!(tokens < 20);
    }

    #[test]
    fn count_multiple_messages() {
        let request = ChatCompletionRequest {
            model: "gpt-4".to_string(),
            messages: vec![
                ChatMessage {
                    role: ChatRole::System,
                    content: Some("You are a helpful assistant.".to_string()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                ChatMessage {
                    role: ChatRole::User,
                    content: Some("What is the weather?".to_string()),
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            stop: None,
            frequency_penalty: None,
            presence_penalty: None,
            tools: None,
            tool_choice: None,
            parallel_tool_calls: None,
        };

        let tokens = count_input_tokens(&request);
        // Should count tokens from both messages plus overhead
        assert!(tokens > 10);
        assert!(tokens < 50);
    }

    #[test]
    fn empty_content_counts_role() {
        let request = ChatCompletionRequest {
            model: "gpt-4".to_string(),
            messages: vec![ChatMessage {
                role: ChatRole::Assistant,
                content: Some("".to_string()),
                tool_calls: None,
                tool_call_id: None,
            }],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: None,
            stop: None,
            frequency_penalty: None,
            presence_penalty: None,
            tools: None,
            tool_choice: None,
            parallel_tool_calls: None,
        };

        let tokens = count_input_tokens(&request);
        // Should still count role token and overhead
        assert!(tokens > 0);
    }
}
