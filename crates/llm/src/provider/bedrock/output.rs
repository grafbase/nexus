//! Output type conversions for AWS Bedrock Converse API.
//!
//! This module handles the transformation from AWS Bedrock's Converse API responses
//! to the unified ChatCompletionResponse format.

use aws_sdk_bedrockruntime::{
    operation::converse::ConverseOutput,
    types::{self, ContentBlock, ContentBlockDelta, ConverseStreamOutput, StopReason, ToolResultContentBlock},
};
use serde_json::{self, Value as SerdeValue};
use std::borrow::Cow;

use crate::messages::unified;

fn document_to_serde(doc: &aws_smithy_types::Document) -> SerdeValue {
    use aws_smithy_types::{Document, Number};
    use serde_json::Number as JsonNumber;

    match doc {
        Document::Null => SerdeValue::Null,
        Document::Bool(b) => SerdeValue::Bool(*b),
        Document::Number(n) => match n {
            Number::PosInt(u) => SerdeValue::Number(JsonNumber::from(*u)),
            Number::NegInt(i) => SerdeValue::Number(JsonNumber::from(*i)),
            Number::Float(f) => {
                if let Some(num) = JsonNumber::from_f64(*f) {
                    SerdeValue::Number(num)
                } else {
                    SerdeValue::Null
                }
            }
        },
        Document::String(s) => SerdeValue::String(s.clone()),
        Document::Array(arr) => SerdeValue::Array(arr.iter().map(document_to_serde).collect()),
        Document::Object(obj) => {
            SerdeValue::Object(obj.iter().map(|(k, v)| (k.clone(), document_to_serde(v))).collect())
        }
    }
}

fn convert_tool_result_block(block: &types::ToolResultBlock) -> unified::UnifiedContent {
    let mut pieces = Vec::new();

    for item in block.content() {
        match item {
            ToolResultContentBlock::Text(text) => pieces.push(text.clone()),
            ToolResultContentBlock::Json(doc) => pieces.push(document_to_string(doc)),
            ToolResultContentBlock::Document(_) => {
                pieces.push("[Document tool result]".to_string());
            }
            ToolResultContentBlock::Image(_) => {
                pieces.push("[Image tool result]".to_string());
            }
            ToolResultContentBlock::Video(_) => {
                pieces.push("[Video tool result]".to_string());
            }
            _ => {
                pieces.push("[Unknown tool result]".to_string());
            }
        }
    }

    let content = match pieces.len() {
        0 => unified::UnifiedToolResultContent::Text(String::new()),
        1 => unified::UnifiedToolResultContent::Text(pieces.into_iter().next().unwrap()),
        _ => unified::UnifiedToolResultContent::Multiple(pieces),
    };

    let is_error = block
        .status()
        .map(|status| matches!(status, types::ToolResultStatus::Error));

    unified::UnifiedContent::ToolResult {
        tool_use_id: block.tool_use_id().to_string(),
        content,
        is_error,
    }
}

fn stop_reason_to_unified(reason: StopReason) -> unified::UnifiedFinishReason {
    match reason {
        StopReason::EndTurn => unified::UnifiedFinishReason::Stop,
        StopReason::MaxTokens => unified::UnifiedFinishReason::Length,
        StopReason::StopSequence => unified::UnifiedFinishReason::Stop,
        StopReason::ToolUse => unified::UnifiedFinishReason::ToolCalls,
        StopReason::ContentFiltered | StopReason::GuardrailIntervened => unified::UnifiedFinishReason::ContentFilter,
        _ => {
            log::warn!("Unknown stop reason: {:?}", reason);
            unified::UnifiedFinishReason::Stop
        }
    }
}

impl From<ConverseOutput> for unified::UnifiedResponse {
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
            other => {
                log::debug!("Unexpected output type in Converse response: {:?}", other);
                types::Message::builder()
                    .build()
                    .expect("Empty message should build successfully")
            }
        };

        if message.content().is_empty() {
            log::debug!("Bedrock Converse API returned empty content");
        }

        let mut unified_blocks = Vec::with_capacity(message.content().len());

        for block in message.content() {
            match block {
                ContentBlock::Text(text) => {
                    unified_blocks.push(unified::UnifiedContent::Text { text: text.clone() });
                }
                ContentBlock::ToolUse(tool_use) => {
                    let input = document_to_serde(&tool_use.input);
                    unified_blocks.push(unified::UnifiedContent::ToolUse {
                        id: tool_use.tool_use_id.clone(),
                        name: tool_use.name.clone(),
                        input,
                    });
                }
                ContentBlock::ToolResult(result) => {
                    unified_blocks.push(convert_tool_result_block(result));
                }
                other => {
                    log::warn!("Unexpected content block type in response: {:?}", other);
                }
            }
        }

        let message = unified::UnifiedMessage {
            role: unified::UnifiedRole::Assistant,
            content: unified::UnifiedContentContainer::Blocks(unified_blocks),
            tool_calls: None,
            tool_call_id: None,
        };

        let finish_reason = Some(stop_reason_to_unified(output.stop_reason));

        let usage = output
            .usage
            .map(|usage| unified::UnifiedUsage {
                prompt_tokens: usage.input_tokens as u32,
                completion_tokens: usage.output_tokens as u32,
                total_tokens: usage.total_tokens as u32,
            })
            .unwrap_or(unified::UnifiedUsage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
            });

        unified::UnifiedResponse {
            id: format!("bedrock-{}", uuid::Uuid::new_v4()),
            model: String::new(),
            choices: vec![unified::UnifiedChoice {
                index: 0,
                message,
                finish_reason,
            }],
            usage,
            created: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            stop_reason: None,
            stop_sequence: None,
        }
    }
}

impl TryFrom<ConverseStreamOutput> for unified::UnifiedChunk {
    type Error = ();

    fn try_from(event: ConverseStreamOutput) -> Result<Self, Self::Error> {
        match event {
            ConverseStreamOutput::MessageStart(_) => Ok(unified::UnifiedChunk {
                id: format!("bedrock-{}", uuid::Uuid::new_v4()).into(),
                model: Cow::Borrowed(""),
                created: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                usage: None,
                choices: vec![unified::UnifiedChoiceDelta {
                    index: 0,
                    delta: unified::UnifiedMessageDelta {
                        role: Some(unified::UnifiedRole::Assistant),
                        content: None,
                        tool_calls: None,
                    },
                    finish_reason: None,
                }],
            }),
            ConverseStreamOutput::ContentBlockDelta(block_delta) => {
                let Some(delta) = block_delta.delta() else {
                    return Err(());
                };

                match delta {
                    ContentBlockDelta::Text(text) => Ok(unified::UnifiedChunk {
                        id: format!("bedrock-{}", uuid::Uuid::new_v4()).into(),
                        model: Cow::Borrowed(""),
                        created: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                        usage: None,
                        choices: vec![unified::UnifiedChoiceDelta {
                            index: 0,
                            delta: unified::UnifiedMessageDelta {
                                role: None,
                                content: Some(text.to_string()),
                                tool_calls: None,
                            },
                            finish_reason: None,
                        }],
                    }),
                    ContentBlockDelta::ToolUse(tool_use_delta) => {
                        let tool_call = unified::UnifiedStreamingToolCall::Delta {
                            index: 0,
                            function: unified::UnifiedFunctionDelta {
                                arguments: tool_use_delta.input().to_string(),
                            },
                        };

                        Ok(unified::UnifiedChunk {
                            id: format!("bedrock-{}", uuid::Uuid::new_v4()).into(),
                            model: Cow::Borrowed(""),
                            created: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs(),
                            usage: None,
                            choices: vec![unified::UnifiedChoiceDelta {
                                index: 0,
                                delta: unified::UnifiedMessageDelta {
                                    role: None,

                                    content: None,
                                    tool_calls: Some(vec![tool_call]),
                                },
                                finish_reason: None,
                            }],
                        })
                    }
                    _ => Err(()),
                }
            }
            ConverseStreamOutput::MessageStop(msg_stop) => {
                let finish_reason = Some(stop_reason_to_unified(msg_stop.stop_reason));

                Ok(unified::UnifiedChunk {
                    id: format!("bedrock-{}", uuid::Uuid::new_v4()).into(),
                    model: Cow::Borrowed(""),
                    created: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    usage: None,
                    choices: vec![unified::UnifiedChoiceDelta {
                        index: 0,
                        delta: unified::UnifiedMessageDelta {
                            role: None,
                            content: None,
                            tool_calls: None,
                        },
                        finish_reason,
                    }],
                })
            }
            ConverseStreamOutput::ContentBlockStart(block_start) => {
                // Extract tool call information if this is a tool use block
                let Some(start) = block_start.start() else {
                    return Err(());
                };

                match start {
                    aws_sdk_bedrockruntime::types::ContentBlockStart::ToolUse(tool_use) => {
                        let tool_call = unified::UnifiedStreamingToolCall::Start {
                            index: 0,
                            id: tool_use.tool_use_id().to_string(),
                            function: unified::UnifiedFunctionStart {
                                name: tool_use.name().to_string(),
                                arguments: String::new(), // Arguments come in delta events
                            },
                        };

                        Ok(unified::UnifiedChunk {
                            id: format!("bedrock-{}", uuid::Uuid::new_v4()).into(),
                            model: Cow::Borrowed(""),
                            created: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs(),
                            usage: None,
                            choices: vec![unified::UnifiedChoiceDelta {
                                index: 0,
                                delta: unified::UnifiedMessageDelta {
                                    role: None,
                                    content: None,
                                    tool_calls: Some(vec![tool_call]),
                                },
                                finish_reason: None,
                            }],
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

                Ok(unified::UnifiedChunk {
                    id: format!("bedrock-{}", uuid::Uuid::new_v4()).into(),
                    model: Cow::Borrowed(""),
                    created: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    usage: Some(unified::UnifiedUsage {
                        prompt_tokens: usage.input_tokens as u32,
                        completion_tokens: usage.output_tokens as u32,
                        total_tokens: usage.total_tokens as u32,
                    }),
                    choices: vec![],
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
        aws_smithy_types::Document::Null => "{}".to_string(),
        aws_smithy_types::Document::Bool(b) => b.to_string(),
        aws_smithy_types::Document::Number(n) => match n {
            aws_smithy_types::Number::PosInt(u) => u.to_string(),
            aws_smithy_types::Number::NegInt(i) => i.to_string(),
            aws_smithy_types::Number::Float(f) => {
                if f.is_finite() {
                    // Use JSON-compatible representation for floats
                    // Avoid trailing ".0" for whole numbers by using serde_json number
                    if let Some(num) = serde_json::Number::from_f64(*f) {
                        num.to_string()
                    } else {
                        "0".to_string()
                    }
                } else {
                    "0".to_string()
                }
            }
        },
        aws_smithy_types::Document::String(s) => serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string()),
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
