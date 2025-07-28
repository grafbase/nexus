mod client;
mod ids;

use config::McpServer;
pub use ids::ToolId;

use client::DownstreamClient;
use futures_util::{
    FutureExt,
    stream::{FuturesUnordered, StreamExt},
};
use rmcp::model::{CallToolRequestMethod, CallToolRequestParam, CallToolResult, ErrorData, Tool};
use secrecy::SecretString;
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
    pub async fn new(config: &config::McpConfig, token: Option<&SecretString>) -> anyhow::Result<Self> {
        struct DownstreamError(String, anyhow::Error);

        // Create futures for initializing each server concurrently
        let mut server_futures = FuturesUnordered::new();

        for (name, server_config) in &config.servers {
            let name = name.clone();

            match server_config.finalize(token) {
                McpServer::Stdio { cmd } if token.is_none() => {
                    server_futures.push(
                        async move {
                            let server = DownstreamClient::new_stdio(&name, &cmd)
                                .await
                                .map_err(|err| DownstreamError(name.clone(), err))?;

                            let tools = server
                                .list_tools()
                                .await
                                .map_err(|err| DownstreamError(name.clone(), err.into()))?;

                            Ok::<_, DownstreamError>((server, tools))
                        }
                        .boxed(),
                    );
                }
                McpServer::Http(http_config) if token.is_some() || !http_config.forwards_authentication() => {
                    server_futures.push(
                        async move {
                            let server = DownstreamClient::new_http(&name, &http_config)
                                .await
                                .map_err(|err| DownstreamError(name.clone(), err))?;

                            let tools = server
                                .list_tools()
                                .await
                                .map_err(|err| DownstreamError(name.clone(), err.into()))?;

                            Ok::<_, DownstreamError>((server, tools))
                        }
                        .boxed(),
                    );
                }
                _ => {}
            }
        }

        // Collect results as they complete
        let mut servers = Vec::new();
        let mut tools = Vec::new();

        while let Some(result) = server_futures.next().await {
            let (server, server_tools) = match result {
                Ok((server, server_tools)) => (server, server_tools),
                Err(err) if token.is_some() => {
                    log::error!(
                        "failed to connect to server '{}': {}. (is the token valid?)",
                        err.0,
                        err.1
                    );

                    continue;
                }
                Err(err) => return Err(err.1),
            };

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
        log::debug!("Downstream::execute called with tool: {}", params.name);

        let error_fn = || ErrorData::method_not_found::<CallToolRequestMethod>();

        let tool_name_str = params.name.to_string();
        let (server_name, tool_name) = tool_name_str.split_once("__").ok_or_else(|| {
            log::error!("Invalid tool name format (missing '__'): {}", params.name);
            error_fn()
        })?;

        let server = self.find_server(server_name).ok_or_else(|| {
            log::debug!("Server not found: {server_name}");

            error_fn()
        })?;

        self.find_tool(&params.name).ok_or_else(|| {
            log::error!("Tool not found in index: {}", params.name);
            error_fn()
        })?;

        params.name = Cow::Owned(tool_name.to_string());

        log::debug!("Calling downstream server {server_name} with tool {tool_name}");

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
