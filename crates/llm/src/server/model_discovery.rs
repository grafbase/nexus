use std::{collections::BTreeMap, sync::Arc, time::Duration};

use anyhow::anyhow;
use config::LlmConfig;
use futures::StreamExt;
use futures::stream::FuturesUnordered;
use tokio::{sync::watch, task::JoinHandle, time};

use crate::{messages::openai::Model, provider::Provider};

/// Default refresh cadence for the background discovery loop.
const DISCOVERY_INTERVAL: Duration = Duration::from_secs(300);

/// Normalized metadata stored for each discovered model.
#[derive(Clone, Debug)]
pub(crate) struct ModelInfo {
    pub provider_name: String,
    pub created: u64,
    pub owned_by: String,
    #[allow(dead_code)]
    pub display_name: Option<String>,
}

/// Thread-safe mapping from model identifier to metadata.
pub(crate) type ModelMap = Arc<BTreeMap<String, ModelInfo>>;

#[derive(Clone)]
pub(crate) struct ModelDiscovery {
    providers: Arc<Vec<Box<dyn Provider>>>,
    config: Arc<LlmConfig>,
    interval: Duration,
}

/// Successful provider fetch paired with its configuration index.
struct ProviderModels {
    index: usize,
    name: String,
    models: Vec<Model>,
}

/// Error reported by a provider while listing models.
struct ProviderError {
    name: String,
    error: anyhow::Error,
}

impl ModelDiscovery {
    /// Create a discovery coordinator using the default interval.
    pub(crate) fn new(providers: Arc<Vec<Box<dyn Provider>>>, config: Arc<LlmConfig>) -> Self {
        Self {
            providers,
            config,
            interval: DISCOVERY_INTERVAL,
        }
    }

    /// Perform a discovery pass and return the populated model map.
    pub(crate) async fn fetch_models(&self) -> anyhow::Result<ModelMap> {
        self.build_model_map()
            .await
            .map(Arc::new)
            .map_err(|errors| self.aggregate_errors(errors))
    }

    /// Spawn a background task that refreshes models on a fixed interval.
    ///
    /// The loop exits automatically once every receiver drops the watch channel.
    pub(crate) fn spawn_updater(&self, sender: watch::Sender<ModelMap>) -> JoinHandle<()> {
        let discovery = self.clone();

        tokio::spawn(async move {
            let mut ticker = time::interval(discovery.interval);

            loop {
                ticker.tick().await;

                match discovery.build_model_map().await {
                    Ok(map) => {
                        if sender.send(Arc::new(map)).is_err() {
                            log::debug!("Model discovery watch channel closed; stopping background task");
                            break;
                        }
                    }
                    Err(errors) => {
                        discovery.log_refresh_errors(errors);
                    }
                }
            }
        })
    }

    /// Build a fresh model map while collecting provider-level failures.
    async fn build_model_map(&self) -> Result<BTreeMap<String, ModelInfo>, Vec<ProviderError>> {
        let (mut provider_models, errors) = self.fetch_provider_models().await;

        if !errors.is_empty() {
            return Err(errors);
        }

        provider_models.sort_by_key(|entry| entry.index);

        let mut map: BTreeMap<String, ModelInfo> = BTreeMap::new();

        for ProviderModels { name, models, .. } in provider_models {
            let filter = self
                .config
                .providers
                .get(&name)
                .and_then(|provider_config| provider_config.model_filter());

            for model in models {
                let is_discovered = !model.id.contains('/');

                if is_discovered && filter.is_some_and(|regex| !regex.is_match(&model.id)) {
                    continue;
                }

                if let Some(existing) = map.get(&model.id) {
                    if is_discovered && existing.provider_name != name {
                        log::warn!(
                            "Model '{}' already claimed by provider '{}', skipping duplicate from provider '{}'",
                            model.id,
                            existing.provider_name,
                            name
                        );
                    } else {
                        log::debug!("Provider '{}' returned duplicate model '{}', skipping", name, model.id);
                    }

                    continue;
                }

                map.insert(
                    model.id,
                    ModelInfo {
                        provider_name: name.clone(),
                        created: model.created,
                        owned_by: model.owned_by,
                        display_name: None,
                    },
                );
            }
        }

        Ok(map)
    }

    /// Fetch models from every provider concurrently, preserving config order.
    async fn fetch_provider_models(&self) -> (Vec<ProviderModels>, Vec<ProviderError>) {
        let mut futures = FuturesUnordered::new();

        for (index, provider) in self.providers.iter().enumerate() {
            let provider_ref = provider.as_ref();
            let name = provider_ref.name().to_string();

            futures.push(async move {
                let result = provider_ref.list_models().await;
                (index, name, result)
            });
        }

        let mut successes = Vec::new();
        let mut errors = Vec::new();

        while let Some((index, name, result)) = futures.next().await {
            match result {
                Ok(models) => successes.push(ProviderModels { index, name, models }),
                Err(error) => errors.push(ProviderError { name, error }),
            }
        }

        (successes, errors)
    }

    /// Collapse provider errors into a single `anyhow::Error` while logging.
    fn aggregate_errors(&self, errors: Vec<ProviderError>) -> anyhow::Error {
        let mut error = anyhow!("model discovery failed for {} provider(s)", errors.len());

        for ProviderError {
            name,
            error: provider_error,
        } in errors
        {
            log::error!("Failed to discover models for provider '{name}': {provider_error}");
            error = error.context(format!("provider {name}: {provider_error}"));
        }

        error
    }

    /// Log refresh errors reported by the background loop and continue.
    fn log_refresh_errors(&self, errors: Vec<ProviderError>) {
        for ProviderError { name, error } in errors {
            log::error!("Failed to refresh models for provider '{name}': {error}");
        }
    }
}
