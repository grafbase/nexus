//! LLM configuration structures for AI model providers.

use std::borrow::Cow;
use std::collections::BTreeMap;

use secrecy::SecretString;
use serde::Deserialize;

/// LLM configuration for AI model integration.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LlmConfig {
    /// Whether the LLM functionality is enabled.
    pub enabled: bool,

    /// The path where the LLM endpoints will be mounted.
    pub path: Cow<'static, str>,

    /// Map of LLM provider configurations.
    pub providers: BTreeMap<String, LlmProvider>,
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

/// LLM provider configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum LlmProvider {
    /// OpenAI provider configuration.
    Openai(OpenAiConfig),
    /// Anthropic provider configuration.
    Anthropic(AnthropicConfig),
    /// Google provider configuration.
    Google(GoogleConfig),
}

/// OpenAI provider configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenAiConfig {
    /// API key for OpenAI (supports environment variable interpolation).
    pub api_key: SecretString,
}

/// Anthropic provider configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnthropicConfig {
    /// API key for Anthropic (supports environment variable interpolation).
    pub api_key: SecretString,
}

/// Google provider configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoogleConfig {
    /// API key for Google (supports environment variable interpolation).
    pub api_key: SecretString,
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
        "#};

        let config: LlmConfig = toml::from_str(config).unwrap();

        assert_debug_snapshot!(&config, @r#"
        LlmConfig {
            enabled: true,
            path: "/llm",
            providers: {
                "openai": Openai(
                    OpenAiConfig {
                        api_key: SecretBox<str>([REDACTED]),
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
        "#};

        let config: LlmConfig = toml::from_str(config).unwrap();

        assert_debug_snapshot!(&config, @r#"
        LlmConfig {
            enabled: true,
            path: "/llm",
            providers: {
                "anthropic": Anthropic(
                    AnthropicConfig {
                        api_key: SecretBox<str>([REDACTED]),
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
        "#};

        let config: LlmConfig = toml::from_str(config).unwrap();

        assert_debug_snapshot!(&config, @r#"
        LlmConfig {
            enabled: true,
            path: "/llm",
            providers: {
                "google": Google(
                    GoogleConfig {
                        api_key: SecretBox<str>([REDACTED]),
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

            [providers.anthropic]
            type = "anthropic"
            api_key = "{{ env.ANTHROPIC_API_KEY }}"

            [providers.google]
            type = "google"
            api_key = "{{ env.GOOGLE_KEY }}"
        "#};

        let config: LlmConfig = toml::from_str(config).unwrap();

        assert_debug_snapshot!(&config, @r#"
        LlmConfig {
            enabled: true,
            path: "/ai",
            providers: {
                "anthropic": Anthropic(
                    AnthropicConfig {
                        api_key: SecretBox<str>([REDACTED]),
                    },
                ),
                "google": Google(
                    GoogleConfig {
                        api_key: SecretBox<str>([REDACTED]),
                    },
                ),
                "openai": Openai(
                    OpenAiConfig {
                        api_key: SecretBox<str>([REDACTED]),
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
        "#};

        let config: LlmConfig = toml::from_str(config).unwrap();

        assert_debug_snapshot!(&config, @r#"
        LlmConfig {
            enabled: true,
            path: "/llm",
            providers: {
                "openai": Openai(
                    OpenAiConfig {
                        api_key: SecretBox<str>([REDACTED]),
                    },
                ),
            },
        }
        "#);
    }
}
