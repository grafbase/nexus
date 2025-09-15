//! Output type conversions for AWS Bedrock Converse API.
//!
//! This module handles the transformation from AWS Bedrock's Converse API responses
//! to the unified ChatCompletionResponse format.

use aws_sdk_bedrockruntime::{
    operation::converse::ConverseOutput,
    types::{self, ContentBlock, ContentBlockDelta, ConverseStreamOutput, StopReason},
};

use crate::messages::{
    ChatChoice, ChatChoiceDelta, ChatCompletionChunk, ChatCompletionResponse, ChatMessage, ChatMessageDelta, ChatRole,
    FinishReason, FunctionCall, FunctionDelta, FunctionStart, ObjectType, StreamingToolCall, ToolCall, ToolCallType,
    Usage,
};

/// Convert a Bedrock Converse response to OpenAI format.
impl From<ConverseOutput> for ChatCompletionResponse {
    fn from(output: ConverseOutput) -> Self {
        let converse_output = output.output.unwrap_or_else(|| {
            log::debug!("Missing output in Converse response - using empty message");

            let message = types::Message::builder()
                .build()
                .expect("Empty message should build successfully");

            types::ConverseOutput::Message(message)
        });

        let message = match converse_output {
            types::ConverseOutput::Message(msg) => msg,
            _ => {
                log::debug!("Unexpected output type in Converse response - using empty message");

                types::Message::builder()
                    .build()
                    .expect("Empty message should build successfully")
            }
        };

        let mut content = String::new();
        let mut tool_calls = Vec::new();

        // Debug logging for empty content
        if message.content().is_empty() {
            log::debug!("Bedrock Converse API returned empty content");
        }

        for block in message.content() {
            match block {
                ContentBlock::Text(text) => {
                    if !content.is_empty() {
                        content.push(' ');
                    }
                    content.push_str(text);
                }
                ContentBlock::ToolUse(tool_use) => {
                    tool_calls.push(ToolCall {
                        id: tool_use.tool_use_id.clone(),
                        tool_type: crate::messages::ToolCallType::Function,
                        function: FunctionCall {
                            name: tool_use.name.clone(),
                            arguments: document_to_string(&tool_use.input),
                        },
                    });
                }
                _ => {
                    log::warn!("Unexpected content block type in response");
                }
            }
        }

        let finish_reason = FinishReason::from(output.stop_reason);

        let message = ChatMessage {
            role: ChatRole::Assistant,
            content: if content.is_empty() { None } else { Some(content) },
            tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls) },
            tool_call_id: None,
        };

        let usage = output
            .usage
            .map(|u| Usage {
                prompt_tokens: u.input_tokens as u32,
                completion_tokens: u.output_tokens as u32,
                total_tokens: u.total_tokens as u32,
            })
            .unwrap_or(Usage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
            });

        ChatCompletionResponse {
            id: format!("bedrock-{}", uuid::Uuid::new_v4()),
            object: ObjectType::ChatCompletion,
            created: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            model: String::new(), // Will be set by provider
            choices: vec![ChatChoice {
                index: 0,
                message,
                finish_reason,
            }],
            usage,
        }
    }
}

/// Convert Bedrock StopReason to OpenAI FinishReason.
impl From<StopReason> for FinishReason {
    fn from(reason: StopReason) -> Self {
        match reason {
            StopReason::EndTurn => FinishReason::Stop,
            StopReason::MaxTokens => FinishReason::Length,
            StopReason::StopSequence => FinishReason::Stop,
            StopReason::ToolUse => FinishReason::ToolCalls,
            StopReason::ContentFiltered => FinishReason::ContentFilter,
            StopReason::GuardrailIntervened => FinishReason::ContentFilter,
            _ => {
                log::warn!("Unknown stop reason: {:?}", reason);
                FinishReason::Stop
            }
        }
    }
}

impl TryFrom<ConverseStreamOutput> for ChatCompletionChunk {
    type Error = ();

    fn try_from(event: ConverseStreamOutput) -> Result<Self, Self::Error> {
        match event {
            ConverseStreamOutput::MessageStart(_) => {
                // First chunk with role
                Ok(ChatCompletionChunk {
                    id: format!("bedrock-{}", uuid::Uuid::new_v4()),
                    object: ObjectType::ChatCompletionChunk,
                    created: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    model: String::new(), // Will be set by provider
                    choices: vec![ChatChoiceDelta {
                        index: 0,
                        delta: ChatMessageDelta {
                            role: Some(ChatRole::Assistant),
                            content: None,
                            tool_calls: None,
                            function_call: None,
                        },
                        finish_reason: None,
                        logprobs: None,
                    }],
                    system_fingerprint: None,
                    usage: None,
                })
            }
            ConverseStreamOutput::ContentBlockDelta(block_delta) => {
                let Some(delta) = block_delta.delta() else {
                    return Err(());
                };

                match delta {
                    ContentBlockDelta::Text(text) => {
                        Ok(ChatCompletionChunk {
                            id: format!("bedrock-{}", uuid::Uuid::new_v4()),
                            object: ObjectType::ChatCompletionChunk,
                            created: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs(),
                            model: String::new(), // Will be set by provider
                            choices: vec![ChatChoiceDelta {
                                index: 0,
                                delta: ChatMessageDelta {
                                    role: None,
                                    content: Some(text.to_string()),
                                    tool_calls: None,
                                    function_call: None,
                                },
                                finish_reason: None,
                                logprobs: None,
                            }],
                            system_fingerprint: None,
                            usage: None,
                        })
                    }
                    ContentBlockDelta::ToolUse(tool_use_delta) => {
                        // Handle incremental tool arguments
                        let tool_call = StreamingToolCall::Delta {
                            index: 0,
                            function: FunctionDelta {
                                arguments: tool_use_delta.input().to_string(),
                            },
                        };

                        Ok(ChatCompletionChunk {
                            id: format!("bedrock-{}", uuid::Uuid::new_v4()),
                            object: ObjectType::ChatCompletionChunk,
                            created: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs(),
                            model: String::new(), // Will be set by provider
                            choices: vec![ChatChoiceDelta {
                                index: 0,
                                delta: ChatMessageDelta {
                                    role: None,
                                    content: None,
                                    tool_calls: Some(vec![tool_call]),
                                    function_call: None,
                                },
                                finish_reason: None,
                                logprobs: None,
                            }],
                            system_fingerprint: None,
                            usage: None,
                        })
                    }
                    _ => Err(()),
                }
            }
            ConverseStreamOutput::MessageStop(msg_stop) => {
                // Final chunk with finish reason
                let finish_reason = Some(FinishReason::from(msg_stop.stop_reason));

                Ok(ChatCompletionChunk {
                    id: format!("bedrock-{}", uuid::Uuid::new_v4()),
                    object: ObjectType::ChatCompletionChunk,
                    created: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    model: String::new(), // Will be set by provider
                    choices: vec![ChatChoiceDelta {
                        index: 0,
                        delta: ChatMessageDelta {
                            role: None,
                            content: None,
                            tool_calls: None,
                            function_call: None,
                        },
                        finish_reason,
                        logprobs: None,
                    }],
                    system_fingerprint: None,
                    usage: None,
                })
            }
            ConverseStreamOutput::ContentBlockStart(block_start) => {
                // Extract tool call information if this is a tool use block
                let Some(start) = block_start.start() else {
                    return Err(());
                };

                match start {
                    aws_sdk_bedrockruntime::types::ContentBlockStart::ToolUse(tool_use) => {
                        // Create tool call start chunk
                        let tool_call = StreamingToolCall::Start {
                            index: 0,
                            id: tool_use.tool_use_id().to_string(),
                            r#type: ToolCallType::Function,
                            function: FunctionStart {
                                name: tool_use.name().to_string(),
                                arguments: String::new(), // Arguments come in delta events
                            },
                        };

                        Ok(ChatCompletionChunk {
                            id: format!("bedrock-{}", uuid::Uuid::new_v4()),
                            object: ObjectType::ChatCompletionChunk,
                            created: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs(),
                            model: String::new(), // Will be set by provider
                            choices: vec![ChatChoiceDelta {
                                index: 0,
                                delta: ChatMessageDelta {
                                    role: None,
                                    content: None,
                                    tool_calls: Some(vec![tool_call]),
                                    function_call: None,
                                },
                                finish_reason: None,
                                logprobs: None,
                            }],
                            system_fingerprint: None,
                            usage: None,
                        })
                    }
                    _ => {
                        // Non-tool content blocks (e.g., text) don't need special handling at start
                        Err(())
                    }
                }
            }
            ConverseStreamOutput::ContentBlockStop(_block_stop) => {
                // End of a content block
                // This is informational only, we don't need to send a chunk for this
                Err(())
            }
            ConverseStreamOutput::Metadata(metadata) => {
                let Some(usage) = metadata.usage else {
                    return Err(());
                };

                Ok(ChatCompletionChunk {
                    id: format!("bedrock-{}", uuid::Uuid::new_v4()),
                    object: ObjectType::ChatCompletionChunk,
                    created: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    model: String::new(), // Will be set by provider
                    choices: vec![],
                    system_fingerprint: None,
                    usage: Some(Usage {
                        prompt_tokens: usage.input_tokens as u32,
                        completion_tokens: usage.output_tokens as u32,
                        total_tokens: usage.total_tokens as u32,
                    }),
                })
            }
            _ => {
                // Unknown event type - log the actual variant for debugging
                log::warn!("Unknown Bedrock stream event type: {event:?}");
                Err(())
            }
        }
    }
}

/// Convert aws_smithy_types::Document to string for display.
pub fn document_to_string(doc: &aws_smithy_types::Document) -> String {
    match doc {
        aws_smithy_types::Document::Null => "null".to_string(),
        aws_smithy_types::Document::Bool(b) => b.to_string(),
        aws_smithy_types::Document::Number(n) => format!("{:?}", n),
        aws_smithy_types::Document::String(s) => format!("\"{}\"", s),
        aws_smithy_types::Document::Array(arr) => {
            let items: Vec<String> = arr.iter().map(document_to_string).collect();
            format!("[{}]", items.join(","))
        }
        aws_smithy_types::Document::Object(obj) => {
            let items: Vec<String> = obj
                .iter()
                .map(|(k, v)| format!("\"{}\": {}", k, document_to_string(v)))
                .collect();
            format!("{{{}}}", items.join(","))
        }
    }
}
