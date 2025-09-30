//! Nexus configuration structures to map the nexus.toml configuration.

#![deny(missing_docs)]

mod client_identification;
mod client_identity;
mod client_ip;
mod cors;
mod csrf;
mod headers;
mod health;
mod http_types;
mod llm;
mod loader;
mod mcp;
mod oauth;
mod rate_limit;
mod server;
mod telemetry;
mod tls;

use std::path::Path;

pub use client_identification::*;
pub use client_identity::ClientIdentity;
pub use client_ip::*;
pub use cors::*;
pub use csrf::CsrfConfig;
pub use headers::{
    HeaderForward, HeaderInsert, HeaderRemove, HeaderRenameDuplicate, HeaderRule, McpHeaderRule, NameOrPattern,
    NamePattern,
};
pub use health::HealthConfig;
pub use http_types::{HeaderName, HeaderValue};
pub use llm::{
    ApiModelConfig, ApiProviderConfig, BedrockModelConfig, BedrockProviderConfig, LlmConfig, LlmProtocol,
    LlmProviderConfig, ModelConfig, ModelFilter, ProviderType,
};
pub use mcp::{
    ClientAuthConfig, HttpConfig, HttpProtocol, McpConfig, McpServer, McpServerRateLimit, StdioConfig, StdioTarget,
    StdioTargetType, TlsClientConfig, ToolAccessConfig,
};
pub use oauth::{OauthConfig, ProtectedResourceConfig};
pub use rate_limit::*;
use serde::Deserialize;
pub use server::ServerConfig;
pub use telemetry::OtlpProtocol;
pub use telemetry::exporters::{
    ExportersConfig, GrpcHeaders, HttpHeaders, OtlpExporterConfig, OtlpGrpcConfig, OtlpHttpConfig,
};
pub use telemetry::tracing::{PropagationConfig, TracingConfig};
pub use telemetry::{LogsConfig, MetricsConfig, TelemetryConfig};
pub use tls::TlsServerConfig;

/// Main configuration structure for the Nexus application.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// HTTP server configuration settings.
    #[serde(default)]
    pub server: ServerConfig,
    /// Model Context Protocol configuration settings.
    #[serde(default)]
    pub mcp: McpConfig,
    /// LLM configuration settings.
    #[serde(default)]
    pub llm: LlmConfig,
    /// Telemetry configuration settings.
    #[serde(default)]
    pub telemetry: Option<TelemetryConfig>,
}

impl Config {
    /// Load configuration from a file path.
    pub fn load<P: AsRef<Path>>(path: P) -> anyhow::Result<Config> {
        loader::load(path)
    }

    /// Validates that the configuration has at least one functional downstream.
    pub fn validate(&self) -> anyhow::Result<()> {
        loader::validate_has_downstreams(self)
    }
}

#[cfg(test)]
mod tests {
    use insta::assert_debug_snapshot;

    use crate::Config;

    #[test]
    fn defaults() {
        let config: Config = toml::from_str("").unwrap();

        assert_debug_snapshot!(&config, @r#"
        Config {
            server: ServerConfig {
                listen_address: None,
                tls: None,
                health: HealthConfig {
                    enabled: true,
                    listen: None,
                    path: "/health",
                },
                cors: None,
                csrf: CsrfConfig {
                    enabled: false,
                    header_name: "X-Nexus-CSRF-Protection",
                },
                oauth: None,
                rate_limits: RateLimitConfig {
                    enabled: false,
                    storage: Memory,
                    global: None,
                    per_ip: None,
                },
                client_identification: None,
                client_ip: ClientIpConfig {
                    x_real_ip: false,
                    x_forwarded_for_trusted_hops: None,
                },
            },
            mcp: McpConfig {
                enabled: true,
                path: "/mcp",
                downstream_cache: McpDownstreamCacheConfig {
                    max_size: 1000,
                    idle_timeout: 600s,
                },
                servers: {},
                enable_structured_content: true,
                headers: [],
            },
            llm: LlmConfig {
                enabled: true,
                protocols: LlmProtocolsConfig {
                    openai: OpenAIProtocolConfig {
                        enabled: true,
                        path: "/llm/openai",
                    },
                    anthropic: AnthropicProtocolConfig {
                        enabled: false,
                        path: "/llm/anthropic",
                    },
                },
                providers: {},
            },
            telemetry: None,
        }
        "#);
    }
}
