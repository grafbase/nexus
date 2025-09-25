use std::{future::Future, pin::Pin};

use crate::TestTool;
use rmcp::model::{CallToolRequestParam, CallToolResult, Content, ErrorData, Tool};
use rmcp::service::{RequestContext, RoleServer};
use serde_json::json;

/// A text processing tool
#[derive(Debug)]
pub struct TextProcessorTool;

impl TestTool for TextProcessorTool {
    fn tool_definition(&self) -> Tool {
        let mut schema = serde_json::Map::new();
        schema.insert("type".to_string(), json!("object"));

        let properties = json!({
            "text": {
                "type": "string",
                "description": "Input text to process"
            },
            "action": {
                "type": "string",
                "enum": ["uppercase", "lowercase", "reverse", "word_count"],
                "description": "Action to perform on the text"
            }
        });

        schema.insert("properties".to_string(), json!(properties));
        schema.insert("required".to_string(), json!(["text", "action"]));

        Tool {
            name: "text_processor".into(),
            description: Some(
                "Processes text with various string manipulation operations like case conversion and reversal".into(),
            ),
            input_schema: std::sync::Arc::new(schema),
            output_schema: None,
            annotations: Some(rmcp::model::ToolAnnotations {
                title: Some("Text Processor".into()),
                ..Default::default()
            }),
            title: None,
            icons: None,
        }
    }

    fn call(
        &self,
        params: CallToolRequestParam,
        _ctx: RequestContext<RoleServer>,
    ) -> Pin<Box<dyn Future<Output = Result<CallToolResult, ErrorData>> + Send + '_>> {
        Box::pin(async move {
            let args = params.arguments.ok_or_else(|| ErrorData {
                code: rmcp::model::ErrorCode(-32602),
                message: "Missing arguments".into(),
                data: None,
            })?;

            let text = args.get("text").and_then(|v| v.as_str()).ok_or_else(|| ErrorData {
                code: rmcp::model::ErrorCode(-32602),
                message: "Missing or invalid parameter 'text'".into(),
                data: None,
            })?;

            let action = args.get("action").and_then(|v| v.as_str()).ok_or_else(|| ErrorData {
                code: rmcp::model::ErrorCode(-32602),
                message: "Missing or invalid parameter 'action'".into(),
                data: None,
            })?;

            let result = match action {
                "uppercase" => text.to_uppercase(),
                "lowercase" => text.to_lowercase(),
                "reverse" => text.chars().rev().collect(),
                "word_count" => text.split_whitespace().count().to_string(),
                _ => {
                    return Err(ErrorData {
                        code: rmcp::model::ErrorCode(-32602),
                        message: "Invalid action".into(),
                        data: None,
                    });
                }
            };

            Ok(CallToolResult::success(vec![Content::text(result)]))
        })
    }
}
