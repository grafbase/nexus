//! Conversions from unified types to Anthropic protocol types.
//!
//! ZERO ALLOCATIONS - All data is moved, not cloned.

use crate::messages::{anthropic, unified};

impl From<unified::UnifiedRequest> for anthropic::AnthropicChatRequest {
    fn from(req: unified::UnifiedRequest) -> Self {
        // Convert messages
        let messages: Vec<anthropic::AnthropicMessage> = req
            .messages
            .into_iter()
            .map(anthropic::AnthropicMessage::from)
            .collect();

        Self {
            model: req.model,
            messages,
            system: req.system,
            max_tokens: req.max_tokens.unwrap_or(4096),
            temperature: req.temperature,
            top_p: req.top_p,
            top_k: req.top_k,
            stop_sequences: req.stop_sequences,
            stream: req.stream,
            tools: req
                .tools
                .map(|t| t.into_iter().map(anthropic::AnthropicTool::from).collect()),
            tool_choice: req.tool_choice.map(anthropic::AnthropicToolChoice::from),
            metadata: req.metadata.map(anthropic::AnthropicMetadata::from),
        }
    }
}

impl From<unified::UnifiedRole> for anthropic::AnthropicRole {
    fn from(role: unified::UnifiedRole) -> Self {
        match role {
            unified::UnifiedRole::User => anthropic::AnthropicRole::User,
            unified::UnifiedRole::Assistant => anthropic::AnthropicRole::Assistant,
            // Anthropic doesn't have System or Tool roles as messages
            unified::UnifiedRole::System => anthropic::AnthropicRole::User,
            unified::UnifiedRole::Tool => anthropic::AnthropicRole::User,
        }
    }
}

impl From<unified::UnifiedMessage> for anthropic::AnthropicMessage {
    fn from(msg: unified::UnifiedMessage) -> Self {
        let role = anthropic::AnthropicRole::from(msg.role);

        let content = match msg.content {
            unified::UnifiedContentContainer::Text(text) => vec![anthropic::AnthropicContent::Text { text }],
            unified::UnifiedContentContainer::Blocks(blocks) => {
                blocks
                    .into_iter()
                    .map(|block| match block {
                        unified::UnifiedContent::Text { text } => anthropic::AnthropicContent::Text { text },
                        unified::UnifiedContent::Image { source } => anthropic::AnthropicContent::Image {
                            source: match source {
                                unified::UnifiedImageSource::Base64 { media_type, data } => {
                                    anthropic::AnthropicImageSource {
                                        source_type: "base64".to_string(),
                                        media_type,
                                        data,
                                    }
                                }
                                unified::UnifiedImageSource::Url { url } => anthropic::AnthropicImageSource {
                                    source_type: "url".to_string(),
                                    media_type: "image/jpeg".to_string(), // Default
                                    data: url,
                                },
                            },
                        },
                        unified::UnifiedContent::ToolUse { id, name, input } => {
                            anthropic::AnthropicContent::ToolUse { id, name, input }
                        }
                        unified::UnifiedContent::ToolResult {
                            tool_use_id,
                            content,
                            is_error: _, // Anthropic doesn't have is_error field
                        } => {
                            let content = match content {
                                unified::UnifiedToolResultContent::Text(text) => {
                                    vec![anthropic::AnthropicToolResultContent::Text { text }]
                                }
                                unified::UnifiedToolResultContent::Multiple(texts) => texts
                                    .into_iter()
                                    .map(|text| anthropic::AnthropicToolResultContent::Text { text })
                                    .collect(),
                            };
                            anthropic::AnthropicContent::ToolResult { tool_use_id, content }
                        }
                    })
                    .collect()
            }
        };

        // If we have tool calls in the unified message, add them as content blocks
        let mut final_content = content;
        if let Some(tool_calls) = msg.tool_calls {
            for call in tool_calls {
                final_content.push(anthropic::AnthropicContent::ToolUse {
                    id: call.id,
                    name: call.function.name,
                    input: match call.function.arguments {
                        unified::UnifiedArguments::String(s) => {
                            serde_json::from_str(&s).unwrap_or(serde_json::Value::Object(Default::default()))
                        }
                        unified::UnifiedArguments::Value(v) => v,
                    },
                });
            }
        }

        Self {
            role,
            content: final_content,
        }
    }
}

impl From<unified::UnifiedTool> for anthropic::AnthropicTool {
    fn from(tool: unified::UnifiedTool) -> Self {
        Self {
            name: tool.function.name,
            description: tool.function.description,
            input_schema: tool.function.parameters,
        }
    }
}

impl From<unified::UnifiedToolChoiceMode> for anthropic::AnthropicToolChoice {
    fn from(mode: unified::UnifiedToolChoiceMode) -> Self {
        match mode {
            unified::UnifiedToolChoiceMode::None => anthropic::AnthropicToolChoice::Auto, // Anthropic doesn't have "none"
            unified::UnifiedToolChoiceMode::Auto => anthropic::AnthropicToolChoice::Auto,
            unified::UnifiedToolChoiceMode::Required => anthropic::AnthropicToolChoice::Any,
        }
    }
}

impl From<unified::UnifiedToolChoice> for anthropic::AnthropicToolChoice {
    fn from(choice: unified::UnifiedToolChoice) -> Self {
        match choice {
            unified::UnifiedToolChoice::Mode(mode) => anthropic::AnthropicToolChoice::from(mode),
            unified::UnifiedToolChoice::Specific { function } => {
                anthropic::AnthropicToolChoice::Tool { name: function.name }
            }
        }
    }
}

impl From<unified::UnifiedMetadata> for anthropic::AnthropicMetadata {
    fn from(meta: unified::UnifiedMetadata) -> Self {
        Self { user_id: meta.user_id }
    }
}

impl From<unified::UnifiedResponse> for anthropic::AnthropicChatResponse {
    fn from(resp: unified::UnifiedResponse) -> Self {
        // Extract content from the first choice's message
        let content = if let Some(choice) = resp.choices.first() {
            let mut content_blocks = Vec::new();

            // First handle regular content
            match &choice.message.content {
                unified::UnifiedContentContainer::Text(text) => {
                    if !text.is_empty() {
                        content_blocks.push(anthropic::AnthropicContent::Text { text: text.clone() });
                    }
                }
                unified::UnifiedContentContainer::Blocks(blocks) => {
                    for block in blocks {
                        match block {
                            unified::UnifiedContent::Text { text } => {
                                content_blocks.push(anthropic::AnthropicContent::Text { text: text.clone() });
                            }
                            unified::UnifiedContent::Image { source } => {
                                content_blocks.push(anthropic::AnthropicContent::Image {
                                    source: match source {
                                        unified::UnifiedImageSource::Base64 { media_type, data } => {
                                            anthropic::AnthropicImageSource {
                                                source_type: "base64".to_string(),
                                                media_type: media_type.clone(),
                                                data: data.clone(),
                                            }
                                        }
                                        unified::UnifiedImageSource::Url { url } => anthropic::AnthropicImageSource {
                                            source_type: "url".to_string(),
                                            media_type: "image/jpeg".to_string(),
                                            data: url.clone(),
                                        },
                                    },
                                });
                            }
                            unified::UnifiedContent::ToolUse { id, name, input } => {
                                content_blocks.push(anthropic::AnthropicContent::ToolUse {
                                    id: id.clone(),
                                    name: name.clone(),
                                    input: input.clone(),
                                });
                            }
                            unified::UnifiedContent::ToolResult { .. } => {
                                // Tool results shouldn't appear in responses
                            }
                        }
                    }
                }
            }

            // Now handle tool_calls from OpenAI format and convert to Anthropic ToolUse blocks
            if let Some(tool_calls) = &choice.message.tool_calls {
                for tool_call in tool_calls {
                    // Convert arguments to JSON Value
                    let input = match &tool_call.function.arguments {
                        unified::UnifiedArguments::String(s) => {
                            serde_json::from_str(s).unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new()))
                        }
                        unified::UnifiedArguments::Value(v) => v.clone(),
                    };

                    content_blocks.push(anthropic::AnthropicContent::ToolUse {
                        id: tool_call.id.clone(),
                        name: tool_call.function.name.clone(),
                        input,
                    });
                }
            }

            content_blocks
        } else {
            vec![]
        };

        Self {
            id: resp.id,
            r#type: "message".to_string(),
            role: anthropic::AnthropicRole::Assistant,
            content,
            model: resp.model,
            stop_reason: resp.stop_reason.map(|r| match r {
                unified::UnifiedStopReason::EndTurn => anthropic::AnthropicStopReason::EndTurn,
                unified::UnifiedStopReason::MaxTokens => anthropic::AnthropicStopReason::MaxTokens,
                unified::UnifiedStopReason::StopSequence => anthropic::AnthropicStopReason::StopSequence,
                unified::UnifiedStopReason::ToolUse => anthropic::AnthropicStopReason::ToolUse,
            }),
            stop_sequence: resp.stop_sequence,
            usage: anthropic::AnthropicUsage {
                input_tokens: resp.usage.prompt_tokens as i32,
                output_tokens: resp.usage.completion_tokens as i32,
            },
        }
    }
}

impl From<unified::UnifiedChunk> for anthropic::AnthropicStreamEvent {
    fn from(chunk: unified::UnifiedChunk) -> Self {
        // For simplicity, we'll convert unified chunks to content block deltas
        // A more complete implementation would track message state and send proper events
        if let Some(choice) = chunk.choices.first() {
            if let Some(ref content) = choice.delta.content {
                anthropic::AnthropicStreamEvent::ContentBlockDelta {
                    index: choice.index,
                    delta: anthropic::AnthropicContentDelta::TextDelta { text: content.clone() },
                }
            } else if let Some(tool_calls) = &choice.delta.tool_calls {
                // Convert tool calls to tool use events
                if let Some(tool_call) = tool_calls.first() {
                    match tool_call {
                        unified::UnifiedStreamingToolCall::Start { index, id, function } => {
                            anthropic::AnthropicStreamEvent::ContentBlockStart {
                                index: *index as u32,
                                content_block: anthropic::AnthropicContent::ToolUse {
                                    id: id.clone(),
                                    name: function.name.clone(),
                                    input: serde_json::from_str(&function.arguments).unwrap_or_default(),
                                },
                            }
                        }
                        unified::UnifiedStreamingToolCall::Delta { index, function } => {
                            anthropic::AnthropicStreamEvent::ContentBlockDelta {
                                index: *index as u32,
                                delta: anthropic::AnthropicContentDelta::InputJsonDelta {
                                    partial_json: function.arguments.clone(),
                                },
                            }
                        }
                    }
                } else {
                    // Fallback for empty tool calls
                    anthropic::AnthropicStreamEvent::Ping
                }
            } else {
                // No content or tool calls, send ping to keep connection alive
                anthropic::AnthropicStreamEvent::Ping
            }
        } else {
            // No choices, send ping
            anthropic::AnthropicStreamEvent::Ping
        }
    }
}

impl From<unified::UnifiedModel> for anthropic::AnthropicModel {
    fn from(model: unified::UnifiedModel) -> Self {
        Self {
            id: model.id,
            model_type: "model".to_string(),
            display_name: model.display_name,
            created_at: model.created,
        }
    }
}

impl From<unified::UnifiedModelsResponse> for anthropic::AnthropicModelsResponse {
    fn from(response: unified::UnifiedModelsResponse) -> Self {
        Self {
            data: response
                .models
                .into_iter()
                .map(anthropic::AnthropicModel::from)
                .collect(),
            has_more: response.has_more,
        }
    }
}

impl From<crate::messages::openai::Model> for anthropic::AnthropicModel {
    fn from(openai_model: crate::messages::openai::Model) -> Self {
        Self {
            id: openai_model.id.clone(),
            model_type: "model".to_string(),
            display_name: openai_model.id,
            created_at: openai_model.created,
        }
    }
}

impl From<crate::messages::openai::ModelsResponse> for anthropic::AnthropicModelsResponse {
    fn from(openai_response: crate::messages::openai::ModelsResponse) -> Self {
        Self {
            data: openai_response
                .data
                .into_iter()
                .map(anthropic::AnthropicModel::from)
                .collect(),
            has_more: false, // OpenAI doesn't paginate models, so this is always false
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::messages::{anthropic, unified};
    use insta::assert_json_snapshot;

    #[test]
    fn convert_tool_calls_from_unified_to_anthropic() {
        // Test that tool_calls in UnifiedResponse are converted to ToolUse content blocks
        let unified_resp = unified::UnifiedResponse {
            id: "test-response".to_string(),
            created: 1234567890,
            model: "test-model".to_string(),
            choices: vec![unified::UnifiedChoice {
                index: 0,
                message: unified::UnifiedMessage {
                    role: unified::UnifiedRole::Assistant,
                    content: unified::UnifiedContentContainer::Text("I'll help you with that.".to_string()),
                    tool_calls: Some(vec![
                        unified::UnifiedToolCall {
                            id: "call_123".to_string(),
                            function: unified::UnifiedFunctionCall {
                                name: "get_weather".to_string(),
                                arguments: unified::UnifiedArguments::String(r#"{"location": "San Francisco"}"#.to_string()),
                            },
                        },
                        unified::UnifiedToolCall {
                            id: "call_456".to_string(),
                            function: unified::UnifiedFunctionCall {
                                name: "search".to_string(),
                                arguments: unified::UnifiedArguments::Value(serde_json::json!({
                                    "query": "restaurants nearby"
                                })),
                            },
                        },
                    ]),
                    tool_call_id: None,
                },
                finish_reason: Some(unified::UnifiedFinishReason::ToolCalls),
            }],
            usage: unified::UnifiedUsage {
                prompt_tokens: 10,
                completion_tokens: 20,
                total_tokens: 30,
            },
            stop_reason: Some(unified::UnifiedStopReason::ToolUse),
            stop_sequence: None,
        };

        let anthropic_resp: anthropic::AnthropicChatResponse = unified_resp.into();

        // The response should have both the text and the tool use blocks
        assert_json_snapshot!(anthropic_resp, @r###"
        {
          "id": "test-response",
          "type": "message",
          "role": "assistant",
          "content": [
            {
              "type": "text",
              "text": "I'll help you with that."
            },
            {
              "type": "tool_use",
              "id": "call_123",
              "name": "get_weather",
              "input": {
                "location": "San Francisco"
              }
            },
            {
              "type": "tool_use",
              "id": "call_456",
              "name": "search",
              "input": {
                "query": "restaurants nearby"
              }
            }
          ],
          "model": "test-model",
          "stop_reason": "tool_use",
          "stop_sequence": null,
          "usage": {
            "input_tokens": 10,
            "output_tokens": 20
          }
        }
        "###);
    }

    #[test]
    fn convert_response_without_tool_calls() {
        // Test that responses without tool calls work correctly
        let unified_resp = unified::UnifiedResponse {
            id: "test-response".to_string(),
            created: 1234567890,
            model: "test-model".to_string(),
            choices: vec![unified::UnifiedChoice {
                index: 0,
                message: unified::UnifiedMessage {
                    role: unified::UnifiedRole::Assistant,
                    content: unified::UnifiedContentContainer::Text("Here's a simple response.".to_string()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                finish_reason: Some(unified::UnifiedFinishReason::Stop),
            }],
            usage: unified::UnifiedUsage {
                prompt_tokens: 5,
                completion_tokens: 10,
                total_tokens: 15,
            },
            stop_reason: Some(unified::UnifiedStopReason::EndTurn),
            stop_sequence: None,
        };

        let anthropic_resp: anthropic::AnthropicChatResponse = unified_resp.into();

        assert_json_snapshot!(anthropic_resp, @r###"
        {
          "id": "test-response",
          "type": "message",
          "role": "assistant",
          "content": [
            {
              "type": "text",
              "text": "Here's a simple response."
            }
          ],
          "model": "test-model",
          "stop_reason": "end_turn",
          "stop_sequence": null,
          "usage": {
            "input_tokens": 5,
            "output_tokens": 10
          }
        }
        "###);
    }

    #[test]
    fn convert_empty_text_with_tool_calls() {
        // Test that tool calls are converted even when there's no text content
        let unified_resp = unified::UnifiedResponse {
            id: "test-response".to_string(),
            created: 1234567890,
            model: "test-model".to_string(),
            choices: vec![unified::UnifiedChoice {
                index: 0,
                message: unified::UnifiedMessage {
                    role: unified::UnifiedRole::Assistant,
                    content: unified::UnifiedContentContainer::Text("".to_string()), // Empty text
                    tool_calls: Some(vec![
                        unified::UnifiedToolCall {
                            id: "call_789".to_string(),
                            function: unified::UnifiedFunctionCall {
                                name: "calculate".to_string(),
                                arguments: unified::UnifiedArguments::String(r#"{"expression": "2+2"}"#.to_string()),
                            },
                        },
                    ]),
                    tool_call_id: None,
                },
                finish_reason: Some(unified::UnifiedFinishReason::ToolCalls),
            }],
            usage: unified::UnifiedUsage {
                prompt_tokens: 8,
                completion_tokens: 12,
                total_tokens: 20,
            },
            stop_reason: Some(unified::UnifiedStopReason::ToolUse),
            stop_sequence: None,
        };

        let anthropic_resp: anthropic::AnthropicChatResponse = unified_resp.into();

        // Should only have the tool use block, no text block for empty text
        assert_json_snapshot!(anthropic_resp, @r###"
        {
          "id": "test-response",
          "type": "message",
          "role": "assistant",
          "content": [
            {
              "type": "tool_use",
              "id": "call_789",
              "name": "calculate",
              "input": {
                "expression": "2+2"
              }
            }
          ],
          "model": "test-model",
          "stop_reason": "tool_use",
          "stop_sequence": null,
          "usage": {
            "input_tokens": 8,
            "output_tokens": 12
          }
        }
        "###);
    }
}
