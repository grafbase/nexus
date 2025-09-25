use std::{future::Future, pin::Pin};

use crate::TestTool;
use rmcp::model::{CallToolRequestParam, CallToolResult, Content, ErrorData, Tool};
use rmcp::service::{RequestContext, RoleServer};
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
            output_schema: None,
            annotations: None,
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
