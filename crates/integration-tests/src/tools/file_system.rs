use std::{future::Future, pin::Pin};

use crate::TestTool;
use rmcp::model::{CallToolRequestParam, CallToolResult, Content, ErrorData, Tool};
use rmcp::service::{RequestContext, RoleServer};
use serde_json::json;

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
