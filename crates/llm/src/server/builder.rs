//! Builder for LLM server with conditional metrics

use std::sync::Arc;

use config::Config;
use rate_limit::TokenRateLimitManager;

use crate::{
    error::LlmError,
    provider::{
        Provider, anthropic::AnthropicProvider, bedrock::BedrockProvider, google::GoogleProvider,
        openai::OpenAIProvider,
    },
    server::{LlmHandler, LlmServer, LlmServerInner, metrics::LlmServerWithMetrics, tracing::LlmServerWithTracing},
};

pub(crate) struct LlmServerBuilder<'a> {
    config: &'a Config,
}

impl<'a> LlmServerBuilder<'a> {
    pub fn new(config: &'a Config) -> Self {
        Self { config }
    }

    pub async fn build(self) -> crate::Result<LlmHandler> {
        log::debug!(
            "Initializing LLM server with {} providers",
            self.config.llm.providers.len()
        );

        let mut providers = Vec::with_capacity(self.config.llm.providers.len());

        for (name, provider_config) in self.config.llm.providers.clone().into_iter() {
            log::debug!("Initializing provider: {name}");

            let provider: Box<dyn Provider> = match provider_config {
                config::LlmProviderConfig::Openai(api_config) => {
                    Box::new(OpenAIProvider::new(name.clone(), api_config)?)
                }
                config::LlmProviderConfig::Anthropic(api_config) => {
                    Box::new(AnthropicProvider::new(name.clone(), api_config)?)
                }
                config::LlmProviderConfig::Google(api_config) => {
                    Box::new(GoogleProvider::new(name.clone(), api_config)?)
                }
                config::LlmProviderConfig::Bedrock(bedrock_config) => {
                    Box::new(BedrockProvider::new(name.clone(), bedrock_config).await?)
                }
            };

            providers.push(provider);
        }

        // Check if any providers were successfully initialized
        if providers.is_empty() {
            return Err(LlmError::InternalError(Some(
                "Failed to initialize any LLM providers.".to_string(),
            )));
        } else {
            log::debug!("LLM server initialized with {} active provider(s)", providers.len());
        }

        // Initialize token rate limiter if any provider has rate limits configured
        let has_token_rate_limits = self
            .config
            .llm
            .providers
            .values()
            .any(|p| p.rate_limits().is_some() || p.models().values().any(|m| m.rate_limits().is_some()));

        let token_rate_limiter = if has_token_rate_limits {
            Some(
                TokenRateLimitManager::new(&self.config.server.rate_limits.storage, self.config.telemetry.as_ref())
                    .await
                    .map_err(|e| {
                        log::error!("Failed to initialize token rate limiter: {e}");
                        LlmError::InternalError(None)
                    })?,
            )
        } else {
            None
        };

        let pattern_routes = super::build_pattern_routes(&self.config.llm, &providers);
        let model_discovery = super::ModelDiscovery::new();

        let server = LlmServer {
            shared: Arc::new(LlmServerInner {
                providers,
                config: self.config.llm.clone(),
                token_rate_limiter,
                pattern_routes,
                model_discovery,
            }),
        };

        // Create handler with metrics and/or tracing based on configuration
        let has_telemetry = self.config.telemetry.is_some();
        let has_tracing = self.config.telemetry.as_ref().is_some_and(|t| t.tracing_enabled());

        let handler = match (has_telemetry, has_tracing) {
            (true, true) => {
                log::debug!("Telemetry and tracing enabled, wrapping LLM server with both middlewares");
                LlmHandler::WithMetricsAndTracing(LlmServerWithTracing::new(LlmServerWithMetrics::new(server)))
            }
            (true, false) => {
                log::debug!("Telemetry enabled, wrapping LLM server with metrics middleware");
                LlmHandler::WithMetrics(LlmServerWithMetrics::new(server))
            }
            (false, true) => {
                // This shouldn't happen (tracing requires telemetry), but handle it gracefully
                log::debug!("Tracing enabled without metrics, wrapping LLM server with tracing middleware only");
                LlmHandler::WithTracing(LlmServerWithTracing::new(server))
            }
            (false, false) => {
                log::debug!("Telemetry disabled, using direct LLM server");
                LlmHandler::Direct(server)
            }
        };

        Ok(handler)
    }
}
