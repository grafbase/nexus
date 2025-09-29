use std::time::{Duration, Instant};

use futures::StreamExt;
use tokio::sync::RwLock;

use crate::messages::openai::Model;
use crate::provider::Provider;

/// Cache TTL - 5 minutes as per RFC.
const CACHE_TTL: Duration = Duration::from_secs(300);

/// Cache entry for all discovered models.
#[derive(Clone, Debug)]
struct CachedModels {
    models: Vec<Model>,
    cached_at: Instant,
}

/// Manages model discovery and caching for providers with pattern-based routing.
pub(crate) struct ModelDiscovery {
    /// Cache for all discovered models.
    cache: RwLock<Option<CachedModels>>,
}

impl ModelDiscovery {
    /// Create a new model discovery manager.
    pub fn new() -> Self {
        Self {
            cache: RwLock::new(None),
        }
    }

    /// Get all models for the providers, including pattern-discovered and explicit models.
    pub async fn get_all_models(&self, providers: &[Box<dyn Provider>]) -> Vec<Model> {
        // Check cache first (read lock)
        {
            let cache = self.cache.read().await;

            if let Some(cached) = cache.as_ref()
                && cached.cached_at.elapsed() < CACHE_TTL
            {
                return cached.models.clone();
            }
        }

        // Acquire write lock to prevent thundering herd
        let mut cache = self.cache.write().await;

        // Double-check: another thread might have refreshed the cache while we were waiting for the write lock
        if let Some(cached) = cache.as_ref()
            && cached.cached_at.elapsed() < CACHE_TTL
        {
            return cached.models.clone();
        }

        // Now we're the only thread that will fetch models
        let mut all_models = Vec::new();

        // Fetch models from all providers in parallel
        let mut futures = providers
            .iter()
            .map(|provider| async move {
                let provider_name = provider.name();
                (provider_name, provider.list_models().await)
            })
            .collect::<futures::stream::FuturesUnordered<_>>();

        while let Some((provider_name, result)) = futures.next().await {
            match result {
                Ok(models) => {
                    // Providers handle prefixing:
                    // - Pattern-matched models: no prefix
                    // - Explicit models: provider prefix added
                    all_models.extend(models);
                }
                Err(e) => {
                    log::warn!("Failed to fetch models for provider {provider_name}: {e}");
                    // Continue with other providers
                }
            }
        }

        // Sort models for consistent ordering:
        // - Pattern-matched models (no slash) sorted alphabetically
        // - Explicit models (with slash) sorted alphabetically
        all_models.sort_by(|a, b| {
            let a_has_slash = a.id.contains('/');
            let b_has_slash = b.id.contains('/');

            match (a_has_slash, b_has_slash) {
                // Both pattern-matched or both explicit - sort by id
                (false, false) | (true, true) => a.id.cmp(&b.id),
                // Pattern-matched (no slash) comes before explicit (with slash)
                (false, true) => std::cmp::Ordering::Less,
                (true, false) => std::cmp::Ordering::Greater,
            }
        });

        // Cache all models together (we already have the write lock)
        *cache = Some(CachedModels {
            models: all_models.clone(),
            cached_at: Instant::now(),
        });

        all_models
    }
}
