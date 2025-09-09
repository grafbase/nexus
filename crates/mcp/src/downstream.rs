mod client;
mod ids;

use config::McpServer;
pub use ids::ToolId;

use client::DownstreamClient;
use futures_util::{
    FutureExt,
    stream::{FuturesUnordered, StreamExt},
};
use rmcp::model::{
    CallToolRequestMethod, CallToolRequestParam, CallToolResult, ErrorData, GetPromptRequestParam, GetPromptResult,
    Prompt, ReadResourceRequestParam, ReadResourceResult, Resource, Tool,
};
use secrecy::SecretString;
use std::{borrow::Cow, collections::HashMap};

/// Represents an MCP server, providing access to multiple downstream servers.
#[derive(Clone)]
pub struct Downstream {
    /// List of downstream servers managed by this instance.
    ///
    /// Must be sorted by the server name.
    servers: Vec<DownstreamClient>,
    /// Aggregated tools from all downstream servers.
    ///
    /// Must be sorted by the tool name.
    tools: Vec<Tool>,
    /// Aggregated prompts from all downstream servers.
    ///
    /// Must be sorted by the prompt name.
    prompts: Vec<Prompt>,
    /// Aggregated resources from all downstream servers.
    ///
    /// Must be sorted by the resource URI.
    resources: Vec<Resource>,
    /// Mapping from resource URI to the server name that owns it.
    ///
    /// Used for routing resource read requests to the correct downstream server.
    resource_to_server: HashMap<String, String>,
}

impl Downstream {
    /// Creates a new Downstream instance from the given configuration.
    ///
    /// This method initializes all configured downstream servers and aggregates
    /// their tools, prefixing each tool name with the server name followed by "__".
    /// Server initialization and tool listing happens concurrently for better performance.
    pub async fn new(config: &config::McpConfig, token: Option<&SecretString>) -> anyhow::Result<Self> {
        struct DownstreamError(String, anyhow::Error);

        // Clone global headers to pass to each downstream client
        let global_headers = config.headers.clone();

        // Create futures for initializing each server concurrently
        let mut server_futures = FuturesUnordered::new();

        for (name, server_config) in &config.servers {
            let name = name.clone();
            let global_headers = global_headers.clone();

            match server_config.finalize(token) {
                McpServer::Stdio(stdio_config) if token.is_none() => {
                    server_futures.push(
                        async move {
                            let server = DownstreamClient::new_stdio(&name, &stdio_config)
                                .await
                                .map_err(|err| DownstreamError(name.clone(), err))?;

                            let tools = server
                                .list_tools()
                                .await
                                .map_err(|err| DownstreamError(name.clone(), err.into()))?;

                            let prompts = server.list_prompts().await.unwrap_or_else(|err| {
                                log::debug!("Unable to retrieve prompts from server '{name}': {err}");
                                Vec::new()
                            });

                            let resources = server.list_resources().await.unwrap_or_else(|err| {
                                log::debug!("Unable to retrieve resources from server '{name}': {err}");
                                Vec::new()
                            });

                            Ok::<_, DownstreamError>((server, tools, prompts, resources))
                        }
                        .boxed(),
                    );
                }
                McpServer::Http(http_config) if token.is_some() || !http_config.forwards_authentication() => {
                    server_futures.push(
                        async move {
                            let server = DownstreamClient::new_http(&name, &http_config, global_headers.iter())
                                .await
                                .map_err(|err| DownstreamError(name.clone(), err))?;

                            let tools = server
                                .list_tools()
                                .await
                                .map_err(|err| DownstreamError(name.clone(), err.into()))?;

                            let prompts = server.list_prompts().await.unwrap_or_else(|err| {
                                log::debug!("Unable to retrieve prompts from server '{name}': {err}");
                                Vec::new()
                            });

                            let resources = server.list_resources().await.unwrap_or_else(|err| {
                                log::debug!("Unable to retrieve resources from server '{name}': {err}");
                                Vec::new()
                            });

                            Ok::<_, DownstreamError>((server, tools, prompts, resources))
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
        let mut prompts = Vec::new();
        let mut resources = Vec::new();
        let mut resource_to_server = HashMap::new();

        while let Some(result) = server_futures.next().await {
            let (server, server_tools, server_prompts, server_resources) = match result {
                Ok((server, server_tools, server_prompts, server_resources)) => {
                    (server, server_tools, server_prompts, server_resources)
                }
                Err(err) if token.is_some() => {
                    log::error!(
                        "Failed to connect to server '{}': {} (authentication token may be invalid)",
                        err.0,
                        err.1
                    );

                    continue;
                }
                Err(err) => {
                    // Log error but allow system to start with remaining healthy servers
                    log::error!(
                        "Failed to initialize server '{}': {}. System will continue without this server.",
                        err.0,
                        err.1
                    );

                    continue;
                }
            };

            for mut tool in server_tools {
                log::debug!("Registering tool '{}' with prefixed name", tool.name);
                tool.name = Cow::Owned(format!("{}__{}", server.name(), tool.name));
                tools.push(tool);
            }

            for mut prompt in server_prompts {
                log::debug!("Registering prompt '{}' with prefixed name", prompt.name);
                prompt.name = format!("{}__{}", server.name(), prompt.name);
                prompts.push(prompt);
            }

            for resource in server_resources {
                log::debug!("Registering resource '{}' from server", resource.uri);

                // Check for duplicate resource URIs
                if let Some(existing_server) = resource_to_server.get(&resource.uri) {
                    // Log warning but don't fail - skip the duplicate resource
                    log::warn!(
                        "Duplicate resource URI '{}' found in servers '{}' and '{}'. Skipping resource from '{}'.",
                        resource.uri,
                        existing_server,
                        server.name(),
                        server.name()
                    );
                    continue;
                }

                resource_to_server.insert(resource.uri.clone(), server.name().to_string());
                resources.push(resource);
            }

            servers.push(server);
        }

        servers.sort_unstable_by(|a, b| a.name().cmp(b.name()));
        tools.sort_unstable_by(|a, b| a.name.cmp(&b.name));
        resources.sort_unstable_by(|a, b| a.uri.cmp(&b.uri));
        prompts.sort_unstable_by(|a, b| a.name.cmp(&b.name));

        // Log initialization results
        if !servers.is_empty() {
            log::debug!(
                "Successfully initialized {} out of {} configured downstream server(s)",
                servers.len(),
                config.servers.len()
            );
        } else if !config.servers.is_empty() {
            // Servers were configured but none initialized successfully
            log::warn!(
                "No MCP servers successfully initialized. {} server(s) were configured but all failed to start. \
                The MCP endpoint will be exposed but no tools will be available.",
                config.servers.len()
            );
        }

        Ok(Self {
            servers,
            tools,
            prompts,
            resources,
            resource_to_server,
        })
    }

    /// Returns an iterator over all available tools from downstream servers.
    ///
    /// Each tool name is prefixed with its server name followed by "__" to ensure
    /// uniqueness across multiple servers. The iterator yields tools in sorted order
    /// by their prefixed names.
    pub fn list_tools(&self) -> impl ExactSizeIterator<Item = &Tool> {
        self.tools.iter()
    }

    /// Returns an iterator over all available prompts from downstream servers.
    ///
    /// Each prompt name is prefixed with its server name followed by "__" to ensure
    /// uniqueness across multiple servers. The iterator yields prompts in sorted order
    /// by their prefixed names.
    pub fn list_prompts(&self) -> impl ExactSizeIterator<Item = &Prompt> {
        self.prompts.iter()
    }

    /// Returns an iterator over all available resources from downstream servers.
    ///
    /// Each resource URI is prefixed with its server name to ensure uniqueness
    /// across multiple servers. The iterator yields resources in sorted order
    /// by their prefixed URIs.
    pub fn list_resources(&self) -> impl ExactSizeIterator<Item = &Resource> {
        self.resources.iter()
    }

    /// Calls a tool on the appropriate downstream server.
    ///
    /// The tool name should be in the format "server_name__tool_name".
    /// This method will parse the server name, find the appropriate server,
    /// and forward the call with the original tool name.
    pub async fn execute(&self, mut params: CallToolRequestParam) -> Result<CallToolResult, ErrorData> {
        log::debug!("Executing downstream tool: '{}'", params.name);

        let error_fn = || ErrorData::method_not_found::<CallToolRequestMethod>();

        let tool_name_str = params.name.to_string();
        let (server_name, tool_name) = tool_name_str.split_once("__").ok_or_else(|| {
            log::error!(
                "Invalid tool name format '{}': missing server separator '__'",
                params.name
            );
            error_fn()
        })?;

        let server = self.find_server(server_name).ok_or_else(|| {
            log::debug!("Server '{server_name}' not found in downstream registry");
            error_fn()
        })?;

        if self.find_tool(&params.name).is_none() {
            log::error!("Tool '{}' not found in tool registry", params.name);
            return Err(error_fn());
        }

        params.name = Cow::Owned(tool_name.to_string());

        server.call_tool(params).await.map_err(|error| match error {
            rmcp::ServiceError::McpError(error_data) => error_data,
            _ => ErrorData::internal_error(error.to_string(), None),
        })
    }

    /// Gets a prompt from the appropriate downstream server.
    ///
    /// The prompt name should be in the format "server_name__prompt_name".
    /// This method will parse the server name, find the appropriate server,
    /// and forward the call with the original prompt name.
    pub async fn get_prompt(&self, mut params: GetPromptRequestParam) -> Result<GetPromptResult, ErrorData> {
        log::debug!("Retrieving downstream prompt: '{}'", params.name);

        let error_fn = || {
            ErrorData::new(
                rmcp::model::ErrorCode::METHOD_NOT_FOUND,
                "Prompt not found".to_string(),
                None,
            )
        };

        let prompt_name_str = params.name.to_string();
        let (server_name, prompt_name) = prompt_name_str.split_once("__").ok_or_else(|| {
            log::error!(
                "Invalid prompt name format '{}': missing server separator '__'",
                params.name
            );

            error_fn()
        })?;

        let server = self.find_server(server_name).ok_or_else(|| {
            log::debug!("Server '{server_name}' not found in downstream registry");
            error_fn()
        })?;

        self.find_prompt(&params.name).ok_or_else(|| {
            log::error!("Prompt '{}' not found in prompt registry", params.name);
            error_fn()
        })?;

        params.name = prompt_name.to_string();

        log::debug!("Forwarding prompt request '{prompt_name}' to server '{server_name}'");

        match server.get_prompt(params).await {
            Ok(result) => Ok(result),
            Err(error) => match error {
                rmcp::ServiceError::McpError(error_data) => Err(error_data),
                _ => Err(ErrorData::internal_error(error.to_string(), None)),
            },
        }
    }

    /// Reads a resource from the appropriate downstream server.
    ///
    /// This method uses the resource URI to lookup which downstream server owns the resource,
    /// then forwards the read request to that server with the original URI unchanged.
    pub async fn read_resource(&self, params: ReadResourceRequestParam) -> Result<ReadResourceResult, ErrorData> {
        log::debug!("Reading downstream resource: '{}'", params.uri);

        let error_fn = || {
            ErrorData::new(
                rmcp::model::ErrorCode::METHOD_NOT_FOUND,
                "Resource not found".to_string(),
                None,
            )
        };

        // Find which server owns this resource
        let server_name = self.resource_to_server.get(params.uri.as_str()).ok_or_else(|| {
            log::error!("Resource URI '{}' not found in resource registry", params.uri);
            error_fn()
        })?;

        let server = self.find_server(server_name).ok_or_else(|| {
            log::debug!("Server '{server_name}' not found in downstream registry");
            error_fn()
        })?;

        // Verify the resource exists in our index
        self.find_resource(params.uri.as_str()).ok_or_else(|| {
            log::error!("Resource '{}' not found in resource registry", params.uri);
            error_fn()
        })?;

        log::debug!(
            "Forwarding resource request for '{}' to server '{}'",
            params.uri,
            server_name
        );

        match server.read_resource(params).await {
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

    fn find_prompt(&self, name: &str) -> Option<&Prompt> {
        self.prompts
            .binary_search_by(|prompt| prompt.name.as_str().cmp(name))
            .ok()
            .map(|index| &self.prompts[index])
    }

    fn find_resource(&self, uri: &str) -> Option<&Resource> {
        self.resources
            .binary_search_by(|resource| resource.uri.as_str().cmp(uri))
            .ok()
            .map(|index| &self.resources[index])
    }
}
