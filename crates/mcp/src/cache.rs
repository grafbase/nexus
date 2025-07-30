use std::sync::Arc;

use config::McpConfig;
use futures_util::lock::Mutex;
use mini_moka::sync::Cache;
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};

use crate::{downstream::Downstream, server::search::SearchTool};

pub struct CachedDownstream {
    pub downstream: Downstream,
    pub search_tool: SearchTool,
}

pub struct DynamicDownstreamCache {
    cache: Cache<String, Arc<CachedDownstream>>,
    config: McpConfig,
    refresh_lock: Mutex<()>,
}

impl DynamicDownstreamCache {
    pub fn new(config: McpConfig) -> Self {
        let cache = Cache::builder()
            .max_capacity(config.downstream_cache.max_size)
            .time_to_idle(config.downstream_cache.idle_timeout)
            .build();

        Self {
            cache,
            config,
            refresh_lock: Mutex::new(()),
        }
    }

    pub async fn get_or_create(&self, token: &SecretString) -> anyhow::Result<Arc<CachedDownstream>> {
        // Hash token for cache key, so we can be sure nobody ever accidentally exposes it
        let cache_key = hash_token(token.expose_secret());

        if let Some(cached) = self.cache.get(&cache_key) {
            return Ok(cached);
        };

        let _guard = self.refresh_lock.lock().await;

        // Somebody else refreshed the cache while we were waiting for the lock
        if let Some(cached) = self.cache.get(&cache_key) {
            return Ok(cached);
        };

        // Create downstream with token - this will use finalize() to inject auth
        let downstream = Downstream::new(&self.config, Some(token)).await?;

        // Create search tool with all downstream tools
        let tools: Vec<_> = downstream.list_tools().cloned().collect();
        let search_tool = SearchTool::new(tools)?;

        let cached = Arc::new(CachedDownstream {
            downstream,
            search_tool,
        });

        self.cache.insert(cache_key, cached.clone());

        Ok(cached)
    }
}

fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    format!("{:x}", hasher.finalize())
}
