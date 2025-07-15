use std::{future::Future, pin::Pin};

use integration_tests::*;
use rmcp::model::{CallToolRequestParam, CallToolResult, Content, ErrorData, Tool};
use serde_json::json;

/// A simple test tool that adds two numbers
#[derive(Debug)]
pub struct AdderTool;

impl TestTool for AdderTool {
    fn tool_definition(&self) -> Tool {
        let mut schema = serde_json::Map::new();
        schema.insert("type".to_string(), json!("object"));

        let properties = json!({
            "a": {
                "type": "number",
                "description": "First number to add"
            },
            "b": {
                "type": "number",
                "description": "Second number to add"
            }
        });

        schema.insert("properties".to_string(), json!(properties));
        schema.insert("required".to_string(), json!(["a", "b"]));

        Tool {
            name: "adder".into(),
            description: Some("Adds two numbers together".into()),
            input_schema: std::sync::Arc::new(schema),
            annotations: None,
        }
    }

    fn call(
        &self,
        params: CallToolRequestParam,
    ) -> Pin<Box<dyn Future<Output = Result<CallToolResult, ErrorData>> + Send + '_>> {
        Box::pin(async move {
            let args = params.arguments.ok_or_else(|| ErrorData {
                code: rmcp::model::ErrorCode(-32602),
                message: "Missing arguments".into(),
                data: None,
            })?;

            let a = args.get("a").and_then(|v| v.as_f64()).ok_or_else(|| ErrorData {
                code: rmcp::model::ErrorCode(-32602),
                message: "Missing or invalid parameter 'a'".into(),
                data: None,
            })?;

            let b = args.get("b").and_then(|v| v.as_f64()).ok_or_else(|| ErrorData {
                code: rmcp::model::ErrorCode(-32602),
                message: "Missing or invalid parameter 'b'".into(),
                data: None,
            })?;

            let result = a + b;

            let text = if a.fract() == 0.0 && b.fract() == 0.0 && result.fract() == 0.0 {
                format!("{} + {} = {}", a as i64, b as i64, result as i64)
            } else {
                format!("{a} + {b} = {result}")
            };

            Ok(CallToolResult::success(vec![Content::text(text)]))
        })
    }
}

/// A test tool that always fails with an error
#[derive(Debug)]
pub struct FailingTool;

impl TestTool for FailingTool {
    fn tool_definition(&self) -> Tool {
        let mut schema = serde_json::Map::new();

        schema.insert("type".to_string(), json!("object"));
        schema.insert("properties".to_string(), json!({}));

        Tool {
            name: "failing_tool".into(),
            description: Some("A tool that always fails for testing error handling".into()),
            input_schema: std::sync::Arc::new(schema),
            annotations: None,
        }
    }

    fn call(
        &self,
        _params: CallToolRequestParam,
    ) -> Pin<Box<dyn Future<Output = Result<CallToolResult, ErrorData>> + Send + '_>> {
        Box::pin(async move {
            Err(ErrorData {
                code: rmcp::model::ErrorCode(-32000),
                message: "This tool always fails".into(),
                data: Some(json!({"reason": "intentional_failure"})),
            })
        })
    }
}

// SSE Service Tests
