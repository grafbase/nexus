use std::{future::Future, pin::Pin};

use crate::TestTool;
use rmcp::model::{CallToolRequestParam, CallToolResult, ErrorData, Tool};
use rmcp::service::{RequestContext, RoleServer};
use serde_json::json;

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
            title: None,
            icons: None,
        }
    }

    fn call(
        &self,
        _params: CallToolRequestParam,
        _ctx: RequestContext<RoleServer>,
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
