use crate::index::ToolIndex;
use crate::tool::{ExecuteTool, RmcpTool, SearchTool};
use config::McpConfig;
use rmcp::{
    RoleServer, ServerHandler,
    model::{
        CallToolRequestParam, CallToolResult, ErrorCode, ErrorData, Implementation, ListToolsResult,
        PaginatedRequestParam, ProtocolVersion, ServerCapabilities, ServerInfo,
    },
    service::RequestContext,
};
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

        let inner = McpServerInner {
            info: ServerInfo {
                protocol_version: ProtocolVersion::V_2025_03_26,
                capabilities: ServerCapabilities::builder().enable_tools().build(),
                server_info: Implementation::from_build_env(),
                instructions: None,
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
