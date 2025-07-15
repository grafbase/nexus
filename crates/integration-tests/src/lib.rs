use std::net::SocketAddr;
use std::sync::Once;
use std::time::Duration;

use config::Config;
use rmcp::{
    model::CallToolRequestParam,
    service::{RunningService, ServiceExt},
    transport::{StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig},
};
use server::ServeConfig;
use tokio::net::TcpListener;
use tokio::time::timeout;

static INIT: Once = Once::new();

fn init_crypto_provider() {
    INIT.call_once(|| {
        rustls::crypto::aws_lc_rs::default_provider()
            .install_default()
            .expect("Failed to install default crypto provider");
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
        self.client
            .post(format!("{}{}", self.base_url, path))
            .json(body)
            .send()
            .await
    }

    /// Send a GET request to the given path
    pub async fn get(&self, path: &str) -> reqwest::Response {
        self.client
            .get(format!("{}{}", self.base_url, path))
            .send()
            .await
            .unwrap()
    }
}

/// MCP client for testing MCP protocol functionality
pub struct McpTestClient {
    service: RunningService<rmcp::RoleClient, ()>,
}

impl McpTestClient {
    /// Create a new MCP test client that connects to the given MCP endpoint URL
    pub async fn new(mcp_url: String) -> Self {
        let transport = if mcp_url.starts_with("https") {
            // For HTTPS, create a client that accepts self-signed certificates
            let client = reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .build()
                .unwrap();
            let config = StreamableHttpClientTransportConfig::with_uri(mcp_url.clone());
            StreamableHttpClientTransport::with_client(client, config)
        } else {
            StreamableHttpClientTransport::from_uri(mcp_url)
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

    /// Call a tool with the given name and arguments
    pub async fn call_tool(&self, name: &str, arguments: serde_json::Value) -> rmcp::model::CallToolResult {
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
    pub async fn call_tool_expect_error(&self, name: &str, arguments: serde_json::Value) -> rmcp::ServiceError {
        let arguments = arguments.as_object().cloned();
        self.service
            .call_tool(CallToolRequestParam {
                name: name.to_string().into(),
                arguments,
            })
            .await
            .unwrap_err()
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
    _handle: tokio::task::JoinHandle<()>,
}

impl TestServer {
    /// Start a new test server with the given TOML configuration
    pub async fn start(config_toml: &str) -> Self {
        // Initialize crypto provider for rustls
        init_crypto_provider();

        // Parse the configuration from TOML
        let config: Config = toml::from_str(config_toml).unwrap();

        // Find an available port
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
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
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Check if the server failed to start (non-blocking check)
        if let Ok(Err(e)) = rx.try_recv() {
            eprintln!("Server failed to start: {e}");
            std::process::exit(1);
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
        let mut retries = 10;
        while retries > 0 {
            if timeout(Duration::from_millis(100), client.get("/")).await.is_ok() {
                break;
            }
            retries -= 1;
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        TestServer {
            client,
            address,
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
}
