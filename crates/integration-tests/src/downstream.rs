use axum::{
    Router,
    extract::Request,
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::Response,
};
use core::fmt;
use dashmap::DashMap;
use pmcp::types::*;
use pmcp::types::Implementation;
use pmcp::{ProtocolVersion, ServerCapabilities as PmcpServerCapabilities};
use std::{net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use std::future::Future;
use std::pin::Pin;

pub trait TestTool: Send + Sync + 'static + std::fmt::Debug {
    fn tool_definition(&self) -> Tool;
    fn call(
        &self,
        params: CallToolRequest,
    ) -> Pin<Box<dyn Future<Output = Result<CallToolResult, pmcp::Error>> + Send + '_>>;
}

#[derive(Clone, Copy)]
pub enum ServiceType {
    Sse,
    StreamableHttp,
}

impl fmt::Display for ServiceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServiceType::Sse => write!(f, "sse"),
            ServiceType::StreamableHttp => write!(f, "streamable-http"),
        }
    }
}

#[derive(Clone)]
pub struct TestService {
    name: String,
    r#type: ServiceType,
    autodetect: bool,
    tools: Arc<DashMap<String, Box<dyn TestTool>>>,
    prompts: Arc<DashMap<String, Prompt>>,
    resources: Arc<DashMap<String, Resource>>,
    tls_config: Option<TlsConfig>,
    auth_token: Option<String>,
    require_auth: bool,
    expected_token: Option<String>,
    forward_auth: bool,
}

#[derive(Clone)]
pub struct TlsConfig {
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
}

impl TestService {
    pub fn sse(name: String) -> Self {
        Self::new(name, ServiceType::Sse, false)
    }

    pub fn sse_autodetect(name: String) -> Self {
        Self::new(name, ServiceType::Sse, true)
    }

    pub fn streamable_http(name: String) -> Self {
        Self::new(name, ServiceType::StreamableHttp, false)
    }

    pub fn streamable_http_autodetect(name: String) -> Self {
        Self::new(name, ServiceType::StreamableHttp, true)
    }

    fn new(name: String, r#type: ServiceType, autodetect: bool) -> Self {
        Self {
            name,
            r#type,
            autodetect,
            tools: Arc::new(DashMap::new()),
            prompts: Arc::new(DashMap::new()),
            resources: Arc::new(DashMap::new()),
            tls_config: None,
            auth_token: None,
            require_auth: false,
            expected_token: None,
            forward_auth: false,
        }
    }

    pub fn r#type(&self) -> ServiceType {
        self.r#type
    }

    pub fn autodetect(&self) -> bool {
        self.autodetect
    }

    pub fn add_tool(&mut self, tool: impl TestTool) {
        let name = tool.tool_definition().name.to_string();
        self.tools.insert(name, Box::new(tool));
    }

    pub fn add_prompt(&mut self, prompt: Prompt) {
        self.prompts.insert(prompt.name.to_string(), prompt);
    }

    pub fn add_resource(&mut self, resource: Resource) {
        self.resources.insert(resource.uri.to_string(), resource);
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn with_tls(mut self, cert_path: PathBuf, key_path: PathBuf) -> Self {
        self.tls_config = Some(TlsConfig { cert_path, key_path });
        self
    }

    pub(super) fn is_tls(&self) -> bool {
        self.tls_config.is_some()
    }

    pub fn with_auth_token(mut self, token: String) -> Self {
        self.auth_token = Some(token);
        self
    }

    pub fn get_auth_token(&self) -> Option<&String> {
        self.auth_token.as_ref()
    }

    pub fn with_required_auth_token(mut self, expected_token: String) -> Self {
        self.require_auth = true;
        self.expected_token = Some(expected_token);
        self
    }

    pub fn requires_auth(&self) -> bool {
        self.require_auth
    }

    pub fn get_expected_token(&self) -> Option<&String> {
        self.expected_token.as_ref()
    }

    pub fn with_forward_auth(mut self) -> Self {
        self.forward_auth = true;
        self
    }

    pub fn forwards_auth(&self) -> bool {
        self.forward_auth
    }

    pub fn get_tls_cert_paths(&self) -> Option<(PathBuf, PathBuf)> {
        self.tls_config
            .as_ref()
            .map(|config| (config.cert_path.clone(), config.key_path.clone()))
    }

    pub async fn spawn(&self) -> (SocketAddr, Option<CancellationToken>) {
        let service = self.clone();

        match self.r#type {
            ServiceType::StreamableHttp => {
                let addr = spawn_streamable_http(service).await;
                (addr, None)
            }
            ServiceType::Sse => {
                let (addr, ct) = spawn_sse(service).await;
                (addr, Some(ct))
            }
        }
    }
}

async fn spawn_sse(service: TestService) -> (SocketAddr, CancellationToken) {
    // For pmcp, we'll create a simple HTTP server that handles MCP requests
    let addr = SocketAddr::from(([127, 0, 0, 1], 0));
    let listener = TcpListener::bind(addr).await.unwrap();
    let address = listener.local_addr().unwrap();

    let ct = CancellationToken::new();
    let service = Arc::new(service);
    
    // Create HTTP router for MCP requests
    let mut router = Router::new()
        .route("/mcp", axum::routing::post(handle_mcp_request))
        .with_state(service.clone());

    // Add authentication middleware if required
    if service.requires_auth() {
        let expected_token = service.get_expected_token().cloned();
        router = router.layer(middleware::from_fn(
            move |headers: HeaderMap, request: Request, next: Next| {
                let expected_token = expected_token.clone();
                async move { auth_middleware(headers, request, next, expected_token).await }
            },
        ));
    }

    let tls_config = service.tls_config.clone();
    let ct_clone = ct.clone();

    // Serve with TLS or regular depending on configuration
    match tls_config {
        Some(tls_config) => {
            use axum_server::tls_rustls::RustlsConfig;

            let rustls_config = RustlsConfig::from_pem_file(&tls_config.cert_path, &tls_config.key_path)
                .await
                .expect("Failed to load TLS certificates");

            let std_listener = listener.into_std().unwrap();

            tokio::spawn(async move {
                tokio::select! {
                    _ = axum_server::from_tcp_rustls(std_listener, rustls_config)
                        .serve(router.into_make_service())
                        => {},
                    _ = ct_clone.cancelled() => {},
                }
            });
        }
        None => {
            tokio::spawn(async move {
                tokio::select! {
                    _ = axum::serve(listener, router) => {},
                    _ = ct_clone.cancelled() => {},
                }
            });
        }
    }

    // Give the server time to fully initialize
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    (address, ct)
}

async fn spawn_streamable_http(service: TestService) -> SocketAddr {
    // For pmcp, use the same HTTP server implementation as SSE
    let addr = SocketAddr::from(([127, 0, 0, 1], 0));
    let listener = TcpListener::bind(addr).await.unwrap();
    let address = listener.local_addr().unwrap();
    
    let service = Arc::new(service);
    
    // Create HTTP router for MCP requests
    let mut router = Router::new()
        .route("/mcp", axum::routing::post(handle_mcp_request))
        .with_state(service.clone());

    // Add authentication middleware if required
    if service.requires_auth() {
        let expected_token = service.get_expected_token().cloned();
        router = router.layer(middleware::from_fn(
            move |headers: HeaderMap, request: Request, next: Next| {
                let expected_token = expected_token.clone();
                async move { auth_middleware(headers, request, next, expected_token).await }
            },
        ));
    }

    let tls_config = service.tls_config.clone();

    match tls_config {
        Some(tls_config) => {
            use axum_server::tls_rustls::RustlsConfig;

            let rustls_config = RustlsConfig::from_pem_file(&tls_config.cert_path, &tls_config.key_path)
                .await
                .expect("Failed to load TLS certificates");

            let std_listener = listener.into_std().unwrap();

            tokio::spawn(async move {
                axum_server::from_tcp_rustls(std_listener, rustls_config)
                    .serve(router.into_make_service())
                    .await
                    .unwrap();
            });
        }
        None => {
            tokio::spawn(async move {
                axum::serve(listener, router).await.unwrap();
            });
        }
    }

    // Give the server time to fully initialize
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    
    address
}

// Handler implementation for TestService to work with pmcp
impl TestService {
    pub async fn initialize(&self) -> Result<InitializeResult, pmcp::Error> {
        Ok(InitializeResult {
            protocol_version: ProtocolVersion("2024-11-05".to_string()),
            capabilities: PmcpServerCapabilities {
                tools: Some(Default::default()),
                prompts: Some(Default::default()),
                resources: Some(Default::default()),
                ..Default::default()
            },
            server_info: Implementation {
                name: self.name.clone(),
                version: "0.1.0".to_string(),
            },
            instructions: None,
        })
    }

    pub async fn list_tools(&self) -> Result<ListToolsResult, pmcp::Error> {
        let tools = self.tools.iter().map(|refer| refer.value().tool_definition()).collect();

        Ok(ListToolsResult {
            tools,
            next_cursor: None,
        })
    }

    pub async fn call_tool(&self, request: CallToolRequest) -> Result<CallToolResult, pmcp::Error> {
        let tool = self.tools.get(&request.name).ok_or_else(|| 
            pmcp::Error::method_not_found(format!("Tool '{}' not found", request.name))
        )?;

        tool.call(request).await
    }

    pub async fn list_prompts(&self) -> Result<ListPromptsResult, pmcp::Error> {
        let prompts = self.prompts.iter().map(|refer| refer.value().clone()).collect();
        Ok(ListPromptsResult {
            prompts,
            next_cursor: None,
        })
    }

    pub async fn get_prompt(&self, request: GetPromptRequest) -> Result<GetPromptResult, pmcp::Error> {
        let prompts = &self.prompts;
        let _prompt = prompts.get(&request.name).ok_or_else(|| 
            pmcp::Error::method_not_found(format!("Prompt '{}' not found", request.name))
        )?;

        // Return a simple prompt result
        Ok(GetPromptResult {
            description: Some(format!("Test prompt: {}", request.name)),
            messages: vec![PromptMessage {
                role: PromptMessageRole::User,
                content: PromptMessageContent::Text {
                    text: format!("This is a test prompt named {}", request.name),
                },
            }],
        })
    }

    pub async fn list_resources(&self) -> Result<ListResourcesResult, pmcp::Error> {
        let resources = self.resources.iter().map(|r| r.value().clone()).collect();
        Ok(ListResourcesResult {
            resources,
            next_cursor: None,
        })
    }

    pub async fn read_resource(&self, request: ReadResourceRequest) -> Result<ReadResourceResult, pmcp::Error> {
        let resources = &self.resources;
        let _resource = resources.get(&request.uri).ok_or_else(|| 
            pmcp::Error::method_not_found(format!("Resource '{}' not found", request.uri))
        )?;

        // Return simple resource content
        Ok(ReadResourceResult {
            contents: vec![], // For now, return empty contents to get compilation working
        })
    }
}

/// Handle MCP requests for the test server
async fn handle_mcp_request(
    axum::extract::State(service): axum::extract::State<Arc<TestService>>,
    body: String,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    use pmcp::types::jsonrpc::{JSONRPCRequest, JSONRPCResponse, ResponsePayload, JSONRPCError};
    
    // Parse the request
    let request: JSONRPCRequest<serde_json::Value> = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(e) => {
            let error_response = JSONRPCResponse::<serde_json::Value, JSONRPCError> {
                jsonrpc: "2.0".to_string(),
                id: pmcp::types::RequestId::String("unknown".to_string()),
                payload: ResponsePayload::Error(JSONRPCError {
                    code: -32700,
                    message: format!("Parse error: {}", e),
                    data: None,
                }),
            };
            return (StatusCode::BAD_REQUEST, axum::Json(error_response)).into_response();
        }
    };
    
    // Handle different methods
    let result = match request.method.as_str() {
        "initialize" => {
            let init_result = service.initialize().await;
            match init_result {
                Ok(result) => ResponsePayload::Result(serde_json::to_value(result).unwrap()),
                Err(e) => ResponsePayload::Error(JSONRPCError {
                    code: -32603,
                    message: e.to_string(),
                    data: None,
                }),
            }
        }
        "tools/list" => {
            let list_result = service.list_tools().await;
            match list_result {
                Ok(result) => ResponsePayload::Result(serde_json::to_value(result).unwrap()),
                Err(e) => ResponsePayload::Error(JSONRPCError {
                    code: -32603,
                    message: e.to_string(),
                    data: None,
                }),
            }
        }
        "tools/call" => {
            let params = match serde_json::from_value::<CallToolRequest>(request.params.clone().unwrap_or_default()) {
                Ok(p) => p,
                Err(e) => {
                    let error_response = JSONRPCResponse::<serde_json::Value, JSONRPCError> {
                        jsonrpc: "2.0".to_string(),
                        id: request.id.clone(),
                        payload: ResponsePayload::Error(JSONRPCError {
                            code: -32602,
                            message: format!("Invalid parameters: {}", e),
                            data: None,
                        }),
                    };
                    return (StatusCode::OK, axum::Json(error_response)).into_response();
                }
            };
            
            let call_result = service.call_tool(params).await;
            match call_result {
                Ok(result) => ResponsePayload::Result(serde_json::to_value(result).unwrap()),
                Err(e) => ResponsePayload::Error(JSONRPCError {
                    code: -32603,
                    message: e.to_string(),
                    data: None,
                }),
            }
        }
        "prompts/list" => {
            let list_result = service.list_prompts().await;
            match list_result {
                Ok(result) => ResponsePayload::Result(serde_json::to_value(result).unwrap()),
                Err(e) => ResponsePayload::Error(JSONRPCError {
                    code: -32603,
                    message: e.to_string(),
                    data: None,
                }),
            }
        }
        "prompts/get" => {
            let params = match serde_json::from_value::<GetPromptRequest>(request.params.clone().unwrap_or_default()) {
                Ok(p) => p,
                Err(e) => {
                    let error_response = JSONRPCResponse::<serde_json::Value, JSONRPCError> {
                        jsonrpc: "2.0".to_string(),
                        id: request.id.clone(),
                        payload: ResponsePayload::Error(JSONRPCError {
                            code: -32602,
                            message: format!("Invalid parameters: {}", e),
                            data: None,
                        }),
                    };
                    return (StatusCode::OK, axum::Json(error_response)).into_response();
                }
            };
            
            let get_result = service.get_prompt(params).await;
            match get_result {
                Ok(result) => ResponsePayload::Result(serde_json::to_value(result).unwrap()),
                Err(e) => ResponsePayload::Error(JSONRPCError {
                    code: -32603,
                    message: e.to_string(),
                    data: None,
                }),
            }
        }
        "resources/list" => {
            let list_result = service.list_resources().await;
            match list_result {
                Ok(result) => ResponsePayload::Result(serde_json::to_value(result).unwrap()),
                Err(e) => ResponsePayload::Error(JSONRPCError {
                    code: -32603,
                    message: e.to_string(),
                    data: None,
                }),
            }
        }
        "resources/read" => {
            let params = match serde_json::from_value::<ReadResourceRequest>(request.params.clone().unwrap_or_default()) {
                Ok(p) => p,
                Err(e) => {
                    let error_response = JSONRPCResponse::<serde_json::Value, JSONRPCError> {
                        jsonrpc: "2.0".to_string(),
                        id: request.id.clone(),
                        payload: ResponsePayload::Error(JSONRPCError {
                            code: -32602,
                            message: format!("Invalid parameters: {}", e),
                            data: None,
                        }),
                    };
                    return (StatusCode::OK, axum::Json(error_response)).into_response();
                }
            };
            
            let read_result = service.read_resource(params).await;
            match read_result {
                Ok(result) => ResponsePayload::Result(serde_json::to_value(result).unwrap()),
                Err(e) => ResponsePayload::Error(JSONRPCError {
                    code: -32603,
                    message: e.to_string(),
                    data: None,
                }),
            }
        }
        _ => {
            ResponsePayload::Error(JSONRPCError {
                code: -32601,
                message: format!("Method not found: {}", request.method),
                data: None,
            })
        }
    };
    
    let response = JSONRPCResponse::<serde_json::Value, JSONRPCError> {
        jsonrpc: "2.0".to_string(),
        id: request.id,
        payload: result,
    };
    
    (StatusCode::OK, axum::Json(response)).into_response()
}

/// Middleware that validates Bearer token authentication
async fn auth_middleware(
    headers: HeaderMap,
    request: Request,
    next: Next,
    expected_token: Option<String>,
) -> Result<Response, StatusCode> {
    let auth_header = headers.get("authorization").and_then(|h| h.to_str().ok());

    match (auth_header, expected_token) {
        (Some(auth), Some(expected)) if auth == format!("Bearer {expected}") => {
            // Valid token, proceed
            Ok(next.run(request).await)
        }
        (Some(auth), Some(_)) if auth.starts_with("Bearer ") => {
            // Invalid token
            Err(StatusCode::UNAUTHORIZED)
        }
        (Some(_), Some(_)) => {
            // Invalid auth format
            Err(StatusCode::BAD_REQUEST)
        }
        (None, Some(_)) => {
            // No auth header when auth is required
            Err(StatusCode::UNAUTHORIZED)
        }
        (_, None) => {
            // Auth not required, proceed
            Ok(next.run(request).await)
        }
    }
}
