//! Access control logic for MCP servers and tools.
//!
//! This module implements the core access resolution logic that determines
//! whether a user with a specific group can access MCP servers and tools.

use std::collections::BTreeSet;

use config::McpServer;

/// Checks if a user can access a specific tool on a server.
///
/// # Access Resolution Order
///
/// 1. Check tool-level rules (if defined)
/// 2. Fall back to server-level rules
/// 3. If no rules are defined, access is allowed
///
/// # Arguments
///
/// * `user_group` - The user's group (if any)
/// * `server_config` - The MCP server configuration
/// * `tool_name` - The name of the tool to check
///
/// # Returns
///
/// `true` if access is allowed, `false` otherwise
pub fn can_access_tool(user_group: Option<&str>, server_config: &McpServer, tool_name: &str) -> bool {
    // First check tool-level access rules
    if let Some(tool_config) = server_config.tool_access_configs().get(tool_name) {
        return check_access(user_group, tool_config.allow.as_ref(), tool_config.deny.as_ref());
    }

    // Fall back to server-level access rules
    can_access_server(user_group, server_config)
}

/// Checks if a user can access any tools on a server.
///
/// This checks only server-level access rules.
///
/// # Arguments
///
/// * `user_group` - The user's group (if any)
/// * `server_config` - The MCP server configuration
///
/// # Returns
///
/// `true` if access is allowed, `false` otherwise
fn can_access_server(user_group: Option<&str>, server_config: &McpServer) -> bool {
    check_access(user_group, server_config.allow(), server_config.deny())
}

/// Core access check logic implementing the two-phase check.
///
/// # Rules
///
/// 1. If `allow` is defined and non-empty, user's group must be in it
/// 2. If `allow` is defined but empty, deny all access
/// 3. If `deny` is defined, user's group must NOT be in it
/// 4. If no rules are defined, allow access
fn check_access(user_group: Option<&str>, allow: Option<&BTreeSet<String>>, deny: Option<&BTreeSet<String>>) -> bool {
    // Phase 1: Check allow-list
    if let Some(allow_list) = allow {
        // Empty allow list means deny all
        if allow_list.is_empty() {
            return false;
        }

        // User must have a group and it must be in the allow list
        match user_group {
            Some(group) => {
                if !allow_list.contains(group) {
                    return false;
                }
            }
            None => {
                // User has no group but allow list requires one
                return false;
            }
        }
    }

    // Phase 2: Check deny-list
    if let Some(deny_list) = deny
        && let Some(group) = user_group
        && deny_list.contains(group)
    {
        return false;
    }

    // Passed both checks
    true
}

/// Gets all accessible tools for a user from a server.
///
/// Returns a list of tool names that the user can access based on their group.
///
/// # Arguments
///
/// * `user_group` - The user's group (if any)
/// * `server_config` - The MCP server configuration
/// * `all_tools` - List of all tool names available on the server
#[allow(dead_code)] // Will be used in Phase 4 for per-group search indexes
pub fn filter_accessible_tools(
    user_group: Option<&str>,
    server_config: &McpServer,
    all_tools: &[String],
) -> Vec<String> {
    all_tools
        .iter()
        .filter(|tool_name| can_access_tool(user_group, server_config, tool_name))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use config::{McpServer, StdioConfig, ToolAccessConfig};
    use std::collections::BTreeMap;

    fn create_test_server(
        allow: Option<BTreeSet<String>>,
        deny: Option<BTreeSet<String>>,
        tools: BTreeMap<String, ToolAccessConfig>,
    ) -> McpServer {
        // Create a test STDIO server with the specified access controls
        let stdio_config = StdioConfig {
            cmd: vec!["test".to_string()],
            env: BTreeMap::new(),
            cwd: None,
            stderr: config::StdioTarget::Simple(config::StdioTargetType::Null),
            rate_limits: None,
            allow,
            deny,
            tools,
        };
        McpServer::Stdio(Box::new(stdio_config))
    }

    #[test]
    fn no_restrictions_allows_all() {
        let server_config = create_test_server(None, None, BTreeMap::new());

        assert!(super::can_access_server(None, &server_config));
        assert!(super::can_access_server(Some("any_group"), &server_config));
        assert!(super::can_access_tool(None, &server_config, "any_tool"));
    }

    #[test]
    fn empty_allow_denies_all() {
        let server_config = create_test_server(Some(BTreeSet::new()), None, BTreeMap::new());

        assert!(!can_access_server(None, &server_config));
        assert!(!can_access_server(Some("any_group"), &server_config));
        assert!(!can_access_tool(Some("any_group"), &server_config, "any_tool"));
    }

    #[test]
    fn allow_restricts_access() {
        let mut allow_list = BTreeSet::new();
        allow_list.insert("premium".to_string());
        allow_list.insert("enterprise".to_string());

        let server_config = create_test_server(Some(allow_list), None, BTreeMap::new());

        assert!(!can_access_server(None, &server_config));
        assert!(!can_access_server(Some("basic"), &server_config));
        assert!(can_access_server(Some("premium"), &server_config));
        assert!(can_access_server(Some("enterprise"), &server_config));
    }

    #[test]
    fn deny_blocks_specific_groups() {
        let mut deny_list = BTreeSet::new();
        deny_list.insert("suspended".to_string());
        deny_list.insert("trial_expired".to_string());

        let server_config = create_test_server(None, Some(deny_list), BTreeMap::new());

        assert!(can_access_server(None, &server_config));
        assert!(can_access_server(Some("premium"), &server_config));
        assert!(!can_access_server(Some("suspended"), &server_config));
        assert!(!can_access_server(Some("trial_expired"), &server_config));
    }

    #[test]
    fn allow_and_deny_combined() {
        let mut allow_list = BTreeSet::new();
        allow_list.insert("premium".to_string());
        allow_list.insert("enterprise".to_string());

        let mut deny_list = BTreeSet::new();
        deny_list.insert("suspended".to_string());

        let server_config = create_test_server(Some(allow_list), Some(deny_list.clone()), BTreeMap::new());

        // Not in allow list
        assert!(!can_access_server(Some("basic"), &server_config));

        // In allow list
        assert!(can_access_server(Some("premium"), &server_config));

        // In allow list but also in deny list (deny takes precedence)
        let mut allow_with_suspended = BTreeSet::new();
        allow_with_suspended.insert("premium".to_string());
        allow_with_suspended.insert("suspended".to_string());
        let server_config2 = create_test_server(Some(allow_with_suspended), Some(deny_list.clone()), BTreeMap::new());
        assert!(!can_access_server(Some("suspended"), &server_config2));
    }

    #[test]
    fn tool_level_overrides_server_level() {
        // Server allows only "basic"
        let mut server_allow = BTreeSet::new();
        server_allow.insert("basic".to_string());

        // Tool allows "premium" (overrides server)
        let mut tool_allow = BTreeSet::new();
        tool_allow.insert("premium".to_string());

        let mut tools = BTreeMap::new();
        tools.insert(
            "advanced_tool".to_string(),
            ToolAccessConfig {
                allow: Some(tool_allow),
                deny: None,
            },
        );

        let server_config = create_test_server(Some(server_allow), None, tools);

        // Server-level access
        assert!(can_access_server(Some("basic"), &server_config));
        assert!(!can_access_server(Some("premium"), &server_config));

        // Tool-level access (overrides server)
        assert!(!can_access_tool(Some("basic"), &server_config, "advanced_tool"));
        assert!(can_access_tool(Some("premium"), &server_config, "advanced_tool"));

        // Tool without specific config uses server-level
        assert!(can_access_tool(Some("basic"), &server_config, "other_tool"));
        assert!(!can_access_tool(Some("premium"), &server_config, "other_tool"));
    }

    #[test]
    fn filter_accessible_tools_works() {
        let mut server_allow = BTreeSet::new();
        server_allow.insert("basic".to_string());

        let mut premium_tool_allow = BTreeSet::new();
        premium_tool_allow.insert("premium".to_string());

        let mut tools = BTreeMap::new();
        tools.insert(
            "premium_tool".to_string(),
            ToolAccessConfig {
                allow: Some(premium_tool_allow),
                deny: None,
            },
        );

        let server_config = create_test_server(Some(server_allow), None, tools);

        let all_tools = vec![
            "basic_tool".to_string(),
            "premium_tool".to_string(),
            "another_tool".to_string(),
        ];

        // Basic user can access all except premium_tool
        let basic_tools = filter_accessible_tools(Some("basic"), &server_config, &all_tools);
        assert_eq!(basic_tools, vec!["basic_tool", "another_tool"]);

        // Premium user can only access premium_tool (not in server allow list)
        let premium_tools = filter_accessible_tools(Some("premium"), &server_config, &all_tools);
        assert_eq!(premium_tools, vec!["premium_tool"]);
    }
}
