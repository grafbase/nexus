//! Conversions from unified types to Anthropic protocol types.

use serde_json::Value;

use crate::messages::{anthropic, openai, unified};

impl From<unified::UnifiedRequest> for anthropic::AnthropicChatRequest {
    fn from(req: unified::UnifiedRequest) -> Self {
        // Convert messages
        let messages: Vec<anthropic::AnthropicMessage> = req
            .messages
            .into_iter()
            .map(anthropic::AnthropicMessage::from)
            .collect();

        let tools = req
            .tools
            .map(|t| t.into_iter().map(anthropic::AnthropicTool::from).collect());

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
            tools,
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

impl From<unified::UnifiedContent> for anthropic::AnthropicContent {
    fn from(content: unified::UnifiedContent) -> Self {
        match content {
            unified::UnifiedContent::Text { text } => anthropic::AnthropicContent::Text { text },
            unified::UnifiedContent::Image { source } => anthropic::AnthropicContent::Image {
                source: anthropic::AnthropicImageSource::from(source),
            },
            unified::UnifiedContent::ToolUse { id, name, input } => {
                anthropic::AnthropicContent::ToolUse { id, name, input }
            }
            unified::UnifiedContent::ToolResult {
                tool_use_id,
                content,
                is_error: _, // Anthropic doesn't have is_error field
            } => anthropic::AnthropicContent::ToolResult {
                tool_use_id,
                content: Vec::<anthropic::AnthropicToolResultContent>::from(content),
            },
        }
    }
}

impl From<unified::UnifiedMessage> for anthropic::AnthropicMessage {
    fn from(msg: unified::UnifiedMessage) -> Self {
        let role = anthropic::AnthropicRole::from(msg.role);

        let content = match msg.content {
            unified::UnifiedContentContainer::Text(text) => vec![anthropic::AnthropicContent::Text { text }],
            unified::UnifiedContentContainer::Blocks(blocks) => {
                blocks.into_iter().map(anthropic::AnthropicContent::from).collect()
            }
        };

        // Note: We don't add tool_calls here to avoid duplication.
        // For Anthropic, tool calls should already be present as ToolUse blocks in the content.
        // The tool_calls field is primarily for OpenAI compatibility and should be computed on-demand.

        Self { role, content }
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
        let content = resp
            .choices
            .into_iter()
            .next()
            .map(|choice| build_content_blocks(choice.message))
            .unwrap_or_default();

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

fn build_content_blocks(message: unified::UnifiedMessage) -> Vec<anthropic::AnthropicContent> {
    let mut content_blocks = Vec::new();

    // Handle regular content
    match message.content {
        unified::UnifiedContentContainer::Text(text) if !text.is_empty() => {
            content_blocks.push(anthropic::AnthropicContent::Text { text });
        }
        unified::UnifiedContentContainer::Blocks(blocks) => {
            content_blocks.extend(blocks.into_iter().filter_map(|block| match block {
                unified::UnifiedContent::Text { text } => Some(anthropic::AnthropicContent::Text { text }),
                unified::UnifiedContent::Image { source } => Some(anthropic::AnthropicContent::Image {
                    source: anthropic::AnthropicImageSource::from(source),
                }),
                unified::UnifiedContent::ToolUse { id, name, input } => {
                    Some(anthropic::AnthropicContent::ToolUse { id, name, input })
                }
                unified::UnifiedContent::ToolResult { .. } => None, // Tool results shouldn't appear in responses
            }));
        }
        _ => {}
    }

    // Handle tool_calls from OpenAI format and convert to Anthropic ToolUse blocks
    if let Some(tool_calls) = message.tool_calls {
        for tool_call in tool_calls {
            let input = normalize_tool_input(Value::from(tool_call.function.arguments));
            content_blocks.push(anthropic::AnthropicContent::ToolUse {
                id: tool_call.id,
                name: tool_call.function.name,
                input,
            });
        }
    }

    content_blocks
}

fn normalize_tool_input(input: Value) -> Value {
    if input.is_null() {
        Value::Object(serde_json::Map::new())
    } else {
        input
    }
}

impl From<unified::UnifiedStreamingToolCall> for anthropic::AnthropicStreamEvent {
    fn from(value: unified::UnifiedStreamingToolCall) -> Self {
        match value {
            unified::UnifiedStreamingToolCall::Start { index, id, function } => {
                let input = normalize_tool_input(parse_argument_string(&function.arguments));
                anthropic::AnthropicStreamEvent::ContentBlockStart {
                    index: index as u32,
                    content_block: anthropic::AnthropicContent::ToolUse {
                        id,
                        name: function.name,
                        input,
                    },
                }
            }
            unified::UnifiedStreamingToolCall::Delta { index, function } => {
                anthropic::AnthropicStreamEvent::ContentBlockDelta {
                    index: index as u32,
                    delta: anthropic::AnthropicContentDelta::InputJsonDelta {
                        partial_json: function.arguments,
                    },
                }
            }
        }
    }
}

impl From<unified::UnifiedChunk> for anthropic::AnthropicStreamEvent {
    fn from(chunk: unified::UnifiedChunk) -> Self {
        let Some(choice) = chunk.choices.into_iter().next() else {
            return anthropic::AnthropicStreamEvent::Ping;
        };

        // Handle text content
        if let Some(content) = choice.delta.content {
            return anthropic::AnthropicStreamEvent::ContentBlockDelta {
                index: choice.index,
                delta: anthropic::AnthropicContentDelta::TextDelta { text: content },
            };
        }

        // Handle tool calls
        if let Some(tool_calls) = choice.delta.tool_calls
            && let Some(tool_call) = tool_calls.into_iter().next()
        {
            return anthropic::AnthropicStreamEvent::from(tool_call);
        }

        // No content or tool calls, send ping
        anthropic::AnthropicStreamEvent::Ping
    }
}

fn parse_argument_string(raw: &str) -> Value {
    // Handle empty string case - return empty object instead of trying to parse
    if raw.is_empty() {
        return Value::Object(serde_json::Map::new());
    }

    match serde_json::from_str(raw) {
        Ok(value) => value,
        Err(_) => {
            // For non-empty invalid JSON, keep as string
            Value::String(raw.to_string())
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

impl From<openai::Model> for anthropic::AnthropicModel {
    fn from(openai_model: openai::Model) -> Self {
        let display_name = openai_model.id.clone();

        Self {
            id: openai_model.id,
            model_type: "model".to_string(),
            display_name,
            created_at: openai_model.created,
        }
    }
}

impl From<openai::ModelsResponse> for anthropic::AnthropicModelsResponse {
    fn from(openai_response: openai::ModelsResponse) -> Self {
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

impl From<unified::UnifiedImageSource> for anthropic::AnthropicImageSource {
    fn from(source: unified::UnifiedImageSource) -> Self {
        match source {
            unified::UnifiedImageSource::Base64 { media_type, data } => Self {
                source_type: "base64".to_string(),
                media_type,
                data,
            },
            unified::UnifiedImageSource::Url { url } => Self {
                source_type: "url".to_string(),
                media_type: "image/jpeg".to_string(), // Default
                data: url,
            },
        }
    }
}

impl From<unified::UnifiedToolResultContent> for Vec<anthropic::AnthropicToolResultContent> {
    fn from(content: unified::UnifiedToolResultContent) -> Self {
        match content {
            unified::UnifiedToolResultContent::Text(text) => {
                vec![anthropic::AnthropicToolResultContent::Text { text }]
            }
            unified::UnifiedToolResultContent::Multiple(texts) => texts
                .into_iter()
                .map(|text| anthropic::AnthropicToolResultContent::Text { text })
                .collect(),
        }
    }
}

impl From<unified::UnifiedArguments> for Value {
    fn from(args: unified::UnifiedArguments) -> Self {
        match args {
            unified::UnifiedArguments::String(s) => serde_json::from_str(&s).unwrap_or(Value::Null),
            unified::UnifiedArguments::Value(v) => v,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::messages::{anthropic, unified};
    use insta::assert_json_snapshot;
    use serde_json::{Value, json};

    #[test]
    fn parse_argument_string_handles_invalid_json() {
        let raw = r#"{"command": "echo "hello""}"#;
        let value = super::parse_argument_string(raw);
        assert!(matches!(value, Value::String(s) if s == raw));
    }

    #[test]
    fn parse_argument_string_handles_empty_string() {
        let value = super::parse_argument_string("");
        assert!(value.is_object());
        assert_eq!(value, json!({}));
    }

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
                                arguments: unified::UnifiedArguments::String(
                                    r#"{"location": "San Francisco"}"#.to_string(),
                                ),
                            },
                        },
                        unified::UnifiedToolCall {
                            id: "call_456".to_string(),
                            function: unified::UnifiedFunctionCall {
                                name: "search".to_string(),
                                arguments: unified::UnifiedArguments::Value(json!({
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
        assert_json_snapshot!(anthropic_resp, @r#"
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
        "#);
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
                    tool_calls: Some(vec![unified::UnifiedToolCall {
                        id: "call_789".to_string(),
                        function: unified::UnifiedFunctionCall {
                            name: "calculate".to_string(),
                            arguments: unified::UnifiedArguments::String(r#"{"expression": "2+2"}"#.to_string()),
                        },
                    }]),
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

    #[test]
    fn no_duplicate_tool_calls_when_both_content_and_tool_calls_present() {
        // Test that we don't create duplicate tool_use blocks when the unified message
        // has both ToolUse content blocks AND a tool_calls field with the same tool call.
        // This was causing "tool_use ids must be unique" errors with Anthropic.
        let unified_message = unified::UnifiedMessage {
            role: unified::UnifiedRole::Assistant,
            content: unified::UnifiedContentContainer::Blocks(vec![
                unified::UnifiedContent::Text {
                    text: "I'll calculate that for you.".to_string(),
                },
                unified::UnifiedContent::ToolUse {
                    id: "tool_123".to_string(),
                    name: "calculator".to_string(),
                    input: serde_json::json!({"expression": "2+2"}),
                },
            ]),
            tool_calls: Some(vec![unified::UnifiedToolCall {
                id: "tool_123".to_string(), // Same ID as in content blocks
                function: unified::UnifiedFunctionCall {
                    name: "calculator".to_string(),
                    arguments: unified::UnifiedArguments::Value(serde_json::json!({"expression": "2+2"})),
                },
            }]),
            tool_call_id: None,
        };

        // Convert to Anthropic format
        let anthropic_message: anthropic::AnthropicMessage = unified_message.into();

        // Verify we only have one tool_use block, not two
        let tool_use_blocks: Vec<_> = anthropic_message
            .content
            .iter()
            .filter_map(|block| match block {
                anthropic::AnthropicContent::ToolUse { id, name, .. } => Some((id, name)),
                _ => None,
            })
            .collect();

        // Should only have one tool_use block with ID "tool_123"
        assert_eq!(
            tool_use_blocks.len(),
            1,
            "Should only have one tool_use block, not duplicates"
        );
        assert_eq!(tool_use_blocks[0].0, "tool_123");
        assert_eq!(tool_use_blocks[0].1, "calculator");

        // Verify the full structure matches expectations
        insta::assert_json_snapshot!(anthropic_message, @r###"
        {
          "role": "assistant",
          "content": [
            {
              "type": "text",
              "text": "I'll calculate that for you."
            },
            {
              "type": "tool_use",
              "id": "tool_123",
              "name": "calculator",
              "input": {
                "expression": "2+2"
              }
            }
          ]
        }
        "###);
    }
}
