use std::{future::Future, pin::Pin};

use crate::TestTool;
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
            output_schema: None,
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
            output_schema: None,
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

/// A file system tool for testing filesystem operations
#[derive(Debug)]
pub struct FileSystemTool;

impl TestTool for FileSystemTool {
    fn tool_definition(&self) -> Tool {
        let mut schema = serde_json::Map::new();
        schema.insert("type".to_string(), json!("object"));

        let properties = json!({
            "path": {
                "type": "string",
                "description": "File or directory path"
            },
            "operation": {
                "type": "string",
                "enum": ["list", "create", "delete", "exists"],
                "description": "Filesystem operation to perform"
            }
        });

        schema.insert("properties".to_string(), json!(properties));
        schema.insert("required".to_string(), json!(["path", "operation"]));

        Tool {
            name: "filesystem".into(),
            description: Some(
                "Manages files and directories with operations like listing, creating, and deleting".into(),
            ),
            input_schema: std::sync::Arc::new(schema),
            output_schema: None,
            annotations: Some(rmcp::model::ToolAnnotations {
                title: Some("File System Manager".into()),
                ..Default::default()
            }),
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

            let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| ErrorData {
                code: rmcp::model::ErrorCode(-32602),
                message: "Missing or invalid parameter 'path'".into(),
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

            // Mock implementation for testing
            let result = match operation {
                "list" => format!("Contents of {path}: file1.txt, file2.txt, directory1/"),
                "create" => format!("Created: {path}"),
                "delete" => format!("Deleted: {path}"),
                "exists" => format!("Path {path} exists: true"),
                _ => {
                    return Err(ErrorData {
                        code: rmcp::model::ErrorCode(-32602),
                        message: "Invalid operation".into(),
                        data: None,
                    });
                }
            };

            Ok(CallToolResult::success(vec![Content::text(result)]))
        })
    }
}
