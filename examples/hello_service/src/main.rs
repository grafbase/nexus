use axum::Router;
use rmcp::{
    handler::server::ServerHandler,
    model::*,
    service::{RequestContext, RoleServer},
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::never::NeverSessionManager,
    },
};
use std::{net::SocketAddr, sync::Arc, time::Duration};

#[derive(Clone)]
pub struct HelloService;

impl HelloService {
    fn new() -> Self {
        Self
    }
}

impl ServerHandler for HelloService {
    async fn list_tools(
        &self,
        _params: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let mut schema_map = serde_json::Map::new();
        schema_map.insert("type".to_string(), serde_json::Value::String("object".to_string()));

        let mut properties = serde_json::Map::new();

        let mut name_prop = serde_json::Map::new();
        name_prop.insert("type".to_string(), serde_json::Value::String("string".to_string()));
        name_prop.insert(
            "description".to_string(),
            serde_json::Value::String("The name of the person to greet".to_string()),
        );
        properties.insert("name".to_string(), serde_json::Value::Object(name_prop));

        schema_map.insert("properties".to_string(), serde_json::Value::Object(properties));
        schema_map.insert(
            "required".to_string(),
            serde_json::Value::Array(vec![serde_json::Value::String("name".to_string())]),
        );

        let tools = vec![Tool {
            name: "hello".into(),
            description: Some("Say hello to someone by name".into()),
            input_schema: Arc::new(schema_map),
            annotations: None,
        }];

        Ok(ListToolsResult {
            tools,
            next_cursor: None,
        })
    }

    async fn call_tool(
        &self,
        params: CallToolRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        if params.name != "hello" {
            return Err(ErrorData {
                code: ErrorCode(-32601),
                message: format!("Tool '{}' not found", params.name).into(),
                data: None,
            });
        }

        let args = params.arguments.ok_or_else(|| ErrorData {
            code: ErrorCode(-32602),
            message: "Missing arguments".into(),
            data: None,
        })?;

        let name = args.get("name").and_then(|v| v.as_str()).ok_or_else(|| ErrorData {
            code: ErrorCode(-32602),
            message: "Missing or invalid parameter 'name'".into(),
            data: None,
        })?;

        let greeting = format!("Hello, {name}!");

        Ok(CallToolResult::success(vec![Content::text(greeting)]))
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let service = HelloService::new();

    let mcp_service = StreamableHttpService::new(
        move || Ok(service.clone()),
        Arc::new(NeverSessionManager::default()),
        StreamableHttpServerConfig {
            sse_keep_alive: Some(Duration::from_secs(5)),
            stateful_mode: false,
        },
    );

    let app = Router::new().route_service("/mcp", mcp_service);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("listening on {addr}");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::HelloService;
    use axum::Router;
    use rmcp::{
        RoleClient,
        model::CallToolRequestParam,
        service::{RunningService, ServiceExt},
        transport::{
            StreamableHttpClientTransport,
            streamable_http_server::{
                StreamableHttpServerConfig, StreamableHttpService, session::never::NeverSessionManager,
            },
        },
    };
    use serde_json::json;
    use std::{sync::Arc, time::Duration};

    // Helper function to create a new MCP client service
    async fn create_test_service() -> Result<RunningService<RoleClient, ()>, Box<dyn std::error::Error>> {
        let service = HelloService::new();

        let mcp_service = StreamableHttpService::new(
            move || Ok(service.clone()),
            Arc::new(NeverSessionManager::default()),
            StreamableHttpServerConfig {
                sse_keep_alive: Some(Duration::from_secs(5)),
                stateful_mode: false,
            },
        );

        let app = Router::new().route_service("/mcp", mcp_service);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server error");
        });

        let mcp_url = format!("http://{addr}/mcp");
        let transport = StreamableHttpClientTransport::from_uri(&*mcp_url);

        // Create an MCP client service with a longer timeout
        let service = tokio::time::timeout(Duration::from_secs(10), ().serve(transport)).await??;

        Ok(service)
    }

    // MCP Integration tests that actually test the protocol using rmcp client
    #[tokio::test]
    async fn test_mcp_list_tools() {
        let service = create_test_service().await.expect("Failed to create test service");

        // List tools
        let tools_result = service
            .list_tools(Default::default())
            .await
            .expect("Failed to list tools");

        // Verify the tools
        assert_eq!(tools_result.tools.len(), 1);
        assert_eq!(tools_result.tools[0].name, "hello");
        assert_eq!(
            tools_result.tools[0].description,
            Some("Say hello to someone by name".into())
        );

        // Verify the input schema
        let input_schema = &tools_result.tools[0].input_schema;
        assert_eq!(input_schema.get("type").unwrap(), &json!("object"));
        assert!(input_schema.get("properties").is_some());

        let properties = input_schema.get("properties").unwrap().as_object().unwrap();
        assert!(properties.contains_key("name"));

        let name_prop = properties.get("name").unwrap().as_object().unwrap();
        assert_eq!(name_prop.get("type").unwrap(), &json!("string"));
        assert_eq!(
            name_prop.get("description").unwrap(),
            &json!("The name of the person to greet")
        );

        // Clean up
        service.cancel().await.expect("Failed to cancel service");
    }

    #[tokio::test]
    async fn test_mcp_call_tool_success() {
        let service = create_test_service().await.expect("Failed to create test service");

        // Call the hello tool
        let result = service
            .call_tool(CallToolRequestParam {
                name: "hello".into(),
                arguments: Some(json!({ "name": "Alice" }).as_object().unwrap().clone()),
            })
            .await
            .expect("Failed to call tool");

        // Verify the result
        assert_eq!(result.is_error, Some(false));
        assert_eq!(result.content.len(), 1);

        // Check the content contains our greeting
        let content_text = result.content[0].raw.as_text().expect("Expected text content");
        assert_eq!(content_text.text, "Hello, Alice!");

        // Clean up
        service.cancel().await.expect("Failed to cancel service");
    }

    #[tokio::test]
    async fn test_mcp_simple_tool_call() {
        let service = create_test_service().await.expect("Failed to create test service");

        // Call the hello tool
        let result = service
            .call_tool(CallToolRequestParam {
                name: "hello".into(),
                arguments: Some(json!({ "name": "world" }).as_object().unwrap().clone()),
            })
            .await
            .expect("Failed to call tool");

        // Verify the result
        assert_eq!(result.is_error, Some(false));
        assert_eq!(result.content.len(), 1);

        let text_content = result
            .content
            .iter()
            .find_map(|c| c.raw.as_text())
            .expect("Expected text content");

        assert_eq!(text_content.text, "Hello, world!");

        // Clean up
        service.cancel().await.expect("Failed to cancel service");
    }
}
