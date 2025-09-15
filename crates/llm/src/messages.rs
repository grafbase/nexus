/// Message types for different LLM protocols.
///
/// This module organizes message formats by protocol, with OpenAI format
/// serving as the current default and primary interchange format.
pub(crate) mod openai;

// Re-export OpenAI types at the module level for backward compatibility
// This allows existing code to continue using `messages::ChatCompletionRequest`
// instead of `messages::openai::ChatCompletionRequest`
pub(crate) use openai::{
    ChatChoice, ChatChoiceDelta, ChatCompletionChunk, ChatCompletionRequest, ChatCompletionResponse, ChatMessage,
    ChatMessageDelta, ChatRole, FinishReason, FunctionCall, FunctionDelta, FunctionStart, Model, ModelsResponse,
    ObjectType, StreamingFunctionCall, StreamingToolCall, Tool, ToolCall, ToolCallType, ToolChoice, ToolChoiceMode,
    Usage,
};
