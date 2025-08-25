//! LLM configuration structures for AI model providers.

use std::borrow::Cow;
use std::collections::BTreeMap;

use crate::headers::HeaderRule;
use crate::rate_limit::TokenRateLimitsConfig;
use secrecy::SecretString;
use serde::{Deserialize, Deserializer};

/// Configuration for an individual model within API-based providers.
#[derive(Debug, Clone, Deserialize)]
pub struct ApiModelConfig {
    /// Optional rename - the actual provider model name.
    /// If not specified, the model ID (map key) is used.
    #[serde(default)]
    pub rename: Option<String>,
    /// Rate limits for this model.
    #[serde(default)]
    pub rate_limits: Option<TokenRateLimitsConfig>,
    /// Header transformation rules for this model.
    #[serde(default)]
    pub headers: Vec<HeaderRule>,
}

/// Configuration for an individual model within Bedrock provider.
/// Note: Bedrock models don't support custom headers due to SigV4 signing.
#[derive(Debug, Clone, Deserialize)]
pub struct BedrockModelConfig {
    /// Optional rename - the actual provider model name.
    /// If not specified, the model ID (map key) is used.
    #[serde(default)]
    pub rename: Option<String>,
    /// Rate limits for this model.
    #[serde(default)]
    pub rate_limits: Option<TokenRateLimitsConfig>,
    // No headers field - Bedrock uses SigV4 signing
}

/// Unified model configuration that can be either API or Bedrock.
#[derive(Debug, Clone)]
pub enum ModelConfig {
    /// API-based model configuration (OpenAI, Anthropic, Google).
    Api(ApiModelConfig),
    /// Bedrock model configuration.
    Bedrock(BedrockModelConfig),
}

impl ModelConfig {
    /// Get the optional rename for this model.
    pub fn rename(&self) -> Option<&str> {
        match self {
            Self::Api(config) => config.rename.as_deref(),
            Self::Bedrock(config) => config.rename.as_deref(),
        }
    }

    /// Get the rate limits for this model.
    pub fn rate_limits(&self) -> Option<&TokenRateLimitsConfig> {
        match self {
            Self::Api(config) => config.rate_limits.as_ref(),
            Self::Bedrock(config) => config.rate_limits.as_ref(),
        }
    }

    /// Get the headers for this model (only available for API models).
    pub fn headers(&self) -> &[HeaderRule] {
        match self {
            Self::Api(config) => &config.headers,
            Self::Bedrock(_) => &[], // Bedrock doesn't support headers
        }
    }
}

/// LLM configuration for AI model integration.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LlmConfig {
    /// Whether the LLM functionality is enabled.
    enabled: bool,

    /// The path where the LLM endpoints will be mounted.
    pub path: Cow<'static, str>,

    /// Map of LLM provider configurations.
    pub providers: BTreeMap<String, LlmProviderConfig>,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            path: Cow::Borrowed("/llm"),
            providers: BTreeMap::new(),
        }
    }
}

impl LlmConfig {
    /// Whether the LLM functionality is enabled.
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Whether there are any LLM providers configured.
    pub fn has_providers(&self) -> bool {
        !self.providers.is_empty()
    }
}

/// Provider type enumeration.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderType {
    /// OpenAI provider.
    Openai,
    /// Anthropic provider.
    Anthropic,
    /// Google provider.
    Google,
    /// AWS Bedrock provider.
    Bedrock,
}

/// Configuration specific to API-based providers.
#[derive(Debug, Clone, Deserialize)]
pub struct ApiProviderConfig {
    /// API key for authentication.
    #[serde(default)]
    pub api_key: Option<SecretString>,

    /// Custom base URL for the provider API.
    #[serde(default)]
    pub base_url: Option<String>,

    /// Enable token forwarding from user requests.
    #[serde(default)]
    pub forward_token: bool,

    /// Explicitly configured models for this provider.
    /// Phase 3: At least one model must be configured.
    #[serde(deserialize_with = "deserialize_non_empty_api_models_with_default")]
    pub models: BTreeMap<String, ApiModelConfig>,

    /// Provider-level rate limits.
    #[serde(default)]
    pub rate_limits: Option<TokenRateLimitsConfig>,

    /// Header transformation rules for this provider.
    #[serde(default)]
    pub headers: Vec<HeaderRule>,
}

/// Configuration specific to AWS Bedrock.
#[derive(Debug, Clone, Deserialize)]
pub struct BedrockProviderConfig {
    /// AWS Access Key ID (optional - uses credential chain if not provided).
    #[serde(default)]
    pub access_key_id: Option<SecretString>,

    /// AWS Secret Access Key (required if access_key_id is provided).
    #[serde(default)]
    pub secret_access_key: Option<SecretString>,

    /// AWS Session Token (optional - for temporary credentials).
    #[serde(default)]
    pub session_token: Option<SecretString>,

    /// AWS Profile name (optional - uses default profile if not specified).
    #[serde(default)]
    pub profile: Option<String>,

    /// AWS region (required for Bedrock).
    pub region: String,

    /// Custom endpoint URL (optional - for VPC endpoints).
    #[serde(default)]
    pub base_url: Option<String>,

    /// Explicitly configured models for this provider.
    /// Bedrock models don't support custom headers due to SigV4 signing.
    #[serde(deserialize_with = "deserialize_non_empty_bedrock_models_with_default")]
    pub models: BTreeMap<String, BedrockModelConfig>,
}

/// Complete LLM provider configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum LlmProviderConfig {
    /// OpenAI provider configuration.
    Openai(ApiProviderConfig),

    /// Anthropic provider configuration.
    Anthropic(ApiProviderConfig),

    /// Google provider configuration.
    Google(ApiProviderConfig),

    /// AWS Bedrock provider configuration.
    Bedrock(BedrockProviderConfig),
}

impl LlmProviderConfig {
    /// Get the provider type for this configuration.
    pub fn provider_type(&self) -> ProviderType {
        match self {
            Self::Openai(_) => ProviderType::Openai,
            Self::Anthropic(_) => ProviderType::Anthropic,
            Self::Google(_) => ProviderType::Google,
            Self::Bedrock(_) => ProviderType::Bedrock,
        }
    }

    /// Get the API key (only available for API-based providers).
    pub fn api_key(&self) -> Option<&SecretString> {
        match self {
            Self::Openai(config) => config.api_key.as_ref(),
            Self::Anthropic(config) => config.api_key.as_ref(),
            Self::Google(config) => config.api_key.as_ref(),
            Self::Bedrock(_) => None, // Bedrock doesn't use API keys
        }
    }

    /// Get the base URL (if applicable for this provider type).
    pub fn base_url(&self) -> Option<&str> {
        match self {
            Self::Openai(config) => config.base_url.as_deref(),
            Self::Anthropic(config) => config.base_url.as_deref(),
            Self::Google(config) => config.base_url.as_deref(),
            Self::Bedrock(config) => config.base_url.as_deref(),
        }
    }

    /// Check if token forwarding is enabled (only applicable for API-based providers).
    pub fn forward_token(&self) -> bool {
        match self {
            Self::Openai(config) => config.forward_token,
            Self::Anthropic(config) => config.forward_token,
            Self::Google(config) => config.forward_token,
            Self::Bedrock(_) => false, // Bedrock doesn't support token forwarding
        }
    }

    /// Get the configured models for this provider as unified ModelConfig.
    pub fn models(&self) -> BTreeMap<String, ModelConfig> {
        match self {
            Self::Openai(config) => config
                .models
                .iter()
                .map(|(k, v)| (k.clone(), ModelConfig::Api(v.clone())))
                .collect(),
            Self::Anthropic(config) => config
                .models
                .iter()
                .map(|(k, v)| (k.clone(), ModelConfig::Api(v.clone())))
                .collect(),
            Self::Google(config) => config
                .models
                .iter()
                .map(|(k, v)| (k.clone(), ModelConfig::Api(v.clone())))
                .collect(),
            Self::Bedrock(config) => config
                .models
                .iter()
                .map(|(k, v)| (k.clone(), ModelConfig::Bedrock(v.clone())))
                .collect(),
        }
    }

    /// Get the rate limits for this provider (only available for API-based providers).
    pub fn rate_limits(&self) -> Option<&TokenRateLimitsConfig> {
        match self {
            Self::Openai(config) => config.rate_limits.as_ref(),
            Self::Anthropic(config) => config.rate_limits.as_ref(),
            Self::Google(config) => config.rate_limits.as_ref(),
            Self::Bedrock(_) => None, // Bedrock doesn't support rate limits yet
        }
    }
}

/// Custom deserializer for API models that ensures at least one model is configured.
/// This handles both missing field (uses default) and empty map cases.
fn deserialize_non_empty_api_models_with_default<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<String, ApiModelConfig>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;

    // First deserialize as Option to handle missing field
    let models_opt = Option::<BTreeMap<String, ApiModelConfig>>::deserialize(deserializer)?;

    // Get the models map, using empty map if field was missing
    let models = models_opt.unwrap_or_default();

    // Now validate that we have at least one model
    if models.is_empty() {
        Err(Error::custom("At least one model must be configured for each provider"))
    } else {
        Ok(models)
    }
}

/// Custom deserializer for Bedrock models that ensures at least one model is configured.
/// This handles both missing field (uses default) and empty map cases.
fn deserialize_non_empty_bedrock_models_with_default<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<String, BedrockModelConfig>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;

    // First deserialize as Option to handle missing field
    let models_opt = Option::<BTreeMap<String, BedrockModelConfig>>::deserialize(deserializer)?;

    // Get the models map, using empty map if field was missing
    let models = models_opt.unwrap_or_default();

    // Now validate that we have at least one model
    if models.is_empty() {
        Err(Error::custom("At least one model must be configured for each provider"))
    } else {
        Ok(models)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use insta::assert_debug_snapshot;

    #[test]
    fn llm_config_defaults() {
        let config: LlmConfig = toml::from_str("").unwrap();

        assert_debug_snapshot!(&config, @r#"
        LlmConfig {
            enabled: true,
            path: "/llm",
            providers: {},
        }
        "#);
    }

    #[test]
    fn llm_config_with_openai() {
        let config = indoc! {r#"
            enabled = true
            path = "/llm"

            [providers.openai]
            type = "openai"
            api_key = "${OPENAI_API_KEY}"
            
            [providers.openai.models.gpt-4]
            
            [providers.openai.models.gpt-3-5-turbo]
        "#};

        let config: LlmConfig = toml::from_str(config).unwrap();

        assert_debug_snapshot!(&config, @r#"
        LlmConfig {
            enabled: true,
            path: "/llm",
            providers: {
                "openai": Openai(
                    ApiProviderConfig {
                        api_key: Some(
                            SecretBox<str>([REDACTED]),
                        ),
                        base_url: None,
                        forward_token: false,
                        models: {
                            "gpt-3-5-turbo": ApiModelConfig {
                                rename: None,
                                rate_limits: None,
                                headers: [],
                            },
                            "gpt-4": ApiModelConfig {
                                rename: None,
                                rate_limits: None,
                                headers: [],
                            },
                        },
                        rate_limits: None,
                        headers: [],
                    },
                ),
            },
        }
        "#);
    }

    #[test]
    fn llm_config_with_anthropic() {
        let config = indoc! {r#"
            enabled = true
            path = "/llm"

            [providers.anthropic]
            type = "anthropic"
            api_key = "{{ env.ANTHROPIC_API_KEY }}"
            
            [providers.anthropic.models.claude-3-opus]
            
            [providers.anthropic.models.claude-3-sonnet]
        "#};

        let config: LlmConfig = toml::from_str(config).unwrap();

        assert_debug_snapshot!(&config, @r#"
        LlmConfig {
            enabled: true,
            path: "/llm",
            providers: {
                "anthropic": Anthropic(
                    ApiProviderConfig {
                        api_key: Some(
                            SecretBox<str>([REDACTED]),
                        ),
                        base_url: None,
                        forward_token: false,
                        models: {
                            "claude-3-opus": ApiModelConfig {
                                rename: None,
                                rate_limits: None,
                                headers: [],
                            },
                            "claude-3-sonnet": ApiModelConfig {
                                rename: None,
                                rate_limits: None,
                                headers: [],
                            },
                        },
                        rate_limits: None,
                        headers: [],
                    },
                ),
            },
        }
        "#);
    }

    #[test]
    fn llm_config_with_google() {
        let config = indoc! {r#"
            [providers.google]
            type = "google"
            api_key = "{{ env.GOOGLE_KEY }}"
            
            [providers.google.models.gemini-pro]
            
            [providers.google.models.gemini-pro-vision]
        "#};

        let config: LlmConfig = toml::from_str(config).unwrap();

        assert_debug_snapshot!(&config, @r#"
        LlmConfig {
            enabled: true,
            path: "/llm",
            providers: {
                "google": Google(
                    ApiProviderConfig {
                        api_key: Some(
                            SecretBox<str>([REDACTED]),
                        ),
                        base_url: None,
                        forward_token: false,
                        models: {
                            "gemini-pro": ApiModelConfig {
                                rename: None,
                                rate_limits: None,
                                headers: [],
                            },
                            "gemini-pro-vision": ApiModelConfig {
                                rename: None,
                                rate_limits: None,
                                headers: [],
                            },
                        },
                        rate_limits: None,
                        headers: [],
                    },
                ),
            },
        }
        "#);
    }

    #[test]
    fn llm_config_with_multiple_providers() {
        let config = indoc! {r#"
            enabled = true
            path = "/ai"

            [providers.openai]
            type = "openai"
            api_key = "${OPENAI_API_KEY}"
            
            [providers.openai.models.gpt-4]

            [providers.anthropic]
            type = "anthropic"
            api_key = "{{ env.ANTHROPIC_API_KEY }}"
            
            [providers.anthropic.models.claude-3-opus]

            [providers.google]
            type = "google"
            api_key = "{{ env.GOOGLE_KEY }}"
            
            [providers.google.models.gemini-pro]
        "#};

        let config: LlmConfig = toml::from_str(config).unwrap();

        assert_debug_snapshot!(&config, @r#"
        LlmConfig {
            enabled: true,
            path: "/ai",
            providers: {
                "anthropic": Anthropic(
                    ApiProviderConfig {
                        api_key: Some(
                            SecretBox<str>([REDACTED]),
                        ),
                        base_url: None,
                        forward_token: false,
                        models: {
                            "claude-3-opus": ApiModelConfig {
                                rename: None,
                                rate_limits: None,
                                headers: [],
                            },
                        },
                        rate_limits: None,
                        headers: [],
                    },
                ),
                "google": Google(
                    ApiProviderConfig {
                        api_key: Some(
                            SecretBox<str>([REDACTED]),
                        ),
                        base_url: None,
                        forward_token: false,
                        models: {
                            "gemini-pro": ApiModelConfig {
                                rename: None,
                                rate_limits: None,
                                headers: [],
                            },
                        },
                        rate_limits: None,
                        headers: [],
                    },
                ),
                "openai": Openai(
                    ApiProviderConfig {
                        api_key: Some(
                            SecretBox<str>([REDACTED]),
                        ),
                        base_url: None,
                        forward_token: false,
                        models: {
                            "gpt-4": ApiModelConfig {
                                rename: None,
                                rate_limits: None,
                                headers: [],
                            },
                        },
                        rate_limits: None,
                        headers: [],
                    },
                ),
            },
        }
        "#);
    }

    #[test]
    fn llm_config_disabled() {
        let config = indoc! {r#"
            enabled = false
        "#};

        let config: LlmConfig = toml::from_str(config).unwrap();

        assert_debug_snapshot!(&config, @r#"
        LlmConfig {
            enabled: false,
            path: "/llm",
            providers: {},
        }
        "#);
    }

    #[test]
    fn llm_config_custom_path() {
        let config = indoc! {r#"
            path = "/models"
        "#};

        let config: LlmConfig = toml::from_str(config).unwrap();

        assert_debug_snapshot!(&config, @r#"
        LlmConfig {
            enabled: true,
            path: "/models",
            providers: {},
        }
        "#);
    }

    #[test]
    fn llm_config_invalid_provider_type() {
        let config = indoc! {r#"
            [providers.invalid]
            type = "unknown-provider"
            api_key = "key"
        "#};

        let result: Result<LlmConfig, _> = toml::from_str(config);
        assert!(result.is_err());

        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("unknown variant"));
    }

    #[test]
    fn llm_config_with_static_api_key() {
        let config = indoc! {r#"
            [providers.openai]
            type = "openai"
            api_key = "sk-1234567890abcdef"
            
            [providers.openai.models.gpt-4]
        "#};

        let config: LlmConfig = toml::from_str(config).unwrap();

        assert_debug_snapshot!(&config, @r#"
        LlmConfig {
            enabled: true,
            path: "/llm",
            providers: {
                "openai": Openai(
                    ApiProviderConfig {
                        api_key: Some(
                            SecretBox<str>([REDACTED]),
                        ),
                        base_url: None,
                        forward_token: false,
                        models: {
                            "gpt-4": ApiModelConfig {
                                rename: None,
                                rate_limits: None,
                                headers: [],
                            },
                        },
                        rate_limits: None,
                        headers: [],
                    },
                ),
            },
        }
        "#);
    }

    #[test]
    fn llm_config_with_explicit_models() {
        let config = indoc! {r#"
            [providers.openai]
            type = "openai"
            api_key = "key"
            
            [providers.openai.models.gpt-4]
            rename = "gpt-4-turbo-preview"
            
            [providers.openai.models.gpt-3-5]
            rename = "gpt-3.5-turbo"
        "#};

        let config: LlmConfig = toml::from_str(config).unwrap();

        assert_debug_snapshot!(&config, @r#"
        LlmConfig {
            enabled: true,
            path: "/llm",
            providers: {
                "openai": Openai(
                    ApiProviderConfig {
                        api_key: Some(
                            SecretBox<str>([REDACTED]),
                        ),
                        base_url: None,
                        forward_token: false,
                        models: {
                            "gpt-3-5": ApiModelConfig {
                                rename: Some(
                                    "gpt-3.5-turbo",
                                ),
                                rate_limits: None,
                                headers: [],
                            },
                            "gpt-4": ApiModelConfig {
                                rename: Some(
                                    "gpt-4-turbo-preview",
                                ),
                                rate_limits: None,
                                headers: [],
                            },
                        },
                        rate_limits: None,
                        headers: [],
                    },
                ),
            },
        }
        "#);
    }

    #[test]
    fn llm_config_models_without_rename() {
        let config = indoc! {r#"
            [providers.openai]
            type = "openai"
            api_key = "key"
            
            [providers.openai.models.gpt-4]
            # No rename - will use "gpt-4" as-is
            
            [providers.openai.models.custom-model]
            # No fields at all
        "#};

        let config: LlmConfig = toml::from_str(config).unwrap();

        assert_debug_snapshot!(&config, @r#"
        LlmConfig {
            enabled: true,
            path: "/llm",
            providers: {
                "openai": Openai(
                    ApiProviderConfig {
                        api_key: Some(
                            SecretBox<str>([REDACTED]),
                        ),
                        base_url: None,
                        forward_token: false,
                        models: {
                            "custom-model": ApiModelConfig {
                                rename: None,
                                rate_limits: None,
                                headers: [],
                            },
                            "gpt-4": ApiModelConfig {
                                rename: None,
                                rate_limits: None,
                                headers: [],
                            },
                        },
                        rate_limits: None,
                        headers: [],
                    },
                ),
            },
        }
        "#);
    }

    #[test]
    fn llm_config_mixed_providers_with_models() {
        let config = indoc! {r#"
            [providers.openai]
            type = "openai"
            api_key = "key1"
            
            [providers.openai.models.gpt-4]
            rename = "gpt-4-turbo"
            
            [providers.anthropic]
            type = "anthropic"
            api_key = "key2"
            
            [providers.anthropic.models.claude-3]
            rename = "claude-3-opus-20240229"
            
            [providers.anthropic.models.claude-instant]
            # No rename
        "#};

        let config: LlmConfig = toml::from_str(config).unwrap();

        assert_debug_snapshot!(&config, @r#"
        LlmConfig {
            enabled: true,
            path: "/llm",
            providers: {
                "anthropic": Anthropic(
                    ApiProviderConfig {
                        api_key: Some(
                            SecretBox<str>([REDACTED]),
                        ),
                        base_url: None,
                        forward_token: false,
                        models: {
                            "claude-3": ApiModelConfig {
                                rename: Some(
                                    "claude-3-opus-20240229",
                                ),
                                rate_limits: None,
                                headers: [],
                            },
                            "claude-instant": ApiModelConfig {
                                rename: None,
                                rate_limits: None,
                                headers: [],
                            },
                        },
                        rate_limits: None,
                        headers: [],
                    },
                ),
                "openai": Openai(
                    ApiProviderConfig {
                        api_key: Some(
                            SecretBox<str>([REDACTED]),
                        ),
                        base_url: None,
                        forward_token: false,
                        models: {
                            "gpt-4": ApiModelConfig {
                                rename: Some(
                                    "gpt-4-turbo",
                                ),
                                rate_limits: None,
                                headers: [],
                            },
                        },
                        rate_limits: None,
                        headers: [],
                    },
                ),
            },
        }
        "#);
    }

    #[test]
    fn provider_rate_limits() {
        let config = indoc! {r#"
            [providers.openai]
            type = "openai"
            api_key = "test-key"
            
            [providers.openai.rate_limits.per_user]
            input_token_limit = 100000
            interval = "60s"
            
            [providers.openai.rate_limits.per_user.groups]
            free = { input_token_limit = 10000, interval = "60s" }
            pro = { input_token_limit = 100000, interval = "60s" }
            
            [providers.openai.models.gpt-4]
        "#};

        let config: LlmConfig = toml::from_str(config).unwrap();

        assert_debug_snapshot!(&config.providers["openai"].rate_limits(), @r#"
        Some(
            TokenRateLimitsConfig {
                per_user: Some(
                    PerUserRateLimits {
                        input_token_limit: 100000,
                        interval: 60s,
                        groups: {
                            "free": TokenRateLimit {
                                input_token_limit: 10000,
                                interval: 60s,
                            },
                            "pro": TokenRateLimit {
                                input_token_limit: 100000,
                                interval: 60s,
                            },
                        },
                    },
                ),
            },
        )
        "#);
    }

    #[test]
    fn model_rate_limits() {
        let config = indoc! {r#"
            [providers.openai]
            type = "openai"
            api_key = "test-key"
            
            [providers.openai.models.gpt-4.rate_limits.per_user]
            input_token_limit = 50000
            interval = "60s"
            
            [providers.openai.models.gpt-4.rate_limits.per_user.groups]
            free = { input_token_limit = 5000, interval = "60s" }
            pro = { input_token_limit = 50000, interval = "60s" }
        "#};

        let config: LlmConfig = toml::from_str(config).unwrap();

        assert_debug_snapshot!(&config.providers["openai"].models().get("gpt-4").unwrap().rate_limits(), @r#"
        Some(
            TokenRateLimitsConfig {
                per_user: Some(
                    PerUserRateLimits {
                        input_token_limit: 50000,
                        interval: 60s,
                        groups: {
                            "free": TokenRateLimit {
                                input_token_limit: 5000,
                                interval: 60s,
                            },
                            "pro": TokenRateLimit {
                                input_token_limit: 50000,
                                interval: 60s,
                            },
                        },
                    },
                ),
            },
        )
        "#);
    }

    #[test]
    fn llm_config_with_forward_token_enabled() {
        let config = indoc! {r#"
            [providers.openai]
            type = "openai"
            api_key = "sk-fallback-key"
            forward_token = true
            
            [providers.openai.models.gpt-4]

            [providers.anthropic]
            type = "anthropic"
            forward_token = true
            # No api_key provided - relies entirely on token forwarding
            
            [providers.anthropic.models.claude-3-opus]

            [providers.google]
            type = "google"
            api_key = "{{ env.GOOGLE_KEY }}"
            forward_token = false  # Explicitly disabled
            
            [providers.google.models.gemini-pro]
        "#};

        let config: LlmConfig = toml::from_str(config).unwrap();

        assert_debug_snapshot!(&config, @r#"
        LlmConfig {
            enabled: true,
            path: "/llm",
            providers: {
                "anthropic": Anthropic(
                    ApiProviderConfig {
                        api_key: None,
                        base_url: None,
                        forward_token: true,
                        models: {
                            "claude-3-opus": ApiModelConfig {
                                rename: None,
                                rate_limits: None,
                                headers: [],
                            },
                        },
                        rate_limits: None,
                        headers: [],
                    },
                ),
                "google": Google(
                    ApiProviderConfig {
                        api_key: Some(
                            SecretBox<str>([REDACTED]),
                        ),
                        base_url: None,
                        forward_token: false,
                        models: {
                            "gemini-pro": ApiModelConfig {
                                rename: None,
                                rate_limits: None,
                                headers: [],
                            },
                        },
                        rate_limits: None,
                        headers: [],
                    },
                ),
                "openai": Openai(
                    ApiProviderConfig {
                        api_key: Some(
                            SecretBox<str>([REDACTED]),
                        ),
                        base_url: None,
                        forward_token: true,
                        models: {
                            "gpt-4": ApiModelConfig {
                                rename: None,
                                rate_limits: None,
                                headers: [],
                            },
                        },
                        rate_limits: None,
                        headers: [],
                    },
                ),
            },
        }
        "#);
    }
}
