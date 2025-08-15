pub mod execute;
pub mod search;

use crate::types::{CallToolRequest, CallToolResult, PROTOCOL_VERSION, ServerCapabilities, ServerInfo};
use crate::{cache::DynamicDownstreamCache, server_builder::McpServerBuilder};
use config::McpConfig;
use indoc::indoc;
use itertools::Itertools;
use search::SearchTool;
use secrecy::SecretString;
use std::collections::{BTreeMap, HashSet};
use std::{ops::Deref, sync::Arc};

use crate::downstream::Downstream;

#[derive(Clone)]
pub(crate) struct McpServer {
    shared: Arc<McpServerInner>,
}

pub(crate) struct McpServerInner {
    pub info: ServerInfo,
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
    pub enable_structured_content: bool,
}

impl Deref for McpServer {
    type Target = McpServerInner;

    fn deref(&self) -> &Self::Target {
        &self.shared
    }
}

impl McpServer {
    /// Create a new MCP server builder.
    pub fn builder(config: config::Config) -> crate::server_builder::McpServerBuilder {
        crate::server_builder::McpServerBuilder::new(config)
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

        let inner = McpServerInner {
            info: ServerInfo {
                name: generate_server_name(&config.mcp),
                version: env!("CARGO_PKG_VERSION").to_string(),
                protocol_version: PROTOCOL_VERSION.to_string(),
                capabilities: ServerCapabilities::new(),
                instructions: Some(generate_instructions(&config.mcp)),
            },
            static_downstream,
            static_search_tool,
            dynamic_server_names,
            cache,
            rate_limit_manager,
            enable_structured_content: config.mcp.enable_structured_content,
        };

        Ok(Self {
            shared: Arc::new(inner),
        })
    }

    /// Get or create cached search tool for the given authentication context
    pub async fn get_search_tool(&self, token: Option<&SecretString>) -> Result<Arc<SearchTool>, String> {
        match token {
            Some(token) if !self.dynamic_server_names.is_empty() => {
                log::debug!("Retrieving combined search tool (static + dynamic servers)");

                // Dynamic case - get from cache
                let cached = self
                    .cache
                    .get_or_create(token)
                    .await
                    .map_err(|e| format!("Failed to load dynamic tools: {e}"))?;

                Ok(Arc::new(cached.search_tool.clone()))
            }
            _ => {
                log::debug!("Retrieving static-only search tool");

                if let Some(search_tool) = &self.static_search_tool {
                    Ok(search_tool.clone())
                } else {
                    // No servers configured - return empty search tool
                    Ok(Arc::new(
                        SearchTool::new(Vec::new()).map_err(|e| format!("Failed to create empty search tool: {e}"))?,
                    ))
                }
            }
        }
    }

    /// Execute a tool by routing to the correct downstream
    pub async fn execute(
        &self,
        params: CallToolRequest,
        token: Option<&SecretString>,
    ) -> Result<CallToolResult, String> {
        // Get the search tool to access all tools
        let search_tool = self.get_search_tool(token).await?;

        // Use binary search to find the tool
        search_tool.find_exact(&params.name).ok_or_else(|| {
            log::debug!("Tool '{}' not found in available tools registry", params.name);
            format!("Tool '{}' not found", params.name)
        })?;

        // Extract server name from tool name
        let (server_name, tool_name) = params
            .name
            .split_once("__")
            .ok_or_else(|| "Invalid tool name format".to_string())?;

        log::debug!(
            "Parsing tool name '{}': server='{server_name}', tool='{tool_name}'",
            params.name
        );

        // Check rate limits for the specific server/tool
        if let Some(manager) = &self.rate_limit_manager {
            log::debug!("Checking rate limits for server '{server_name}', tool '{tool_name}'");
            let rate_limit_request = rate_limit::RateLimitRequest::builder()
                .server_tool(server_name, tool_name)
                .build();

            if let Err(e) = manager.check_request(&rate_limit_request).await {
                log::debug!("Rate limit exceeded for tool '{}': {e:?}", params.name);
                return Err("Rate limit exceeded".to_string());
            }
            log::debug!("Rate limit check passed for tool '{}'", params.name);
        } else {
            log::debug!("Rate limit manager not configured - skipping rate limit checks");
        }

        // Route to appropriate downstream
        if self.dynamic_server_names.contains(server_name) {
            // Dynamic server - need token
            let token = token.ok_or_else(|| "Authentication required for this tool".to_string())?;

            let cached = self
                .cache
                .get_or_create(token)
                .await
                .map_err(|e| format!("Failed to initialize: {e}"))?;

            cached.downstream.execute(params).await.map_err(|e| e.to_string())
        } else {
            // Static server
            let downstream = self
                .static_downstream
                .as_ref()
                .ok_or_else(|| "Tool not found".to_string())?;

            downstream.execute(params).await.map_err(|e| e.to_string())
        }
    }

    /// Get the appropriate downstream instance for the given token
    pub async fn get_downstream(&self, token: Option<&SecretString>) -> Result<Arc<Downstream>, String> {
        match token {
            Some(token) if !self.dynamic_server_names.is_empty() => {
                log::debug!("Retrieving combined downstream instance (static + dynamic)");

                // Dynamic case - get from cache
                let cached = self
                    .cache
                    .get_or_create(token)
                    .await
                    .map_err(|e| format!("Failed to load dynamic downstream: {e}"))?;

                Ok(Arc::new(cached.downstream.clone()))
            }
            _ => {
                log::debug!("Retrieving static-only downstream instance");

                self.static_downstream
                    .clone()
                    .ok_or_else(|| "No servers configured".to_string())
            }
        }
    }

    /// Get server information
    pub async fn get_info(&self) -> &ServerInfo {
        &self.info
    }

    /// List all available tools
    pub async fn list_tools(&self) -> Result<crate::types::ListToolsResult, String> {
        // For now, just return the built-in tools (search and execute)
        use crate::types::{Tool, ListToolsResult};
        
        Ok(ListToolsResult {
            tools: vec![
                Tool {
                    name: "search".to_string(),
                    description: Some("Search for relevant tools".to_string()),
                    input_schema: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "keywords": {
                                "type": "array",
                                "items": { "type": "string" }
                            }
                        },
                        "required": ["keywords"]
                    }),
                },
                Tool {
                    name: "execute".to_string(),
                    description: Some("Executes a tool with the given parameters".to_string()),
                    input_schema: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" },
                            "arguments": { "type": "object" }
                        },
                        "required": ["name", "arguments"]
                    }),
                },
            ],
            next_cursor: None,
        })
    }

    /// Call a tool (redirects to execute)
    pub async fn call_tool(&self, params: crate::types::CallToolRequest, token: Option<&SecretString>) -> Result<crate::types::CallToolResult, String> {
        // Built-in tools: search and execute
        match params.name.as_str() {
            "search" => {
                // Handle search tool
                let keywords = params.arguments
                    .get("keywords")
                    .and_then(|v| v.as_array())
                    .ok_or("keywords parameter is required and must be an array")?
                    .iter()
                    .filter_map(|v| v.as_str())
                    .map(String::from)
                    .collect::<Vec<_>>();
                
                let search_tool = self.get_search_tool(token).await?;
                let results = search_tool.find_by_keywords(keywords).await
                    .map_err(|e| format!("Search failed: {}", e))?;
                
                Ok(crate::types::CallToolResult {
                    content: vec![crate::types::Content::Text {
                        text: serde_json::to_string(&results).unwrap_or_else(|_| "[]".to_string()),
                    }],
                    is_error: false,
                })
            }
            "execute" => {
                // Handle execute tool - extract name and arguments
                let tool_name = params.arguments
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or("name parameter is required")?;
                let tool_args = params.arguments
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({}));
                
                // Create a new CallToolRequest for the actual tool
                let execute_params = crate::types::CallToolRequest {
                    name: tool_name.to_string(),
                    arguments: tool_args,
                };
                
                self.execute(execute_params, token).await
            }
            _ => {
                // Direct tool call
                self.execute(params, token).await
            }
        }
    }

    /// List available prompts
    pub async fn list_prompts(&self, token: Option<&SecretString>) -> Result<crate::types::ListPromptsResult, String> {
        let downstream = self.get_downstream(token).await?;
        let prompts: Vec<_> = downstream.list_prompts().cloned().collect();
        Ok(crate::types::ListPromptsResult {
            prompts,
            next_cursor: None,
        })
    }

    /// Get a specific prompt
    pub async fn get_prompt(&self, params: crate::types::GetPromptRequest, token: Option<&SecretString>) -> Result<crate::types::GetPromptResult, String> {
        let downstream = self.get_downstream(token).await?;
        downstream.get_prompt(params).await.map_err(|e| e.to_string())
    }

    /// List available resources
    pub async fn list_resources(&self, token: Option<&SecretString>) -> Result<crate::types::ListResourcesResult, String> {
        let downstream = self.get_downstream(token).await?;
        let resources: Vec<_> = downstream.list_resources().cloned().collect();
        Ok(crate::types::ListResourcesResult {
            resources,
            next_cursor: None,
        })
    }

    /// Read a specific resource
    pub async fn read_resource(&self, params: crate::types::ReadResourceRequest, token: Option<&SecretString>) -> Result<crate::types::ReadResourceResult, String> {
        let downstream = self.get_downstream(token).await?;
        downstream.read_resource(params).await.map_err(|e| e.to_string())
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

