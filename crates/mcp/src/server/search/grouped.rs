//! Per-group search index implementation for access-controlled tool discovery.
//!
//! This module provides search functionality where each user group has its own
//! pre-filtered search index containing only the tools they can access.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use config::{Config, McpConfig};
use rmcp::model::Tool;

use super::SearchTool;
use crate::access;

/// A search tool that maintains separate indexes based on whether groups are configured.
///
/// This trades memory for search quality and performance by pre-filtering
/// tools into group-specific indexes rather than filtering search results.
pub enum GroupedSearchTool {
    /// When groups are defined in client identification - each group has its own index
    WithGroups {
        /// Per-group search indexes
        group_indexes: BTreeMap<String, Arc<SearchTool>>,
    },
    /// When no groups are defined - single index with accessible tools
    NoGroups {
        /// Search index with all accessible tools (excluding those with empty allow_groups)
        all_tools_index: Arc<SearchTool>,
    },
}

impl GroupedSearchTool {
    /// Creates a new grouped search tool from the available tools and configuration.
    ///
    /// This builds separate search indexes based on whether groups are configured:
    /// - WithGroups: Each group gets its own pre-filtered index
    /// - NoGroups: Single index excluding tools with empty allow_groups
    pub fn new(tools: Vec<Tool>, config: &Config) -> anyhow::Result<Self> {
        // Validate that configured tool access rules reference actual tools
        validate_tool_configs(&tools, &config.mcp);

        // Extract defined groups from client identification config
        let defined_groups = config
            .server
            .client_identification
            .as_ref()
            .map(|ci| &ci.validation.group_values)
            .filter(|groups| !groups.is_empty());

        // If no groups are defined, create a single index
        let Some(groups) = defined_groups else {
            log::debug!("No groups defined, creating single search index");

            // Filter out tools with empty allow_groups (which means "deny all")
            let accessible_tools = filter_tools_by_access(&tools, &config.mcp, None);

            log::debug!(
                "Created no-groups index with {} accessible tools (from {} total)",
                accessible_tools.len(),
                tools.len()
            );

            let all_tools_index = Arc::new(SearchTool::new(accessible_tools)?);

            return Ok(Self::NoGroups { all_tools_index });
        };

        // Groups are defined - create per-group indexes
        log::debug!("Building per-group search indexes for {} groups", groups.len());

        let mut group_indexes = BTreeMap::new();

        for group in groups {
            let group_tools = filter_tools_by_access(&tools, &config.mcp, Some(group));

            log::debug!(
                "Creating search index for group '{}' with {} accessible tools",
                group,
                group_tools.len()
            );

            let search_tool = SearchTool::new(group_tools)?;
            group_indexes.insert(group.clone(), Arc::new(search_tool));
        }

        Ok(Self::WithGroups { group_indexes })
    }

    /// Gets the appropriate search tool for the given user group.
    pub fn get_search_tool(&self, user_group: Option<&str>) -> Arc<SearchTool> {
        match self {
            Self::NoGroups { all_tools_index } => all_tools_index.clone(),
            Self::WithGroups { group_indexes } => {
                let group = match user_group {
                    Some(g) => g,
                    None => {
                        // When groups are configured, users MUST have a group
                        // This should be caught by middleware, but handle gracefully
                        log::warn!("User without group when groups are configured - returning empty search tool");
                        return create_empty_search_tool();
                    }
                };

                group_indexes.get(group).cloned().unwrap_or_else(|| {
                    log::debug!("Unknown group '{}', returning empty search tool", group);
                    create_empty_search_tool()
                })
            }
        }
    }
}

/// Creates an empty search tool.
///
/// This cannot fail in practice as it only creates an in-memory index with no documents.
fn create_empty_search_tool() -> Arc<SearchTool> {
    Arc::new(SearchTool::new(Vec::new()).expect("BUG: Failed to create empty SearchTool. This should never happen."))
}

/// Validates that tool-level access configurations reference actual tools that exist.
///
/// Logs warnings for any tool configurations that reference non-existent tools.
fn validate_tool_configs(tools: &[Tool], config: &McpConfig) {
    // Build a set of actual tool names for each server
    let mut server_tools: HashMap<_, HashSet<String>> = HashMap::new();

    for tool in tools {
        if let Some((server_name, tool_name)) = tool.name.split_once("__") {
            server_tools
                .entry(server_name.to_string())
                .or_default()
                .insert(tool_name.to_string());
        }
    }

    // Check each server's tool configurations
    for (server_name, server_config) in &config.servers {
        // Get the actual tools for this server
        let actual_tools = server_tools.get(server_name);

        // Check each configured tool access rule
        for configured_tool_name in server_config.tool_access_configs().keys() {
            // If we have no tools for this server, or the tool doesn't exist
            let tool_exists = actual_tools
                .map(|tools| tools.contains(configured_tool_name))
                .unwrap_or(false);

            if tool_exists {
                continue;
            }

            log::warn!(
                "Tool access configuration for '{configured_tool_name}' on server '{server_name}' references a non-existent tool. \
                 This configuration will be ignored.",
            );
        }
    }
}

/// Common filtering logic for tools based on access control.
fn filter_tools_by_access(tools: &[Tool], config: &McpConfig, user_group: Option<&str>) -> Vec<Tool> {
    let mut filtered_tools = Vec::new();

    for tool in tools {
        // Parse server name from tool name (format: "server__tool")
        let Some((server_name, tool_name)) = tool.name.split_once("__") else {
            log::warn!("Skipping tool with invalid name format: {}", tool.name);
            continue;
        };

        // Get server configuration
        let Some(server_config) = config.servers.get(server_name) else {
            log::warn!("Tool '{}' references unknown server '{}'", tool.name, server_name);
            continue;
        };

        // Check if user can access this tool
        if access::can_access_tool(user_group, server_config, tool_name) {
            filtered_tools.push(tool.clone());
        }
    }

    filtered_tools
}
