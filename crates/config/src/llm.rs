//! LLM configuration structures for AI model providers.

use std::borrow::Cow;
use std::collections::BTreeMap;

use secrecy::SecretString;
use serde::{Deserialize, Deserializer};

/// Configuration for an individual model within a provider.
#[derive(Debug, Clone, Deserialize)]
pub struct ModelConfig {
    /// Optional rename - the actual provider model name.
    /// If not specified, the model ID (map key) is used.
    #[serde(default)]
    pub rename: Option<String>,
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
    #[serde(deserialize_with = "deserialize_non_empty_models_with_default")]
    pub models: BTreeMap<String, ModelConfig>,
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
    #[serde(deserialize_with = "deserialize_non_empty_models_with_default")]
    pub models: BTreeMap<String, ModelConfig>,
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

    /// Get the configured models for this provider.
    pub fn models(&self) -> &BTreeMap<String, ModelConfig> {
        match self {
            Self::Openai(config) => &config.models,
            Self::Anthropic(config) => &config.models,
            Self::Google(config) => &config.models,
            Self::Bedrock(config) => &config.models,
        }
    }

    /// Get AWS-specific configuration (only for Bedrock).
    pub fn aws_config(&self) -> Option<BedrockAwsConfig<'_>> {
        match self {
            Self::Bedrock(config) => Some(BedrockAwsConfig {
                access_key_id: config.access_key_id.as_ref(),
                secret_access_key: config.secret_access_key.as_ref(),
                session_token: config.session_token.as_ref(),
                profile: config.profile.as_deref(),
                region: config.region.as_str(),
            }),
            _ => None,
        }
    }
}

/// AWS-specific configuration for Bedrock provider.
#[derive(Debug, Clone)]
pub struct BedrockAwsConfig<'a> {
    /// AWS Access Key ID (optional - uses credential chain if not provided).
    pub access_key_id: Option<&'a SecretString>,
    /// AWS Secret Access Key (required if access_key_id is provided).
    pub secret_access_key: Option<&'a SecretString>,
    /// AWS Session Token (optional - for temporary credentials).
    pub session_token: Option<&'a SecretString>,
    /// AWS Profile name (optional - uses default profile if not specified).
    pub profile: Option<&'a str>,
    /// AWS region (required for Bedrock).
    pub region: &'a str,
}

/// Custom deserializer that ensures at least one model is configured.
/// This handles both missing field (uses default) and empty map cases.
fn deserialize_non_empty_models_with_default<'de, D>(deserializer: D) -> Result<BTreeMap<String, ModelConfig>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;

    // First deserialize as Option to handle missing field
    let models_opt = Option::<BTreeMap<String, ModelConfig>>::deserialize(deserializer)?;

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
                            "gpt-3-5-turbo": ModelConfig {
                                rename: None,
                            },
                            "gpt-4": ModelConfig {
                                rename: None,
                            },
                        },
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
                            "claude-3-opus": ModelConfig {
                                rename: None,
                            },
                            "claude-3-sonnet": ModelConfig {
                                rename: None,
                            },
                        },
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
                            "gemini-pro": ModelConfig {
                                rename: None,
                            },
                            "gemini-pro-vision": ModelConfig {
                                rename: None,
                            },
                        },
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
                            "claude-3-opus": ModelConfig {
                                rename: None,
                            },
                        },
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
                            "gemini-pro": ModelConfig {
                                rename: None,
                            },
                        },
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
                            "gpt-4": ModelConfig {
                                rename: None,
                            },
                        },
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
            
            [providers.invalid.models.test-model]
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
                            "gpt-4": ModelConfig {
                                rename: None,
                            },
                        },
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
                            "gpt-3-5": ModelConfig {
                                rename: Some(
                                    "gpt-3.5-turbo",
                                ),
                            },
                            "gpt-4": ModelConfig {
                                rename: Some(
                                    "gpt-4-turbo-preview",
                                ),
                            },
                        },
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
                            "custom-model": ModelConfig {
                                rename: None,
                            },
                            "gpt-4": ModelConfig {
                                rename: None,
                            },
                        },
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
                            "claude-3": ModelConfig {
                                rename: Some(
                                    "claude-3-opus-20240229",
                                ),
                            },
                            "claude-instant": ModelConfig {
                                rename: None,
                            },
                        },
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
                            "gpt-4": ModelConfig {
                                rename: Some(
                                    "gpt-4-turbo",
                                ),
                            },
                        },
                    },
                ),
            },
        }
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
                            "claude-3-opus": ModelConfig {
                                rename: None,
                            },
                        },
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
                            "gemini-pro": ModelConfig {
                                rename: None,
                            },
                        },
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
                            "gpt-4": ModelConfig {
                                rename: None,
                            },
                        },
                    },
                ),
            },
        }
        "#);
    }

    #[test]
    fn llm_config_with_bedrock_minimal() {
        let config = indoc! {r#"
            [providers.bedrock]
            type = "bedrock"
            region = "us-east-1"
            
            [providers.bedrock.models.claude-3-sonnet]
            rename = "anthropic.claude-3-sonnet-20240229-v1:0"
        "#};

        let config: LlmConfig = toml::from_str(config).unwrap();

        assert_debug_snapshot!(&config, @r#"
        LlmConfig {
            enabled: true,
            path: "/llm",
            providers: {
                "bedrock": Bedrock(
                    BedrockProviderConfig {
                        access_key_id: None,
                        secret_access_key: None,
                        session_token: None,
                        profile: None,
                        region: "us-east-1",
                        base_url: None,
                        models: {
                            "claude-3-sonnet": ModelConfig {
                                rename: Some(
                                    "anthropic.claude-3-sonnet-20240229-v1:0",
                                ),
                            },
                        },
                    },
                ),
            },
        }
        "#);
    }

    #[test]
    fn llm_config_with_bedrock_explicit_credentials() {
        let config = indoc! {r#"
            [providers.bedrock]
            type = "bedrock"
            region = "us-west-2"
            access_key_id = "${AWS_ACCESS_KEY_ID}"
            secret_access_key = "${AWS_SECRET_ACCESS_KEY}"
            session_token = "${AWS_SESSION_TOKEN}"
            
            [providers.bedrock.models.titan-express]
            rename = "amazon.titan-text-express-v1"
            
            [providers.bedrock.models.llama3-70b]
            rename = "meta.llama3-70b-instruct-v1:0"
        "#};

        let config: LlmConfig = toml::from_str(config).unwrap();

        assert_debug_snapshot!(&config, @r#"
        LlmConfig {
            enabled: true,
            path: "/llm",
            providers: {
                "bedrock": Bedrock(
                    BedrockProviderConfig {
                        access_key_id: Some(
                            SecretBox<str>([REDACTED]),
                        ),
                        secret_access_key: Some(
                            SecretBox<str>([REDACTED]),
                        ),
                        session_token: Some(
                            SecretBox<str>([REDACTED]),
                        ),
                        profile: None,
                        region: "us-west-2",
                        base_url: None,
                        models: {
                            "llama3-70b": ModelConfig {
                                rename: Some(
                                    "meta.llama3-70b-instruct-v1:0",
                                ),
                            },
                            "titan-express": ModelConfig {
                                rename: Some(
                                    "amazon.titan-text-express-v1",
                                ),
                            },
                        },
                    },
                ),
            },
        }
        "#);
    }

    #[test]
    fn llm_config_with_bedrock_profile() {
        let config = indoc! {r#"
            [providers.bedrock]
            type = "bedrock"
            region = "eu-west-1"
            profile = "production"
            
            [providers.bedrock.models.claude-3-opus]
            rename = "anthropic.claude-3-opus-20240229-v1:0"
            
            [providers.bedrock.models.mistral-7b]
            rename = "mistral.mistral-7b-instruct-v0:2"
        "#};

        let config: LlmConfig = toml::from_str(config).unwrap();

        assert_debug_snapshot!(&config, @r#"
        LlmConfig {
            enabled: true,
            path: "/llm",
            providers: {
                "bedrock": Bedrock(
                    BedrockProviderConfig {
                        access_key_id: None,
                        secret_access_key: None,
                        session_token: None,
                        profile: Some(
                            "production",
                        ),
                        region: "eu-west-1",
                        base_url: None,
                        models: {
                            "claude-3-opus": ModelConfig {
                                rename: Some(
                                    "anthropic.claude-3-opus-20240229-v1:0",
                                ),
                            },
                            "mistral-7b": ModelConfig {
                                rename: Some(
                                    "mistral.mistral-7b-instruct-v0:2",
                                ),
                            },
                        },
                    },
                ),
            },
        }
        "#);
    }

    #[test]
    fn llm_config_with_bedrock_multiple_regions() {
        let config = indoc! {r#"
            [providers.bedrock-east]
            type = "bedrock"
            region = "us-east-1"
            
            [providers.bedrock-east.models.claude-3-sonnet]
            rename = "anthropic.claude-3-sonnet-20240229-v1:0"

            [providers.bedrock-west]
            type = "bedrock"
            region = "us-west-2"
            
            [providers.bedrock-west.models.titan-express]
            rename = "amazon.titan-text-express-v1"
        "#};

        let config: LlmConfig = toml::from_str(config).unwrap();

        assert_debug_snapshot!(&config, @r#"
        LlmConfig {
            enabled: true,
            path: "/llm",
            providers: {
                "bedrock-east": Bedrock(
                    BedrockProviderConfig {
                        access_key_id: None,
                        secret_access_key: None,
                        session_token: None,
                        profile: None,
                        region: "us-east-1",
                        base_url: None,
                        models: {
                            "claude-3-sonnet": ModelConfig {
                                rename: Some(
                                    "anthropic.claude-3-sonnet-20240229-v1:0",
                                ),
                            },
                        },
                    },
                ),
                "bedrock-west": Bedrock(
                    BedrockProviderConfig {
                        access_key_id: None,
                        secret_access_key: None,
                        session_token: None,
                        profile: None,
                        region: "us-west-2",
                        base_url: None,
                        models: {
                            "titan-express": ModelConfig {
                                rename: Some(
                                    "amazon.titan-text-express-v1",
                                ),
                            },
                        },
                    },
                ),
            },
        }
        "#);
    }

    #[test]
    fn llm_config_bedrock_invalid_without_region() {
        let config = indoc! {r#"
            [providers.bedrock]
            type = "bedrock"
            # Missing region - should fail to parse since region is required
            
            [providers.bedrock.models.claude-3-sonnet]
            rename = "anthropic.claude-3-sonnet-20240229-v1:0"
        "#};

        // This should fail to parse since region is now required for Bedrock
        let result: Result<LlmConfig, _> = toml::from_str(config);
        assert!(result.is_err());

        let error = result.unwrap_err();
        insta::assert_snapshot!(error.to_string(), @r"
        TOML parse error at line 1, column 1
          |
        1 | [providers.bedrock]
          | ^^^^^^^^^^^^^^^^^^^
        missing field `region`
        ");
    }

    #[test]
    fn llm_config_bedrock_with_session_token() {
        let config = indoc! {r#"
            [providers.bedrock]
            type = "bedrock"
            region = "us-east-1"
            session_token = "${AWS_SESSION_TOKEN}"
            
            [providers.bedrock.models.claude-3-sonnet]
            rename = "anthropic.claude-3-sonnet-20240229-v1:0"
        "#};

        let config: LlmConfig = toml::from_str(config).unwrap();

        assert_debug_snapshot!(&config, @r#"
        LlmConfig {
            enabled: true,
            path: "/llm",
            providers: {
                "bedrock": Bedrock(
                    BedrockProviderConfig {
                        access_key_id: None,
                        secret_access_key: None,
                        session_token: Some(
                            SecretBox<str>([REDACTED]),
                        ),
                        profile: None,
                        region: "us-east-1",
                        base_url: None,
                        models: {
                            "claude-3-sonnet": ModelConfig {
                                rename: Some(
                                    "anthropic.claude-3-sonnet-20240229-v1:0",
                                ),
                            },
                        },
                    },
                ),
            },
        }
        "#);
    }

    #[test]
    fn llm_config_bedrock_with_base_url() {
        let config = indoc! {r#"
            [providers.bedrock]
            type = "bedrock"
            region = "us-west-2"
            base_url = "https://bedrock-vpc.us-west-2.amazonaws.com"
            
            [providers.bedrock.models.titan-express]
            rename = "amazon.titan-text-express-v1"
        "#};

        let config: LlmConfig = toml::from_str(config).unwrap();

        assert_debug_snapshot!(&config, @r#"
        LlmConfig {
            enabled: true,
            path: "/llm",
            providers: {
                "bedrock": Bedrock(
                    BedrockProviderConfig {
                        access_key_id: None,
                        secret_access_key: None,
                        session_token: None,
                        profile: None,
                        region: "us-west-2",
                        base_url: Some(
                            "https://bedrock-vpc.us-west-2.amazonaws.com",
                        ),
                        models: {
                            "titan-express": ModelConfig {
                                rename: Some(
                                    "amazon.titan-text-express-v1",
                                ),
                            },
                        },
                    },
                ),
            },
        }
        "#);
    }

    #[test]
    fn llm_config_bedrock_without_models_should_fail() {
        let config = indoc! {r#"
            [providers.bedrock]
            type = "bedrock"
            region = "us-east-1"
            
            [providers.bedrock.models]
        "#};

        // Should fail because models are required (empty models section)
        let result: Result<LlmConfig, _> = toml::from_str(config);
        assert!(result.is_err());

        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("At least one model must be configured for each provider"));
    }

    #[test]
    fn llm_config_bedrock_mixed_credentials() {
        // Test partial credential configuration - only access_key_id without secret_access_key
        let config = indoc! {r#"
            [providers.bedrock]
            type = "bedrock"
            region = "us-east-1"
            access_key_id = "${AWS_ACCESS_KEY_ID}"
            # Missing secret_access_key - should still parse but might fail at runtime
            
            [providers.bedrock.models.claude-3-sonnet]
            rename = "anthropic.claude-3-sonnet-20240229-v1:0"
        "#};

        let config: LlmConfig = toml::from_str(config).unwrap();

        // Should parse successfully - runtime validation happens elsewhere
        assert!(config.providers.contains_key("bedrock"));
    }

    #[test]
    fn llm_config_bedrock_all_model_families() {
        let config = indoc! {r#"
            [providers.bedrock]
            type = "bedrock"
            region = "us-east-1"
            
            [providers.bedrock.models.claude-3-sonnet]
            rename = "anthropic.claude-3-sonnet-20240229-v1:0"
            
            [providers.bedrock.models.titan-express]
            rename = "amazon.titan-text-express-v1"
            
            [providers.bedrock.models.llama3-70b]
            rename = "meta.llama3-70b-instruct-v1:0"
            
            [providers.bedrock.models.mistral-7b]
            rename = "mistral.mistral-7b-instruct-v0:2"
            
            [providers.bedrock.models.command-text]
            rename = "cohere.command-text-v14"
            
            [providers.bedrock.models.j2-ultra]
            rename = "ai21.j2-ultra-v1"
            
            [providers.bedrock.models.stable-diffusion]
            rename = "stability.stable-diffusion-xl-base-v1:0"
        "#};

        let config: LlmConfig = toml::from_str(config).unwrap();

        let bedrock_config = match &config.providers["bedrock"] {
            LlmProviderConfig::Bedrock(config) => config,
            _ => unreachable!("Expected Bedrock config"),
        };

        // Should have all 7 model families configured
        assert_eq!(bedrock_config.models.len(), 7);
        assert!(bedrock_config.models.contains_key("claude-3-sonnet"));
        assert!(bedrock_config.models.contains_key("titan-express"));
        assert!(bedrock_config.models.contains_key("llama3-70b"));
        assert!(bedrock_config.models.contains_key("mistral-7b"));
        assert!(bedrock_config.models.contains_key("command-text"));
        assert!(bedrock_config.models.contains_key("j2-ultra"));
        assert!(bedrock_config.models.contains_key("stable-diffusion"));
    }
}
