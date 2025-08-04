use axum::{
    Router,
    extract::Request,
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::Response,
};
use core::fmt;
use dashmap::DashMap;
use rmcp::{
    handler::server::ServerHandler,
    model::*,
    service::{RequestContext, RoleServer},
    transport::{
        sse_server::{SseServer, SseServerConfig},
        streamable_http_server::{
            StreamableHttpServerConfig, StreamableHttpService, session::never::NeverSessionManager,
        },
    },
};
use std::{net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use std::future::Future;
use std::pin::Pin;

pub trait TestTool: Send + Sync + 'static + std::fmt::Debug {
    fn tool_definition(&self) -> Tool;
    fn call(
        &self,
        params: CallToolRequestParam,
    ) -> Pin<Box<dyn Future<Output = Result<CallToolResult, ErrorData>> + Send + '_>>;
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
    let addr = SocketAddr::from(([127, 0, 0, 1], 0));
    let listener = TcpListener::bind(addr).await.unwrap();
    let address = listener.local_addr().unwrap();

    let ct = CancellationToken::new();

    let sse_config = SseServerConfig {
        // Use dummy bind address like grafbase - the actual binding happens with axum::serve
        bind: SocketAddr::from(([127, 0, 0, 1], 8080)),
        sse_path: "/mcp".to_string(),
        post_path: "/mcp".to_string(),
        ct: ct.clone(),
        sse_keep_alive: Some(Duration::from_secs(5)),
    };

    let (sse_server, mut router) = SseServer::new(sse_config);
    let tls_config = service.tls_config.clone();

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

    let service_ct = sse_server.with_service(move || {
        log::debug!("SSE server: initializing service handler for test server");
        service.clone()
    });

    // Create a combined cancellation token that cancels both when dropped
    let combined_ct = CancellationToken::new();
    let combined_ct_clone = combined_ct.clone();
    let ct_clone = ct.clone();

    tokio::spawn(async move {
        tokio::select! {
            _ = combined_ct_clone.cancelled() => {
                ct_clone.cancel();
                service_ct.cancel();
            }
        }
    });

    // Serve with TLS or regular depending on configuration
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
                    .expect("TLS SSE server failed");
            });
        }
        None => {
            tokio::spawn(async move {
                if let Err(e) = axum::serve(listener, router).await {
                    eprintln!("SSE server failed: {e}");
                }
            });
        }
    }

    // Give the SSE server time to fully initialize
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    (address, combined_ct)
}

async fn spawn_streamable_http(service: TestService) -> SocketAddr {
    // Check if TLS is configured before moving service
    let tls_config = service.tls_config.clone();
    let requires_auth = service.requires_auth();
    let expected_token = service.get_expected_token().cloned();

    let mcp_service = StreamableHttpService::new(
        move || Ok(service.clone()),
        Arc::new(NeverSessionManager::default()),
        StreamableHttpServerConfig {
            sse_keep_alive: Some(Duration::from_secs(5)),
            stateful_mode: false,
        },
    );

    let mut app = Router::new().route_service("/mcp", mcp_service);

    // Add authentication middleware if required
    if requires_auth {
        app = app.layer(middleware::from_fn(
            move |headers: HeaderMap, request: Request, next: Next| {
                let expected_token = expected_token.clone();
                async move { auth_middleware(headers, request, next, expected_token).await }
            },
        ));
    }

    let addr = SocketAddr::from(([127, 0, 0, 1], 0));
    let listener = TcpListener::bind(addr).await.unwrap();
    let address = listener.local_addr().unwrap();

    match tls_config {
        Some(tls_config) => {
            use axum_server::tls_rustls::RustlsConfig;

            let rustls_config = RustlsConfig::from_pem_file(&tls_config.cert_path, &tls_config.key_path)
                .await
                .expect("Failed to load TLS certificates");

            let std_listener = listener.into_std().unwrap();

            tokio::spawn(async move {
                axum_server::from_tcp_rustls(std_listener, rustls_config)
                    .serve(app.into_make_service())
                    .await
                    .unwrap();
            });
        }
        None => {
            tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
        }
    }

    address
}

impl ServerHandler for TestService {
    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParam>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let tools = self.tools.iter().map(|refer| refer.value().tool_definition()).collect();

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
        let tool = self.tools.get(params.name.as_ref()).ok_or_else(|| ErrorData {
            code: ErrorCode(-32601),
            message: format!("Tool '{}' not found", params.name).into(),
            data: None,
        })?;

        tool.call(params).await
    }

    async fn list_prompts(
        &self,
        _: Option<PaginatedRequestParam>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, ErrorData> {
        let prompts = self.prompts.iter().map(|refer| refer.value().clone()).collect();
        Ok(ListPromptsResult {
            prompts,
            next_cursor: None,
        })
    }

    async fn get_prompt(
        &self,
        params: GetPromptRequestParam,
        _: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, ErrorData> {
        let prompts = &self.prompts;
        let _prompt = prompts.get(params.name.as_str()).ok_or_else(|| ErrorData {
            code: ErrorCode(-32601),
            message: format!("Prompt '{}' not found", params.name).into(),
            data: None,
        })?;

        // Return a simple prompt result
        Ok(GetPromptResult {
            description: Some(format!("Test prompt: {}", params.name)),
            messages: vec![PromptMessage {
                role: PromptMessageRole::User,
                content: PromptMessageContent::Text {
                    text: format!("This is a test prompt named {}", params.name),
                },
            }],
        })
    }

    async fn list_resources(
        &self,
        _: Option<PaginatedRequestParam>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        let resources = self.resources.iter().map(|r| r.value().clone()).collect();
        Ok(ListResourcesResult {
            resources,
            next_cursor: None,
        })
    }

    async fn read_resource(
        &self,
        params: ReadResourceRequestParam,
        _: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, ErrorData> {
        let resources = &self.resources;
        let _resource = resources.get(params.uri.as_str()).ok_or_else(|| ErrorData {
            code: ErrorCode(-32601),
            message: format!("Resource '{}' not found", params.uri).into(),
            data: None,
        })?;

        // Return simple resource content
        Ok(ReadResourceResult {
            contents: vec![], // For now, return empty contents to get compilation working
        })
    }
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
