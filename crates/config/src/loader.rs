use std::{path::Path, str::FromStr};

use anyhow::bail;
use indoc::indoc;
use serde::Deserialize;
use serde_dynamic_string::DynamicString;
use std::fmt::Write;
use toml::Value;

use crate::{ClientIdentificationConfig, Config, LlmProviderConfig};

pub fn load<P: AsRef<Path>>(path: P) -> anyhow::Result<Config> {
    let path = path.as_ref().to_path_buf();
    let content = std::fs::read_to_string(&path)?;
    let mut raw_config: Value = toml::from_str(&content)?;

    expand_dynamic_strings(&mut Vec::new(), &mut raw_config)?;

    let config = Config::deserialize(raw_config)?;
    validate_has_downstreams(&config)?;

    // Validate LLM rate limit configuration and log warnings
    let warnings = validate_rate_limits(&config)?;

    for warning in warnings {
        log::warn!("{warning}");
    }

    Ok(config)
}

pub(crate) fn validate_has_downstreams(config: &Config) -> anyhow::Result<()> {
    // Check if any downstreams are actually configured (not just enabled)
    let has_mcp_servers = config.mcp.enabled() && config.mcp.has_servers();
    let has_llm_providers = config.llm.enabled() && config.llm.has_providers();

    if !has_mcp_servers && !has_llm_providers {
        bail!(indoc! {r#"
            No downstream servers configured. Nexus requires at least one MCP server or LLM provider to function.

            Example configuration:

            For MCP servers:

              [mcp.servers.example]
              cmd = ["path/to/mcp-server"]

            For LLM providers:

              [llm.providers.openai]
              type = "openai"
              api_key = "{{ env.OPENAI_API_KEY }}"

            See https://nexusrouter.com/docs for more configuration examples.
        "#});
    }

    // If LLM is enabled and has providers, it must have at least one protocol endpoint
    if has_llm_providers && !config.llm.has_protocol_endpoints() {
        bail!(indoc! {r#"
            LLM providers are configured but no protocol endpoints are enabled. At least one protocol endpoint must be enabled.

            Both protocols are enabled by default. To explicitly enable them in configuration:

              [llm.protocols.openai]
              enabled = true
              path = "/llm/openai"

              [llm.protocols.anthropic]
              enabled = true
              path = "/llm/anthropic"
        "#});
    }

    // Validate that protocol endpoint paths are unique if both are enabled
    if config.llm.protocols.openai.enabled
        && config.llm.protocols.anthropic.enabled
        && config.llm.protocols.openai.path == config.llm.protocols.anthropic.path
    {
        bail!(
            "Duplicate LLM protocol endpoint path: OpenAI and Anthropic protocols cannot use the same path ({})",
            config.llm.protocols.openai.path
        );
    }

    Ok(())
}

fn expand_dynamic_strings<'a>(path: &mut Vec<Result<&'a str, usize>>, value: &'a mut Value) -> anyhow::Result<()> {
    match value {
        Value::String(s) => match DynamicString::<String>::from_str(s) {
            Ok(out) => *s = out.into_inner(),
            Err(err) => {
                let mut p = String::new();

                for segment in path {
                    match segment {
                        Ok(s) => {
                            p.push_str(s);
                            p.push('.');
                        }
                        Err(i) => write!(p, "[{i}]").unwrap(),
                    }
                }

                if p.ends_with('.') {
                    p.pop();
                }

                bail!("Failed to expand dynamic string at path '{p}': {err}");
            }
        },
        Value::Array(values) => {
            for (i, value) in values.iter_mut().enumerate() {
                path.push(Err(i));
                expand_dynamic_strings(path, value)?;
                path.pop();
            }
        }
        Value::Table(map) => {
            for (key, value) in map {
                path.push(Ok(key.as_str()));
                expand_dynamic_strings(path, value)?;
                path.pop();
            }
        }
        Value::Integer(_) | Value::Float(_) | Value::Boolean(_) | Value::Datetime(_) => (),
    }

    Ok(())
}

/// Validates the rate limit configuration and returns warnings.
pub(crate) fn validate_rate_limits(config: &Config) -> anyhow::Result<Vec<String>> {
    let mut warnings = Vec::new();

    // Check if any LLM provider has rate limits defined
    let has_llm_rate_limits = check_if_any_llm_rate_limits_exist(config);

    // If we do not have LLM rate limits, skip further validation
    if !has_llm_rate_limits {
        return Ok(Vec::new());
    }

    // If we have LLM rate limits, client identification MUST be enabled
    let Some(client_identification) = config.server.client_identification.as_ref() else {
        anyhow::bail!(
            "LLM rate limits are configured but client identification is not enabled. Enable client identification in [server.client_identification]"
        );
    };

    // If LLM rate limits are defined, client identification MUST be enabled
    if !client_identification.enabled {
        anyhow::bail!(
            "LLM rate limits are configured but client identification is not enabled. Enable client identification in [server.client_identification]"
        );
    }

    // If group_id is configured, group_values MUST be defined
    if client_identification.group_id.is_some() && client_identification.validation.group_values.is_empty() {
        anyhow::bail!(
            "group_id is configured for client identification but validation.group_values is empty. Define group_values in [server.client_identification.validation]"
        );
    }

    // Check if any provider has group-based rate limits
    let has_group_rate_limits = check_if_any_group_rate_limits_exist(config);

    if !has_group_rate_limits {
        return Ok(warnings);
    }

    // If group rate limits are defined, group identification MUST be configured
    if client_identification.group_id.is_none() {
        anyhow::bail!(indoc! {r#"
            Group-based rate limits are configured but group_id is not set in client identification.
            To fix this, add a group_id configuration to your [server.client_identification] section, for example:

            [server.client_identification]
            enabled = true
            client_id.http_header = "X-Client-ID"      # or client_id.jwt_claim = "sub"
            group_id.http_header = "X-Group-ID"        # or group_id.jwt_claim = "groups"
            
            [server.client_identification.validation]
            group_values = ["basic", "premium", "enterprise"]
        "#});
    }

    // Validate all group names in rate limits exist in group_values
    for (provider_name, provider) in &config.llm.providers {
        validate_provider_groups(client_identification, provider_name, provider)?;

        // Generate warnings for fallback scenarios
        if client_identification.validation.group_values.is_empty() {
            continue;
        }

        for group in &client_identification.validation.group_values {
            check_group_fallbacks(group, provider_name, provider, &mut warnings);
        }
    }

    Ok(warnings)
}

fn validate_provider_groups(
    config: &ClientIdentificationConfig,
    provider_name: &str,
    provider: &LlmProviderConfig,
) -> anyhow::Result<()> {
    // Check provider-level group rate limits
    if let Some(rate_limits) = &provider.rate_limits()
        && let Some(per_user) = &rate_limits.per_user
    {
        for group_name in per_user.groups.keys() {
            if config.validation.group_values.contains(group_name) {
                continue;
            }

            anyhow::bail!("Group '{group_name}' in provider '{provider_name}' rate limits not found in group_values",);
        }
    }

    // Check model-level group rate limits
    for (model_name, model) in provider.models() {
        let Some(rate_limits) = model.rate_limits() else {
            continue;
        };

        if let Some(per_user) = &rate_limits.per_user {
            for group_name in per_user.groups.keys() {
                if config.validation.group_values.contains(group_name) {
                    continue;
                }

                anyhow::bail!(
                    "Group '{group_name}' in model '{provider_name}/{model_name}' rate limits not found in group_values",
                );
            }
        }
    }

    Ok(())
}

/// Check if any LLM provider or model has rate limits configured.
fn check_if_any_llm_rate_limits_exist(config: &Config) -> bool {
    for provider in config.llm.providers.values() {
        // Check if provider has any rate limits
        if provider.rate_limits().is_some() {
            return true;
        }

        // Check if provider has group-specific rate limits
        if let Some(limits) = provider.rate_limits()
            && let Some(per_user) = &limits.per_user
            && !per_user.groups.is_empty()
        {
            return true;
        }

        // Check if any model has rate limits
        for model in provider.models().values() {
            if model.rate_limits().is_some() {
                return true;
            }

            // Check if model has group-specific rate limits
            if let Some(limits) = model.rate_limits()
                && let Some(per_user) = &limits.per_user
                && !per_user.groups.is_empty()
            {
                return true;
            }
        }
    }

    false
}

/// Check if any provider or model has group-based rate limits.
fn check_if_any_group_rate_limits_exist(config: &Config) -> bool {
    for provider in config.llm.providers.values() {
        // Check provider-level group rate limits
        if let Some(limits) = provider.rate_limits()
            && let Some(per_user) = &limits.per_user
            && !per_user.groups.is_empty()
        {
            return true;
        }

        // Check model-level group rate limits
        for model in provider.models().values() {
            if let Some(limits) = model.rate_limits()
                && let Some(per_user) = &limits.per_user
                && !per_user.groups.is_empty()
            {
                return true;
            }
        }
    }

    false
}

fn check_group_fallbacks(group: &str, provider_name: &str, provider: &LlmProviderConfig, warnings: &mut Vec<String>) {
    // Check each model's fallback situation
    for (model_name, model) in provider.models() {
        // Check if this model has a specific rate limit for this group
        let has_model_group = model_has_group_limit(&model, group);

        if has_model_group {
            continue;
        }

        // Model doesn't have a group-specific limit, check fallbacks
        let has_model_default = model.rate_limits().is_some();
        let has_provider_group = provider_has_group_limit(provider, group);
        let has_provider_default = provider.rate_limits().is_some();

        let warning = match (has_model_default, has_provider_group, has_provider_default) {
            (true, _, _) => {
                format!("Group '{group}' for model '{provider_name}/{model_name}' will use model default rate limit")
            }
            (false, true, _) => {
                format!("Group '{group}' for model '{provider_name}/{model_name}' will use provider group rate limit")
            }
            (false, false, true) => {
                format!(
                    "Group '{group}' for model '{provider_name}/{model_name}' will fall back to provider default rate limit"
                )
            }
            (false, false, false) => {
                format!("Group '{group}' for model '{provider_name}/{model_name}' has no rate limit configured")
            }
        };

        warnings.push(warning);
    }

    // Check if group has no specific limits at all for this provider
    let has_provider_limit = provider_has_group_limit(provider, group);
    let has_any_model_limit = provider_has_any_model_with_group_limit(provider, group);

    if !has_provider_limit && !has_any_model_limit && provider.rate_limits().is_none() {
        let warning = format!("Group '{group}' has no rate limits configured for provider '{provider_name}'");
        warnings.push(warning);
    }
}

/// Check if a model has a rate limit for a specific group.
fn model_has_group_limit(model: &crate::ModelConfig, group: &str) -> bool {
    model
        .rate_limits()
        .and_then(|limits| limits.per_user.as_ref())
        .map(|per_user| per_user.groups.contains_key(group))
        .unwrap_or(false)
}

/// Check if a provider has a rate limit for a specific group.
fn provider_has_group_limit(provider: &LlmProviderConfig, group: &str) -> bool {
    provider
        .rate_limits()
        .as_ref()
        .and_then(|limits| limits.per_user.as_ref())
        .map(|per_user| per_user.groups.contains_key(group))
        .unwrap_or(false)
}

/// Check if any model in a provider has a rate limit for a specific group.
fn provider_has_any_model_with_group_limit(provider: &LlmProviderConfig, group: &str) -> bool {
    provider
        .models()
        .values()
        .any(|model| model_has_group_limit(model, group))
}

#[cfg(test)]
mod tests {
    use indoc::indoc;
    use insta::assert_debug_snapshot;
    use insta::assert_snapshot;

    use crate::Config;

    #[test]
    fn validation_logic_identifies_no_downstreams() {
        // Test that validation logic correctly identifies when no downstreams are configured
        let config = Config::default();
        let result = super::validate_has_downstreams(&config);
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();

        assert_snapshot!(error_msg, @r#"
        No downstream servers configured. Nexus requires at least one MCP server or LLM provider to function.

        Example configuration:

        For MCP servers:

          [mcp.servers.example]
          cmd = ["path/to/mcp-server"]

        For LLM providers:

          [llm.providers.openai]
          type = "openai"
          api_key = "{{ env.OPENAI_API_KEY }}"

        See https://nexusrouter.com/docs for more configuration examples.
        "#);
    }

    #[test]
    fn validation_fails_when_both_disabled() {
        let config_str = indoc! {r#"
            [mcp]
            enabled = false

            [llm]
            enabled = false
        "#};

        let config: Config = toml::from_str(config_str).unwrap();
        let result = super::validate_has_downstreams(&config);
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();

        assert_snapshot!(error_msg, @r#"
        No downstream servers configured. Nexus requires at least one MCP server or LLM provider to function.

        Example configuration:

        For MCP servers:

          [mcp.servers.example]
          cmd = ["path/to/mcp-server"]

        For LLM providers:

          [llm.providers.openai]
          type = "openai"
          api_key = "{{ env.OPENAI_API_KEY }}"

        See https://nexusrouter.com/docs for more configuration examples.
        "#);
    }

    #[test]
    fn validation_fails_when_mcp_enabled_but_no_servers() {
        let config_str = indoc! {r#"
            [mcp]
            enabled = true

            [llm]
            enabled = false
        "#};

        let config: Config = toml::from_str(config_str).unwrap();
        let result = super::validate_has_downstreams(&config);
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();

        assert_snapshot!(error_msg, @r#"
        No downstream servers configured. Nexus requires at least one MCP server or LLM provider to function.

        Example configuration:

        For MCP servers:

          [mcp.servers.example]
          cmd = ["path/to/mcp-server"]

        For LLM providers:

          [llm.providers.openai]
          type = "openai"
          api_key = "{{ env.OPENAI_API_KEY }}"

        See https://nexusrouter.com/docs for more configuration examples.
        "#);
    }

    #[test]
    fn validation_fails_when_llm_enabled_but_no_providers() {
        let config_str = indoc! {r#"
            [mcp]
            enabled = false

            [llm]
            enabled = true
        "#};

        let config: Config = toml::from_str(config_str).unwrap();
        let result = super::validate_has_downstreams(&config);
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();

        assert_snapshot!(error_msg, @r#"
        No downstream servers configured. Nexus requires at least one MCP server or LLM provider to function.

        Example configuration:

        For MCP servers:

          [mcp.servers.example]
          cmd = ["path/to/mcp-server"]

        For LLM providers:

          [llm.providers.openai]
          type = "openai"
          api_key = "{{ env.OPENAI_API_KEY }}"

        See https://nexusrouter.com/docs for more configuration examples.
        "#);
    }

    #[test]
    fn validation_passes_with_mcp_server() {
        let config_str = indoc! {r#"
            [mcp.servers.test]
            cmd = ["echo", "test"]
        "#};

        let config: Config = toml::from_str(config_str).unwrap();
        let result = super::validate_has_downstreams(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn validation_passes_with_llm_provider() {
        let config_str = indoc! {r#"
            [llm.protocols.openai]
            enabled = true
            path = "/llm"

            [llm.providers.openai]
            type = "openai"
            api_key = "test-key"

            [llm.providers.openai.models.gpt-4]
        "#};

        let config: Config = toml::from_str(config_str).unwrap();
        let result = super::validate_has_downstreams(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn validation_passes_with_both_mcp_and_llm() {
        let config_str = indoc! {r#"
            [mcp.servers.test]
            cmd = ["echo", "test"]

            [llm.protocols.openai]
            enabled = true
            path = "/llm"

            [llm.providers.openai]
            type = "openai"
            api_key = "test-key"

            [llm.providers.openai.models.gpt-4]
        "#};

        let config: Config = toml::from_str(config_str).unwrap();
        let result = super::validate_has_downstreams(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn validation_passes_when_mcp_disabled_but_llm_has_providers() {
        let config_str = indoc! {r#"
            [mcp]
            enabled = false

            [llm.protocols.openai]
            enabled = true
            path = "/llm"

            [llm.providers.openai]
            type = "openai"
            api_key = "test-key"

            [llm.providers.openai.models.gpt-4]
        "#};

        let config: Config = toml::from_str(config_str).unwrap();
        let result = super::validate_has_downstreams(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn validation_passes_when_llm_disabled_but_mcp_has_servers() {
        let config_str = indoc! {r#"
            [llm]
            enabled = false

            [mcp.servers.test]
            cmd = ["echo", "test"]
        "#};

        let config: Config = toml::from_str(config_str).unwrap();
        let result = super::validate_has_downstreams(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn validation_fails_when_llm_has_providers_but_no_endpoints() {
        let config_str = indoc! {r#"
            [llm.protocols.openai]
            enabled = false

            [llm.protocols.anthropic]
            enabled = false

            [llm.providers.openai]
            type = "openai"
            api_key = "test-key"

            [llm.providers.openai.models.gpt-4]
        "#};

        let config: Config = toml::from_str(config_str).unwrap();
        let result = super::validate_has_downstreams(&config);
        assert!(result.is_err());
        let error = result.unwrap_err().to_string();
        assert!(error.contains("no protocol endpoints are enabled"));
    }

    #[test]
    fn validation_fails_with_duplicate_endpoint_paths() {
        let config_str = indoc! {r#"
            [llm.protocols.openai]
            enabled = true
            path = "/llm"

            [llm.protocols.anthropic]
            enabled = true
            path = "/llm"

            [llm.providers.openai]
            type = "openai"
            api_key = "test-key"

            [llm.providers.openai.models.gpt-4]
        "#};

        let config: Config = toml::from_str(config_str).unwrap();
        let result = super::validate_has_downstreams(&config);
        assert!(result.is_err());
        let error = result.unwrap_err().to_string();
        assert!(error.contains("Duplicate LLM protocol endpoint path"));
    }

    #[test]
    fn rate_limit_validation_with_groups() {
        let config = indoc! {r#"
            [server.client_identification]
            enabled = true
            client_id.jwt_claim = "sub"
            group_id.jwt_claim = "plan"

            [server.client_identification.validation]
            group_values = ["free", "pro"]

            [llm.protocols.openai]
            enabled = true
            path = "/llm"

            [llm.providers.openai]
            type = "openai"
            api_key = "test-key"

            [llm.providers.openai.rate_limits.per_user]
            input_token_limit = 50000
            interval = "60s"

            [llm.providers.openai.rate_limits.per_user.groups]
            free = { input_token_limit = 10000, interval = "60s" }
            pro = { input_token_limit = 100000, interval = "60s" }

            [llm.providers.openai.models.gpt-4]
        "#};

        let config: Config = toml::from_str(config).unwrap();
        let warnings = super::validate_rate_limits(&config).unwrap();

        // Should have warnings about model fallbacks
        assert_debug_snapshot!(warnings, @r#"
        [
            "Group 'free' for model 'openai/gpt-4' will use provider group rate limit",
            "Group 'pro' for model 'openai/gpt-4' will use provider group rate limit",
        ]
        "#);
    }

    #[test]
    fn rate_limits_without_client_identification_fails() {
        let config = indoc! {r#"
            [server.client_identification]
            enabled = false

            [llm.protocols.openai]
            enabled = true
            path = "/llm"

            [llm.providers.openai]
            type = "openai"
            api_key = "test-key"

            [llm.providers.openai.rate_limits.per_user]
            input_token_limit = 10000
            interval = "60s"

            [llm.providers.openai.models.gpt-4]
        "#};

        let config: Config = toml::from_str(config).unwrap();
        let result = super::validate_rate_limits(&config);

        assert!(result.is_err());
        let error = result.unwrap_err().to_string();

        assert_snapshot!(error, @"LLM rate limits are configured but client identification is not enabled. Enable client identification in [server.client_identification]");
    }

    #[test]
    fn model_rate_limits_without_client_identification_fails() {
        let config = indoc! {r#"
            [server.client_identification]
            enabled = false

            [llm.protocols.openai]
            enabled = true
            path = "/llm"

            [llm.providers.openai]
            type = "openai"
            api_key = "test-key"

            [llm.providers.openai.models.gpt-4.rate_limits.per_user]
            input_token_limit = 5000
            interval = "60s"
        "#};

        let config: Config = toml::from_str(config).unwrap();
        let result = super::validate_rate_limits(&config);

        assert!(result.is_err());
        let error = result.unwrap_err().to_string();
        assert_snapshot!(error, @"LLM rate limits are configured but client identification is not enabled. Enable client identification in [server.client_identification]");
    }

    #[test]
    fn group_id_without_allowed_groups_fails() {
        let config = indoc! {r#"
            [server.client_identification]
            enabled = true
            client_id.jwt_claim = "sub"
            group_id.jwt_claim = "plan"

            [llm.protocols.openai]
            enabled = true
            path = "/llm"

            [llm.providers.openai]
            type = "openai"
            api_key = "test-key"

            [llm.providers.openai.rate_limits.per_user]
            input_token_limit = 5000
            interval = "60s"

            [llm.providers.openai.models.gpt-4]
        "#};

        let config: Config = toml::from_str(config).unwrap();
        let result = super::validate_rate_limits(&config);

        assert!(result.is_err());
        let error = result.unwrap_err().to_string();

        assert_snapshot!(error, @"group_id is configured for client identification but validation.group_values is empty. Define group_values in [server.client_identification.validation]");
    }

    #[test]
    fn group_rate_limits_without_group_id_fails() {
        let config = indoc! {r#"
            [server.client_identification]
            enabled = true
            client_id.jwt_claim = "sub"

            [llm.protocols.openai]
            enabled = true
            path = "/llm"

            [llm.providers.openai]
            type = "openai"
            api_key = "test-key"

            [llm.providers.openai.rate_limits.per_user]
            input_token_limit = 5000
            interval = "60s"

            [llm.providers.openai.rate_limits.per_user.groups]
            free = { input_token_limit = 10000, interval = "60s" }

            [llm.providers.openai.models.gpt-4]
        "#};

        let config: Config = toml::from_str(config).unwrap();
        let result = super::validate_rate_limits(&config);

        assert!(result.is_err());
        let error = result.unwrap_err().to_string();

        assert_snapshot!(error, @r#"
        Group-based rate limits are configured but group_id is not set in client identification.
        To fix this, add a group_id configuration to your [server.client_identification] section, for example:

        [server.client_identification]
        enabled = true
        client_id.http_header = "X-Client-ID"      # or client_id.jwt_claim = "sub"
        group_id.http_header = "X-Group-ID"        # or group_id.jwt_claim = "groups"

        [server.client_identification.validation]
        group_values = ["basic", "premium", "enterprise"]
        "#);
    }

    #[test]
    fn rate_limit_validation_invalid_group() {
        let config = indoc! {r#"
            [server.client_identification]
            enabled = true
            client_id.jwt_claim = "sub"
            group_id.jwt_claim = "plan"

            [server.client_identification.validation]
            group_values = ["free", "pro"]

            [llm.protocols.openai]
            enabled = true
            path = "/llm"

            [llm.providers.openai]
            type = "openai"
            api_key = "test-key"

            [llm.providers.openai.rate_limits.per_user]
            input_token_limit = 50000
            interval = "60s"

            [llm.providers.openai.rate_limits.per_user.groups]
            enterprise = { input_token_limit = 1000000, interval = "60s" }

            [llm.providers.openai.models.gpt-4]
        "#};

        let config: Config = toml::from_str(config).unwrap();
        let result = super::validate_rate_limits(&config);

        assert!(result.is_err());
        let error = result.unwrap_err().to_string();

        assert_snapshot!(error, @"Group 'enterprise' in provider 'openai' rate limits not found in group_values");
    }
}
