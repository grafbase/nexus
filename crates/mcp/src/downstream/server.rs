use std::{io::Read, sync::Arc};

use config::{McpServer, SseConfig, StreamableHttpConfig, TlsClientConfig};
use reqwest::{Certificate, Identity};
use rmcp::{
    RoleClient, ServiceError, ServiceExt,
    model::{CallToolRequestParam, CallToolResult, Tool},
    service::RunningService,
    transport::{
        SseClientTransport, StreamableHttpClientTransport, common::client_side_sse::FixedInterval,
        sse_client::SseClientConfig, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};

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
            McpServer::StreamableHttp(config) => streamable_http_service(config).await?,
            McpServer::Sse(config) => sse_service(config).await?,
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

/// Creates a running service for streamable-http protocol.
async fn streamable_http_service(config: &StreamableHttpConfig) -> anyhow::Result<RunningService<RoleClient, ()>> {
    log::debug!("creating a streamable-http downstream service");

    let client = create_client(config.tls.as_ref())?;
    let config = StreamableHttpClientTransportConfig::with_uri(config.uri.to_string());
    let transport = StreamableHttpClientTransport::with_client(client, config);

    Ok(().serve(transport).await?)
}

/// Creates a running service for SSE (Server-Sent Events) protocol.
async fn sse_service(config: &SseConfig) -> anyhow::Result<RunningService<RoleClient, ()>> {
    log::debug!("creating an SSE downstream service");

    let client_config = SseClientConfig {
        sse_endpoint: config.sse_endpoint.to_string().into(),
        retry_policy: Arc::new(FixedInterval::default()),
        use_message_endpoint: config.message_endpoint.as_ref().map(|u| u.to_string()),
    };

    log::debug!(
        "SSE client config: sse_endpoint={}, message_endpoint={:?}",
        config.sse_endpoint,
        config.message_endpoint
    );

    let client = create_client(config.tls.as_ref())?;
    log::debug!("Created HTTP client for SSE transport");

    let transport = SseClientTransport::start_with_client(client, client_config).await?;
    log::debug!("SSE transport started successfully");

    let service = ().serve(transport).await?;
    log::debug!("SSE service created and ready");

    Ok(service)
}

/// Creates a configured reqwest HTTP client with optional TLS settings.
fn create_client(tls: Option<&TlsClientConfig>) -> anyhow::Result<reqwest::Client> {
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

    Ok(builder.build()?)
}
