use crate::index::ToolIndex;
use crate::tool::{ExecuteTool, RmcpTool, SearchTool};
use config::McpConfig;
use indoc::indoc;
use itertools::Itertools;
use rmcp::{
    RoleServer, ServerHandler,
    model::{
        CallToolRequestParam, CallToolResult, ErrorCode, ErrorData, Implementation, ListToolsResult,
        PaginatedRequestParam, ServerCapabilities, ServerInfo,
    },
    service::RequestContext,
};
use std::collections::BTreeMap;
use std::{ops::Deref, sync::Arc};

use crate::downstream::Downstream;

#[derive(Clone)]
pub(crate) struct McpServer(Arc<McpServerInner>);

pub(crate) struct McpServerInner {
    info: ServerInfo,
    tools: Vec<Box<dyn RmcpTool>>,
}

impl Deref for McpServer {
    type Target = McpServerInner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl McpServer {
    pub(crate) async fn new(config: &McpConfig) -> anyhow::Result<Self> {
        let downstream = Downstream::new(config).await?;
        let mut index = ToolIndex::new()?;

        for (id, tool) in downstream.list_tools().enumerate() {
            index.add_tool(tool, id.into())?;
        }

        index.commit()?;

        let index = Arc::new(index);
        let downstream = Arc::new(downstream);

        let server_info = Implementation {
            name: generate_server_name(config),
            version: env!("CARGO_PKG_VERSION").to_string(),
        };

        let inner = McpServerInner {
            info: ServerInfo {
                protocol_version: crate::PROTOCOL_VERSION,
                capabilities: ServerCapabilities::builder().enable_tools().build(),
                server_info,
                instructions: Some(generate_instructions(&downstream)),
            },
            tools: vec![
                Box::new(SearchTool::new(downstream.clone(), index.clone())),
                Box::new(ExecuteTool::new(downstream, index)),
            ],
        };

        Ok(Self(Arc::new(inner)))
    }
}

impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerInfo {
        self.info.clone()
    }

    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParam>, // TODO: do we need to care about pagination?
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            next_cursor: None,
            tools: self.tools.iter().map(|tool| tool.to_tool()).collect(),
        })
    }

    async fn call_tool(
        &self,
        CallToolRequestParam { name, arguments }: CallToolRequestParam,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        if let Some(tool) = self.tools.iter().find(|tool| tool.name() == name) {
            return tool.call(ctx, arguments).await;
        }

        Err(ErrorData::new(
            ErrorCode::INVALID_PARAMS,
            format!("Unknown tool '{name}'"),
            None,
        ))
    }
}

fn generate_server_name(config: &McpConfig) -> String {
    if config.servers.is_empty() {
        "Tool Aggregator".to_string()
    } else {
        let server_names = config.servers.keys().map(|s| s.as_str()).join(", ");
        format!("Tool Aggregator ({server_names})")
    }
}

fn generate_instructions(downstream: &Downstream) -> String {
    let mut servers_info = BTreeMap::new();

    // Group tools by server name
    for tool in downstream.list_tools() {
        if let Some((server_name, tool_name)) = tool.name.split_once("__") {
            let server_tools = servers_info.entry(server_name.to_string()).or_insert_with(Vec::new);
            server_tools.push((tool_name, tool.description.as_deref().unwrap_or("No description")));
        }
    }

    let mut instructions = indoc! {r#"
        This is an MCP server aggregator providing access to many tools through two main functions:
        `search` and `execute`.

        **Instructions:**
        1.  **Search for tools:** To find out what tools are available, use the `search` tool. Provide a
            clear description of your goal as the query. The search will return a list of relevant tools,
            including their exact names and required parameters.
        2.  **Execute a tool:** Once you have found a suitable tool using `search`, call the `execute` tool.
            You must provide the `name` of the tool and its `parameters` exactly as specified in the search results.

        Always use the `search` tool first to discover available tools. Do not guess tool names.

    "#}
    .to_string();

    if !servers_info.is_empty() {
        instructions.push_str("**Available Servers and Tools:**\n\n");

        for (server_name, tools) in servers_info {
            instructions.push_str(&format!("**{server_name}:**\n"));

            for (tool_name, description) in tools {
                instructions.push_str(&format!("- `{server_name}__{tool_name}`: {description}\n"));
            }
            instructions.push('\n');
        }

        instructions
            .push_str("**Note:** When executing tools, use the full name format `server__tool` as shown above.\n");
    } else {
        instructions.push_str("**No downstream servers are currently configured.**\n");
    }

    instructions
}
