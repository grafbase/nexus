//! Tool that inspects and returns the headers it receives

use crate::TestTool;
use crate::headers::HeaderRecorder;
use axum::http::request::Parts;
use rmcp::model::{CallToolRequestParam, CallToolResult, Content, ErrorData, Tool};
use rmcp::service::{RequestContext, RoleServer};
use serde_json::json;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

/// A tool that captures headers from incoming requests for testing
#[derive(Debug, Clone)]
pub struct HeaderInspectorTool {
    /// Store the last captured headers
    last_headers: Arc<Mutex<Vec<(String, String)>>>,
}

impl Default for HeaderInspectorTool {
    fn default() -> Self {
        Self::new()
    }
}

impl HeaderInspectorTool {
    pub fn new() -> Self {
        Self {
            last_headers: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Get a header recorder that can be used to inspect headers after the tool is moved
    pub fn header_recorder(&self) -> HeaderRecorder {
        HeaderRecorder::new(self.last_headers.clone())
    }
}

impl TestTool for HeaderInspectorTool {
    fn tool_definition(&self) -> Tool {
        let mut schema = serde_json::Map::new();
        schema.insert("type".to_string(), json!("object"));

        let properties = json!({
            "echo": {
                "type": "boolean",
                "description": "Whether to echo back the headers"
            }
        });

        schema.insert("properties".to_string(), json!(properties));

        Tool {
            name: "header_inspector".into(),
            description: Some("Inspects and returns headers from the request".into()),
            input_schema: std::sync::Arc::new(schema),
            output_schema: None,
            annotations: None,
        }
    }

    fn call(
        &self,
        params: CallToolRequestParam,
        ctx: RequestContext<RoleServer>,
    ) -> Pin<Box<dyn Future<Output = Result<CallToolResult, ErrorData>> + Send + '_>> {
        let headers_storage = self.last_headers.clone();

        Box::pin(async move {
            // Extract headers from the request context
            let mut captured_headers = Vec::new();

            // First check for Nexus-transformed headers in the arguments
            if let Some(args) = &params.arguments
                && let Some(nexus_headers) = args.get("_nexus_transformed_headers")
                && let Some(headers_obj) = nexus_headers.as_object()
            {
                for (name, value) in headers_obj {
                    if let Some(value_str) = value.as_str() {
                        captured_headers.push((name.clone(), value_str.to_string()));
                    }
                }
            }

            // If no transformed headers were found, fall back to original request headers
            if captured_headers.is_empty()
                && let Some(parts) = ctx.extensions.get::<Parts>()
            {
                for (name, value) in &parts.headers {
                    if let Ok(value_str) = value.to_str() {
                        captured_headers.push((name.to_string(), value_str.to_string()));
                    }
                }
            }

            // Store the captured headers
            *headers_storage.lock().unwrap() = captured_headers.clone();

            let echo = params
                .arguments
                .as_ref()
                .and_then(|args| args.get("echo"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);

            if echo {
                let headers_text = if captured_headers.is_empty() {
                    "No headers captured".to_string()
                } else {
                    captured_headers
                        .iter()
                        .map(|(k, v)| format!("{}: {}", k, v))
                        .collect::<Vec<_>>()
                        .join("\n")
                };

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Headers received:\n{}",
                    headers_text
                ))]))
            } else {
                Ok(CallToolResult::success(vec![Content::text("Headers inspected")]))
            }
        })
    }
}
