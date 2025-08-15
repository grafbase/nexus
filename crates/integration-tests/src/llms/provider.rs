use indoc::formatdoc;
use std::future::Future;
use std::net::SocketAddr;

use super::openai::ModelConfig;

#[derive(Clone, Debug, Copy)]
pub enum ProviderType {
    OpenAI,
    Anthropic,
    Google,
    Bedrock,
}

/// Configuration for a test LLM provider
pub struct LlmProviderConfig {
    pub name: String,
    pub address: SocketAddr,
    pub provider_type: ProviderType,
    pub model_configs: Vec<ModelConfig>,
}

/// Trait for test LLM providers
pub trait TestLlmProvider: Send + Sync + 'static {
    /// Get the provider type for config (e.g., "openai", "anthropic")
    fn provider_type(&self) -> &str;

    /// Get the provider name (used as the config key)
    fn name(&self) -> &str;

    /// Get model configurations
    fn model_configs(&self) -> Vec<ModelConfig>;

    /// Start the mock server and return its configuration
    fn spawn(self: Box<Self>) -> impl Future<Output = anyhow::Result<LlmProviderConfig>> + Send;

    /// Generate the configuration snippet for this provider
    fn generate_config(&self, config: &LlmProviderConfig) -> String {
        generate_config_for_type(config.provider_type, config)
    }
}

/// Generate configuration for a given provider type
pub fn generate_config_for_type(provider_type: ProviderType, config: &LlmProviderConfig) -> String {
    // Generate model configuration section
    let mut models_section = String::new();
    for model_config in &config.model_configs {
        // Use quoted keys for model IDs to handle dots
        models_section.push_str(&format!(
            "\n            [llm.providers.{}.models.\"{}\"]",
            config.name, model_config.id
        ));
        if let Some(rename) = &model_config.rename {
            models_section.push_str(&format!("\n            rename = \"{}\"", rename));
        }
    }

    match provider_type {
        ProviderType::OpenAI | ProviderType::Anthropic | ProviderType::Google => {
            let (provider_type_str, base_url_path) = match provider_type {
                ProviderType::OpenAI => ("openai", "/v1"),
                ProviderType::Anthropic => ("anthropic", "/v1"),
                ProviderType::Google => ("google", "/v1beta"),
                _ => unreachable!(),
            };

            formatdoc! {r#"

                [llm.providers.{}]
                type = "{}"
                api_key = "test-key"
                base_url = "http://{}{}"
                {}
            "#, config.name, provider_type_str, config.address, base_url_path, models_section}
        }
        ProviderType::Bedrock => {
            // Bedrock uses different configuration
            formatdoc! {r#"

                [llm.providers.{}]
                type = "bedrock"
                region = "us-east-1"
                access_key_id = "test-access-key"
                secret_access_key = "test-secret-key"
                base_url = "http://{}"
                {}
            "#, config.name, config.address, models_section}
        }
    }
}
