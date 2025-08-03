mod downstream;
pub mod tools;

use std::str::FromStr;
use std::sync::Once;
use std::time::Duration;
use std::{net::SocketAddr, path::PathBuf};

use config::Config;
use logforth::filter::EnvFilter;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use rmcp::{
    model::CallToolRequestParam,
    service::{RunningService, ServiceExt},
    transport::{StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig},
};
use serde_json::json;
use server::ServeConfig;
use tokio::net::TcpListener;

pub use downstream::{ServiceType, TestService, TestTool};
use tokio_util::sync::CancellationToken;

pub fn get_test_cert_paths() -> (PathBuf, PathBuf) {
    let cert_path = PathBuf::from("test-certs/cert.pem");
    let key_path = PathBuf::from("test-certs/key.pem");

    (cert_path, key_path)
}

static INIT: Once = Once::new();
static LOGGER_INIT: Once = Once::new();

#[ctor::ctor]
fn init_crypto_provider() {
    INIT.call_once(|| {
        rustls::crypto::aws_lc_rs::default_provider()
            .install_default()
            .expect("Failed to install default crypto provider");
    });
}

/// Initialize logger for integration tests
/// Logs will only be shown when TEST_LOG environment variable is set
#[ctor::ctor]
fn init_logger() {
    LOGGER_INIT.call_once(|| {
        // Only initialize logger if TEST_LOG is set
        // This allows running tests with `TEST_LOG=1 cargo test` to see logs
        if std::env::var("TEST_LOG").is_ok() {
            logforth::builder()
                .dispatch(|d| {
                    d.filter(EnvFilter::from_str("warn,server=debug,mcp=debug,config=debug").unwrap())
                        .append(logforth::append::Stderr::default())
                })
                .apply();
        }
    });
}

/// Test client for making HTTP requests to the test server
pub struct TestClient {
    base_url: String,
    client: reqwest::Client,
}

impl TestClient {
    /// Create a new test client for the given base URL
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            client: reqwest::Client::new(),
        }
    }

    /// Create a new test client that accepts invalid TLS certificates
    pub fn new_with_tls(base_url: String) -> Self {
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .build()
            .expect("Failed to create client with invalid cert acceptance");

        Self { base_url, client }
    }

    /// Send a POST request to the given path with JSON body
    pub async fn post<T: serde::Serialize>(&self, path: &str, body: &T) -> reqwest::Result<reqwest::Response> {
        let mut req = self.client
            .post(format!("{}{}", self.base_url, path))
            .json(body);
            
        // Add MCP headers if this is an MCP endpoint
        if path == "/mcp" {
            req = req.header("Accept", "application/json, text/event-stream");
        }
        
        req.send().await
    }

    /// Send a GET request to the given path
    pub async fn get(&self, path: &str) -> reqwest::Response {
        self.client
            .get(format!("{}{}", self.base_url, path))
            .send()
            .await
            .unwrap()
    }

    /// Send a GET request to the given path, returning Result instead of panicking
    pub async fn try_get(&self, path: &str) -> reqwest::Result<reqwest::Response> {
        self.client.get(format!("{}{}", self.base_url, path)).send().await
    }

    /// Send a custom HTTP request for CORS testing
    pub fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.client.request(method, format!("{}{}", self.base_url, path))
    }

    /// Get the base URL of this test client
    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

/// MCP client for testing MCP protocol functionality
pub struct McpTestClient {
    service: RunningService<rmcp::RoleClient, ()>,
}

impl McpTestClient {
    /// Create a new MCP test client that connects to the given MCP endpoint URL
    pub async fn new(mcp_url: String) -> Self {
        Self::new_with_auth(mcp_url, None).await
    }

    /// Create a new MCP test client with OAuth2 authentication
    pub async fn new_with_auth(mcp_url: String, auth_token: Option<&str>) -> Self {
        let transport = if mcp_url.starts_with("https") {
            // For HTTPS, create a client that accepts self-signed certificates
            let mut builder = reqwest::Client::builder().danger_accept_invalid_certs(true);

            // Add OAuth2 authentication if provided
            if let Some(token) = auth_token {
                let mut headers = HeaderMap::new();
                let auth_value = HeaderValue::from_str(&format!("Bearer {token}")).unwrap();

                headers.insert(AUTHORIZATION, auth_value);
                builder = builder.default_headers(headers);
            }

            let client = builder.build().unwrap();
            let config = StreamableHttpClientTransportConfig::with_uri(mcp_url.clone());
            StreamableHttpClientTransport::with_client(client, config)
        } else {
            // For HTTP, create a client with optional authentication
            if let Some(token) = auth_token {
                let mut headers = HeaderMap::new();
                let auth_value = HeaderValue::from_str(&format!("Bearer {token}")).unwrap();

                headers.insert(AUTHORIZATION, auth_value);

                let client = reqwest::Client::builder().default_headers(headers).build().unwrap();
                let config = StreamableHttpClientTransportConfig::with_uri(mcp_url.clone());

                StreamableHttpClientTransport::with_client(client, config)
            } else {
                StreamableHttpClientTransport::from_uri(mcp_url)
            }
        };

        let service = ().serve(transport).await.unwrap();

        Self { service }
    }

    /// Get server information
    pub fn get_server_info(&self) -> &rmcp::model::InitializeResult {
        self.service.peer_info().unwrap()
    }

    /// List available tools
    pub async fn list_tools(&self) -> rmcp::model::ListToolsResult {
        self.service.list_tools(Default::default()).await.unwrap()
    }

    pub async fn search(&self, keywords: &[&str]) -> Vec<serde_json::Value> {
        self.call_tool("search", json!({ "keywords": keywords }))
            .await
            .content
            .into_iter()
            .filter_map(|content| match content.raw.as_text() {
                Some(content) => serde_json::from_str(&content.text).ok(),
                None => todo!(),
            })
            .collect()
    }

    pub async fn execute(&self, tool: &str, arguments: serde_json::Value) -> rmcp::model::CallToolResult {
        let arguments = json!({
            "name": tool,
            "arguments": arguments,
        });

        self.call_tool("execute", arguments).await
    }

    pub async fn execute_expect_error(&self, tool: &str, arguments: serde_json::Value) -> rmcp::ServiceError {
        let arguments = json!({
            "name": tool,
            "arguments": arguments,
        });

        self.call_tool_expect_error("execute", arguments).await
    }

    /// Call a tool with the given name and arguments
    async fn call_tool(&self, name: &str, arguments: serde_json::Value) -> rmcp::model::CallToolResult {
        let arguments = arguments.as_object().cloned();
        self.service
            .call_tool(CallToolRequestParam {
                name: name.to_string().into(),
                arguments,
            })
            .await
            .unwrap()
    }

    /// Call a tool and expect it to fail
    async fn call_tool_expect_error(&self, name: &str, arguments: serde_json::Value) -> rmcp::ServiceError {
        let arguments = arguments.as_object().cloned();
        self.service
            .call_tool(CallToolRequestParam {
                name: name.to_string().into(),
                arguments,
            })
            .await
            .unwrap_err()
    }

    /// List available prompts
    pub async fn list_prompts(&self) -> rmcp::model::ListPromptsResult {
        self.service.list_prompts(Default::default()).await.unwrap()
    }

    /// Get a prompt by name
    pub async fn get_prompt(
        &self,
        name: &str,
        arguments: Option<serde_json::Map<String, serde_json::Value>>,
    ) -> rmcp::model::GetPromptResult {
        self.service
            .get_prompt(rmcp::model::GetPromptRequestParam {
                name: name.to_string(),
                arguments,
            })
            .await
            .unwrap()
    }

    /// List available resources
    pub async fn list_resources(&self) -> rmcp::model::ListResourcesResult {
        self.service.list_resources(Default::default()).await.unwrap()
    }

    /// Read a resource by URI
    pub async fn read_resource(&self, uri: &str) -> rmcp::model::ReadResourceResult {
        self.service
            .read_resource(rmcp::model::ReadResourceRequestParam { uri: uri.to_string() })
            .await
            .unwrap()
    }

    /// Disconnect the client
    pub async fn disconnect(self) {
        self.service.cancel().await.unwrap();
    }
}

/// Test server that manages the lifecycle of a server instance
pub struct TestServer {
    pub client: TestClient,
    pub address: SocketAddr,
    pub cancellation_tokens: Vec<CancellationToken>,
    _handle: tokio::task::JoinHandle<()>,
}

impl TestServer {
    pub fn builder() -> TestServerBuilder {
        TestServerBuilder::default()
    }

    /// Start a new test server with the given TOML configuration
    async fn start(config_toml: &str, cancellation_tokens: Vec<CancellationToken>) -> Self {
        // Parse the configuration from TOML
        let config: Config = toml::from_str(config_toml).unwrap();

        // Find an available port
        let mut listener = TcpListener::bind("127.0.0.1:0").await;

        #[allow(clippy::panic)]
        while let Err(e) = listener {
            if e.kind() == std::io::ErrorKind::AddrInUse {
                listener = TcpListener::bind("127.0.0.1:0").await;
            } else {
                panic!("Failed to bind to address: {e}");
            }
        }

        let listener = listener.unwrap();

        let address = listener.local_addr().unwrap();

        // Check if TLS is configured before moving config into spawn task
        let has_tls = config.server.tls.is_some();

        // Create the server configuration
        let serve_config = ServeConfig {
            listen_address: address,
            config,
        };

        // Start the server in a background task
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        let handle = tokio::spawn(async move {
            // Drop the listener so the server can bind to the address
            drop(listener);

            match server::serve(serve_config).await {
                Ok(()) => {
                    let _ = tx.send(Ok(()));
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                }
            }
        });

        // Wait for the server to start up or fail
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Check if the server failed to start (non-blocking check)
        #[allow(clippy::panic)]
        if let Ok(Err(e)) = rx.try_recv() {
            panic!("Server failed to start: {e}");
        }

        // Create the test client - use HTTPS if TLS is configured
        let protocol = if has_tls { "https" } else { "http" };
        let base_url = format!("{protocol}://{address}");

        let client = if has_tls {
            TestClient::new_with_tls(base_url)
        } else {
            TestClient::new(base_url)
        };

        // Verify the server is actually running by making a simple request
        let mut retries = 30;
        let mut last_error = None;

        while retries > 0 {
            match client.try_get("/health").await {
                Ok(_) => break,
                Err(e) => {
                    last_error = Some(e);
                }
            }
            retries -= 1;
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        if retries == 0 {
            #[allow(clippy::panic)]
            if let Some(e) = last_error {
                panic!("Server failed to become ready after 30 retries. Last error: {e}");
            } else {
                panic!("Server failed to become ready after 30 retries. No specific error.");
            }
        }

        TestServer {
            client,
            address,
            cancellation_tokens,
            _handle: handle,
        }
    }

    /// Create an MCP client that connects to this server's MCP endpoint
    pub async fn mcp_client(&self, path: &str) -> McpTestClient {
        let protocol = if self.client.base_url.starts_with("https") {
            "https"
        } else {
            "http"
        };

        let mcp_url = format!("{protocol}://{}{}", self.address, path);

        McpTestClient::new(mcp_url).await
    }

    /// Create an MCP client with OAuth2 authentication
    pub async fn mcp_client_with_auth(&self, path: &str, auth_token: &str) -> McpTestClient {
        let protocol = if self.client.base_url.starts_with("https") {
            "https"
        } else {
            "http"
        };

        let mcp_url = format!("{protocol}://{}{}", self.address, path);

        McpTestClient::new_with_auth(mcp_url, Some(auth_token)).await
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        for token in &self.cancellation_tokens {
            token.cancel();
        }
    }
}

#[derive(Default)]
pub struct TestServerBuilder {
    config: String,
    cancellation_tokens: Vec<CancellationToken>,
}

impl TestServerBuilder {
    pub async fn spawn_service(&mut self, service: TestService) {
        let (listen_addr, ct) = service.spawn().await;

        if let Some(ct) = ct {
            self.cancellation_tokens.push(ct);
        }

        let protocol = if service.is_tls() { "https" } else { "http" };

        let mut config = match service.r#type() {
            _ if service.autodetect() => {
                indoc::formatdoc! {r#"
                    [mcp.servers.{}]
                    url = "{protocol}://{listen_addr}/mcp"
                "#, service.name()}
            }
            ServiceType::Sse => {
                indoc::formatdoc! {r#"
                    [mcp.servers.{}]
                    protocol = "sse"
                    url = "{protocol}://{listen_addr}/mcp"
                "#, service.name()}
            }
            ServiceType::StreamableHttp => {
                indoc::formatdoc! {r#"
                    [mcp.servers.{}]
                    protocol = "streamable-http"
                    url = "{protocol}://{listen_addr}/mcp"
                "#, service.name()}
            }
        };

        // Add TLS configuration if the service has TLS enabled
        if let Some((cert_path, key_path)) = service.get_tls_cert_paths() {
            let tls_config = indoc::formatdoc! {r#"

                [mcp.servers.{}.tls]
                verify_certs = false
                accept_invalid_hostnames = true
                root_ca_cert_path = "{cert_path}"
                client_cert_path = "{cert_path}"
                client_key_path = "{key_path}"
            "#, service.name(), cert_path = cert_path.display(), key_path = key_path.display()};

            config.push_str(&tls_config);
        }

        // Add authentication configuration if the service has auth token
        if let Some(token) = service.get_auth_token() {
            let auth_config = indoc::formatdoc! {r#"

                [mcp.servers.{}.auth]
                token = "{token}"
            "#, service.name()};

            config.push_str(&auth_config);
        } else if service.forwards_auth() {
            let auth_config = indoc::formatdoc! {r#"

                [mcp.servers.{}.auth]
                type = "forward"
            "#, service.name()};

            config.push_str(&auth_config);
        }

        self.config.push_str(&format!("\n{config}"));
    }

    pub async fn build(self, config: &str) -> TestServer {
        let config = format!("{config}\n{}", self.config);

        TestServer::start(&config, self.cancellation_tokens).await
    }
}
