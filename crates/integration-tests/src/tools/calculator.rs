use std::{future::Future, pin::Pin};

use crate::TestTool;
use rmcp::model::{CallToolRequestParam, CallToolResult, Content, ErrorData, Tool};
use rmcp::service::{RequestContext, RoleServer};
use serde_json::json;

/// A calculator tool with various mathematical operations
#[derive(Debug)]
pub struct CalculatorTool;

impl TestTool for CalculatorTool {
    fn tool_definition(&self) -> Tool {
        let mut schema = serde_json::Map::new();
        schema.insert("type".to_string(), json!("object"));

        let properties = json!({
            "operation": {
                "type": "string",
                "enum": ["add", "subtract", "multiply", "divide"],
                "description": "Mathematical operation to perform"
            },
            "x": {
                "type": "number",
                "description": "First operand"
            },
            "y": {
                "type": "number",
                "description": "Second operand"
            }
        });

        schema.insert("properties".to_string(), json!(properties));
        schema.insert("required".to_string(), json!(["operation", "x", "y"]));

        Tool {
            name: "calculator".into(),
            description: Some(
                "Performs basic mathematical calculations including addition, subtraction, multiplication and division"
                    .into(),
            ),
            input_schema: std::sync::Arc::new(schema),
            output_schema: None,
            annotations: Some(rmcp::model::ToolAnnotations {
                title: Some("Scientific Calculator".into()),
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

            let operation = args
                .get("operation")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ErrorData {
                    code: rmcp::model::ErrorCode(-32602),
                    message: "Missing or invalid parameter 'operation'".into(),
                    data: None,
                })?;

            let x = args.get("x").and_then(|v| v.as_f64()).ok_or_else(|| ErrorData {
                code: rmcp::model::ErrorCode(-32602),
                message: "Missing or invalid parameter 'x'".into(),
                data: None,
            })?;

            let y = args.get("y").and_then(|v| v.as_f64()).ok_or_else(|| ErrorData {
                code: rmcp::model::ErrorCode(-32602),
                message: "Missing or invalid parameter 'y'".into(),
                data: None,
            })?;

            let result = match operation {
                "add" => x + y,
                "subtract" => x - y,
                "multiply" => x * y,
                "divide" => {
                    if y == 0.0 {
                        return Err(ErrorData {
                            code: rmcp::model::ErrorCode(-32000),
                            message: "Division by zero".into(),
                            data: None,
                        });
                    }
                    x / y
                }
                _ => {
                    return Err(ErrorData {
                        code: rmcp::model::ErrorCode(-32602),
                        message: "Invalid operation".into(),
                        data: None,
                    });
                }
            };

            let text = format!("{x} {operation} {y} = {result}");
            Ok(CallToolResult::success(vec![Content::text(text)]))
        })
    }
}
