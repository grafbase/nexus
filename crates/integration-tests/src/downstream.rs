use axum::Router;
use core::fmt;
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
use std::{collections::BTreeMap, net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};
use tokio::{net::TcpListener, sync::Mutex};
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
    tools: Arc<Mutex<BTreeMap<String, Box<dyn TestTool>>>>,
    tls_config: Option<TlsConfig>,
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
            tools: Arc::new(Mutex::new(BTreeMap::new())),
            tls_config: None,
        }
    }

    pub fn r#type(&self) -> ServiceType {
        self.r#type
    }

    pub fn autodetect(&self) -> bool {
        self.autodetect
    }

    pub async fn add_tool(&mut self, tool: impl TestTool) {
        let mut tools = self.tools.lock().await;
        let name = tool.tool_definition().name.to_string();

        tools.insert(name, Box::new(tool));
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

    let (sse_server, router) = SseServer::new(sse_config);
    let tls_config = service.tls_config.clone();

    let service_ct = sse_server.with_service(move || {
        log::debug!("with_service closure called - creating service handler");
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

    let mcp_service = StreamableHttpService::new(
        move || Ok(service.clone()),
        Arc::new(NeverSessionManager::default()),
        StreamableHttpServerConfig {
            sse_keep_alive: Some(Duration::from_secs(5)),
            stateful_mode: false,
        },
    );

    let app = Router::new().route_service("/mcp", mcp_service);
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
        let tools = self
            .tools
            .lock()
            .await
            .values()
            .map(|tool| tool.tool_definition())
            .collect();

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
        let tools = self.tools.lock().await;

        let tool = tools.get(params.name.as_ref()).ok_or_else(|| ErrorData {
            code: ErrorCode(-32601),
            message: format!("Tool '{}' not found", params.name).into(),
            data: None,
        })?;

        tool.call(params).await
    }
}
