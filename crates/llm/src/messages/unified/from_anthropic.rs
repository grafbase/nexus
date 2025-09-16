//! Conversions from Anthropic protocol types to unified types.

use crate::messages::{anthropic, unified};
use std::borrow::Cow;

impl From<anthropic::AnthropicChatRequest> for unified::UnifiedRequest {
    fn from(req: anthropic::AnthropicChatRequest) -> Self {
        // Move all data - no clones!
        Self {
            model: req.model,
            messages: req.messages.into_iter().map(unified::UnifiedMessage::from).collect(),
            system: req.system,
            max_tokens: Some(req.max_tokens),
            temperature: req.temperature,
            top_p: req.top_p,
            top_k: req.top_k,
            frequency_penalty: None,
            presence_penalty: None,
            stop_sequences: req.stop_sequences,
            stream: req.stream,
            tools: req
                .tools
                .map(|t| t.into_iter().map(unified::UnifiedTool::from).collect()),
            tool_choice: req.tool_choice.map(unified::UnifiedToolChoice::from),
            parallel_tool_calls: None,
            metadata: req.metadata.map(unified::UnifiedMetadata::from),
        }
    }
}

impl From<anthropic::AnthropicRole> for unified::UnifiedRole {
    fn from(role: anthropic::AnthropicRole) -> Self {
        match role {
            anthropic::AnthropicRole::User => unified::UnifiedRole::User,
            anthropic::AnthropicRole::Assistant => unified::UnifiedRole::Assistant,
        }
    }
}

impl From<anthropic::AnthropicContent> for unified::UnifiedContent {
    fn from(content: anthropic::AnthropicContent) -> Self {
        match content {
            anthropic::AnthropicContent::Text { text } => unified::UnifiedContent::Text { text },
            anthropic::AnthropicContent::Image { source } => unified::UnifiedContent::Image {
                source: unified::UnifiedImageSource::Base64 {
                    media_type: source.media_type,
                    data: source.data,
                },
            },
            anthropic::AnthropicContent::ToolUse { id, name, input } => {
                unified::UnifiedContent::ToolUse { id, name, input }
            }
            anthropic::AnthropicContent::ToolResult { tool_use_id, content } => {
                // Move the content vector into unified format
                let content = if content.len() == 1 {
                    // Single item - extract as simple text
                    match content.into_iter().next().unwrap() {
                        anthropic::AnthropicToolResultContent::Text { text } => {
                            unified::UnifiedToolResultContent::Text(text)
                        }
                        anthropic::AnthropicToolResultContent::Error { error } => {
                            unified::UnifiedToolResultContent::Text(error)
                        }
                    }
                } else {
                    // Multiple items - collect as vector
                    unified::UnifiedToolResultContent::Multiple(
                        content
                            .into_iter()
                            .map(|c| match c {
                                anthropic::AnthropicToolResultContent::Text { text } => text,
                                anthropic::AnthropicToolResultContent::Error { error } => error,
                            })
                            .collect(),
                    )
                };
                unified::UnifiedContent::ToolResult {
                    tool_use_id,
                    content,
                    is_error: None,
                }
            }
        }
    }
}

impl From<anthropic::AnthropicStopReason> for unified::UnifiedFinishReason {
    fn from(reason: anthropic::AnthropicStopReason) -> Self {
        match reason {
            anthropic::AnthropicStopReason::EndTurn => unified::UnifiedFinishReason::Stop,
            anthropic::AnthropicStopReason::MaxTokens => unified::UnifiedFinishReason::Length,
            anthropic::AnthropicStopReason::StopSequence => unified::UnifiedFinishReason::Stop,
            anthropic::AnthropicStopReason::ToolUse => unified::UnifiedFinishReason::ToolCalls,
        }
    }
}

impl From<anthropic::AnthropicMessage> for unified::UnifiedMessage {
    fn from(msg: anthropic::AnthropicMessage) -> Self {
        let role = unified::UnifiedRole::from(msg.role);

        // For assistant messages, we may need tool calls
        // Start with a small capacity - most messages don't have many tool uses
        let mut tool_calls = if role == unified::UnifiedRole::Assistant {
            Some(Vec::with_capacity(1))
        } else {
            None
        };

        let content: Vec<unified::UnifiedContent> = msg
            .content
            .into_iter()
            .map(|block| {
                // For assistant messages with ToolUse, also create tool calls
                if let anthropic::AnthropicContent::ToolUse { ref id, ref name, ref input } = block {
                    if let Some(ref mut calls) = tool_calls {
                        calls.push(unified::UnifiedToolCall {
                            id: id.clone(),
                            function: unified::UnifiedFunctionCall {
                                name: name.clone(),
                                arguments: unified::UnifiedArguments::Value(input.clone()),
                            },
                        });
                    }
                }
                unified::UnifiedContent::from(block)
            })
            .collect();

        // Clean up tool_calls if empty
        let tool_calls = tool_calls.filter(|calls| !calls.is_empty());

        Self {
            role,
            content: unified::UnifiedContentContainer::Blocks(content),
            tool_calls,
            tool_call_id: None,
        }
    }
}

impl From<anthropic::AnthropicTool> for unified::UnifiedTool {
    fn from(tool: anthropic::AnthropicTool) -> Self {
        Self {
            function: unified::UnifiedFunction {
                name: tool.name,
                description: tool.description,
                parameters: tool.input_schema,
                strict: None,
            },
        }
    }
}

impl From<anthropic::AnthropicToolChoice> for unified::UnifiedToolChoice {
    fn from(choice: anthropic::AnthropicToolChoice) -> Self {
        match choice {
            anthropic::AnthropicToolChoice::Auto => {
                unified::UnifiedToolChoice::Mode(unified::UnifiedToolChoiceMode::Auto)
            }
            anthropic::AnthropicToolChoice::Any => {
                unified::UnifiedToolChoice::Mode(unified::UnifiedToolChoiceMode::Required)
            }
            anthropic::AnthropicToolChoice::Tool { name } => unified::UnifiedToolChoice::Specific {
                function: unified::UnifiedFunctionChoice { name },
            },
        }
    }
}

impl From<anthropic::AnthropicMetadata> for unified::UnifiedMetadata {
    fn from(meta: anthropic::AnthropicMetadata) -> Self {
        Self { user_id: meta.user_id }
    }
}

impl From<anthropic::AnthropicChatResponse> for unified::UnifiedResponse {
    fn from(resp: anthropic::AnthropicChatResponse) -> Self {
        // Start with small capacity - most responses don't have many tool uses
        let mut tool_calls = Vec::with_capacity(1);

        let content: Vec<unified::UnifiedContent> = resp
            .content
            .into_iter()
            .filter_map(|block| {
                // For ToolUse blocks, also populate tool_calls
                if let anthropic::AnthropicContent::ToolUse { ref id, ref name, ref input } = block {
                    tool_calls.push(unified::UnifiedToolCall {
                        id: id.clone(),
                        function: unified::UnifiedFunctionCall {
                            name: name.clone(),
                            arguments: unified::UnifiedArguments::Value(input.clone()),
                        },
                    });
                }

                match block {
                    anthropic::AnthropicContent::ToolResult { .. } => {
                        // Tool results shouldn't appear in responses
                        None
                    }
                    other => Some(unified::UnifiedContent::from(other)),
                }
            })
            .collect();

        let message = unified::UnifiedMessage {
            role: unified::UnifiedRole::Assistant,
            content: unified::UnifiedContentContainer::Blocks(content),
            tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls) },
            tool_call_id: None,
        };

        // Convert stop reason
        let (finish_reason, stop_reason) = match resp.stop_reason {
            Some(reason) => {
                let finish = unified::UnifiedFinishReason::from(reason.clone());
                let stop = unified::UnifiedStopReason::from(reason);
                (Some(finish), Some(stop))
            }
            None => (None, None),
        };

        Self {
            id: resp.id,
            model: resp.model,
            choices: vec![unified::UnifiedChoice {
                index: 0,
                message,
                finish_reason,
            }],
            usage: unified::UnifiedUsage::from(resp.usage),
            created: 0, // Anthropic doesn't provide timestamp
            stop_reason,
            stop_sequence: resp.stop_sequence,
        }
    }
}

impl From<anthropic::AnthropicUsage> for unified::UnifiedUsage {
    fn from(usage: anthropic::AnthropicUsage) -> Self {
        let input_tokens = usage.input_tokens as u32;
        let output_tokens = usage.output_tokens as u32;
        Self {
            prompt_tokens: input_tokens,
            completion_tokens: output_tokens,
            total_tokens: input_tokens + output_tokens,
        }
    }
}

impl From<anthropic::AnthropicStopReason> for unified::UnifiedStopReason {
    fn from(reason: anthropic::AnthropicStopReason) -> Self {
        match reason {
            anthropic::AnthropicStopReason::EndTurn => unified::UnifiedStopReason::EndTurn,
            anthropic::AnthropicStopReason::MaxTokens => unified::UnifiedStopReason::MaxTokens,
            anthropic::AnthropicStopReason::StopSequence => unified::UnifiedStopReason::StopSequence,
            anthropic::AnthropicStopReason::ToolUse => unified::UnifiedStopReason::ToolUse,
        }
    }
}

impl From<anthropic::AnthropicModel> for unified::UnifiedModel {
    fn from(model: anthropic::AnthropicModel) -> Self {
        Self {
            id: model.id.clone(),
            object_type: unified::UnifiedObjectType::Model,
            display_name: model.display_name,
            created: model.created_at,
            owned_by: "anthropic".to_string(),
        }
    }
}

impl From<anthropic::AnthropicModelsResponse> for unified::UnifiedModelsResponse {
    fn from(response: anthropic::AnthropicModelsResponse) -> Self {
        Self {
            object_type: unified::UnifiedObjectType::List,
            models: response.data.into_iter().map(unified::UnifiedModel::from).collect(),
            has_more: response.has_more,
        }
    }
}

impl From<anthropic::AnthropicContentDelta> for unified::UnifiedMessageDelta {
    fn from(delta: anthropic::AnthropicContentDelta) -> Self {
        match delta {
            anthropic::AnthropicContentDelta::TextDelta { text } => unified::UnifiedMessageDelta {
                role: None,
                content: Some(text),
                tool_calls: None,
            },
            anthropic::AnthropicContentDelta::InputJsonDelta { partial_json } => unified::UnifiedMessageDelta {
                role: None,
                content: None,
                tool_calls: Some(vec![unified::UnifiedStreamingToolCall::Delta {
                    index: 0, // Will be overridden by caller with actual index
                    function: unified::UnifiedFunctionDelta {
                        arguments: partial_json,
                    },
                }]),
            },
        }
    }
}

impl From<anthropic::AnthropicStreamEvent> for unified::UnifiedChunk {
    fn from(event: anthropic::AnthropicStreamEvent) -> Self {
        use anthropic::AnthropicStreamEvent;

        match event {
            AnthropicStreamEvent::MessageStart { message } => Self {
                id: Cow::Owned(message.id),
                model: Cow::Owned(message.model),
                choices: vec![unified::UnifiedChoiceDelta {
                    index: 0,
                    delta: unified::UnifiedMessageDelta {
                        role: Some(unified::UnifiedRole::Assistant),
                        content: None,
                        tool_calls: None,
                    },
                    finish_reason: None,
                }],
                usage: Some(unified::UnifiedUsage::from(message.usage)),
                created: 0,
            },
            AnthropicStreamEvent::ContentBlockStart { index, content_block } => {
                let delta = match content_block {
                    anthropic::AnthropicContent::Text { text } => unified::UnifiedMessageDelta {
                        role: None,
                        content: Some(text),
                        tool_calls: None,
                    },
                    anthropic::AnthropicContent::ToolUse { id, name, input } => unified::UnifiedMessageDelta {
                        role: None,
                        content: None,
                        tool_calls: Some(vec![unified::UnifiedStreamingToolCall::Start {
                            index: index as usize,
                            id,
                            function: unified::UnifiedFunctionStart {
                                name,
                                arguments: serde_json::to_string(&input).unwrap_or_else(|_| String::from("{}")),
                            },
                        }]),
                    },
                    _ => unified::UnifiedMessageDelta {
                        role: None,
                        content: None,
                        tool_calls: None,
                    },
                };

                Self {
                    id: Cow::Borrowed(""),
                    model: Cow::Borrowed(""),
                    choices: vec![unified::UnifiedChoiceDelta {
                        index: 0,
                        delta,
                        finish_reason: None,
                    }],
                    usage: None,
                    created: 0,
                }
            }
            AnthropicStreamEvent::ContentBlockDelta { index, delta } => {
                let mut unified_delta = unified::UnifiedMessageDelta::from(delta);

                // Fix the index for tool call deltas
                if let Some(ref mut tool_calls) = unified_delta.tool_calls {
                    for tool_call in tool_calls {
                        if let unified::UnifiedStreamingToolCall::Delta { index: call_index, .. } = tool_call {
                            *call_index = index as usize;
                        }
                    }
                }

                Self {
                    id: Cow::Borrowed(""),
                    model: Cow::Borrowed(""),
                    choices: vec![unified::UnifiedChoiceDelta {
                        index: 0,
                        delta: unified_delta,
                        finish_reason: None,
                    }],
                    usage: None,
                    created: 0,
                }
            }
            AnthropicStreamEvent::MessageDelta { delta, usage } => {
                let finish_reason = delta.stop_reason.map(unified::UnifiedFinishReason::from);

                Self {
                    id: Cow::Borrowed(""),
                    model: Cow::Borrowed(""),
                    choices: vec![unified::UnifiedChoiceDelta {
                        index: 0,
                        delta: unified::UnifiedMessageDelta {
                            role: None,
                            content: None,
                            tool_calls: None,
                        },
                        finish_reason,
                    }],
                    usage: Some(unified::UnifiedUsage::from(usage)),
                    created: 0,
                }
            }
            _ => Self {
                id: Cow::Borrowed(""),
                model: Cow::Borrowed(""),
                choices: vec![],
                usage: None,
                created: 0,
            },
        }
    }
}
