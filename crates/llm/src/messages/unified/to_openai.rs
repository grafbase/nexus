//! Conversions from unified types to OpenAI protocol types.
//!
//! ZERO ALLOCATIONS - All data is moved, not cloned.

use crate::messages::{openai, unified};

impl From<unified::UnifiedRequest> for openai::ChatCompletionRequest {
    fn from(req: unified::UnifiedRequest) -> Self {
        // Convert messages and extract system messages
        let mut messages = Vec::with_capacity(req.messages.len() + if req.system.is_some() { 1 } else { 0 });

        // Add system message if present
        if let Some(system) = req.system {
            messages.push(openai::ChatMessage {
                role: openai::ChatRole::System,
                content: Some(system),
                tool_calls: None,
                tool_call_id: None,
            });
        }

        // Convert all messages
        for msg in req.messages {
            messages.push(openai::ChatMessage::from(msg));
        }

        Self {
            model: req.model,
            messages,
            temperature: req.temperature,
            max_tokens: req.max_tokens,
            top_p: req.top_p,
            frequency_penalty: req.frequency_penalty,
            presence_penalty: req.presence_penalty,
            stop: req.stop_sequences,
            stream: req.stream,
            tools: req.tools.map(|t| t.into_iter().map(openai::Tool::from).collect()),
            tool_choice: req.tool_choice.map(openai::ToolChoice::from),
            parallel_tool_calls: req.parallel_tool_calls,
        }
    }
}

impl From<unified::UnifiedRole> for openai::ChatRole {
    fn from(role: unified::UnifiedRole) -> Self {
        match role {
            unified::UnifiedRole::System => openai::ChatRole::System,
            unified::UnifiedRole::User => openai::ChatRole::User,
            unified::UnifiedRole::Assistant => openai::ChatRole::Assistant,
            unified::UnifiedRole::Tool => openai::ChatRole::Tool,
        }
    }
}

impl From<unified::UnifiedMessage> for openai::ChatMessage {
    fn from(msg: unified::UnifiedMessage) -> Self {
        let role = openai::ChatRole::from(msg.role);

        let content = match msg.content {
            unified::UnifiedContentContainer::Text(text) => Some(text),
            unified::UnifiedContentContainer::Blocks(blocks) => {
                // Convert blocks to text - OpenAI doesn't support structured content in the same way
                // Extract text from blocks
                let text_parts: Vec<String> = blocks
                    .into_iter()
                    .filter_map(|block| match block {
                        unified::UnifiedContent::Text { text } => Some(text),
                        unified::UnifiedContent::ToolResult { content, .. } => match content {
                            unified::UnifiedToolResultContent::Text(text) => Some(text),
                            unified::UnifiedToolResultContent::Multiple(texts) => Some(texts.join("\n")),
                        },
                        _ => None,
                    })
                    .collect();

                if text_parts.is_empty() {
                    None
                } else {
                    Some(text_parts.join("\n"))
                }
            }
        };

        Self {
            role,
            content,
            tool_calls: msg.tool_calls.map(|calls| {
                calls
                    .into_iter()
                    .map(|call| openai::ToolCall {
                        id: call.id,
                        tool_type: openai::ToolCallType::Function,
                        function: openai::FunctionCall {
                            name: call.function.name,
                            arguments: match call.function.arguments {
                                unified::UnifiedArguments::String(s) => s,
                                unified::UnifiedArguments::Value(v) => {
                                    serde_json::to_string(&v).unwrap_or_else(|_| "{}".to_string())
                                }
                            },
                        },
                    })
                    .collect()
            }),
            tool_call_id: msg.tool_call_id,
        }
    }
}

impl From<unified::UnifiedTool> for openai::Tool {
    fn from(tool: unified::UnifiedTool) -> Self {
        Self {
            tool_type: openai::ToolCallType::Function,
            function: openai::FunctionDefinition {
                name: tool.function.name,
                description: tool.function.description,
                parameters: tool.function.parameters,
            },
        }
    }
}

impl From<unified::UnifiedToolChoiceMode> for openai::ToolChoiceMode {
    fn from(mode: unified::UnifiedToolChoiceMode) -> Self {
        match mode {
            unified::UnifiedToolChoiceMode::None => openai::ToolChoiceMode::None,
            unified::UnifiedToolChoiceMode::Auto => openai::ToolChoiceMode::Auto,
            unified::UnifiedToolChoiceMode::Required => openai::ToolChoiceMode::Required,
        }
    }
}

impl From<unified::UnifiedToolChoice> for openai::ToolChoice {
    fn from(choice: unified::UnifiedToolChoice) -> Self {
        match choice {
            unified::UnifiedToolChoice::Mode(mode) => openai::ToolChoice::Mode(openai::ToolChoiceMode::from(mode)),
            unified::UnifiedToolChoice::Specific { function } => openai::ToolChoice::Specific {
                tool_type: openai::ToolCallType::Function,
                function: openai::ToolChoiceFunction { name: function.name },
            },
        }
    }
}

impl From<unified::UnifiedResponse> for openai::ChatCompletionResponse {
    fn from(resp: unified::UnifiedResponse) -> Self {
        Self {
            id: resp.id,
            object: openai::ObjectType::ChatCompletion,
            created: resp.created,
            model: resp.model,
            choices: resp
                .choices
                .into_iter()
                .map(|choice| openai::ChatChoice {
                    index: choice.index,
                    message: openai::ChatMessage::from(choice.message),
                    finish_reason: choice
                        .finish_reason
                        .map(openai::FinishReason::from)
                        .unwrap_or(openai::FinishReason::Stop),
                })
                .collect(),
            usage: openai::Usage {
                prompt_tokens: resp.usage.prompt_tokens,
                completion_tokens: resp.usage.completion_tokens,
                total_tokens: resp.usage.total_tokens,
            },
        }
    }
}

impl From<unified::UnifiedFinishReason> for openai::FinishReason {
    fn from(reason: unified::UnifiedFinishReason) -> Self {
        match reason {
            unified::UnifiedFinishReason::Stop => openai::FinishReason::Stop,
            unified::UnifiedFinishReason::Length => openai::FinishReason::Length,
            unified::UnifiedFinishReason::ContentFilter => openai::FinishReason::ContentFilter,
            unified::UnifiedFinishReason::ToolCalls => openai::FinishReason::ToolCalls,
        }
    }
}

impl From<unified::UnifiedStreamingToolCall> for openai::StreamingToolCall {
    fn from(call: unified::UnifiedStreamingToolCall) -> Self {
        match call {
            unified::UnifiedStreamingToolCall::Start { index, id, function } => openai::StreamingToolCall::Start {
                index,
                id,
                r#type: openai::ToolCallType::Function,
                function: openai::FunctionStart {
                    name: function.name,
                    arguments: function.arguments,
                },
            },
            unified::UnifiedStreamingToolCall::Delta { index, function } => openai::StreamingToolCall::Delta {
                index,
                function: openai::FunctionDelta {
                    arguments: function.arguments,
                },
            },
        }
    }
}

impl From<unified::UnifiedChunk> for openai::ChatCompletionChunk {
    fn from(chunk: unified::UnifiedChunk) -> Self {
        Self {
            id: chunk.id.into_owned(),
            object: openai::ObjectType::ChatCompletionChunk,
            created: chunk.created,
            model: chunk.model.into_owned(),
            system_fingerprint: None,
            choices: chunk
                .choices
                .into_iter()
                .map(|choice| openai::ChatChoiceDelta {
                    index: choice.index,
                    logprobs: None,
                    delta: openai::ChatMessageDelta {
                        role: choice.delta.role.map(openai::ChatRole::from),
                        content: choice.delta.content,
                        function_call: None,
                        tool_calls: choice
                            .delta
                            .tool_calls
                            .map(|calls| calls.into_iter().map(openai::StreamingToolCall::from).collect()),
                    },
                    finish_reason: choice.finish_reason.map(openai::FinishReason::from),
                })
                .collect(),
            usage: chunk.usage.map(|u| openai::Usage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
                total_tokens: u.total_tokens,
            }),
        }
    }
}

impl From<unified::UnifiedModel> for openai::Model {
    fn from(model: unified::UnifiedModel) -> Self {
        Self {
            id: model.id,
            object: openai::ObjectType::Model,
            created: model.created,
            owned_by: model.owned_by,
        }
    }
}

impl From<unified::UnifiedModelsResponse> for openai::ModelsResponse {
    fn from(response: unified::UnifiedModelsResponse) -> Self {
        Self {
            object: openai::ObjectType::List,
            data: response.models.into_iter().map(openai::Model::from).collect(),
        }
    }
}
