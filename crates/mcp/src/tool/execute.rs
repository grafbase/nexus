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
    pub name: String,
    pub arguments: Option<Map<String, Value>>,
}

impl JsonSchema for ExecuteParameters {
    fn schema_name() -> Cow<'static, str> {
        "ExecuteParameters".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        schemars::json_schema!({
            "type": "object",
            "properties": {
                "name": generator.subschema_for::<String>(),
                // This is for Cursor, who does not like optional arguments, but will
                // happily send us an empty object.
                "arguments": generator.subschema_for::<Map<String, Value>>()
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
