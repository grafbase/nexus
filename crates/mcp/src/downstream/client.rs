use std::{io::Read, sync::Arc};

use config::{ClientAuthConfig, HttpConfig, McpServer, TlsClientConfig};
use reqwest::{
    Certificate, Identity,
    header::{AUTHORIZATION, HeaderMap, HeaderValue},
};
use rmcp::{
    RoleClient, ServiceError, ServiceExt,
    model::{CallToolRequestParam, CallToolResult, Tool},
    service::RunningService,
    transport::{
        SseClientTransport, StreamableHttpClientTransport, common::client_side_sse::FixedInterval,
        sse_client::SseClientConfig, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use secrecy::ExposeSecret;

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
    /// Creates a new DownstreamServer with the given name and configuration.
    pub async fn new(name: &str, config: &McpServer) -> anyhow::Result<Self> {
        let service = match config {
            McpServer::Stdio { .. } => todo!(),
            McpServer::Http(config) => http_service(config).await?,
        };

        Ok(Self {
            inner: Arc::new(Inner {
                name: name.to_string(),
                service,
            }),
        })
    }

    /// Lists all tools available from the downstream MCP server.
    pub async fn list_tools(&self) -> Result<Vec<Tool>, ServiceError> {
        log::debug!("listing tools for {}", self.name());
        Ok(self.inner.service.list_tools(Default::default()).await?.tools)
    }

    /// Calls a tool on the downstream MCP server.
    pub async fn call_tool(&self, params: CallToolRequestParam) -> Result<CallToolResult, ServiceError> {
        self.inner.service.call_tool(params).await
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
        log::debug!("config explicitly wants streamable-http");
        return streamable_http_service(config).await;
    }

    if config.uses_sse() {
        log::debug!("config explicitly wants SSE");
        return sse_service(config).await;
    }

    log::debug!("detecting protocol, starting with streamable-http");
    match streamable_http_service(config).await {
        Ok(service) => Ok(service),
        Err(_) => {
            log::warn!("streamable-http failed for url ({}), trying SSE", config.url);
            sse_service(config).await
        }
    }
}

/// Creates a running service for streamable-http protocol.
async fn streamable_http_service(config: &HttpConfig) -> anyhow::Result<RunningService<RoleClient, ()>> {
    log::debug!("creating a streamable-http downstream service");

    let client = create_client(config.tls.as_ref(), config.auth.as_ref())?;
    let config = StreamableHttpClientTransportConfig::with_uri(config.url.to_string());
    let transport = StreamableHttpClientTransport::with_client(client, config);

    Ok(().serve(transport).await?)
}

/// Creates a running service for SSE (Server-Sent Events) protocol.
async fn sse_service(config: &HttpConfig) -> anyhow::Result<RunningService<RoleClient, ()>> {
    log::debug!("creating an SSE downstream service");

    let client_config = SseClientConfig {
        sse_endpoint: config.url.to_string().into(),
        retry_policy: Arc::new(FixedInterval::default()),
        use_message_endpoint: config.message_url.as_ref().map(|u| u.to_string()),
    };

    log::debug!(
        "SSE client config: sse_url={}, message_url={:?}",
        config.url,
        config.message_url
    );

    let client = create_client(config.tls.as_ref(), config.auth.as_ref())?;
    log::debug!("Created HTTP client for SSE transport");

    let transport = SseClientTransport::start_with_client(client, client_config).await?;
    log::debug!("SSE transport started successfully");

    let service = ().serve(transport).await?;
    log::debug!("SSE service created and ready");

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

            let mut file = std::fs::File::open(path)?;
            file.read_to_end(&mut pem)?;

            let cert = Certificate::from_pem(&pem)?;
            builder = builder.add_root_certificate(cert);
        }

        let identity = tls.client_cert_path.as_ref().zip(tls.client_key_path.as_ref());

        if let Some((cert_path, key_path)) = identity {
            let mut cert_pem = Vec::new();
            let mut cert_file = std::fs::File::open(cert_path)?;
            cert_file.read_to_end(&mut cert_pem)?;

            // Read client private key
            let mut key_pem = Vec::new();
            let mut key_file = std::fs::File::open(key_path)?;
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
