//! Conversions from OpenAI protocol types to unified types.
//!
//! ZERO ALLOCATIONS - All data is moved, not cloned.

use crate::messages::{openai, unified};
use std::borrow::Cow;

impl From<openai::ChatCompletionRequest> for unified::UnifiedRequest {
    fn from(req: openai::ChatCompletionRequest) -> Self {
        // Most requests don't have multiple system messages, start with capacity 1
        let mut system_content = Vec::with_capacity(1);
        // Pre-allocate for messages (most will be non-system)
        let mut unified_messages = Vec::with_capacity(req.messages.len());

        for msg in req.messages {
            if msg.role == openai::ChatRole::System {
                if let Some(content) = msg.content {
                    system_content.push(content);
                }
            } else {
                unified_messages.push(unified::UnifiedMessage::from(msg));
            }
        }

        // Move system content to system field
        let system = if system_content.is_empty() {
            None
        } else if system_content.len() == 1 {
            Some(system_content.into_iter().next().unwrap())
        } else {
            Some(system_content.join("\n"))
        };

        Self {
            model: req.model,
            messages: unified_messages,
            system,
            max_tokens: req.max_tokens,
            temperature: req.temperature,
            top_p: req.top_p,
            top_k: None,
            frequency_penalty: req.frequency_penalty,
            presence_penalty: req.presence_penalty,
            stop_sequences: req.stop,
            stream: req.stream,
            tools: req
                .tools
                .map(|t| t.into_iter().map(unified::UnifiedTool::from).collect()),
            tool_choice: req.tool_choice.map(unified::UnifiedToolChoice::from),
            parallel_tool_calls: req.parallel_tool_calls,
            metadata: None,
        }
    }
}

impl From<openai::ChatRole> for unified::UnifiedRole {
    fn from(role: openai::ChatRole) -> Self {
        match role {
            openai::ChatRole::System => unified::UnifiedRole::System,
            openai::ChatRole::User => unified::UnifiedRole::User,
            openai::ChatRole::Assistant => unified::UnifiedRole::Assistant,
            openai::ChatRole::Tool => unified::UnifiedRole::Tool,
            openai::ChatRole::Other(_) => unified::UnifiedRole::Assistant, // Default to assistant
        }
    }
}

impl From<openai::ChatMessage> for unified::UnifiedMessage {
    fn from(msg: openai::ChatMessage) -> Self {
        let role = unified::UnifiedRole::from(msg.role);

        let content = if let Some(text) = msg.content {
            unified::UnifiedContentContainer::Text(text)
        } else {
            unified::UnifiedContentContainer::Blocks(vec![])
        };

        let tool_calls = msg.tool_calls.map(|calls| {
            calls
                .into_iter()
                .map(|call| unified::UnifiedToolCall {
                    id: call.id,
                    function: unified::UnifiedFunctionCall {
                        name: call.function.name,
                        arguments: unified::UnifiedArguments::String(call.function.arguments),
                    },
                })
                .collect()
        });

        Self {
            role,
            content,
            tool_calls,
            tool_call_id: msg.tool_call_id,
        }
    }
}

impl From<openai::Tool> for unified::UnifiedTool {
    fn from(tool: openai::Tool) -> Self {
        Self {
            function: unified::UnifiedFunction {
                name: tool.function.name,
                description: tool.function.description,
                parameters: tool.function.parameters,
                strict: None,
            },
        }
    }
}

impl From<openai::ToolChoiceMode> for unified::UnifiedToolChoiceMode {
    fn from(mode: openai::ToolChoiceMode) -> Self {
        match mode {
            openai::ToolChoiceMode::None => unified::UnifiedToolChoiceMode::None,
            openai::ToolChoiceMode::Auto => unified::UnifiedToolChoiceMode::Auto,
            openai::ToolChoiceMode::Required | openai::ToolChoiceMode::Any => unified::UnifiedToolChoiceMode::Required,
            openai::ToolChoiceMode::Other(_) => unified::UnifiedToolChoiceMode::Auto, // Default
        }
    }
}

impl From<openai::ToolChoice> for unified::UnifiedToolChoice {
    fn from(choice: openai::ToolChoice) -> Self {
        match choice {
            openai::ToolChoice::Mode(mode) => {
                unified::UnifiedToolChoice::Mode(unified::UnifiedToolChoiceMode::from(mode))
            }
            openai::ToolChoice::Specific { function, .. } => unified::UnifiedToolChoice::Specific {
                function: unified::UnifiedFunctionChoice { name: function.name },
            },
        }
    }
}

impl From<openai::ChatCompletionResponse> for unified::UnifiedResponse {
    fn from(resp: openai::ChatCompletionResponse) -> Self {
        Self {
            id: resp.id,
            model: resp.model,
            choices: resp
                .choices
                .into_iter()
                .map(|choice| unified::UnifiedChoice {
                    index: choice.index,
                    message: unified::UnifiedMessage::from(choice.message),
                    finish_reason: Some(unified::UnifiedFinishReason::from(choice.finish_reason)),
                })
                .collect(),
            usage: unified::UnifiedUsage {
                prompt_tokens: resp.usage.prompt_tokens,
                completion_tokens: resp.usage.completion_tokens,
                total_tokens: resp.usage.total_tokens,
            },
            created: resp.created,
            stop_reason: None,
            stop_sequence: None,
        }
    }
}

impl From<openai::FinishReason> for unified::UnifiedFinishReason {
    fn from(reason: openai::FinishReason) -> Self {
        match reason {
            openai::FinishReason::Stop => unified::UnifiedFinishReason::Stop,
            openai::FinishReason::Length => unified::UnifiedFinishReason::Length,
            openai::FinishReason::ContentFilter => unified::UnifiedFinishReason::ContentFilter,
            openai::FinishReason::ToolCalls => unified::UnifiedFinishReason::ToolCalls,
            openai::FinishReason::Other(_) => unified::UnifiedFinishReason::Stop, // Default
        }
    }
}

impl From<openai::StreamingToolCall> for unified::UnifiedStreamingToolCall {
    fn from(call: openai::StreamingToolCall) -> Self {
        match call {
            openai::StreamingToolCall::Start {
                index,
                id,
                r#type: _,
                function,
            } => unified::UnifiedStreamingToolCall::Start {
                index,
                id,
                function: unified::UnifiedFunctionStart {
                    name: function.name,
                    arguments: function.arguments,
                },
            },
            openai::StreamingToolCall::Delta { index, function } => unified::UnifiedStreamingToolCall::Delta {
                index,
                function: unified::UnifiedFunctionDelta {
                    arguments: function.arguments,
                },
            },
        }
    }
}

impl From<openai::ChatCompletionChunk> for unified::UnifiedChunk {
    fn from(chunk: openai::ChatCompletionChunk) -> Self {
        Self {
            id: Cow::Owned(chunk.id),
            model: Cow::Owned(chunk.model),
            choices: chunk
                .choices
                .into_iter()
                .map(|choice| unified::UnifiedChoiceDelta {
                    index: choice.index,
                    delta: unified::UnifiedMessageDelta {
                        role: choice.delta.role.map(unified::UnifiedRole::from),
                        content: choice.delta.content,
                        tool_calls: choice
                            .delta
                            .tool_calls
                            .map(|calls| calls.into_iter().map(unified::UnifiedStreamingToolCall::from).collect()),
                    },
                    finish_reason: choice.finish_reason.map(unified::UnifiedFinishReason::from),
                })
                .collect(),
            usage: chunk.usage.map(|u| unified::UnifiedUsage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
                total_tokens: u.total_tokens,
            }),
            created: chunk.created,
        }
    }
}
