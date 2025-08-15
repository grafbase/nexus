use std::sync::Arc;
use std::process::Stdio as ProcessStdio;

use crate::types::{
    CallToolRequest, CallToolResult, GetPromptRequest, GetPromptResult, McpError, Prompt, ReadResourceRequest,
    ReadResourceResult, Resource, Tool,
};
use anyhow::Result;
use config::{HttpConfig, StdioConfig, StdioTarget, StdioTargetType};
use pmcp::{Client, ClientCapabilities};
use pmcp::shared::{StreamableHttpTransport, StreamableHttpTransportConfig};
use tokio::sync::Mutex;
use tokio::process::Command;
use std::fs::OpenOptions;

/// Custom stdio transport that works with child processes
mod process_stdio {
    use async_trait::async_trait;
    use pmcp::shared::transport::{Transport, TransportMessage};
    use pmcp::error::{Result, TransportError};
    use tokio::process::{Child, ChildStdin, ChildStdout};
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    use tokio::sync::Mutex;

    const CONTENT_LENGTH_HEADER: &str = "Content-Length: ";

    #[derive(Debug)]
    pub struct ProcessStdioTransport {
        _child: Child,
        stdin: Mutex<ChildStdin>,
        stdout: Mutex<BufReader<ChildStdout>>,
        closed: std::sync::atomic::AtomicBool,
    }

    impl ProcessStdioTransport {
        pub fn new(mut child: Child) -> Result<Self> {
            let stdin = child.stdin.take().ok_or_else(|| TransportError::Io(
                "Failed to get stdin".to_string()
            ))?;
            let stdout = child.stdout.take().ok_or_else(|| TransportError::Io(
                "Failed to get stdout".to_string()
            ))?;

            Ok(Self {
                _child: child,
                stdin: Mutex::new(stdin),
                stdout: Mutex::new(BufReader::new(stdout)),
                closed: std::sync::atomic::AtomicBool::new(false),
            })
        }

        fn parse_content_length(line: &str) -> Option<usize> {
            line.strip_prefix(CONTENT_LENGTH_HEADER)
                .and_then(|content| content.trim().parse().ok())
        }

        async fn read_headers(&self) -> Result<usize> {
            let mut stdout = self.stdout.lock().await;
            let mut content_length = None;

            loop {
                let mut line = String::new();
                let bytes_read = stdout.read_line(&mut line).await.map_err(|e| TransportError::Io(e.to_string()))?;
                
                if bytes_read == 0 {
                    return Err(TransportError::ConnectionClosed.into());
                }

                let line = line.trim();
                if line.is_empty() {
                    break;
                }

                if let Some(length) = Self::parse_content_length(line) {
                    content_length = Some(length);
                }
            }

            content_length.ok_or_else(|| TransportError::InvalidMessage(
                "Missing Content-Length header".to_string()
            ).into())
        }

        async fn read_message_body(&self, content_length: usize) -> Result<Vec<u8>> {
            let mut stdout = self.stdout.lock().await;
            let mut buffer = vec![0u8; content_length];
            stdout.read_exact(&mut buffer).await.map_err(|e| TransportError::Io(e.to_string()))?;
            Ok(buffer)
        }

        async fn write_message(&self, json_bytes: &[u8]) -> Result<()> {
            let mut stdin = self.stdin.lock().await;
            
            let header = format!(
                "Content-Length: {}\r\n\r\n",
                json_bytes.len()
            );
            
            stdin.write_all(header.as_bytes()).await.map_err(|e| TransportError::Io(e.to_string()))?;
            stdin.write_all(json_bytes).await.map_err(|e| TransportError::Io(e.to_string()))?;
            stdin.flush().await.map_err(|e| TransportError::Io(e.to_string()))?;
            
            Ok(())
        }
    }

    #[async_trait]
    impl Transport for ProcessStdioTransport {
        async fn send(&mut self, message: TransportMessage) -> Result<()> {
            if self.closed.load(std::sync::atomic::Ordering::Acquire) {
                return Err(TransportError::ConnectionClosed.into());
            }

            let json_bytes = serde_json::to_vec(&message).map_err(|e| TransportError::Serialization(e.to_string()))?;
            self.write_message(&json_bytes).await
        }

        async fn receive(&mut self) -> Result<TransportMessage> {
            if self.closed.load(std::sync::atomic::Ordering::Acquire) {
                return Err(TransportError::ConnectionClosed.into());
            }

            let content_length = self.read_headers().await?;
            let buffer = self.read_message_body(content_length).await?;
            serde_json::from_slice(&buffer).map_err(|e| TransportError::Deserialization(e.to_string()).into())
        }

        async fn close(&mut self) -> Result<()> {
            self.closed
                .store(true, std::sync::atomic::Ordering::Release);
            Ok(())
        }
    }
}

use process_stdio::ProcessStdioTransport;

/// Wrapper enum for different pmcp client transports
enum ClientTransport {
    Stdio(Client<ProcessStdioTransport>),
    StreamableHttp(Client<StreamableHttpTransport>),
}

/// An MCP server which acts as proxy for a downstream MCP server, no matter the protocol.
#[derive(Clone)]
pub struct DownstreamClient {
    inner: Arc<Inner>,
}

/// Internal data structure for DownstreamServer.
struct Inner {
    /// The name of the downstream server.
    name: String,
    /// The pmcp client for communication
    client: Arc<Mutex<ClientTransport>>,
}

impl DownstreamClient {
    /// Creates a running service for STDIO-based MCP communication.
    ///
    /// This function spawns a child process and establishes STDIO communication with it.
    pub async fn new_stdio(name: &str, config: &StdioConfig) -> anyhow::Result<Self> {
        log::debug!("Creating STDIO downstream service for server '{name}'");

        // Spawn the child process
        log::debug!("Spawning STDIO process: {} {:?}", config.executable(), config.args());
        let mut command = Command::new(config.executable());
        command.args(config.args())
            .stdin(ProcessStdio::piped())
            .stdout(ProcessStdio::piped());
        
        // Configure stderr based on config
        match config.stderr() {
            StdioTarget::Simple(StdioTargetType::Pipe) => {
                command.stderr(ProcessStdio::piped());
            }
            StdioTarget::Simple(StdioTargetType::Inherit) => {
                command.stderr(ProcessStdio::inherit());
            }
            StdioTarget::Simple(StdioTargetType::Null) => {
                command.stderr(ProcessStdio::null());
            }
            StdioTarget::File { file } => {
                // Create or open the file for stderr
                let stderr_file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(file)?;
                command.stderr(stderr_file);
            }
        }
        
        // Set environment variables
        if let Some(env) = config.env() {
            for (key, value) in env {
                log::debug!("Setting environment variable {key}={value}");
                command.env(key, value);
            }
        }
        
        // Set working directory
        if let Some(cwd) = config.cwd() {
            log::debug!("Setting working directory: {cwd}");
            command.current_dir(cwd);
        }
        
        let child = command.spawn()?;
        log::debug!("STDIO process spawned successfully");
        
        // Create our custom process stdio transport
        let transport = ProcessStdioTransport::new(child)?;
        log::debug!("STDIO transport created");
        
        // Create pmcp client
        let mut client = Client::new(transport);
        log::debug!("PMCP client created");
        
        // Initialize the client
        let capabilities = ClientCapabilities::default();
        log::debug!("Initializing PMCP client with capabilities: {:?}", capabilities);
        client.initialize(capabilities).await?;
        log::debug!("PMCP client initialized successfully");

        Ok(Self {
            inner: Arc::new(Inner {
                name: name.to_string(),
                client: Arc::new(Mutex::new(ClientTransport::Stdio(client))),
            }),
        })
    }

    pub async fn new_http(name: &str, config: &HttpConfig) -> anyhow::Result<Self> {
        log::debug!("Creating HTTP downstream service for server '{name}'");

        // Build headers including auth if configured
        let mut headers = vec![];
        let _needs_auth = if let Some(auth) = &config.auth {
            match auth {
                config::ClientAuthConfig::Token { token } => {
                    use secrecy::ExposeSecret;
                    headers.push((
                        "Authorization".to_string(),
                        format!("Bearer {}", token.expose_secret()),
                    ));
                    log::debug!("Adding authorization header for server '{name}'");
                    true
                }
                config::ClientAuthConfig::Forward { .. } => {
                    // Forward type should have been handled by finalize()
                    log::debug!("Auth forwarding configured for server '{name}'");
                    false
                }
            }
        } else {
            false
        };

        // Always use StreamableHttpTransport as that's what the MCP servers expect
        // StreamableHttpTransportConfig supports headers directly, so we can pass auth headers
        log::debug!("Using StreamableHttpTransport for server '{name}' with {} auth headers", headers.len());
        
        let streamable_config = StreamableHttpTransportConfig {
            url: config.url.clone(),
            extra_headers: headers, // Pass the headers here (includes auth if present)
            auth_provider: None,
            session_id: None,
            enable_json_response: false, // Use SSE for proper streaming
            on_resumption_token: None,
        };
        let transport = StreamableHttpTransport::new(streamable_config);
        let mut client = Client::new(transport);
        
        // Initialize the client
        let capabilities = ClientCapabilities::default();
        client.initialize(capabilities).await?;
        
        let client_transport = ClientTransport::StreamableHttp(client);

        Ok(Self {
            inner: Arc::new(Inner {
                name: name.to_string(),
                client: Arc::new(Mutex::new(client_transport)),
            }),
        })
    }

    /// Lists all tools available from the downstream MCP server.
    pub async fn list_tools(&self) -> Result<Vec<Tool>, McpError> {
        log::debug!("Requesting tool list from downstream server '{}'", self.name());

        let mut client = self.inner.client.lock().await;
        let result = match &mut *client {
            ClientTransport::Stdio(c) => c.list_tools(None).await,
            ClientTransport::StreamableHttp(c) => c.list_tools(None).await,
        }.map_err(|e| pmcp::Error::internal(e.to_string()))?;
        
        Ok(result.tools)
    }

    /// Calls a tool on the downstream MCP server.
    pub async fn call_tool(&self, params: CallToolRequest) -> Result<CallToolResult, McpError> {
        log::debug!("Invoking tool '{}' on downstream server '{}'", params.name, self.name());

        let mut client = self.inner.client.lock().await;
        let result = match &mut *client {
            ClientTransport::Stdio(c) => c.call_tool(params.name, params.arguments).await,
            ClientTransport::StreamableHttp(c) => c.call_tool(params.name, params.arguments).await,
        }.map_err(|e| pmcp::Error::internal(e.to_string()))?;
        
        Ok(result)
    }

    /// Gets the list of prompts from the downstream MCP server.
    pub async fn list_prompts(&self) -> Result<Vec<Prompt>, McpError> {
        log::debug!("Requesting prompt list from downstream server '{}'", self.name());

        let mut client = self.inner.client.lock().await;
        let result = match &mut *client {
            ClientTransport::Stdio(c) => c.list_prompts(None).await,
            ClientTransport::StreamableHttp(c) => c.list_prompts(None).await,
        }.map_err(|e| pmcp::Error::internal(e.to_string()))?;
        
        Ok(result.prompts)
    }

    /// Gets a prompt from the downstream MCP server.
    pub async fn get_prompt(&self, params: GetPromptRequest) -> Result<GetPromptResult, McpError> {
        log::debug!(
            "Getting prompt '{}' from downstream server '{}'",
            params.name,
            self.name()
        );

        let mut client = self.inner.client.lock().await;
        let result = match &mut *client {
            ClientTransport::Stdio(c) => c.get_prompt(params.name, params.arguments).await,
            ClientTransport::StreamableHttp(c) => c.get_prompt(params.name, params.arguments).await,
        }.map_err(|e| pmcp::Error::internal(e.to_string()))?;
        
        Ok(result)
    }

    /// Gets the list of resources from the downstream MCP server.
    pub async fn list_resources(&self) -> Result<Vec<Resource>, McpError> {
        log::debug!("Requesting resource list from downstream server '{}'", self.name());

        let mut client = self.inner.client.lock().await;
        let result = match &mut *client {
            ClientTransport::Stdio(c) => c.list_resources(None).await,
            ClientTransport::StreamableHttp(c) => c.list_resources(None).await,
        }.map_err(|e| pmcp::Error::internal(e.to_string()))?;
        
        Ok(result.resources)
    }

    /// Reads a resource from the downstream MCP server.
    pub async fn read_resource(&self, params: ReadResourceRequest) -> Result<ReadResourceResult, McpError> {
        log::debug!(
            "Reading resource '{}' from downstream server '{}'",
            params.uri,
            self.name()
        );

        let mut client = self.inner.client.lock().await;
        let result = match &mut *client {
            ClientTransport::Stdio(c) => c.read_resource(params.uri).await,
            ClientTransport::StreamableHttp(c) => c.read_resource(params.uri).await,
        }.map_err(|e| pmcp::Error::internal(e.to_string()))?;
        
        Ok(result)
    }

    /// Gets the name of the downstream server.
    pub fn name(&self) -> &str {
        &self.inner.name
    }
}
