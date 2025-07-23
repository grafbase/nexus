mod client;
mod ids;

pub use ids::ToolId;

use client::DownstreamClient;
use futures_util::stream::{FuturesUnordered, StreamExt};
use rmcp::model::{CallToolRequestMethod, CallToolRequestParam, CallToolResult, ErrorData, Tool};
use std::borrow::Cow;

/// Represents an MCP server, providing access to multiple downstream servers.
pub struct Downstream {
    /// List of downstream servers managed by this instance.
    ///
    /// Must be sorted by the server name.
    servers: Vec<DownstreamClient>,
    /// Aggregated tools from all downstream servers.
    ///
    /// Must be sorted by the tool name.
    tools: Vec<Tool>,
}

impl Downstream {
    /// Creates a new Downstream instance from the given configuration.
    ///
    /// This method initializes all configured downstream servers and aggregates
    /// their tools, prefixing each tool name with the server name followed by "__".
    /// Server initialization and tool listing happens concurrently for better performance.
    pub async fn new(config: &config::McpConfig) -> anyhow::Result<Self> {
        // Create futures for initializing each server concurrently
        let mut server_futures = FuturesUnordered::new();

        for (name, server_config) in &config.servers {
            let name = name.clone();
            let server_config = server_config.clone();

            server_futures.push(async move {
                let server = DownstreamClient::new(&name, &server_config).await?;
                let tools = server.list_tools().await?;
                Ok::<_, anyhow::Error>((server, tools))
            });
        }

        // Collect results as they complete
        let mut servers = Vec::new();
        let mut tools = Vec::new();

        while let Some(result) = server_futures.next().await {
            let (server, server_tools) = result?;

            for mut tool in server_tools {
                log::debug!("creating tool {}", tool.name);
                tool.name = Cow::Owned(format!("{}__{}", server.name(), tool.name));
                tools.push(tool);
            }

            servers.push(server);
        }

        servers.sort_unstable_by(|a, b| a.name().cmp(b.name()));
        tools.sort_unstable_by(|a, b| a.name.cmp(&b.name));

        Ok(Self { servers, tools })
    }

    /// Returns an iterator over all available tools from downstream servers.
    ///
    /// Each tool name is prefixed with its server name followed by "__" to ensure
    /// uniqueness across multiple servers. The iterator yields tools in sorted order
    /// by their prefixed names.
    pub fn list_tools(&self) -> impl ExactSizeIterator<Item = &Tool> {
        self.tools.iter()
    }

    /// Calls a tool on the appropriate downstream server.
    ///
    /// The tool name should be in the format "server_name__tool_name".
    /// This method will parse the server name, find the appropriate server,
    /// and forward the call with the original tool name.
    pub async fn execute(&self, mut params: CallToolRequestParam) -> Result<CallToolResult, ErrorData> {
        let error_fn = || ErrorData::method_not_found::<CallToolRequestMethod>();

        let (server_name, tool_name) = params.name.split_once("__").ok_or_else(error_fn)?;
        let server = self.find_server(server_name).ok_or_else(error_fn)?;
        self.find_tool(&params.name).ok_or_else(error_fn)?;

        params.name = Cow::Owned(tool_name.to_string());

        match server.call_tool(params).await {
            Ok(result) => Ok(result),
            Err(error) => match error {
                rmcp::ServiceError::McpError(error_data) => Err(error_data),
                _ => Err(ErrorData::internal_error(error.to_string(), None)),
            },
        }
    }

    fn find_server(&self, name: &str) -> Option<&DownstreamClient> {
        self.servers
            .binary_search_by(|server| server.name().cmp(name))
            .ok()
            .map(|index| &self.servers[index])
    }

    fn find_tool(&self, name: &str) -> Option<&Tool> {
        self.tools
            .binary_search_by(|tool| tool.name.as_ref().cmp(name))
            .ok()
            .map(|index| &self.tools[index])
    }
}
