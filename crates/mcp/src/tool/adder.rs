//! A simple adder tool just for demonstration. Let's remove this when we get actual tools.

use std::borrow::Cow;

use http::HeaderMap;
use rmcp::{
    model::{CallToolResult, Content, ToolAnnotations},
    schemars::JsonSchema,
};

use super::Tool;

#[derive(Debug, Clone, PartialEq, Eq, JsonSchema, serde::Deserialize)]
pub struct Request {
    pub a: i32,
    pub b: i32,
}

pub struct Adder;

impl Tool for Adder {
    type Parameters = Request;

    fn name() -> &'static str {
        "adder"
    }

    fn description(&self) -> Cow<'_, str> {
        "adds a and b together".into()
    }

    fn annotations(&self) -> ToolAnnotations {
        ToolAnnotations::new()
            .idempotent(true)
            .read_only(true)
            .destructive(false)
            .open_world(false)
    }

    async fn call(&self, Request { a, b }: Self::Parameters, _: Option<HeaderMap>) -> anyhow::Result<CallToolResult> {
        let content = Content::text(format!("{a} + {b} = {}", a + b));

        Ok(CallToolResult::success(vec![content]))
    }
}
