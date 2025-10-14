//! Builder for LLM server with conditional metrics

use std::{collections::BTreeMap, sync::Arc};

use config::Config;
use rate_limit::TokenRateLimitManager;
use tokio::sync::watch;

use crate::{
    error::LlmError,
    provider::{
        Provider, anthropic::AnthropicProvider, bedrock::BedrockProvider, google::GoogleProvider,
        openai::OpenAIProvider,
    },
    server::{LlmServer, LlmServerInner, Server, metrics::LlmServerWithMetrics, tracing::LlmServerWithTracing},
};

use super::model_discovery::{ModelDiscovery, ModelMap};

pub(crate) struct LlmServerBuilder<'a> {
    config: &'a Config,
    force_tracing: bool,
    force_metrics: bool,
}

impl<'a> LlmServerBuilder<'a> {
    pub fn new(config: &'a Config, force_tracing: bool, force_metrics: bool) -> Self {
        Self {
            config,
            force_tracing,
            force_metrics,
        }
    }

    pub async fn build(self) -> crate::Result<Server> {
        log::debug!(
            "Initializing LLM server with {} providers",
            self.config.llm.providers.len()
        );

        let (model_sender, model_receiver) = watch::channel::<ModelMap>(Arc::new(BTreeMap::new()));

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

        let providers = Arc::new(providers);
        let llm_config = Arc::new(self.config.llm.clone());

        let model_discovery = ModelDiscovery::new(Arc::clone(&providers), Arc::clone(&llm_config));

        // Initialize token rate limiter if any provider has rate limits configured
        let has_token_rate_limits = llm_config
            .providers
            .values()
            .any(|p| p.rate_limits().is_some() || p.models().values().any(|m| m.rate_limits().is_some()));

        let token_rate_limiter = if has_token_rate_limits {
            Some(
                TokenRateLimitManager::new(&self.config.server.rate_limits.storage, &self.config.telemetry)
                    .await
                    .map_err(|e| {
                        log::error!("Failed to initialize token rate limiter: {e}");
                        LlmError::InternalError(None)
                    })?,
            )
        } else {
            None
        };

        let initial_models = model_discovery.fetch_models().await.map_err(|error| {
            log::error!("Initial model discovery failed: {error}");
            LlmError::InternalError(Some("Model discovery failed during startup".to_string()))
        })?;

        if model_sender.send(initial_models).is_err() {
            return Err(LlmError::InternalError(Some(
                "Model discovery channel closed before startup completed".to_string(),
            )));
        }

        let _discovery_task = model_discovery.spawn_updater(model_sender);

        let server = LlmServer {
            shared: Arc::new(LlmServerInner {
                providers,
                config: llm_config,
                token_rate_limiter,
                model_map: model_receiver,
            }),
        };

        // Create handler with metrics and/or tracing based on configuration
        let has_metrics = self.config.telemetry.metrics_enabled() || self.force_metrics;
        let has_tracing = self.config.telemetry.tracing_enabled() || self.force_tracing;

        let handler = match (has_metrics, has_tracing) {
            (true, true) => {
                log::debug!("Telemetry and tracing enabled, wrapping LLM server with both middlewares");
                Server::WithMetricsAndTracing(LlmServerWithTracing::new(LlmServerWithMetrics::new(server)))
            }
            (true, false) => {
                log::debug!("Telemetry enabled, wrapping LLM server with metrics middleware");
                Server::WithMetrics(LlmServerWithMetrics::new(server))
            }
            (false, true) => {
                // This shouldn't happen (tracing requires telemetry), but handle it gracefully
                log::debug!("Tracing enabled without metrics, wrapping LLM server with tracing middleware only");
                Server::WithTracing(LlmServerWithTracing::new(server))
            }
            (false, false) => {
                log::debug!("Telemetry disabled, using direct LLM server");
                Server::Direct(server)
            }
        };

        Ok(handler)
    }
}
