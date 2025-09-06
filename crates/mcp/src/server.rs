pub mod builder;
pub mod execute;
pub mod handler;
pub mod metrics;
pub mod search;

use self::builder::McpServerBuilder;
use crate::cache::DynamicDownstreamCache;
use config::{Config, McpConfig};
use execute::ExecuteParameters;
use http::request::Parts;
use indoc::indoc;
use itertools::Itertools;
use rmcp::{
    RoleServer, ServerHandler,
    model::{
        CallToolRequestMethod, CallToolRequestParam, CallToolResult, Content, ErrorCode, ErrorData,
        GetPromptRequestParam, GetPromptResult, Implementation, ListPromptsResult, ListResourcesResult,
        ListToolsResult, PaginatedRequestParam, ReadResourceRequestParam, ReadResourceResult, ServerCapabilities,
        ServerInfo, Tool,
    },
    service::RequestContext,
};
use search::{SearchParameters, SearchTool};
use secrecy::SecretString;
use std::collections::{BTreeMap, HashSet};
use std::{ops::Deref, sync::Arc};

use crate::downstream::Downstream;

#[derive(Clone)]
pub(crate) struct McpServer {
    shared: Arc<McpServerInner>,
}

pub(crate) struct McpServerInner {
    info: ServerInfo,
    // Static downstream (servers without auth forwarding)
    static_downstream: Option<Arc<Downstream>>,
    // Static search tool cache
    static_search_tool: Option<Arc<SearchTool>>,
    // Names of servers that require auth forwarding
    dynamic_server_names: HashSet<String>,
    // Cache for dynamic downstream instances
    cache: Arc<DynamicDownstreamCache>,
    // Rate limit manager for server/tool limits
    rate_limit_manager: Option<Arc<rate_limit::RateLimitManager>>,
    // Configuration for structured content responses
    enable_structured_content: bool,
    // List of tools
    tools: Vec<Tool>,
}

impl Deref for McpServer {
    type Target = McpServerInner;

    fn deref(&self) -> &Self::Target {
        &self.shared
    }
}

impl McpServer {
    /// Create a new MCP server builder.
    pub fn builder(config: Config) -> McpServerBuilder {
        McpServerBuilder::new(config)
    }

    pub(crate) async fn new(
        McpServerBuilder {
            config,
            rate_limit_manager,
        }: McpServerBuilder,
    ) -> anyhow::Result<Self> {
        // Identify which servers need dynamic initialization
        let mut dynamic_server_names = HashSet::new();
        let mut static_config = config.mcp.clone();

        static_config.servers.retain(|name, server_config| {
            if server_config.forwards_authentication() {
                dynamic_server_names.insert(name.clone());

                false
            } else {
                true
            }
        });

        // Create static downstream if there are any static servers
        let (static_downstream, static_search_tool) = if !static_config.servers.is_empty() {
            log::debug!(
                "Initializing {} static MCP server(s) at startup",
                static_config.servers.len()
            );

            let downstream = Downstream::new(&static_config, None).await?;
            let tools = downstream.list_tools().cloned().collect();
            let static_search_tool = SearchTool::new(tools)?;

            (Some(Arc::new(downstream)), Some(Arc::new(static_search_tool)))
        } else {
            (None, None)
        };

        // Create cache for dynamic instances
        let cache = Arc::new(DynamicDownstreamCache::new(config.mcp.clone()));

        let server_info = Implementation {
            name: generate_server_name(&config.mcp),
            version: env!("CARGO_PKG_VERSION").to_string(),
        };

        let inner = McpServerInner {
            info: ServerInfo {
                protocol_version: crate::PROTOCOL_VERSION,
                capabilities: ServerCapabilities::builder()
                    .enable_tools()
                    .enable_prompts()
                    .enable_resources()
                    .build(),
                server_info,
                instructions: Some(generate_instructions(&config.mcp)),
            },
            static_downstream,
            static_search_tool,
            dynamic_server_names,
            cache,
            rate_limit_manager,
            enable_structured_content: config.mcp.enable_structured_content,
            tools: vec![search::rmcp_tool(), execute::rmcp_tool()],
        };

        Ok(Self {
            shared: Arc::new(inner),
        })
    }

    /// Get or create cached search tool for the given authentication context
    async fn get_search_tool(&self, token: Option<&SecretString>) -> Result<Arc<SearchTool>, ErrorData> {
        match token {
            Some(token) if !self.dynamic_server_names.is_empty() => {
                log::debug!("Retrieving combined search tool (static + dynamic servers)");

                // Dynamic case - get from cache
                let cached = self
                    .cache
                    .get_or_create(token)
                    .await
                    .map_err(|e| ErrorData::internal_error(format!("Failed to load dynamic tools: {e}"), None))?;

                Ok(Arc::new(cached.search_tool.clone()))
            }
            _ => {
                log::debug!("Retrieving static-only search tool");

                if let Some(search_tool) = &self.static_search_tool {
                    Ok(search_tool.clone())
                } else {
                    // No servers configured - return empty search tool
                    Ok(Arc::new(SearchTool::new(Vec::new()).map_err(|e| {
                        ErrorData::internal_error(format!("Failed to create empty search tool: {e}"), None)
                    })?))
                }
            }
        }
    }

    /// Execute a tool by routing to the correct downstream
    async fn execute(
        &self,
        params: CallToolRequestParam,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        // Extract token from request
        let parts = ctx.extensions.get::<Parts>();
        let token = parts.and_then(|p| p.extensions.get::<SecretString>()).cloned();

        // Get the search tool to access all tools
        let search_tool = self.get_search_tool(token.as_ref()).await?;

        // Check if tool exists in our registry
        if search_tool.find_exact(&params.name).is_none() {
            log::debug!("Tool '{}' not found in available tools registry", params.name);
            return Err(ErrorData::method_not_found::<CallToolRequestMethod>());
        }

        let (server_name, tool_name) = params.name.split_once("__").ok_or_else(|| {
            log::error!("Invalid tool name format: '{}'", params.name);
            ErrorData::invalid_params("Invalid tool name format", None)
        })?;

        log::debug!(
            "Parsing tool name '{}': server='{server_name}', tool='{tool_name}'",
            params.name,
        );

        // Check rate limits for the specific server/tool
        if let Some(manager) = &self.rate_limit_manager {
            log::debug!("Checking rate limits for server '{server_name}', tool '{tool_name}'");

            let rate_limit_request = rate_limit::RateLimitRequest::builder()
                .server_tool(server_name, tool_name)
                .build();

            if let Err(e) = manager.check_request(&rate_limit_request).await {
                log::debug!("Rate limit exceeded for tool '{}': {e:?}", params.name);
                // Use -32000 for rate limit errors (server-defined error in JSON-RPC 2.0 spec)
                return Err(ErrorData::new(ErrorCode(-32000), "Rate limit exceeded", None));
            }

            log::debug!("Rate limit check passed for tool '{}'", params.name);
        } else {
            log::debug!("Rate limit manager not configured - skipping rate limit checks");
        }

        // MCP header rules are applied at client initialization time, not per-request
        // No dynamic header transformation needed here

        // Route to appropriate downstream
        if self.dynamic_server_names.contains(server_name) {
            // Dynamic server - need token
            let token_ref = token.as_ref().ok_or_else(|| {
                ErrorData::new(
                    ErrorCode::INVALID_REQUEST,
                    "Authentication required for this tool",
                    None,
                )
            })?;

            let cached = self
                .cache
                .get_or_create(token_ref)
                .await
                .map_err(|e| ErrorData::internal_error(format!("Failed to initialize: {e}"), None))?;

            cached.downstream.execute(params).await
        } else {
            // Static server
            let downstream = self
                .static_downstream
                .as_ref()
                .ok_or_else(ErrorData::method_not_found::<CallToolRequestMethod>)?; // Tool not found

            downstream.execute(params).await
        }
    }

    /// Get the appropriate downstream instance for the given token
    async fn get_downstream(&self, token: Option<&SecretString>) -> Result<Arc<Downstream>, ErrorData> {
        match token {
            Some(token) if !self.dynamic_server_names.is_empty() => {
                log::debug!("Retrieving combined downstream instance (static + dynamic)");

                // Dynamic case - get from cache
                let cached =
                    self.cache.get_or_create(token).await.map_err(|e| {
                        ErrorData::internal_error(format!("Failed to load dynamic downstream: {e}"), None)
                    })?;

                Ok(Arc::new(cached.downstream.clone()))
            }
            _ => {
                log::debug!("Retrieving static-only downstream instance");

                self.static_downstream
                    .clone()
                    .ok_or_else(|| ErrorData::internal_error("No servers configured".to_string(), None))
            }
        }
    }
}

impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerInfo {
        self.info.clone()
    }

    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParam>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            next_cursor: None,
            tools: self.shared.tools.clone(),
        })
    }

    async fn call_tool(
        &self,
        params: CallToolRequestParam,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        log::debug!("Processing tool invocation for '{}'", params.name);

        // Extract token from request extensions
        let parts = ctx.extensions.get::<Parts>();
        let token = parts.and_then(|p| p.extensions.get::<SecretString>());

        match params.name.as_ref() {
            "search" => {
                log::debug!("Executing search tool to find available MCP tools");

                // Get cached search tool
                let search_tool = self.get_search_tool(token).await?;

                let search_params: SearchParameters =
                    serde_json::from_value(serde_json::Value::Object(params.arguments.unwrap_or_default()))
                        .map_err(|e| ErrorData::invalid_params(e.to_string(), None))?;

                let tools = search_tool
                    .find_by_keywords(search_params.keywords)
                    .await
                    .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

                // Choose response format based on configuration
                if self.enable_structured_content {
                    // Modern format: structuredContent only (better performance)
                    // Use SearchResponse wrapper type to match the output schema
                    let response = search::SearchResponse { results: tools };

                    Ok(CallToolResult {
                        content: Vec::new(),
                        structured_content: Some(serde_json::to_value(response).unwrap()),
                        is_error: None,
                        meta: None,
                    })
                } else {
                    // Legacy format: content field with Content::json objects
                    // For legacy format, keep individual tool objects for backward compatibility
                    let mut content = Vec::with_capacity(tools.len());
                    for tool in tools {
                        content.push(Content::json(tool)?);
                    }

                    Ok(CallToolResult {
                        content,
                        structured_content: None,
                        is_error: None,
                        meta: None,
                    })
                }
            }
            "execute" => {
                log::debug!("Executing downstream tool via execute endpoint");

                // Parse execute parameters
                let exec_params: ExecuteParameters =
                    serde_json::from_value(serde_json::Value::Object(params.arguments.unwrap_or_default()))
                        .map_err(|e| ErrorData::invalid_params(e.to_string(), None))?;

                log::debug!("Executing downstream tool: '{}'", exec_params.name);

                let params = CallToolRequestParam {
                    name: exec_params.name.clone().into(),
                    arguments: Some(exec_params.arguments),
                };

                // Execute the tool with proper routing
                self.execute(params, ctx).await
            }
            tool_name => {
                log::debug!("Unknown tool requested: '{tool_name}' - returning method not found");

                Err(ErrorData::method_not_found::<CallToolRequestMethod>())
            }
        }
    }

    async fn list_prompts(
        &self,
        _: Option<PaginatedRequestParam>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, ErrorData> {
        log::debug!("Listing all available MCP prompts");

        // Extract token from request extensions
        let token = ctx
            .extensions
            .get::<Parts>()
            .and_then(|parts| parts.extensions.get::<SecretString>());

        let downstream = self.get_downstream(token).await?;
        let prompts = downstream.list_prompts().cloned().collect();

        Ok(ListPromptsResult {
            prompts,
            next_cursor: None,
        })
    }

    async fn get_prompt(
        &self,
        params: GetPromptRequestParam,
        ctx: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, ErrorData> {
        log::debug!("Retrieving prompt details for '{}'", params.name);

        // Extract token from request extensions
        let token = ctx
            .extensions
            .get::<Parts>()
            .and_then(|parts| parts.extensions.get::<SecretString>());

        let downstream = self.get_downstream(token).await?;
        downstream.get_prompt(params).await
    }

    async fn list_resources(
        &self,
        _: Option<PaginatedRequestParam>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        log::debug!("Listing all available MCP resources");

        // Extract token from request extensions
        let token = ctx
            .extensions
            .get::<Parts>()
            .and_then(|parts| parts.extensions.get::<SecretString>());

        let downstream = self.get_downstream(token).await?;
        let resources = downstream.list_resources().cloned().collect();

        Ok(ListResourcesResult {
            resources,
            next_cursor: None,
        })
    }

    async fn read_resource(
        &self,
        params: ReadResourceRequestParam,
        ctx: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, ErrorData> {
        log::debug!("Reading resource content for URI: '{}'", params.uri);

        // Extract token from request extensions
        let token = ctx
            .extensions
            .get::<Parts>()
            .and_then(|parts| parts.extensions.get::<SecretString>());

        let downstream = self.get_downstream(token).await?;
        downstream.read_resource(params).await
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

fn generate_instructions(config: &McpConfig) -> String {
    let mut servers_info = BTreeMap::<String, Vec<String>>::new();

    // Group tools by server name
    for server_name in config.servers.keys() {
        servers_info.insert(server_name.clone(), Vec::new());
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
        instructions.push_str("**Available Servers:**\n\n");

        for server_name in servers_info.keys() {
            instructions.push_str(&format!("- **{server_name}**\n"));
        }

        instructions.push_str("\n**Note:** Use the `search` tool to discover what tools each server provides.\n");
    } else {
        instructions.push_str("**No downstream servers are currently configured.**\n");
    }

    instructions
}
