use crate::index::ToolIndex;
use indoc::indoc;
use rmcp::model::{Tool, ToolAnnotations};
use rmcp::serde_json;
use schemars::{JsonSchema, Schema, generate::SchemaSettings, transform::Transform};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Deserialize, JsonSchema)]
pub struct SearchParameters {
    /// A list of keywords to search with.
    pub keywords: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SearchResult {
    /// The name of the tool (format: "server__tool")
    pub name: String,
    /// Description of what the tool does
    pub description: String,
    /// The input schema for the tool's parameters
    pub input_schema: serde_json::Value,
    /// The relevance score for this result (higher is more relevant)
    pub score: f32,
}

// Wrapper type to work around MCP Inspector bug that expects type: "object" at root
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SearchResponse {
    /// The list of search results
    pub results: Vec<SearchResult>,
}

#[derive(Clone)]
pub struct SearchTool {
    /// All available tools (both static and dynamic)
    tools: Vec<Tool>,
    /// Index built from all available tools
    index: Arc<ToolIndex>,
}

impl SearchTool {
    /// Creates a new search tool with all available tools pre-indexed
    pub fn new(mut tools: Vec<Tool>) -> anyhow::Result<Self> {
        log::debug!("Initializing search tool with {} available tools", tools.len());

        // Sort tools by name for binary search
        tools.sort_by(|a, b| a.name.cmp(&b.name));

        let mut index = ToolIndex::new()?;

        // Index all tools - indices now correspond to sorted positions
        for (id, tool) in tools.iter().enumerate() {
            log::debug!("Indexing tool '{}' for search capabilities", tool.name);
            index.add_tool(tool, id.into())?;
        }

        index.commit()?;

        Ok(Self {
            tools,
            index: Arc::new(index),
        })
    }

    /// Find a tool by name using binary search
    pub fn find_exact(&self, tool_name: &str) -> Option<&Tool> {
        self.tools
            .binary_search_by(|tool| tool.name.as_ref().cmp(tool_name))
            .ok()
            .map(|idx| &self.tools[idx])
    }

    /// Find tools by keywords using the index
    pub async fn find_by_keywords(&self, keywords: Vec<String>) -> anyhow::Result<Vec<SearchResult>> {
        log::debug!("Searching for tools matching keywords: [{}]", keywords.join(", "));

        let mut results = Vec::new();

        for result in self.index.search(keywords.iter().map(|s| s.as_str()))? {
            // Get the tool from our local vector using the index
            let tool_idx: usize = result.tool_id.into();

            // Safety check
            if tool_idx >= self.tools.len() {
                log::error!(
                    "Tool index {} exceeds bounds (total tools: {})",
                    tool_idx,
                    self.tools.len()
                );
                continue;
            }

            let tool = &self.tools[tool_idx];

            let search_result = SearchResult {
                name: tool.name.to_string(),
                description: tool
                    .description
                    .as_deref()
                    .unwrap_or("No description available")
                    .to_string(),
                input_schema: serde_json::Value::Object(tool.input_schema.as_ref().clone()),
                score: result.score,
            };

            results.push(search_result);
        }

        Ok(results)
    }
}

/// Transform that removes fields that cause issues with MCP Inspector.
/// This includes format fields (like "float", "double") and meta fields (like "$schema").
#[derive(Clone, Debug)]
struct McpCompatibilityTransform;

impl Transform for McpCompatibilityTransform {
    fn transform(&mut self, schema: &mut Schema) {
        // Remove problematic fields from the schema
        schema.remove("$schema");
        schema.remove("format");

        // Apply recursively to all subschemas
        schemars::transform::transform_subschemas(self, schema);
    }
}

/// Creates schema settings optimized for MCP Inspector compatibility.
fn mcp_schema_settings() -> SchemaSettings {
    let mut settings = SchemaSettings::default();
    // Don't include meta schema to avoid validation issues
    settings.meta_schema = None;
    // Apply our transform to remove problematic fields
    settings.transforms.push(Box::new(McpCompatibilityTransform));
    settings
}

pub fn rmcp_tool() -> Tool {
    // Create a schema generator with MCP-compatible settings
    let settings = mcp_schema_settings();
    let generator = settings.into_generator();

    // Generate input schema as a root schema (not a reference)
    let search_schema_root = generator.into_root_schema_for::<SearchParameters>();
    let search_schema_obj = match search_schema_root.to_value() {
        serde_json::Value::Object(obj) => obj,
        _ => {
            log::warn!("Failed to generate input schema for search tool");
            serde_json::Map::new()
        }
    };

    // Generate output schema as a root schema
    let settings = mcp_schema_settings();
    let generator = settings.into_generator();
    let output_schema_root = generator.into_root_schema_for::<SearchResponse>();
    let output_schema_obj = match output_schema_root.to_value() {
        serde_json::Value::Object(obj) => obj,
        _ => {
            log::warn!("Failed to generate output schema for search tool");
            serde_json::Map::new()
        }
    };

    let description = indoc! {r#"
       Search for relevant tools. A list of matching tools with their\nscore is returned with a map of input fields and their types.

       Using this information, you can call the execute tool with the\nname of the tool you want to execute, and defining the input parameters.

       Tool names are in the format "server__tool" where "server" is the name of the MCP server providing
       the tool.
    "#};

    Tool {
        name: "search".into(),
        description: Some(description.into()),
        input_schema: Arc::new(search_schema_obj),
        output_schema: Some(Arc::new(output_schema_obj)),
        annotations: Some(ToolAnnotations::new().read_only(true)),
        title: None,
        icons: None,
    }
}
