//! Nexus configuration structures to map the nexus.toml configuration.

#![deny(missing_docs)]

mod client_identification;
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
mod proxy;
mod rate_limit;
mod server;
mod telemetry;
mod tls;

use std::path::Path;

pub use client_identification::*;
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
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// HTTP server configuration settings.
    pub server: ServerConfig,
    /// Model Context Protocol configuration settings.
    pub mcp: McpConfig,
    /// LLM configuration settings.
    pub llm: LlmConfig,
    /// Telemetry configuration settings.
    pub telemetry: TelemetryConfig,
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
                client_identification: ClientIdentificationConfig {
                    enabled: false,
                    validation: ClientIdentificationValidation {
                        group_values: {},
                    },
                    client_id: JwtClaim {
                        jwt_claim: "sub",
                    },
                    group_id: None,
                },
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
                proxy: ProxyConfig {
                    anthropic: AnthropicProxyConfig {
                        enabled: false,
                        path: "/proxy/anthropic",
                    },
                },
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
            telemetry: TelemetryConfig {
                service_name: None,
                resource_attributes: {},
                exporters: ExportersConfig {
                    otlp: OtlpExporterConfig {
                        enabled: false,
                        endpoint: Url {
                            scheme: "http",
                            cannot_be_a_base: false,
                            username: "",
                            password: None,
                            host: Some(
                                Domain(
                                    "localhost",
                                ),
                            ),
                            port: Some(
                                4317,
                            ),
                            path: "/",
                            query: None,
                            fragment: None,
                        },
                        protocol: Grpc,
                        timeout: 60s,
                        batch_export: BatchExportConfig {
                            scheduled_delay: 5s,
                            max_queue_size: 2048,
                            max_export_batch_size: 512,
                            max_concurrent_exports: 1,
                        },
                        grpc: None,
                        http: None,
                    },
                },
                tracing: TracingConfig {
                    sampling: 0.15,
                    parent_based_sampler: false,
                    collect: CollectConfig {
                        max_events_per_span: 128,
                        max_attributes_per_span: 128,
                        max_links_per_span: 128,
                        max_attributes_per_event: 128,
                        max_attributes_per_link: 128,
                    },
                    propagation: PropagationConfig {
                        trace_context: false,
                        aws_xray: false,
                    },
                    exporters: None,
                },
                metrics: MetricsConfig {
                    exporters: None,
                },
                logs: LogsConfig {
                    exporters: None,
                },
            },
        }
        "#);
    }
}
