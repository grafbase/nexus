use std::sync::Arc;

use indoc::indoc;
use rmcp::model::{Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, JsonSchema)]
#[schemars(
    description = "Parameters for executing a tool. You must call search if you have trouble finding the right arguments here."
)]
pub struct ExecuteParameters {
    /// The exact name of the tool to execute. This must match the tool name returned by the search function. For example: 'calculator__adder', 'web_search__search', or 'file_reader__read'.
    pub name: String,
    /// The arguments to pass to the tool, as a JSON object. Each tool expects specific arguments - use the search function to discover what arguments each tool requires. For example: {"query": "weather in NYC"} or {"x": 5, "y": 10}.
    pub arguments: serde_json::Map<String, serde_json::Value>,
}

pub fn rmcp_tool() -> Tool {
    let description = indoc! {r#"
       Executes a tool with the given parameters. Before using, you must call the search function to retrieve the tools you need for your task. If you do not know how to call this tool, call search first.

       The tool name and parameters are specified in the request body. The tool name must be a string,
       and the parameters must be a map of strings to JSON values.
    "#};

    let execute_schema = serde_json::to_value(ExecuteParameters::json_schema(&mut Default::default()))
        .unwrap()
        .as_object()
        .unwrap()
        .clone();

    Tool {
        name: "execute".into(),
        description: Some(description.into()),
        input_schema: Arc::new(execute_schema),
        annotations: Some(ToolAnnotations::new().destructive(true).open_world(true)),
    }
}
