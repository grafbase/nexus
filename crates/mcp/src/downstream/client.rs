use std::{io::Read, sync::Arc};

use config::{ClientAuthConfig, HttpConfig, McpServer};

use reqwest::{
    Certificate, Identity,
    header::{AUTHORIZATION, HeaderMap, HeaderValue},
};
use rmcp::{
    RoleClient, ServiceError, ServiceExt,
    model::{CallToolRequestParam, CallToolResult, Tool},
    service::RunningService,
    transport::{
        SseClientTransport, StreamableHttpClientTransport,
        auth::{AuthClient, AuthorizationManager, OAuthClientConfig},
        common::client_side_sse::FixedInterval,
        sse_client::{SseClient, SseClientConfig},
        streamable_http_client::{StreamableHttpClient, StreamableHttpClientTransportConfig},
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
        log::debug!("creating a downstream server connection for {name}");

        let service = match config {
            McpServer::Stdio { .. } => todo!(),
            McpServer::Http(config) => create_http_client(config).await?,
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

async fn create_http_client(config: &HttpConfig) -> Result<RunningService<RoleClient, ()>, anyhow::Error> {
    let mut builder = reqwest::Client::builder();

    if let Some(ref tls) = config.tls {
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

    let client = match config.auth {
        Some(ClientAuthConfig::Token(ref token_config)) => {
            log::debug!("using token-based authentication");
            let auth_header = format!("Bearer {}", token_config.token.expose_secret());
            let auth_value = HeaderValue::from_str(&auth_header)?;

            let mut headers = HeaderMap::new();
            headers.insert(AUTHORIZATION, auth_value);

            let client = builder.default_headers(headers).build()?;
            create_service(client, config).await?
        }
        Some(ClientAuthConfig::Oauth(ref oauth_config)) => {
            log::debug!("using OAuth2 authentication");
            let base_client = builder.build()?;

            let mut url = reqwest::Url::parse(&oauth_config.token_url)?;
            url.set_path("");
            url.set_query(None);

            let mut manager = AuthorizationManager::new(url.as_str()).await?;

            manager.configure_client(OAuthClientConfig {
                client_id: oauth_config.client_id.clone(),
                client_secret: Some(oauth_config.client_secret.expose_secret().to_string()),
                scopes: oauth_config.scopes.clone(),
                // Special standardized OAuth2 redirect URI that stands for "out-of-band".
                // E.g. we do not expect a redirection. This is server to server.
                redirect_uri: "urn:ietf:wg:oauth:2.0:oob".to_string(),
            })?;

            let auth_client = AuthClient::new(base_client, manager);
            create_service(auth_client, config).await?
        }
        None => {
            log::debug!("using no authentication");
            create_service(builder.build()?, config).await?
        }
    };

    Ok(client)
}

async fn create_service<C>(client: C, config: &HttpConfig) -> anyhow::Result<RunningService<RoleClient, ()>>
where
    C: StreamableHttpClient + SseClient + Clone + Send + Sync + 'static,
{
    if config.uses_streamable_http() {
        log::debug!("config explicitly wants streamable-http");
        return streamable_http_service(client, config).await;
    }

    if config.uses_sse() {
        log::debug!("config explicitly wants SSE");
        return sse_service(client, config).await;
    }

    log::debug!("detecting protocol, starting with streamable-http");
    match streamable_http_service(client.clone(), config).await {
        Ok(service) => Ok(service),
        Err(_) => {
            log::warn!("streamable-http failed, trying SSE");
            sse_service(client, config).await
        }
    }
}

/// Creates a running service for streamable-http protocol.
async fn streamable_http_service<C>(client: C, config: &HttpConfig) -> anyhow::Result<RunningService<RoleClient, ()>>
where
    C: StreamableHttpClient + Send + Sync + 'static,
{
    log::debug!("creating a streamable-http downstream service");

    let transport_config = StreamableHttpClientTransportConfig::with_uri(config.url.to_string());
    let transport = StreamableHttpClientTransport::with_client(client, transport_config);

    Ok(().serve(transport).await?)
}

/// Creates a running service for SSE (Server-Sent Events) protocol.
async fn sse_service<C>(client: C, config: &HttpConfig) -> anyhow::Result<RunningService<RoleClient, ()>>
where
    C: SseClient + Send + Sync + 'static,
{
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
    log::debug!("Created HTTP client for SSE transport");

    let transport = SseClientTransport::start_with_client(client, client_config).await?;
    log::debug!("SSE transport started successfully");

    let service = ().serve(transport).await?;
    log::debug!("SSE service created and ready");

    Ok(service)
}
