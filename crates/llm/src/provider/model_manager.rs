use std::collections::BTreeMap;

use config::ModelConfig;

use crate::messages::{Model, ObjectType};

/// Manages model configuration and resolution for LLM providers.
///
/// This struct handles the common logic for:
/// - Resolving model names (with rename support)
/// - Listing configured models
/// - Backward compatibility (empty config = allow all models)
pub(crate) struct ModelManager {
    models: BTreeMap<String, ModelConfig>,
    owner: String,
}

impl ModelManager {
    /// Create a new ModelManager from model configuration.
    pub fn new(models: BTreeMap<String, ModelConfig>, owner: impl Into<String>) -> Self {
        Self {
            models,
            owner: owner.into(),
        }
    }

    /// Check if a model is configured and resolve its actual name.
    ///
    /// Returns the renamed model if configured, or the original name if rename is not specified.
    /// Returns None if the model is not configured.
    pub fn resolve_model(&self, requested_model: &str) -> Option<String> {
        // Phase 3: Models must be explicitly configured
        // Check if the requested model is explicitly configured
        self.models
            .get(requested_model)
            .map(|model_config| model_config.rename().unwrap_or(requested_model).to_string())
    }

    /// Get list of configured models for the /models endpoint.
    ///
    /// Returns an error if no models are configured (Phase 3 enforcement).
    pub fn get_configured_models(&self) -> Vec<Model> {
        self.models
            .keys()
            .map(|model_id| Model {
                id: model_id.clone(),
                object: ObjectType::Model,
                created: 1719475200, // Fixed timestamp
                owned_by: self.owner.clone(),
            })
            .collect()
    }

    /// Get the configuration for a specific model.
    ///
    /// Returns None if the model is not configured.
    pub fn get_model_config(&self, model_name: &str) -> Option<&ModelConfig> {
        self.models.get(model_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use config::ModelConfig;

    #[test]
    fn empty_config_rejects_all_models() {
        let models = BTreeMap::new();
        let manager = ModelManager::new(models, "test");

        // Phase 3: No models configured means all models are rejected
        assert_eq!(manager.resolve_model("gpt-4"), None);
        assert_eq!(manager.resolve_model("any-model"), None);
    }

    #[test]
    fn configured_model_without_rename() {
        let mut models = BTreeMap::new();
        models.insert(
            "gpt-4".to_string(),
            ModelConfig::Api(config::ApiModelConfig {
                rename: None,
                rate_limits: None,
                headers: Vec::new(),
            }),
        );

        let manager = ModelManager::new(models, "test");

        assert_eq!(manager.resolve_model("gpt-4"), Some("gpt-4".to_string()));
        assert_eq!(manager.resolve_model("gpt-3.5"), None);
    }

    #[test]
    fn configured_model_with_rename() {
        let mut models = BTreeMap::new();
        models.insert(
            "claude".to_string(),
            ModelConfig::Api(config::ApiModelConfig {
                rename: Some("claude-3-opus-20240229".to_string()),
                rate_limits: None,
                headers: Vec::new(),
            }),
        );

        let manager = ModelManager::new(models, "anthropic");

        assert_eq!(
            manager.resolve_model("claude"),
            Some("claude-3-opus-20240229".to_string())
        );
        assert_eq!(manager.resolve_model("claude-3-opus-20240229"), None);
    }

    #[test]
    fn get_configured_models_returns_list() {
        let mut models = BTreeMap::new();
        models.insert(
            "gpt-4".to_string(),
            ModelConfig::Api(config::ApiModelConfig {
                rename: None,
                rate_limits: None,
                headers: Vec::new(),
            }),
        );
        models.insert(
            "gpt-3.5-turbo".to_string(),
            ModelConfig::Api(config::ApiModelConfig {
                rename: None,
                rate_limits: None,
                headers: Vec::new(),
            }),
        );

        let manager = ModelManager::new(models, "openai");
        let model_list = manager.get_configured_models();

        assert_eq!(model_list.len(), 2);
        assert!(model_list.iter().any(|m| m.id == "gpt-4"));
        assert!(model_list.iter().any(|m| m.id == "gpt-3.5-turbo"));
        assert!(model_list.iter().all(|m| m.owned_by == "openai"));
    }
}
