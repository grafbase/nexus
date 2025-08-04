use std::{fs, io::Read, sync::Arc};

use config::{ClientAuthConfig, HttpConfig, StdioTarget, StdioTargetType, TlsClientConfig};
use reqwest::{
    Certificate, Identity,
    header::{AUTHORIZATION, HeaderMap, HeaderValue},
};
use rmcp::{
    RoleClient, ServiceError, ServiceExt,
    model::{
        CallToolRequestParam, CallToolResult, GetPromptRequestParam, GetPromptResult, Prompt, ReadResourceRequestParam,
        ReadResourceResult, Resource, Tool,
    },
    service::RunningService,
    transport::{
        SseClientTransport, StreamableHttpClientTransport, TokioChildProcess, common::client_side_sse::FixedInterval,
        sse_client::SseClientConfig, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use secrecy::ExposeSecret;
use std::process::Stdio;
use tokio::process::Command;

/// An MCP server which acts as proxy for a downstream MCP server, no matter the protocol.
#[derive(Clone)]
pub struct DownstreamClient {
    inner: Arc<Inner>,
}

/// Internal data structure for DownstreamServer.
struct Inner {
    /// The name of the downstream server.
    name: String,
    /// The running service that handles MCP communication.
    service: RunningService<RoleClient, ()>,
}

impl DownstreamClient {
    /// Creates a running service for STDIO-based MCP communication.
    ///
    /// This function spawns a child process and establishes STDIO communication with it.
    pub async fn new_stdio(name: &str, config: &config::StdioConfig) -> anyhow::Result<Self> {
        log::debug!("Creating STDIO downstream service for server '{name}'");

        let mut command = Command::new(config.executable());
        command.args(config.args());

        // Set environment variables
        for (key, value) in &config.env {
            command.env(key, value);
        }

        // Set working directory if specified
        if let Some(cwd) = &config.cwd {
            command.current_dir(cwd);
        }

        let stderr_stdio = stdio_target(&config.stderr)?;
        log::debug!(
            "STDIO configuration for '{name}': stderr output directed to {:?}",
            config.stderr
        );

        let transport = TokioChildProcess::builder(command)
            .stderr(stderr_stdio)
            .spawn()
            .map(|(transport, _stderr)| transport)?;

        let service = ().serve(transport).await?;

        Ok(Self {
            inner: Arc::new(Inner {
                name: name.to_string(),
                service,
            }),
        })
    }

    pub async fn new_http(name: &str, config: &HttpConfig) -> anyhow::Result<Self> {
        log::debug!("Creating HTTP downstream service for server '{name}'");
        let service = http_service(config).await?;

        Ok(Self {
            inner: Arc::new(Inner {
                name: name.to_string(),
                service,
            }),
        })
    }

    /// Lists all tools available from the downstream MCP server.
    pub async fn list_tools(&self) -> Result<Vec<Tool>, ServiceError> {
        log::debug!("Requesting tool list from downstream server '{}'", self.name());
        Ok(self.inner.service.list_tools(Default::default()).await?.tools)
    }

    /// Calls a tool on the downstream MCP server.
    pub async fn call_tool(&self, params: CallToolRequestParam) -> Result<CallToolResult, ServiceError> {
        log::debug!("Invoking tool '{}' on downstream server '{}'", params.name, self.name());
        self.inner.service.call_tool(params).await
    }

    /// Lists all prompts available from the downstream MCP server.
    pub async fn list_prompts(&self) -> Result<Vec<Prompt>, ServiceError> {
        log::debug!("Requesting prompt list from downstream server '{}'", self.name());
        Ok(self.inner.service.list_prompts(Default::default()).await?.prompts)
    }

    /// Gets a prompt from the downstream MCP server.
    pub async fn get_prompt(&self, params: GetPromptRequestParam) -> Result<GetPromptResult, ServiceError> {
        log::debug!(
            "Retrieving prompt '{}' from downstream server '{}'",
            params.name,
            self.name()
        );
        self.inner.service.get_prompt(params).await
    }

    /// Lists all resources available from the downstream MCP server.
    pub async fn list_resources(&self) -> Result<Vec<Resource>, ServiceError> {
        log::debug!("Requesting resource list from downstream server '{}'", self.name());
        Ok(self.inner.service.list_resources(Default::default()).await?.resources)
    }

    /// Reads a resource from the downstream MCP server.
    pub async fn read_resource(&self, params: ReadResourceRequestParam) -> Result<ReadResourceResult, ServiceError> {
        log::debug!(
            "Reading resource '{}' from downstream server '{}'",
            params.uri,
            self.name()
        );
        self.inner.service.read_resource(params).await
    }

    /// Returns the name of the downstream MCP server.
    pub(super) fn name(&self) -> &str {
        &self.inner.name
    }
}

/// Creates a running service for HTTP-based MCP communication.
///
/// This function handles protocol detection and fallback between streamable-http and SSE protocols.
/// If the configuration explicitly specifies a protocol, it will use that protocol directly.
/// Otherwise, it will attempt streamable-http first and fall back to SSE if that fails.
async fn http_service(config: &HttpConfig) -> anyhow::Result<RunningService<RoleClient, ()>> {
    if config.uses_streamable_http() {
        log::debug!("Configuration explicitly requests streamable-http protocol");
        return streamable_http_service(config).await;
    }

    if config.uses_sse() {
        log::debug!("Configuration explicitly requests SSE protocol");
        return sse_service(config).await;
    }

    log::debug!("Auto-detecting protocol: attempting streamable-http first");
    match streamable_http_service(config).await {
        Ok(service) => Ok(service),
        Err(_) => {
            log::warn!(
                "Streamable-http connection failed for URL '{}', falling back to SSE protocol",
                config.url
            );
            sse_service(config).await
        }
    }
}

/// Creates a running service for streamable-http protocol.
async fn streamable_http_service(config: &HttpConfig) -> anyhow::Result<RunningService<RoleClient, ()>> {
    log::debug!("Initializing streamable-http downstream service");

    let client = create_client(config.tls.as_ref(), config.auth.as_ref())?;
    let config = StreamableHttpClientTransportConfig::with_uri(config.url.to_string());
    let transport = StreamableHttpClientTransport::with_client(client, config);

    Ok(().serve(transport).await?)
}

/// Creates a running service for SSE (Server-Sent Events) protocol.
async fn sse_service(config: &HttpConfig) -> anyhow::Result<RunningService<RoleClient, ()>> {
    log::debug!("Initializing SSE (Server-Sent Events) downstream service");

    let client_config = SseClientConfig {
        sse_endpoint: config.url.to_string().into(),
        retry_policy: Arc::new(FixedInterval::default()),
        use_message_endpoint: config.message_url.as_ref().map(|u| u.to_string()),
    };

    log::debug!(
        "SSE client configuration: endpoint='{}', message_endpoint={:?}",
        config.url,
        config.message_url
    );

    let client = create_client(config.tls.as_ref(), config.auth.as_ref())?;
    log::debug!("Successfully created HTTP client for SSE transport");

    let transport = SseClientTransport::start_with_client(client, client_config).await?;
    log::debug!("SSE transport connection established successfully");

    let service = ().serve(transport).await?;
    log::debug!("SSE service initialized and ready to handle requests");

    Ok(service)
}

/// Creates a configured reqwest HTTP client with optional TLS settings.
fn create_client(tls: Option<&TlsClientConfig>, auth: Option<&ClientAuthConfig>) -> anyhow::Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder();

    if let Some(tls) = tls {
        builder = builder
            .danger_accept_invalid_certs(!tls.verify_certs)
            .danger_accept_invalid_hostnames(tls.accept_invalid_hostnames);

        if let Some(ref path) = tls.root_ca_cert_path {
            let mut pem = Vec::new();

            let mut file = fs::File::open(path)?;
            file.read_to_end(&mut pem)?;

            let cert = Certificate::from_pem(&pem)?;
            builder = builder.add_root_certificate(cert);
        }

        let identity = tls.client_cert_path.as_ref().zip(tls.client_key_path.as_ref());

        if let Some((cert_path, key_path)) = identity {
            let mut cert_pem = Vec::new();
            let mut cert_file = fs::File::open(cert_path)?;
            cert_file.read_to_end(&mut cert_pem)?;

            // Read client private key
            let mut key_pem = Vec::new();
            let mut key_file = fs::File::open(key_path)?;
            key_file.read_to_end(&mut key_pem)?;

            // Combine certificate and key into a single PEM bundle
            let mut combined_pem = Vec::new();
            combined_pem.extend_from_slice(&cert_pem);
            combined_pem.extend_from_slice(b"\n");
            combined_pem.extend_from_slice(&key_pem);

            // Create identity from the combined PEM
            let identity = Identity::from_pem(&combined_pem)?;
            builder = builder.identity(identity);
        }
    }

    if let Some(ClientAuthConfig::Token { token }) = auth {
        let mut headers = HeaderMap::new();
        let auth_value = HeaderValue::from_str(&format!("Bearer {}", token.expose_secret()))?;

        headers.insert(AUTHORIZATION, auth_value);

        builder = builder.default_headers(headers);
    }

    Ok(builder.build()?)
}

/// Converts a StdioTarget configuration to a tokio::process::Stdio.
fn stdio_target(target: &StdioTarget) -> anyhow::Result<Stdio> {
    match target {
        StdioTarget::Simple(StdioTargetType::Pipe) => Ok(Stdio::piped()),
        StdioTarget::Simple(StdioTargetType::Inherit) => Ok(Stdio::inherit()),
        StdioTarget::Simple(StdioTargetType::Null) => Ok(Stdio::null()),
        StdioTarget::File { file } => {
            let file = fs::OpenOptions::new().create(true).append(true).open(file)?;

            Ok(Stdio::from(file))
        }
    }
}
