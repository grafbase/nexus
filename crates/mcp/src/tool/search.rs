use std::{borrow::Cow, sync::Arc};

use http::request::Parts;
use indoc::indoc;
use rmcp::model::{CallToolResult, Content, ToolAnnotations};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{downstream::Downstream, index::ToolIndex};

use super::Tool;

#[derive(Deserialize, JsonSchema)]
pub struct SearchParameters {
    /// A list of keywords to search with.
    keywords: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchResult<'a> {
    /// The tool that matched the search query
    pub tool: &'a rmcp::model::Tool,
    /// The relevance score for this result (higher is more relevant)
    pub score: f32,
}

pub struct SearchTool {
    downstream: Arc<Downstream>,
    index: Arc<ToolIndex>,
}

impl SearchTool {
    pub fn new(downstream: Arc<Downstream>, index: Arc<ToolIndex>) -> Self {
        Self { downstream, index }
    }
}

impl Tool for SearchTool {
    type Parameters = SearchParameters;

    fn name() -> &'static str {
        "search"
    }

    fn description(&self) -> Cow<'_, str> {
        let description = indoc! {r#"
            Search for relevant tools. A list of matching tools with their
            score is returned with a map of input fields and their types.

            Using this information, you can call the execute tool with the
            name of the tool you want to execute, and defining the input
            parameters.
        "#};

        Cow::Borrowed(description)
    }

    fn annotations(&self) -> ToolAnnotations {
        ToolAnnotations::new().read_only(true)
    }

    async fn call(&self, _: Parts, parameters: Self::Parameters) -> anyhow::Result<CallToolResult> {
        let SearchParameters { keywords } = parameters;

        let mut content = Vec::new();

        for result in self.index.search(keywords.iter().map(|s| s.as_str()))? {
            let result = Content::json(SearchResult {
                tool: &self.downstream[result.tool_id],
                score: result.score,
            })?;

            content.push(result);
        }

        Ok(CallToolResult {
            content,
            is_error: None,
        })
    }
}
