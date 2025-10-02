//! Middleware for recording MCP tool call metrics

use http::request::Parts;
use rmcp::{
    RoleServer, ServerHandler,
    model::{
        CallToolRequestParam, CallToolResult, ErrorCode, ErrorData, ListPromptsResult, ListToolsResult,
        PaginatedRequestParam,
    },
    service::RequestContext,
};
use telemetry::metrics::{
    MCP_PROMPT_REQUEST_DURATION, MCP_RESOURCE_REQUEST_DURATION, MCP_TOOL_CALL_DURATION, MCP_TOOLS_LIST_DURATION,
    Recorder,
};

/// Wrapper that adds metrics recording to an MCP server
#[derive(Clone)]
pub struct MetricsMiddleware<S> {
    inner: S,
}

impl<S> MetricsMiddleware<S> {
    /// Create a new metrics middleware wrapping the given handler
    pub fn new(inner: S) -> Self {
        Self { inner }
    }
}

impl<S> ServerHandler for MetricsMiddleware<S>
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
        let tool_name = params.name.to_string();

        // Start recording and add base attributes
        let mut recorder = Recorder::new(MCP_TOOL_CALL_DURATION);
        add_client_identity(&mut recorder, &context);

        // Add tool-specific attributes
        let actual_tool = add_tool_attributes(&mut recorder, &tool_name, &params);

        // Call inner handler
        let result = self.inner.call_tool(params, context).await;

        // Add result-specific attributes and record
        match &result {
            Ok(res) => add_success_attributes(&mut recorder, &tool_name, actual_tool.as_deref(), res),
            Err(e) => add_error_attributes(&mut recorder, &tool_name, actual_tool.as_deref(), e),
        };

        recorder.record();

        result
    }

    // Forward all other methods to the inner handler
    async fn list_tools(
        &self,
        params: Option<PaginatedRequestParam>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let mut recorder = create_method_recorder("list_tools", &context);
        let result = self.inner.list_tools(params, context).await;

        map_result_attributes(&mut recorder, &result);
        recorder.record();

        result
    }

    async fn list_prompts(
        &self,
        params: Option<PaginatedRequestParam>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, ErrorData> {
        let mut recorder = create_method_recorder("list_prompts", &context);
        let result = self.inner.list_prompts(params, context).await;

        map_result_attributes(&mut recorder, &result);
        recorder.record();

        result
    }

    async fn get_prompt(
        &self,
        params: rmcp::model::GetPromptRequestParam,
        context: RequestContext<RoleServer>,
    ) -> Result<rmcp::model::GetPromptResult, ErrorData> {
        let mut recorder = create_method_recorder("get_prompt", &context);
        let result = self.inner.get_prompt(params, context).await;

        map_result_attributes(&mut recorder, &result);
        recorder.record();

        result
    }

    async fn list_resources(
        &self,
        params: Option<PaginatedRequestParam>,
        context: RequestContext<RoleServer>,
    ) -> Result<rmcp::model::ListResourcesResult, ErrorData> {
        let mut recorder = create_method_recorder("list_resources", &context);
        let result = self.inner.list_resources(params, context).await;

        map_result_attributes(&mut recorder, &result);
        recorder.record();

        result
    }

    async fn read_resource(
        &self,
        params: rmcp::model::ReadResourceRequestParam,
        context: RequestContext<RoleServer>,
    ) -> Result<rmcp::model::ReadResourceResult, ErrorData> {
        let mut recorder = create_method_recorder("read_resource", &context);
        let result = self.inner.read_resource(params, context).await;

        map_result_attributes(&mut recorder, &result);
        recorder.record();

        result
    }
}

/// Add client identity attributes to the recorder
fn add_client_identity(recorder: &mut Recorder, context: &RequestContext<RoleServer>) {
    if let Some(parts) = context.extensions.get::<Parts>()
        && let Some(identity) = parts.extensions.get::<context::ClientIdentity>()
    {
        recorder.push_attribute("client.id", identity.client_id.clone());

        if let Some(ref group) = identity.group {
            recorder.push_attribute("client.group", group.clone());
        }
    }
}

/// Add tool-specific attributes based on the tool name and arguments
fn add_tool_attributes(recorder: &mut Recorder, tool_name: &str, params: &CallToolRequestParam) -> Option<String> {
    match tool_name {
        "search" => {
            recorder.push_attribute("tool_type", "builtin");
            recorder.push_attribute("tool_name", "search");

            // Add keyword count if available
            if let Some(args) = &params.arguments
                && let Some(keywords) = args.get("keywords")
                && let Some(keywords_array) = keywords.as_array()
            {
                recorder.push_attribute("keyword_count", keywords_array.len() as i64);
            }

            None
        }
        "execute" => {
            // Extract the actual tool being executed
            let actual_tool = params
                .arguments
                .as_ref()
                .and_then(|args| args.get("name"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            recorder.push_attribute("tool_type", "downstream");

            actual_tool
        }
        _ => {
            recorder.push_attribute("tool_type", "downstream");

            None
        }
    }
}

/// Add success-specific attributes
fn add_success_attributes(
    recorder: &mut Recorder,
    tool_name: &str,
    actual_tool: Option<&str>,
    result: &CallToolResult,
) {
    recorder.push_attribute("status", "success");

    match tool_name {
        "search" => {
            add_search_result_count(recorder, result);
        }
        "execute" => {
            add_execute_tool_name(recorder, actual_tool);
        }
        _ => {
            parse_tool_name(recorder, tool_name, true);
        }
    }
}

/// Add error-specific attributes
fn add_error_attributes(recorder: &mut Recorder, tool_name: &str, actual_tool: Option<&str>, error: &ErrorData) {
    recorder.push_attribute("status", "error");
    recorder.push_attribute("error.type", map_error_type(error.code));

    match tool_name {
        "search" => {
            // Search tool name already set in add_tool_attributes
        }
        "execute" => {
            add_execute_tool_name_error(recorder, actual_tool);
        }
        _ => {
            recorder.push_attribute("tool_name", tool_name.to_string());
        }
    }
}

/// Add tool name for successful execute calls
fn add_execute_tool_name(recorder: &mut Recorder, actual_tool: Option<&str>) {
    if let Some(actual_tool) = actual_tool {
        parse_tool_name(recorder, actual_tool, true);
    } else {
        recorder.push_attribute("tool_name", "execute");
    }
}

/// Add tool name for failed execute calls (keep full name)
fn add_execute_tool_name_error(recorder: &mut Recorder, actual_tool: Option<&str>) {
    if let Some(actual_tool) = actual_tool {
        recorder.push_attribute("tool_name", actual_tool.to_string());
    } else {
        recorder.push_attribute("tool_name", "execute");
    }
}

/// Parse tool name into server_name and tool_name components
fn parse_tool_name(recorder: &mut Recorder, tool_name: &str, is_success: bool) {
    if is_success && tool_name.contains("__") {
        if let Some((server, tool)) = tool_name.split_once("__") {
            recorder.push_attribute("server_name", server.to_string());
            recorder.push_attribute("tool_name", tool.to_string());
        } else {
            recorder.push_attribute("tool_name", tool_name.to_string());
        }
    } else {
        recorder.push_attribute("tool_name", tool_name.to_string());
    }
}

/// Add result count for search tool
fn add_search_result_count(recorder: &mut Recorder, res: &CallToolResult) {
    if let Some(structured) = &res.structured_content {
        // Modern format: structured content
        if let Some(response) = structured.as_object()
            && let Some(results) = response.get("results")
            && let Some(results_array) = results.as_array()
        {
            recorder.push_attribute("result_count", results_array.len() as i64);
        }
    } else {
        // Legacy format: content items
        recorder.push_attribute("result_count", res.content.len() as i64);
    }
}

/// Map error codes to readable error types
fn map_error_type(code: ErrorCode) -> &'static str {
    match code {
        // JSON-RPC 2.0 standard error codes
        ErrorCode::PARSE_ERROR => "parse_error",         // -32700: Invalid JSON
        ErrorCode::INVALID_REQUEST => "invalid_request", // -32600: Not a valid request
        ErrorCode::METHOD_NOT_FOUND => "method_not_found", // -32601: Method does not exist
        ErrorCode::INVALID_PARAMS => "invalid_params",   // -32602: Invalid method parameters
        ErrorCode::INTERNAL_ERROR => "internal_error",   // -32603: Internal server error

        // Server-defined errors (-32000 to -32099)
        // These might be used for application-specific errors like rate limiting
        _ if code.0 == -32000 => "rate_limit_exceeded",
        _ if code.0 >= -32099 && code.0 <= -32001 => "server_error",

        // Any other error
        _ => "unknown",
    }
}

/// Create a recorder for MCP method calls with the appropriate metric
fn create_method_recorder(method_name: &str, context: &RequestContext<RoleServer>) -> Recorder {
    let metric_name = match method_name {
        "list_tools" => MCP_TOOLS_LIST_DURATION,
        "list_prompts" | "get_prompt" => MCP_PROMPT_REQUEST_DURATION,
        "list_resources" | "read_resource" => MCP_RESOURCE_REQUEST_DURATION,
        _ => MCP_TOOL_CALL_DURATION, // Fallback
    };

    let mut recorder = Recorder::new(metric_name);
    recorder.push_attribute("method", method_name.to_string());
    add_client_identity(&mut recorder, context);

    recorder
}

/// Map result status to recorder attributes
fn map_result_attributes<T>(recorder: &mut Recorder, result: &Result<T, ErrorData>) {
    match result {
        Ok(_) => {
            recorder.push_attribute("status", "success");
        }
        Err(e) => {
            recorder.push_attribute("status", "error");
            recorder.push_attribute("error.type", map_error_type(e.code));
        }
    }
}
