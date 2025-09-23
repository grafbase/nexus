use std::sync::Arc;

use config::McpConfig;
use futures_util::lock::Mutex;
use mini_moka::sync::Cache;
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};

use crate::{access, downstream::Downstream, server::search::SearchTool};

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

    pub async fn get_or_create(
        &self,
        token: &SecretString,
        user_group: Option<&str>,
    ) -> anyhow::Result<Arc<CachedDownstream>> {
        // Include user group in cache key to ensure group-specific filtering
        let cache_key = if let Some(group) = user_group {
            format!("{}_{}", hash_token(token.expose_secret()), group)
        } else {
            hash_token(token.expose_secret())
        };

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

        // Get all tools from downstream servers
        let all_tools: Vec<_> = downstream.list_tools().cloned().collect();

        // Filter tools based on user group access control
        let filtered_tools = filter_tools_for_dynamic_servers(user_group, &all_tools, &self.config);

        // Create search tool with filtered tools
        let search_tool = SearchTool::new(filtered_tools)?;

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

/// Filters tools from dynamic servers based on user group access control.
///
/// Dynamic servers use auth forwarding, so they already get user-specific tools.
/// This function applies an additional layer of group-based access control.
fn filter_tools_for_dynamic_servers(
    user_group: Option<&str>,
    tools: &[rmcp::model::Tool],
    config: &McpConfig,
) -> Vec<rmcp::model::Tool> {
    let mut filtered_tools = Vec::new();

    for tool in tools {
        // Parse server name from tool name (format: "server__tool")
        let Some((server_name, tool_name)) = tool.name.split_once("__") else {
            log::warn!("Dynamic tool with invalid name format: {}", tool.name);
            continue;
        };

        // Get server configuration
        let Some(server_config) = config.servers.get(server_name) else {
            log::warn!(
                "Dynamic tool '{}' references unknown server '{}'",
                tool.name,
                server_name
            );
            continue;
        };

        // Check if user can access this tool
        if access::can_access_tool(user_group, server_config, tool_name) {
            filtered_tools.push(tool.clone());
        } else {
            log::debug!("Filtered out dynamic tool '{}' for group {:?}", tool.name, user_group);
        }
    }

    filtered_tools
}
