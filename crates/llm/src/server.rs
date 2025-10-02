mod builder;
mod handler;
mod metrics;
mod model_discovery;
mod service;
mod tracing;

pub(crate) use builder::LlmServerBuilder;
pub(crate) use handler::Server;
use model_discovery::ModelMap;
pub(crate) use service::LlmService;

use std::{fmt, sync::Arc};

use config::LlmConfig;
use futures::stream::StreamExt;
use itertools::Itertools;
use rate_limit::{TokenRateLimitManager, TokenRateLimitRequest};
use tokio::sync::watch;

use crate::{
    error::LlmError,
    messages::{
        anthropic::CountTokensResponse,
        openai::{Model, ModelsResponse, ObjectType},
        unified::{UnifiedRequest, UnifiedResponse},
    },
    provider::{ChatCompletionStream, Provider},
    request::RequestContext,
};

#[derive(Clone)]
pub struct LlmServer {
    shared: Arc<LlmServerInner>,
}

pub(crate) struct LlmServerInner {
    /// Live provider handles that service requests.
    pub(crate) providers: Arc<Vec<Box<dyn Provider>>>,
    /// Resolved configuration snapshot used for routing and limits.
    pub(crate) config: Arc<LlmConfig>,
    /// Optional token rate limiter shared across providers.
    pub(crate) token_rate_limiter: Option<TokenRateLimitManager>,
    /// Watch channel receiver for discovered models.
    model_map: watch::Receiver<ModelMap>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModelRouteSource {
    LegacyPrefix,
    Discovery,
}

struct ResolvedModelRoute<'providers, 'model> {
    providers: &'providers [Box<dyn Provider>],
    provider_index: usize,
    model_name: &'model str,
    source: ModelRouteSource,
}

impl<'providers, 'model> ResolvedModelRoute<'providers, 'model> {
    fn provider(&self) -> &dyn Provider {
        self.providers[self.provider_index].as_ref()
    }

    fn provider_name(&self) -> &str {
        self.provider().name()
    }
}

impl fmt::Debug for ResolvedModelRoute<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResolvedModelRoute")
            .field("provider", &self.provider_name())
            .field("model_name", &self.model_name)
            .field("source", &self.source)
            .finish()
    }
}

impl LlmServer {
    /// Check token rate limits for a request.
    ///
    /// Returns the duration to wait before retrying if rate limited, or None if the request can proceed.
    async fn check_token_rate_limit(
        &self,
        request: &UnifiedRequest,
        context: &RequestContext,
        route: &ResolvedModelRoute<'_, '_>,
    ) -> Option<std::time::Duration> {
        // Check if client identification is available
        let Some(ref client_identity) = context.client_identity else {
            log::debug!(
                "No client_id found in request context. \
                Token rate limiting requires client identification to be enabled and a client_id to be present."
            );
            return None;
        };

        let provider_name = route.provider_name();

        log::debug!(
            "Checking token rate limit for client_id={}, group={:?}, model={}, provider={}, route_source={:?}",
            client_identity.client_id,
            client_identity.group,
            route.model_name,
            provider_name,
            route.source
        );

        // Get provider config
        let provider_config = self.shared.config.providers.get(provider_name)?;

        // Get model config if it exists
        let models = provider_config.models();
        let model_config = models.get(route.model_name);

        // Check rate limit if token rate limiter is configured
        let Some(ref token_rate_limiter) = self.shared.token_rate_limiter else {
            log::debug!(
                "Token rate limiter not initialized - no providers have token rate limits configured. \
                Allowing request without token rate limiting."
            );
            return None;
        };

        // Gather provider and model rate limit configurations
        let (provider_limits, model_limits) = (
            provider_config.rate_limits(),
            model_config.and_then(|m| m.rate_limits()),
        );

        // Count request tokens (input only, no output buffering)
        let input_tokens = crate::token_counter::count_input_tokens(request);

        log::debug!("Token accounting: input={input_tokens} (output tokens not counted for rate limiting)",);

        // Create token rate limit request
        let token_request = TokenRateLimitRequest {
            client_id: client_identity.client_id.clone(),
            group: client_identity.group.clone(),
            provider: provider_name.to_string(),
            model: Some(route.model_name.to_string()),
            input_tokens,
        };

        match token_rate_limiter
            .check_request(&token_request, provider_limits, model_limits)
            .await
        {
            Ok(duration) => duration,
            Err(e) => {
                log::error!("Error checking token rate limit: {e}");
                None
            }
        }
    }

    /// Get a provider by name.
    fn resolve_model_route<'a>(&'a self, requested_model: &'a str) -> crate::Result<ResolvedModelRoute<'a, 'a>> {
        if let Some((provider_name, model_name)) = requested_model.split_once('/') {
            if model_name.is_empty() {
                return Err(LlmError::InvalidModelFormat(requested_model.to_string()));
            }

            let Some(provider_index) = self
                .shared
                .providers
                .iter()
                .position(|provider| provider.name() == provider_name)
            else {
                log::debug!(
                    "Provider '{provider_name}' not found. Available providers: [{providers}]",
                    providers = self.shared.providers.iter().map(|p| p.name()).join(", ")
                );

                return Err(LlmError::ProviderNotFound(provider_name.to_string()));
            };

            return Ok(ResolvedModelRoute {
                providers: self.shared.providers.as_slice(),
                provider_index,
                model_name,
                source: ModelRouteSource::LegacyPrefix,
            });
        }

        let model_map = self.shared.model_map.borrow().clone();

        let Some(model_info) = model_map.get(requested_model) else {
            log::debug!("Model '{requested_model}' not found in discovery map");
            return Err(LlmError::ModelNotFound(format!("Model '{requested_model}' not found")));
        };

        let Some(provider_index) = self
            .shared
            .providers
            .iter()
            .position(|provider| provider.name() == model_info.provider_name)
        else {
            log::error!(
                "Model '{requested_model}' mapped to unknown provider '{provider}'",
                provider = model_info.provider_name
            );

            return Err(LlmError::ProviderNotFound(model_info.provider_name.clone()));
        };

        Ok(ResolvedModelRoute {
            providers: self.shared.providers.as_slice(),
            provider_index,
            model_name: requested_model,
            source: ModelRouteSource::Discovery,
        })
    }

    /// Check rate limits and return an error if exceeded.
    async fn check_and_enforce_rate_limit(
        &self,
        request: &UnifiedRequest,
        context: &RequestContext,
        route: &ResolvedModelRoute<'_, '_>,
    ) -> crate::Result<()> {
        if let Some(wait_duration) = self.check_token_rate_limit(request, context, route).await {
            // Duration::MAX is used as a sentinel value to indicate the request can never succeed
            // (requires more tokens than the rate limit allows)
            if wait_duration == std::time::Duration::MAX {
                log::debug!("Request requires more tokens than rate limit allows - cannot be fulfilled");

                return Err(LlmError::RateLimitExceeded {
                    message: "Token rate limit exceeded. Request requires more tokens than the configured limit allows and cannot be fulfilled.".to_string(),
                });
            } else {
                log::debug!("Request rate limited, need to wait {wait_duration:?}");

                return Err(LlmError::RateLimitExceeded {
                    message: "Token rate limit exceeded. Please try again later.".to_string(),
                });
            }
        }
        Ok(())
    }
}

impl LlmService for LlmServer {
    async fn models(&self) -> ModelsResponse {
        let model_map = self.shared.model_map.borrow().clone();

        let mut discovered = Vec::new();
        let mut explicit = Vec::new();

        for (id, info) in model_map.iter() {
            let model = Model {
                id: id.clone(),
                object: ObjectType::Model,
                created: info.created,
                owned_by: info.owned_by.clone(),
            };

            if id.contains('/') {
                explicit.push(model);
            } else {
                discovered.push(model);
            }
        }

        discovered.sort_by(|a, b| a.id.cmp(&b.id));
        explicit.sort_by(|a, b| a.id.cmp(&b.id));
        discovered.extend(explicit);

        let data = discovered;

        ModelsResponse {
            object: ObjectType::List,
            data,
        }
    }

    async fn completions(&self, request: UnifiedRequest, context: &RequestContext) -> crate::Result<UnifiedResponse> {
        // Resolve routing for the requested model
        let original_model = request.model.clone();
        let route = self.resolve_model_route(&original_model)?;

        // Check token rate limits first
        self.check_and_enforce_rate_limit(&request, context, &route).await?;

        let provider = self.shared.providers[route.provider_index].as_ref();

        // Create a modified request with the routed model name
        let mut modified_request = request;
        modified_request.model = route.model_name.to_string();

        // Call provider with unified types directly
        let unified_response = provider.chat_completion(modified_request, context).await?;

        // Restore the full model name with provider prefix in the response
        let mut final_response = unified_response;
        final_response.model = original_model;

        Ok(final_response)
    }

    async fn completions_stream(
        &self,
        request: UnifiedRequest,
        context: &RequestContext,
    ) -> crate::Result<ChatCompletionStream> {
        // Resolve routing for the requested model
        let original_model = request.model.clone();
        let route = self.resolve_model_route(&original_model)?;

        // Check token rate limits first
        self.check_and_enforce_rate_limit(&request, context, &route).await?;

        let provider = self.shared.providers[route.provider_index].as_ref();

        // Check if provider supports streaming
        if !provider.supports_streaming() {
            let provider_name = route.provider_name();
            log::debug!("Provider '{provider_name}' does not support streaming");
            return Err(LlmError::StreamingNotSupported);
        }

        // Create a modified request with the stripped model name
        let mut modified_request = request;
        modified_request.model = route.model_name.to_string();

        // Get the stream from the provider
        let stream = provider.chat_completion_stream(modified_request, context).await?;

        // Transform the stream to restore the full model name with prefix
        let transformed_stream = stream.map(move |chunk_result| {
            chunk_result.map(|mut chunk| {
                // Restore the full model name with provider prefix
                chunk.model = original_model.clone().into();
                chunk
            })
        });

        Ok(Box::pin(transformed_stream))
    }

    async fn count_tokens(
        &self,
        request: UnifiedRequest,
        context: &RequestContext,
    ) -> crate::Result<CountTokensResponse> {
        let original_model = request.model.clone();
        let route = self.resolve_model_route(&original_model)?;

        let mut modified_request = request;
        modified_request.model = route.model_name.to_string();

        route.provider().count_tokens(modified_request, context).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use indoc::indoc;
    use insta::assert_debug_snapshot;
    use std::{collections::BTreeMap, sync::Arc};
    use tokio::sync::watch;

    use super::model_discovery::ModelInfo;

    struct DummyProvider {
        name: String,
    }

    impl DummyProvider {
        fn new(name: impl Into<String>) -> Self {
            Self { name: name.into() }
        }
    }

    #[async_trait]
    impl Provider for DummyProvider {
        async fn chat_completion(
            &self,
            _request: crate::messages::unified::UnifiedRequest,
            _context: &crate::request::RequestContext,
        ) -> crate::Result<crate::messages::unified::UnifiedResponse> {
            Err(crate::error::LlmError::InternalError(None))
        }

        async fn chat_completion_stream(
            &self,
            _request: crate::messages::unified::UnifiedRequest,
            _context: &crate::request::RequestContext,
        ) -> crate::Result<crate::provider::ChatCompletionStream> {
            Err(crate::error::LlmError::StreamingNotSupported)
        }

        async fn list_models(&self) -> anyhow::Result<Vec<crate::messages::openai::Model>> {
            Ok(Vec::new())
        }

        fn name(&self) -> &str {
            &self.name
        }

        fn supports_streaming(&self) -> bool {
            false
        }
    }

    fn build_test_server(toml: &str, models: BTreeMap<String, ModelInfo>) -> LlmServer {
        let config: LlmConfig = toml::from_str(toml).expect("valid LLM config");
        let provider_names: Vec<String> = config.providers.keys().cloned().collect();

        let providers: Vec<Box<dyn Provider>> = provider_names
            .iter()
            .map(|name| Box::new(DummyProvider::new(name.clone())) as Box<dyn Provider>)
            .collect();

        let providers = Arc::new(providers);
        let config = Arc::new(config);

        let (sender, receiver) = watch::channel::<ModelMap>(Arc::new(models));
        drop(sender);

        LlmServer {
            shared: Arc::new(LlmServerInner {
                providers,
                config,
                token_rate_limiter: None,
                model_map: receiver,
            }),
        }
    }

    #[test]
    fn routes_prefixed_models_using_legacy_format() {
        let server = build_test_server(
            indoc! {r#"
                [providers.openai]
                type = "openai"
                api_key = "test"
                model_filter = ".*"
            "#},
            BTreeMap::new(),
        );

        let route = server
            .resolve_model_route("openai/gpt-4o-mini")
            .expect("route should resolve");

        assert_debug_snapshot!((&route.provider_name(), route.model_name, route.source), @r###"
        (
            "openai",
            "gpt-4o-mini",
            LegacyPrefix,
        )
        "###);
    }

    #[test]
    fn routes_models_using_discovery_map() {
        let mut models = BTreeMap::new();
        models.insert(
            "gpt-4o-mini".to_string(),
            ModelInfo {
                provider_name: "openai".to_string(),
                created: 0,
                owned_by: "openai".to_string(),
                display_name: None,
            },
        );

        let server = build_test_server(
            indoc! {r#"
                [providers.openai]
                type = "openai"
                api_key = "test"
                model_filter = ".*"
            "#},
            models,
        );

        let route = server.resolve_model_route("gpt-4o-mini").expect("route should resolve");

        assert_debug_snapshot!((&route.provider_name(), route.model_name, route.source), @r###"
        (
            "openai",
            "gpt-4o-mini",
            Discovery,
        )
        "###);
    }

    #[test]
    fn returns_error_when_model_cannot_be_resolved() {
        let server = build_test_server(
            indoc! {r#"
                [providers.openai]
                type = "openai"
                api_key = "test"
                model_filter = ".*"
            "#},
            BTreeMap::new(),
        );

        let error = server
            .resolve_model_route("unknown-model")
            .expect_err("route resolution should fail");

        let message = error.to_string();
        insta::assert_snapshot!(message, @r###"Model 'unknown-model' not found"###);
    }
}
