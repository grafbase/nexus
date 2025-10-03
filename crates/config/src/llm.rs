//! LLM configuration structures for AI model providers.

use std::{borrow::Cow, collections::BTreeMap, fmt};

use indexmap::IndexMap;

use crate::headers::HeaderRule;
use crate::rate_limit::TokenRateLimitsConfig;
use regex::{Regex, RegexBuilder};
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

/// Case-insensitive regex filter for matching model identifiers.
#[derive(Clone)]
pub struct ModelFilter {
    regex: Regex,
}

impl ModelFilter {
    /// Create a new validated model filter.
    fn new(pattern: &str) -> Result<Self, String> {
        let trimmed = pattern.trim();

        if trimmed.is_empty() {
            return Err("model_filter cannot be empty".to_string());
        }

        let regex = RegexBuilder::new(trimmed)
            .case_insensitive(true)
            .build()
            .map_err(|err| format!("invalid model_filter regex: {err}"))?;

        Ok(Self { regex })
    }

    /// Return the original pattern string.
    pub fn pattern(&self) -> &str {
        self.regex.as_str()
    }

    /// Return the compiled regex.
    pub fn regex(&self) -> &Regex {
        &self.regex
    }

    /// Check whether the supplied model identifier matches the pattern.
    pub fn is_match(&self, model: &str) -> bool {
        self.regex.is_match(model)
    }
}

impl fmt::Debug for ModelFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModelFilter").field("pattern", &self.pattern()).finish()
    }
}

impl<'de> Deserialize<'de> for ModelFilter {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let pattern = Cow::<'de, str>::deserialize(deserializer)?;
        ModelFilter::new(pattern.as_ref()).map_err(serde::de::Error::custom)
    }
}

/// Protocol type for LLM endpoints.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LlmProtocol {
    /// OpenAI protocol (default).
    OpenAI,
    /// Anthropic protocol.
    Anthropic,
}

impl Default for LlmProtocol {
    fn default() -> Self {
        Self::OpenAI
    }
}

/// OpenAI protocol configuration
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OpenAIProtocolConfig {
    pub enabled: bool,
    pub path: String,
}

impl Default for OpenAIProtocolConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            path: "/llm/openai".to_string(),
        }
    }
}

/// Anthropic protocol configuration
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AnthropicProtocolConfig {
    pub enabled: bool,
    pub path: String,
}

impl Default for AnthropicProtocolConfig {
    fn default() -> Self {
        Self {
            enabled: false, // TODO: Enable when Anthropic protocol is implemented
            path: "/llm/anthropic".to_string(),
        }
    }
}

/// Configuration for all LLM protocol endpoints.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct LlmProtocolsConfig {
    /// OpenAI protocol endpoint configuration
    pub openai: OpenAIProtocolConfig,

    /// Anthropic protocol endpoint configuration
    pub anthropic: AnthropicProtocolConfig,
}

/// LLM configuration for AI model integration.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LlmConfig {
    /// Whether the LLM functionality is enabled.
    enabled: bool,

    /// Protocol-specific endpoint configurations.
    pub protocols: LlmProtocolsConfig,

    /// Map of LLM provider configurations.
    pub providers: IndexMap<String, LlmProviderConfig>,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            protocols: LlmProtocolsConfig::default(),
            providers: IndexMap::new(),
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

    /// Whether there are any protocol endpoints enabled.
    pub fn has_protocol_endpoints(&self) -> bool {
        self.protocols.openai.enabled || self.protocols.anthropic.enabled
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
#[derive(Debug, Clone)]
pub struct ApiProviderConfig {
    /// API key for authentication.
    pub api_key: Option<SecretString>,

    /// Custom base URL for the provider API.
    pub base_url: Option<String>,

    /// Enable token forwarding from user requests.
    pub forward_token: bool,

    /// Regular expression filter for automatically routing models to this provider.
    pub model_filter: Option<ModelFilter>,

    /// Explicitly configured models for this provider.
    pub models: BTreeMap<String, ApiModelConfig>,

    /// Provider-level rate limits.
    pub rate_limits: Option<TokenRateLimitsConfig>,

    /// Header transformation rules for this provider.
    pub headers: Vec<HeaderRule>,
}

impl<'de> Deserialize<'de> for ApiProviderConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        struct ApiProviderConfigSerde {
            #[serde(default)]
            api_key: Option<SecretString>,
            #[serde(default)]
            base_url: Option<String>,
            #[serde(default)]
            forward_token: bool,
            #[serde(default)]
            model_filter: Option<ModelFilter>,
            #[serde(default)]
            models: BTreeMap<String, ApiModelConfig>,
            #[serde(default)]
            rate_limits: Option<TokenRateLimitsConfig>,
            #[serde(default)]
            headers: Vec<HeaderRule>,
        }

        let raw = ApiProviderConfigSerde::deserialize(deserializer)?;

        Ok(Self {
            api_key: raw.api_key,
            base_url: raw.base_url,
            forward_token: raw.forward_token,
            model_filter: raw.model_filter,
            models: raw.models,
            rate_limits: raw.rate_limits,
            headers: raw.headers,
        })
    }
}

/// Configuration specific to AWS Bedrock.
#[derive(Debug, Clone)]
pub struct BedrockProviderConfig {
    /// AWS Access Key ID (optional - uses credential chain if not provided).
    pub access_key_id: Option<SecretString>,

    /// AWS Secret Access Key (required if access_key_id is provided).
    pub secret_access_key: Option<SecretString>,

    /// AWS Session Token (optional - for temporary credentials).
    pub session_token: Option<SecretString>,

    /// AWS Profile name (optional - uses default profile if not specified).
    pub profile: Option<String>,

    /// AWS region (required for Bedrock).
    pub region: String,

    /// Custom endpoint URL (optional - for VPC endpoints).
    pub base_url: Option<String>,

    /// Regular expression filter for automatically routing models to this provider.
    pub model_filter: Option<ModelFilter>,

    /// Explicitly configured models for this provider.
    /// Bedrock models don't support custom headers due to SigV4 signing.
    pub models: BTreeMap<String, BedrockModelConfig>,
}

impl<'de> Deserialize<'de> for BedrockProviderConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct BedrockProviderConfigSerde {
            #[serde(default)]
            pub access_key_id: Option<SecretString>,
            #[serde(default)]
            pub secret_access_key: Option<SecretString>,
            #[serde(default)]
            pub session_token: Option<SecretString>,
            #[serde(default)]
            pub profile: Option<String>,
            pub region: String,
            #[serde(default)]
            pub base_url: Option<String>,
            #[serde(default)]
            pub model_filter: Option<ModelFilter>,
            #[serde(default)]
            pub models: BTreeMap<String, BedrockModelConfig>,
        }

        let raw = BedrockProviderConfigSerde::deserialize(deserializer)?;

        Ok(Self {
            access_key_id: raw.access_key_id,
            secret_access_key: raw.secret_access_key,
            session_token: raw.session_token,
            profile: raw.profile,
            region: raw.region,
            base_url: raw.base_url,
            model_filter: raw.model_filter,
            models: raw.models,
        })
    }
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

    /// Get the configured model filter for this provider, if any.
    pub fn model_filter(&self) -> Option<&ModelFilter> {
        match self {
            Self::Openai(config) => config.model_filter.as_ref(),
            Self::Anthropic(config) => config.model_filter.as_ref(),
            Self::Google(config) => config.model_filter.as_ref(),
            Self::Bedrock(config) => config.model_filter.as_ref(),
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
            protocols: LlmProtocolsConfig {
                openai: OpenAIProtocolConfig {
                    enabled: true,
                    path: "/llm/openai",
                },
                anthropic: AnthropicProtocolConfig {
                    enabled: false,
                    path: "/llm/anthropic",
                },
            },
            providers: {},
        }
        "#);
    }

    #[test]
    fn llm_config_with_openai() {
        let config = indoc! {r#"
            enabled = true

            [protocols.openai]
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
            protocols: LlmProtocolsConfig {
                openai: OpenAIProtocolConfig {
                    enabled: true,
                    path: "/llm",
                },
                anthropic: AnthropicProtocolConfig {
                    enabled: false,
                    path: "/llm/anthropic",
                },
            },
            providers: {
                "openai": Openai(
                    ApiProviderConfig {
                        api_key: Some(
                            SecretBox<str>([REDACTED]),
                        ),
                        base_url: None,
                        forward_token: false,
                        model_filter: None,
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

            [protocols.anthropic]
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
            protocols: LlmProtocolsConfig {
                openai: OpenAIProtocolConfig {
                    enabled: true,
                    path: "/llm/openai",
                },
                anthropic: AnthropicProtocolConfig {
                    enabled: true,
                    path: "/llm",
                },
            },
            providers: {
                "anthropic": Anthropic(
                    ApiProviderConfig {
                        api_key: Some(
                            SecretBox<str>([REDACTED]),
                        ),
                        base_url: None,
                        forward_token: false,
                        model_filter: None,
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
            [protocols.openai]
            enabled = true
            path = "/llm"

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
            protocols: LlmProtocolsConfig {
                openai: OpenAIProtocolConfig {
                    enabled: true,
                    path: "/llm",
                },
                anthropic: AnthropicProtocolConfig {
                    enabled: false,
                    path: "/llm/anthropic",
                },
            },
            providers: {
                "google": Google(
                    ApiProviderConfig {
                        api_key: Some(
                            SecretBox<str>([REDACTED]),
                        ),
                        base_url: None,
                        forward_token: false,
                        model_filter: None,
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

            [protocols.openai]
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
            protocols: LlmProtocolsConfig {
                openai: OpenAIProtocolConfig {
                    enabled: true,
                    path: "/ai",
                },
                anthropic: AnthropicProtocolConfig {
                    enabled: false,
                    path: "/llm/anthropic",
                },
            },
            providers: {
                "anthropic": Anthropic(
                    ApiProviderConfig {
                        api_key: Some(
                            SecretBox<str>([REDACTED]),
                        ),
                        base_url: None,
                        forward_token: false,
                        model_filter: None,
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
                        model_filter: None,
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
                        model_filter: None,
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
    fn llm_config_with_model_filter_only() {
        let config = indoc! {r#"
            enabled = true

            [protocols.openai]
            enabled = true
            path = "/llm"

            [providers.anthropic]
            type = "anthropic"
            api_key = "test"
            model_filter = "^claude-.*"
        "#};

        let config: LlmConfig = toml::from_str(config).unwrap();

        let provider = config.providers.get("anthropic").unwrap();
        let pattern = provider.model_filter().unwrap();
        assert_eq!(pattern.pattern(), "^claude-.*");
        assert!(pattern.is_match("CLAUDE-3-OPUS"));
        assert!(provider.models().is_empty());
    }

    #[test]
    fn llm_config_rejects_empty_model_filter() {
        let config = indoc! {r#"
            [providers.anthropic]
            type = "anthropic"
            api_key = "test"
            model_filter = ""
        "#};

        let err = toml::from_str::<LlmConfig>(config).unwrap_err();
        assert!(err.to_string().contains("model_filter cannot be empty"));
    }

    #[test]
    fn llm_config_allows_model_filter_with_slash() {
        let config = indoc! {r#"
            [providers.anthropic]
            type = "anthropic"
            api_key = "test"
            model_filter = "anthropic/claude"
        "#};

        let config: LlmConfig = toml::from_str(config).unwrap();
        let provider = config.providers.get("anthropic").unwrap();
        let filter = provider.model_filter().expect("filter missing");
        assert!(filter.is_match("anthropic/claude"));
    }

    #[test]
    fn llm_config_rejects_invalid_regex_model_filter() {
        let config = indoc! {r#"
            [providers.anthropic]
            type = "anthropic"
            api_key = "test"
            model_filter = "["
        "#};

        let err = toml::from_str::<LlmConfig>(config).unwrap_err();
        assert!(err.to_string().contains("invalid model_filter regex"));
    }

    #[test]
    fn bedrock_allows_missing_models_and_filter() {
        let config = indoc! {r#"
            [providers.bedrock]
            type = "bedrock"
            region = "us-east-1"
        "#};

        let config: LlmConfig = toml::from_str(config).unwrap();
        let provider = config.providers.get("bedrock").unwrap();

        assert!(provider.model_filter().is_none());
        assert!(provider.models().is_empty());
    }

    #[test]
    fn bedrock_allows_filter_only() {
        let config = indoc! {r#"
            [providers.bedrock]
            type = "bedrock"
            region = "us-east-1"
            model_filter = "^anthropic-.*"
        "#};

        let config: LlmConfig = toml::from_str(config).unwrap();
        let provider = config.providers.get("bedrock").unwrap();
        let filter = provider.model_filter().unwrap();
        assert!(filter.is_match("ANTHROPIC-CLAUDE"));
        assert!(provider.models().is_empty());
        assert_debug_snapshot!(provider, @r#"
        Bedrock(
            BedrockProviderConfig {
                access_key_id: None,
                secret_access_key: None,
                session_token: None,
                profile: None,
                region: "us-east-1",
                base_url: None,
                model_filter: Some(
                    ModelFilter {
                        pattern: "^anthropic-.*",
                    },
                ),
                models: {},
            },
        )
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
            protocols: LlmProtocolsConfig {
                openai: OpenAIProtocolConfig {
                    enabled: true,
                    path: "/llm/openai",
                },
                anthropic: AnthropicProtocolConfig {
                    enabled: false,
                    path: "/llm/anthropic",
                },
            },
            providers: {},
        }
        "#);
    }

    #[test]
    fn llm_config_custom_path() {
        let config = indoc! {r#"
            [protocols.openai]
enabled = true
path = "/models"
"#};

        let config: LlmConfig = toml::from_str(config).unwrap();

        assert_debug_snapshot!(&config, @r#"
        LlmConfig {
            enabled: true,
            protocols: LlmProtocolsConfig {
                openai: OpenAIProtocolConfig {
                    enabled: true,
                    path: "/models",
                },
                anthropic: AnthropicProtocolConfig {
                    enabled: false,
                    path: "/llm/anthropic",
                },
            },
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
            [protocols.openai]
enabled = true
path = "/llm"
[providers.openai]
            type = "openai"
            api_key = "sk-1234567890abcdef"

            [providers.openai.models.gpt-4]
        "#};

        let config: LlmConfig = toml::from_str(config).unwrap();

        assert_debug_snapshot!(&config, @r#"
        LlmConfig {
            enabled: true,
            protocols: LlmProtocolsConfig {
                openai: OpenAIProtocolConfig {
                    enabled: true,
                    path: "/llm",
                },
                anthropic: AnthropicProtocolConfig {
                    enabled: false,
                    path: "/llm/anthropic",
                },
            },
            providers: {
                "openai": Openai(
                    ApiProviderConfig {
                        api_key: Some(
                            SecretBox<str>([REDACTED]),
                        ),
                        base_url: None,
                        forward_token: false,
                        model_filter: None,
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
            [protocols.openai]
enabled = true
path = "/llm"
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
            protocols: LlmProtocolsConfig {
                openai: OpenAIProtocolConfig {
                    enabled: true,
                    path: "/llm",
                },
                anthropic: AnthropicProtocolConfig {
                    enabled: false,
                    path: "/llm/anthropic",
                },
            },
            providers: {
                "openai": Openai(
                    ApiProviderConfig {
                        api_key: Some(
                            SecretBox<str>([REDACTED]),
                        ),
                        base_url: None,
                        forward_token: false,
                        model_filter: None,
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
            [protocols.openai]
enabled = true
path = "/llm"
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
            protocols: LlmProtocolsConfig {
                openai: OpenAIProtocolConfig {
                    enabled: true,
                    path: "/llm",
                },
                anthropic: AnthropicProtocolConfig {
                    enabled: false,
                    path: "/llm/anthropic",
                },
            },
            providers: {
                "openai": Openai(
                    ApiProviderConfig {
                        api_key: Some(
                            SecretBox<str>([REDACTED]),
                        ),
                        base_url: None,
                        forward_token: false,
                        model_filter: None,
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
            [protocols.openai]
enabled = true
path = "/llm"
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
            protocols: LlmProtocolsConfig {
                openai: OpenAIProtocolConfig {
                    enabled: true,
                    path: "/llm",
                },
                anthropic: AnthropicProtocolConfig {
                    enabled: false,
                    path: "/llm/anthropic",
                },
            },
            providers: {
                "anthropic": Anthropic(
                    ApiProviderConfig {
                        api_key: Some(
                            SecretBox<str>([REDACTED]),
                        ),
                        base_url: None,
                        forward_token: false,
                        model_filter: None,
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
                        model_filter: None,
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
            [protocols.openai]
enabled = true
path = "/llm"
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
            [protocols.openai]
enabled = true
path = "/llm"
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
            [protocols.openai]
enabled = true
path = "/llm"
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
            protocols: LlmProtocolsConfig {
                openai: OpenAIProtocolConfig {
                    enabled: true,
                    path: "/llm",
                },
                anthropic: AnthropicProtocolConfig {
                    enabled: false,
                    path: "/llm/anthropic",
                },
            },
            providers: {
                "anthropic": Anthropic(
                    ApiProviderConfig {
                        api_key: None,
                        base_url: None,
                        forward_token: true,
                        model_filter: None,
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
                        model_filter: None,
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
                        model_filter: None,
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
    fn llm_config_multiple_endpoints() {
        let config = indoc! {r#"
            [protocols.openai]
            enabled = true
            path = "/llm"

            [protocols.anthropic]
            enabled = true
            path = "/claude"

            [providers.openai]
            type = "openai"
            api_key = "test-key"

            [providers.openai.models.gpt-4]
        "#};

        let config: LlmConfig = toml::from_str(config).unwrap();

        assert_debug_snapshot!(&config, @r#"
        LlmConfig {
            enabled: true,
            protocols: LlmProtocolsConfig {
                openai: OpenAIProtocolConfig {
                    enabled: true,
                    path: "/llm",
                },
                anthropic: AnthropicProtocolConfig {
                    enabled: true,
                    path: "/claude",
                },
            },
            providers: {
                "openai": Openai(
                    ApiProviderConfig {
                        api_key: Some(
                            SecretBox<str>([REDACTED]),
                        ),
                        base_url: None,
                        forward_token: false,
                        model_filter: None,
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
    fn llm_config_anthropic_protocol() {
        let config = indoc! {r#"
            [protocols.openai]
            enabled = true
            path = "/v1"

            [providers.anthropic]
            type = "anthropic"
            api_key = "test-key"

            [providers.anthropic.models.claude-3-opus]
        "#};

        let config: LlmConfig = toml::from_str(config).unwrap();

        assert_debug_snapshot!(&config, @r#"
        LlmConfig {
            enabled: true,
            protocols: LlmProtocolsConfig {
                openai: OpenAIProtocolConfig {
                    enabled: true,
                    path: "/v1",
                },
                anthropic: AnthropicProtocolConfig {
                    enabled: false,
                    path: "/llm/anthropic",
                },
            },
            providers: {
                "anthropic": Anthropic(
                    ApiProviderConfig {
                        api_key: Some(
                            SecretBox<str>([REDACTED]),
                        ),
                        base_url: None,
                        forward_token: false,
                        model_filter: None,
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
            },
        }
        "#);
    }

    #[test]
    fn protocol_config_default_paths() {
        // Test that the default configs have correct paths
        let openai_config = OpenAIProtocolConfig::default();
        assert_eq!(openai_config.path, "/llm/openai");
        assert!(openai_config.enabled);

        let anthropic_config = AnthropicProtocolConfig::default();
        assert_eq!(anthropic_config.path, "/llm/anthropic");
        assert!(!anthropic_config.enabled); // Disabled by default until implemented
    }
}
