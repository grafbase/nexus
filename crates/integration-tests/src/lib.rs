mod downstream;
pub mod headers;
pub mod llms;
pub mod telemetry;
pub mod tools;

use std::sync::Once;
use std::time::Duration;
use std::{net::SocketAddr, path::PathBuf};

use config::Config;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue};
use rmcp::{
    model::CallToolRequestParam,
    service::{RunningService, ServiceExt},
    transport::{StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig},
};
use serde_json::json;
use server::ServeConfig;
use tokio::net::TcpListener;

pub use downstream::{ServiceType, TestService, TestTool};
pub use llms::TestOpenAIServer;
use tokio_util::sync::CancellationToken;

pub fn get_test_cert_paths() -> (PathBuf, PathBuf) {
    let cert_path = PathBuf::from("test-certs/cert.pem");
    let key_path = PathBuf::from("test-certs/key.pem");

    (cert_path, key_path)
}

static INIT: Once = Once::new();

#[ctor::ctor]
fn init_crypto_provider() {
    INIT.call_once(|| {
        rustls::crypto::aws_lc_rs::default_provider()
            .install_default()
            .expect("Failed to install default crypto provider");
    });
}

/// Test client for making HTTP requests to the test server
#[derive(Clone)]
pub struct TestClient {
    base_url: String,
    client: reqwest::Client,
    custom_headers: HeaderMap,
}

impl TestClient {
    /// Create a new test client for the given base URL
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            client: reqwest::Client::new(),
            custom_headers: HeaderMap::new(),
        }
    }

    /// Create a new test client that accepts invalid TLS certificates
    pub fn new_with_tls(base_url: String) -> Self {
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .build()
            .expect("Failed to create client with invalid cert acceptance");

        Self {
            base_url,
            client,
            custom_headers: HeaderMap::new(),
        }
    }

    /// Add a custom header to be included in all requests
    pub fn push_header(&mut self, key: &str, value: impl AsRef<str>) {
        let header_name = reqwest::header::HeaderName::from_bytes(key.as_bytes()).unwrap();
        let header_value = HeaderValue::from_str(value.as_ref()).unwrap();
        self.custom_headers.insert(header_name, header_value);
    }

    /// Send a POST request to the given path with JSON body
    pub async fn post<T: serde::Serialize>(&self, path: &str, body: &T) -> reqwest::Result<reqwest::Response> {
        let mut req = self.client.post(format!("{}{}", self.base_url, path)).json(body);

        // Add custom headers
        for (key, value) in &self.custom_headers {
            req = req.header(key.clone(), value.clone());
        }

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

    /// Create a request with the given method and path
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
        let mut headers = HeaderMap::new();
        if let Some(token) = auth_token {
            let auth_value = HeaderValue::from_str(&format!("Bearer {token}")).unwrap();
            headers.insert(AUTHORIZATION, auth_value);
        }
        Self::new_with_headers(mcp_url, headers).await
    }

    /// Create a new MCP test client with custom headers
    pub async fn new_with_headers(mcp_url: String, headers: HeaderMap) -> Self {
        let transport = if mcp_url.starts_with("https") {
            // For HTTPS, create a client that accepts self-signed certificates
            let client = reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .default_headers(headers)
                .build()
                .unwrap();
            let config = StreamableHttpClientTransportConfig::with_uri(mcp_url.clone());
            StreamableHttpClientTransport::with_client(client, config)
        } else {
            // For HTTP, create a client with custom headers
            if headers.is_empty() {
                StreamableHttpClientTransport::from_uri(mcp_url)
            } else {
                let client = reqwest::Client::builder().default_headers(headers).build().unwrap();
                let config = StreamableHttpClientTransportConfig::with_uri(mcp_url.clone());
                StreamableHttpClientTransport::with_client(client, config)
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
        let result = self.call_tool("search", json!({ "keywords": keywords })).await;

        // Prefer structured_content if available (new in rmcp 0.4.0)
        if let Some(structured) = result.structured_content {
            // The structured content is wrapped in a "results" object to work around MCP Inspector bug
            if let Some(obj) = structured.as_object()
                && let Some(results) = obj.get("results")
            {
                return results.as_array().cloned().unwrap_or_default();
            }
            // Fallback to treating it as an array directly for backward compatibility
            structured.as_array().cloned().unwrap_or_default()
        } else if !result.content.is_empty() {
            // Fallback to parsing from content field (legacy behavior)
            result
                .content
                .into_iter()
                .filter_map(|content| match content.raw.as_text() {
                    Some(content) => serde_json::from_str(&content.text).ok(),
                    None => None,
                })
                .collect()
        } else {
            // Neither structured_content nor content is available
            Vec::new()
        }
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

    /// Get a specific prompt (returns Result for error testing)
    pub async fn get_prompt_result(
        &self,
        name: &str,
        arguments: Option<serde_json::Map<String, serde_json::Value>>,
    ) -> Result<rmcp::model::GetPromptResult, rmcp::ServiceError> {
        self.service
            .get_prompt(rmcp::model::GetPromptRequestParam {
                name: name.to_string(),
                arguments,
            })
            .await
    }

    /// Read a specific resource (returns Result for error testing)
    pub async fn read_resource_result(&self, uri: &str) -> Result<rmcp::model::ReadResourceResult, rmcp::ServiceError> {
        self.service
            .read_resource(rmcp::model::ReadResourceRequestParam { uri: uri.to_string() })
            .await
    }

    /// Disconnect the client
    pub async fn disconnect(self) {
        self.service.cancel().await.unwrap();
    }
}

/// Builder for OpenAI completions requests with fluent API for headers
pub struct OpenAICompletionsRequest<'a> {
    test_server: &'a TestServer,
    request: serde_json::Value,
    headers: HeaderMap,
}

impl<'a> OpenAICompletionsRequest<'a> {
    /// Add a header to the request
    pub fn header(mut self, key: &str, value: &str) -> Self {
        let header_name = HeaderName::from_bytes(key.as_bytes()).unwrap();
        let header_value = HeaderValue::from_str(value).unwrap();
        self.headers.insert(header_name, header_value);
        self
    }

    /// Send the request and return the response
    pub async fn send(self) -> serde_json::Value {
        let openai_path = &self.test_server.config.llm.protocols.openai.path;
        let url = format!("http://{}{}/v1/chat/completions", self.test_server.address, openai_path);

        let mut request_builder = self.test_server.client.client.post(&url).json(&self.request);

        // Add all headers to the request
        for (key, value) in &self.headers {
            if let Ok(value) = value.to_str() {
                request_builder = request_builder.header(key.as_str(), value);
            }
        }

        let response = request_builder.send().await.unwrap();
        let status = response.status();

        #[allow(clippy::panic)]
        if status != 200 {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unable to read error".to_string());
            panic!("Expected 200 status, got {status}: {error_text}");
        }

        response.json().await.unwrap()
    }

    /// Send the request and return status code and body (for error testing)
    pub async fn send_raw(self) -> (u16, serde_json::Value) {
        let openai_path = &self.test_server.config.llm.protocols.openai.path;
        let url = format!("http://{}{}/v1/chat/completions", self.test_server.address, openai_path);

        let mut request_builder = self.test_server.client.client.post(&url).json(&self.request);

        // Add all headers to the request
        for (key, value) in &self.headers {
            if let Ok(value) = value.to_str() {
                request_builder = request_builder.header(key.as_str(), value);
            }
        }

        let response = request_builder.send().await.unwrap();
        let status = response.status().as_u16();
        let body = response.json().await.unwrap();
        (status, body)
    }
}

/// Builder for OpenAI streaming completions requests with fluent API for headers
pub struct OpenAICompletionsStreamRequest<'a> {
    test_server: &'a TestServer,
    request: serde_json::Value,
    headers: HeaderMap,
}

impl<'a> OpenAICompletionsStreamRequest<'a> {
    /// Add a header to the request
    pub fn header(mut self, key: &str, value: &str) -> Self {
        let header_name = HeaderName::from_bytes(key.as_bytes()).unwrap();
        let header_value = HeaderValue::from_str(value).unwrap();
        self.headers.insert(header_name, header_value);
        self
    }

    /// Send the request and return streaming chunks
    pub async fn send(self) -> Vec<serde_json::Value> {
        use eventsource_stream::Eventsource;
        use futures::StreamExt;

        let openai_path = &self.test_server.config.llm.protocols.openai.path;
        let url = format!("http://{}{}/v1/chat/completions", self.test_server.address, openai_path);

        // Ensure streaming is enabled
        let mut request = self.request;
        request["stream"] = json!(true);

        let mut request_builder = self.test_server.client.client.post(&url).json(&request);

        // Add all headers to the request
        for (key, value) in &self.headers {
            if let Ok(value) = value.to_str() {
                request_builder = request_builder.header(key.as_str(), value);
            }
        }

        let response = request_builder.send().await.unwrap();

        assert_eq!(response.status(), 200);
        assert_eq!(response.headers().get("content-type").unwrap(), "text/event-stream");

        // Convert the response bytes stream to SSE event stream
        let byte_stream = response.bytes_stream();
        let event_stream = byte_stream.eventsource();

        // Transform SSE events to JSON values
        let stream = event_stream.filter_map(|event| async move {
            match event {
                Ok(event) => {
                    // Skip empty events and [DONE] marker
                    if event.data.is_empty() || event.data == "[DONE]" {
                        None
                    } else {
                        // Parse as JSON Value
                        serde_json::from_str::<serde_json::Value>(&event.data).ok()
                    }
                }
                Err(_) => None,
            }
        });

        futures::pin_mut!(stream);

        let mut chunks = Vec::new();
        while let Some(chunk) = stream.next().await {
            chunks.push(chunk);
        }

        chunks
    }
}

/// Builder for Anthropic completions requests with fluent API for headers
pub struct AnthropicCompletionsRequest<'a> {
    test_server: &'a TestServer,
    request: serde_json::Value,
    headers: HeaderMap,
}

impl<'a> AnthropicCompletionsRequest<'a> {
    /// Add a header to the request
    pub fn header(mut self, key: &str, value: &str) -> Self {
        let header_name = HeaderName::from_bytes(key.as_bytes()).unwrap();
        let header_value = HeaderValue::from_str(value).unwrap();
        self.headers.insert(header_name, header_value);
        self
    }

    /// Send the request and return the response
    pub async fn send(self) -> serde_json::Value {
        let anthropic_path = &self.test_server.config.llm.protocols.anthropic.path;
        let url = format!("http://{}{}/v1/messages", self.test_server.address, anthropic_path);

        let mut request_builder = self.test_server.client.client.post(&url).json(&self.request);

        // Add all headers to the request
        for (key, value) in &self.headers {
            if let Ok(value) = value.to_str() {
                request_builder = request_builder.header(key.as_str(), value);
            }
        }

        let response = request_builder.send().await.unwrap();
        let status = response.status();

        #[allow(clippy::panic)]
        if status != 200 {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unable to read error".to_string());
            panic!("Expected 200 status, got {status}: {error_text}");
        }

        response.json().await.unwrap()
    }
}

/// Builder for Anthropic streaming completions requests with fluent API for headers
pub struct AnthropicCompletionsStreamRequest<'a> {
    test_server: &'a TestServer,
    request: serde_json::Value,
    headers: HeaderMap,
}

impl<'a> AnthropicCompletionsStreamRequest<'a> {
    /// Add a header to the request
    pub fn header(mut self, key: &str, value: &str) -> Self {
        let header_name = HeaderName::from_bytes(key.as_bytes()).unwrap();
        let header_value = HeaderValue::from_str(value).unwrap();
        self.headers.insert(header_name, header_value);
        self
    }

    /// Send the request and return streaming chunks
    pub async fn send(self) -> Vec<serde_json::Value> {
        use eventsource_stream::Eventsource;
        use futures::StreamExt;

        let anthropic_path = &self.test_server.config.llm.protocols.anthropic.path;
        let url = format!("http://{}{}/v1/messages", self.test_server.address, anthropic_path);

        // Ensure streaming is enabled
        let mut request = self.request;
        request["stream"] = json!(true);

        let mut request_builder = self.test_server.client.client.post(&url).json(&request);

        // Add all headers to the request
        for (key, value) in &self.headers {
            if let Ok(value) = value.to_str() {
                request_builder = request_builder.header(key.as_str(), value);
            }
        }

        let response = request_builder.send().await.unwrap();

        assert_eq!(response.status(), 200);
        assert_eq!(response.headers().get("content-type").unwrap(), "text/event-stream");

        // Convert the response bytes stream to SSE event stream
        let byte_stream = response.bytes_stream();
        let event_stream = byte_stream.eventsource();

        // Transform SSE events to JSON values
        let stream = event_stream.filter_map(|event| async move {
            match event {
                Ok(event) => {
                    // Skip empty events and [DONE] marker
                    if event.data.is_empty() || event.data == "[DONE]" {
                        None
                    } else {
                        // Parse as JSON Value
                        serde_json::from_str::<serde_json::Value>(&event.data).ok()
                    }
                }
                Err(_) => None,
            }
        });

        futures::pin_mut!(stream);

        let mut chunks = Vec::new();
        while let Some(chunk) = stream.next().await {
            chunks.push(chunk);
        }

        chunks
    }
}

/// Test server that manages the lifecycle of a server instance
pub struct TestServer {
    pub client: TestClient,
    pub address: SocketAddr,
    /// Configuration used by this test server
    pub config: Config,
    /// Cancellation tokens for test services (MCP mocks, LLM mocks, etc.)
    pub test_service_tokens: Vec<CancellationToken>,
    /// Handle to the main Nexus server task
    _nexus_task_handle: tokio::task::JoinHandle<()>,
    /// Shutdown signal for the main Nexus server
    nexus_shutdown_signal: CancellationToken,
}

impl TestServer {
    pub fn builder() -> TestServerBuilder {
        TestServerBuilder::default()
    }

    /// Start a new test server with the given TOML configuration
    async fn start(config_toml: &str, test_service_tokens: Vec<CancellationToken>) -> Self {
        // Write config to a temporary file and use the proper loader to ensure validation
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("config.toml");
        std::fs::write(&config_path, config_toml).unwrap();

        // Use the proper config loader which includes validation
        let config = Config::load(&config_path).unwrap();

        // The server crate will handle telemetry and logger initialization

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

        // Create a cancellation token for graceful shutdown of Nexus server
        let nexus_shutdown_signal = CancellationToken::new();
        let nexus_shutdown_signal_clone = nexus_shutdown_signal.clone();

        // Create the server configuration with telemetry guard
        let serve_config = ServeConfig {
            listen_address: address,
            config: config.clone(),
            shutdown_signal: nexus_shutdown_signal_clone,
            log_filter: "server=debug,mcp=debug,telemetry=debug,rate_limit=debug,llm=debug,config=debug,integration_tests=debug,nexus=debug".to_string(),
        };

        // Start the server in a background task
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        let nexus_task_handle = tokio::spawn(async move {
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
            config,
            test_service_tokens,
            _nexus_task_handle: nexus_task_handle,
            nexus_shutdown_signal,
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

    /// Create an MCP client with custom headers
    pub async fn mcp_client_with_headers(&self, path: &str, headers: HeaderMap) -> McpTestClient {
        let protocol = if self.client.base_url.starts_with("https") {
            "https"
        } else {
            "http"
        };

        let mcp_url = format!("{protocol}://{}{}", self.address, path);

        McpTestClient::new_with_headers(mcp_url, headers).await
    }

    /// Create an OpenAI completions request builder
    pub fn openai_completions(&self, request: serde_json::Value) -> OpenAICompletionsRequest<'_> {
        OpenAICompletionsRequest {
            test_server: self,
            request,
            headers: HeaderMap::new(),
        }
    }

    /// Create an OpenAI streaming completions request builder
    pub fn openai_completions_stream(&self, request: serde_json::Value) -> OpenAICompletionsStreamRequest<'_> {
        OpenAICompletionsStreamRequest {
            test_server: self,
            request,
            headers: HeaderMap::new(),
        }
    }

    /// Create an Anthropic completions request builder
    pub fn anthropic_completions(&self, request: serde_json::Value) -> AnthropicCompletionsRequest<'_> {
        AnthropicCompletionsRequest {
            test_server: self,
            request,
            headers: HeaderMap::new(),
        }
    }

    /// Create an Anthropic streaming completions request builder
    pub fn anthropic_completions_stream(&self, request: serde_json::Value) -> AnthropicCompletionsStreamRequest<'_> {
        AnthropicCompletionsStreamRequest {
            test_server: self,
            request,
            headers: HeaderMap::new(),
        }
    }

    /// List models for OpenAI protocol
    pub async fn openai_list_models(&self) -> serde_json::Value {
        let openai_path = &self.config.llm.protocols.openai.path;
        let url = format!("http://{}{}/v1/models", self.address, openai_path);

        let response = self.client.client.get(&url).send().await.unwrap();

        assert_eq!(response.status(), 200);
        response.json().await.unwrap()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        // First, shutdown test services (MCP mocks, LLM mocks)
        for token in &self.test_service_tokens {
            token.cancel();
        }

        // Then signal graceful shutdown to the main Nexus server
        eprintln!("DEBUG: Cancelling Nexus server shutdown signal");
        self.nexus_shutdown_signal.cancel();

        // We can't wait for the task to complete in Drop (not async)
        // but the cancellation signal will trigger graceful shutdown
        // The TelemetryGuard will flush when the server task exits
    }
}

#[derive(Default)]
pub struct TestServerBuilder {
    config: String,
    /// Cancellation tokens for test services that will be spawned
    test_service_tokens: Vec<CancellationToken>,
}

impl TestServerBuilder {
    /// Spawn a test LLM provider and configure Nexus to connect to it
    pub async fn spawn_llm(&mut self, provider: impl llms::TestLlmProvider) {
        let boxed_provider = Box::new(provider);
        let model_configs = boxed_provider.model_configs();
        let mut config = boxed_provider.spawn().await.unwrap();
        config.model_configs = model_configs;
        let provider_config_snippet = llms::generate_config_for_type(config.provider_type, &config);

        // Don't add [llm] section here - let build() handle it
        // This avoids conflicts with tests that define their own [llm] configuration
        self.config.push_str(&provider_config_snippet);
    }

    /// Spawn a test LLM server and configure Nexus to connect to it (legacy method for backward compatibility)
    pub async fn spawn_llm_server(&mut self, provider_name: &str) -> TestOpenAIServer {
        let llm_server = TestOpenAIServer::start().await;

        // Don't add [llm] section here - let build() handle it
        // This avoids conflicts with tests that define their own [llm] configuration

        // Add LLM configuration pointing to the test server
        // Include /v1 in the URL as the OpenAI provider expects this format
        let config = indoc::formatdoc! {r#"

            [llm.providers.{provider_name}]
            type = "openai"
            api_key = "test-key"
            base_url = "{}/v1"
        "#, llm_server.base_url()};

        self.config.push_str(&config);
        llm_server
    }

    pub async fn spawn_service(&mut self, service: TestService) {
        let (listen_addr, ct) = service.spawn().await;

        if let Some(ct) = ct {
            self.test_service_tokens.push(ct);
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
        let mut final_config = config.to_string();

        // If test config doesn't have [llm] section but we have LLM providers configured
        // (either in the test config or builder config), add default [llm] configuration with endpoints
        let has_llm_section = config.contains("[llm]") || final_config.contains("[llm]");
        let has_providers = config.contains("[llm.providers.") || self.config.contains("[llm.providers.");

        if !has_llm_section && has_providers {
            final_config.push_str(indoc::indoc! {r#"

                [llm]
                enabled = true

                [llm.protocols.openai]
                enabled = true
                path = "/llm/openai"
            "#});
        }

        final_config.push('\n');
        final_config.push_str(&self.config);

        TestServer::start(&final_config, self.test_service_tokens).await
    }
}
