use std::{borrow::Cow, sync::Arc};

use http::request::Parts;
use itertools::Itertools;
use rmcp::{
    model::{CallToolRequestParam, CallToolResult, ErrorCode, ToolAnnotations},
    serde_json::{Map, Value},
};
use schemars::{JsonSchema, Schema, SchemaGenerator};

use crate::{downstream::Downstream, index::ToolIndex};

use super::Tool;

pub struct ExecuteTool {
    downstream: Arc<Downstream>,
    index: Arc<ToolIndex>,
}

impl ExecuteTool {
    pub fn new(downstream: Arc<Downstream>, index: Arc<ToolIndex>) -> Self {
        Self { downstream, index }
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct ExecuteParameters {
    /// The name of the tool to execute. You find this by calling search first.
    pub name: String,
    /// The arguments to pass to the tool. You find these by calling search first.
    pub arguments: Option<Map<String, Value>>,
}

impl JsonSchema for ExecuteParameters {
    fn schema_name() -> Cow<'static, str> {
        "ExecuteParameters".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        schemars::json_schema!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "The exact name of the tool to execute. This must match the tool name returned by the search function. For example: 'calculator__adder', 'web_search__search', or 'file_reader__read'."
                },
                "arguments": {
                    "type": "object",
                    "description": "The arguments to pass to the tool, as a JSON object. Each tool expects specific arguments - use the search function to discover what arguments each tool requires. For example: {\"query\": \"weather in NYC\"} or {\"x\": 5, \"y\": 10}.",
                    "additionalProperties": true
                }
            },
            "required": ["name", "arguments"]
        })
    }
}

impl Tool for ExecuteTool {
    type Parameters = ExecuteParameters;

    fn name() -> &'static str {
        "execute"
    }

    fn description(&self) -> Cow<'_, str> {
        let description = indoc::indoc! {r#"
            Executes a tool with the given parameters. Before using, you must call the
            search function to retrieve the tools you need for your task. If you do not
            know how to call this tool, call search first.

            The tool name and parameters are specified in the request body. The tool name
            must be a string, and the parameters must be a map of strings to JSON values.
        "#};

        Cow::Borrowed(description)
    }

    fn annotations(&self) -> ToolAnnotations {
        ToolAnnotations::new().destructive(true).open_world(true)
    }

    async fn call(&self, _: Parts, parameters: Self::Parameters) -> anyhow::Result<CallToolResult> {
        let ExecuteParameters { name, arguments } = parameters;

        let param = CallToolRequestParam {
            name: name.clone().into(),
            arguments,
        };

        match self.downstream.execute(param).await {
            Ok(result) => Ok(result),
            Err(mut error_data) => {
                if error_data.code == ErrorCode::METHOD_NOT_FOUND {
                    let did_you_mean = self.index.search([name.as_str()])?;

                    if !did_you_mean.is_empty() {
                        let did_you_mean = did_you_mean
                            .into_iter()
                            .map(|s| &self.downstream[s.tool_id].name)
                            .join(", ");

                        error_data.message = format!("{}. Did you mean: {did_you_mean}", error_data.message).into();
                    }

                    Err(anyhow::Error::new(error_data))
                } else {
                    Err(anyhow::Error::new(error_data))
                }
            }
        }
    }
}
