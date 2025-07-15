use std::sync::Arc;

use config::McpConfig;
use rmcp::{
    RoleServer, ServerHandler,
    model::{
        CallToolRequestParam, CallToolResult, ErrorData, Implementation, ListToolsResult, PaginatedRequestParam,
        ProtocolVersion, ServerCapabilities, ServerInfo,
    },
    service::RequestContext,
};

use crate::downstream::Downstream;

#[derive(Clone)]
pub(crate) struct McpServer(Arc<McpServerInner>);

pub(crate) struct McpServerInner {
    info: ServerInfo,
    downstream: Downstream,
}

impl std::ops::Deref for McpServer {
    type Target = McpServerInner;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl McpServer {
    pub(crate) async fn new(config: &McpConfig) -> anyhow::Result<Self> {
        let downstream = Downstream::new(config).await?;

        let inner = McpServerInner {
            info: ServerInfo {
                protocol_version: ProtocolVersion::V_2024_11_05,
                capabilities: ServerCapabilities::builder().enable_tools().build(),
                server_info: Implementation::from_build_env(),
                instructions: None,
            },
            downstream,
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
        let tools = self.downstream.list_tools().cloned().collect();
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        params: CallToolRequestParam,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        self.downstream.call_tool(params).await
    }
}
