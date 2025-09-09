//! Middleware for recording MCP distributed traces

use config::McpConfig;
use fastrace::{Span, collector::SpanContext, future::FutureExt, prelude::LocalSpan};
use http::request::Parts;
use rmcp::{
    RoleServer, ServerHandler,
    model::{
        CallToolRequestParam, CallToolResult, ErrorData, GetPromptRequestParam, GetPromptResult, ListPromptsResult,
        ListResourcesResult, ListToolsResult, PaginatedRequestParam, ReadResourceRequestParam, ReadResourceResult,
    },
    service::RequestContext,
};

/// Wrapper that adds distributed tracing to an MCP server
#[derive(Clone)]
pub struct TracingMiddleware<S> {
    inner: S,
    config: McpConfig,
}

impl<S> TracingMiddleware<S> {
    /// Create a new tracing middleware wrapping the given handler
    pub fn new(inner: S, config: McpConfig) -> Self {
        Self { inner, config }
    }
}

impl<S> ServerHandler for TracingMiddleware<S>
where
    S: ServerHandler,
{
    fn get_info(&self) -> rmcp::model::ServerInfo {
        self.inner.get_info()
    }

    async fn call_tool(
        &self,
        params: CallToolRequestParam,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let span = create_span_with_context(&context, "tools/call");

        // Add tool-specific properties
        span.add_property(|| ("mcp.tool.name", params.name.clone()));

        // Determine tool type and transport
        let (tool_type, transport) = categorize_tool(&params.name, &self.config);
        span.add_property(|| ("mcp.tool.type", tool_type));
        if let Some(transport) = transport {
            span.add_property(|| ("mcp.transport", transport));
        }

        // Special handling for search tool to capture keywords
        if params.name == "search" {
            if let Some(ref arguments) = params.arguments
                && let Some(keywords) = arguments.get("keywords")
                && let Some(keywords_array) = keywords.as_array()
            {
                let keywords_str: Vec<_> = keywords_array.iter().filter_map(|v| v.as_str()).collect();
                span.add_property(|| ("mcp.search.keywords", keywords_str.join(",")));
                span.add_property(|| ("mcp.search.keyword_count", keywords_str.len().to_string()));
            }
        }
        // Special handling for execute tool to capture target
        else if params.name == "execute"
            && let Some(ref arguments) = params.arguments
            && let Some(name) = arguments.get("name")
            && let Some(tool_name) = name.as_str()
        {
            span.add_property(|| ("mcp.execute.target_tool", tool_name.to_string()));
            // Extract server name from tool name
            if let Some(server_name) = tool_name.split("__").next() {
                span.add_property(|| ("mcp.execute.target_server", server_name.to_string()));
            }
        }

        // Create the future and wrap it with the span
        let fut = async move {
            let result = self.inner.call_tool(params, context).await;

            // Add error info if failed (only error code, no message for PII safety)
            if let Err(ref e) = result {
                LocalSpan::add_property(|| ("error", "true"));
                LocalSpan::add_property(|| ("mcp.error.code", e.code.0.to_string()));
            }

            result
        };

        fut.in_span(span).await
    }

    async fn list_tools(
        &self,
        params: Option<PaginatedRequestParam>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let span = create_span_with_context(&context, "tools/list");

        let fut = async move {
            let result = self.inner.list_tools(params, context).await;

            if let Err(ref e) = result {
                LocalSpan::add_property(|| ("error", "true"));
                LocalSpan::add_property(|| ("mcp.error.code", e.code.0.to_string()));
            }

            result
        };

        fut.in_span(span).await
    }

    async fn list_prompts(
        &self,
        params: Option<PaginatedRequestParam>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, ErrorData> {
        let span = create_span_with_context(&context, "prompts/list");

        let fut = async move {
            let result = self.inner.list_prompts(params, context).await;

            if let Err(ref e) = result {
                LocalSpan::add_property(|| ("error", "true"));
                LocalSpan::add_property(|| ("mcp.error.code", e.code.0.to_string()));
            }

            result
        };

        fut.in_span(span).await
    }

    async fn get_prompt(
        &self,
        params: GetPromptRequestParam,
        context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, ErrorData> {
        let span = create_span_with_context(&context, "prompts/get");

        // Add prompt-specific properties
        span.add_property(|| ("mcp.prompt.name", params.name.clone()));

        let fut = async move {
            let result = self.inner.get_prompt(params, context).await;

            if let Err(ref e) = result {
                LocalSpan::add_property(|| ("error", "true"));
                LocalSpan::add_property(|| ("mcp.error.code", e.code.0.to_string()));
            }

            result
        };

        fut.in_span(span).await
    }

    async fn list_resources(
        &self,
        params: Option<PaginatedRequestParam>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        let span = create_span_with_context(&context, "resources/list");

        let fut = async move {
            let result = self.inner.list_resources(params, context).await;

            if let Err(ref e) = result {
                LocalSpan::add_property(|| ("error", "true"));
                LocalSpan::add_property(|| ("mcp.error.code", e.code.0.to_string()));
            }

            result
        };

        fut.in_span(span).await
    }

    async fn read_resource(
        &self,
        params: ReadResourceRequestParam,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, ErrorData> {
        let span = create_span_with_context(&context, "resources/read");

        // Resource URI might be sensitive, only log if it's a known safe pattern
        if params.uri.starts_with("tool://") || params.uri.starts_with("prompt://") {
            span.add_property(|| ("mcp.resource.uri", params.uri.clone()));
        }

        let fut = async move {
            let result = self.inner.read_resource(params, context).await;

            if let Err(ref e) = result {
                LocalSpan::add_property(|| ("error", "true"));
                LocalSpan::add_property(|| ("mcp.error.code", e.code.0.to_string()));
            }

            result
        };

        fut.in_span(span).await
    }
}

/// Add client identity to a span
fn add_client_identity_to_span(span: &Span, parts: &Parts) {
    // Check for x-client-id header
    if let Some(client_id) = parts.headers.get("x-client-id")
        && let Ok(id) = client_id.to_str()
    {
        span.add_property(|| ("client.id", id.to_string()));
    }

    // Check for x-client-group header
    if let Some(group) = parts.headers.get("x-client-group")
        && let Ok(g) = group.to_str()
    {
        span.add_property(|| ("client.group", g.to_string()));
    }
}

/// Categorize a tool and determine its transport type
fn categorize_tool(tool_name: &str, config: &McpConfig) -> (&'static str, Option<&'static str>) {
    match tool_name {
        "search" | "execute" => ("builtin", None),
        name => {
            // Extract server name from tool name (before "__")
            if let Some(server_name) = name.split("__").next() {
                if let Some(_server_config) = config.servers.get(server_name) {
                    // For now, we'll just mark it as downstream without transport details
                    // since the server config structure doesn't expose these methods
                    ("downstream", None)
                } else {
                    ("unknown", None)
                }
            } else {
                ("unknown", None)
            }
        }
    }
}

/// Helper to create a span with proper parent context from the HTTP layer
fn create_span_with_context(context: &RequestContext<RoleServer>, name: &'static str) -> Span {
    let span = if let Some(parts) = context.extensions.get::<Parts>()
        && let Some(trace_context) = parts.extensions.get::<SpanContext>()
    {
        // Create a root span in the same trace as the HTTP span
        // Due to task spawning in StreamableHttpService, this will be a sibling, not a child
        Span::root(name, *trace_context)
    } else {
        // Fallback to local parent (likely won't work due to task spawning)
        Span::enter_with_local_parent(name)
    };

    // Add client identification if available
    if let Some(parts) = context.extensions.get::<Parts>() {
        add_client_identity_to_span(&span, parts);
    }

    // Add method name
    span.add_property(|| ("mcp.method", name));

    // Determine if auth is being forwarded
    let auth_forwarded = context
        .extensions
        .get::<Parts>()
        .and_then(|parts| parts.headers.get("authorization"))
        .is_some();
    span.add_property(|| ("mcp.auth_forwarded", auth_forwarded.to_string()));

    span
}
