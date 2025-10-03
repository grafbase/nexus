pub(crate) mod proxy;

use axum::http;
use secrecy::{ExposeSecret, SecretString};

use crate::{LlmError, request::RequestContext};

pub(crate) enum SupportedProviderMode {
    AnthropicProxy,
    RouterWithClientKey(http::HeaderName),
    RouterWithOwnKey(SecretString),
}

pub(crate) enum ProviderMode {
    Proxy,
    RouterWithClientApiKey(http::header::HeaderValue),
    RouterWithOwnedApiKey(http::header::HeaderValue),
}

impl ProviderMode {
    pub(crate) fn determine(ctx: &RequestContext, supported: &[SupportedProviderMode]) -> crate::Result<ProviderMode> {
        if ctx.authentication.has_anthropic_authorization {
            return supported
                .iter()
                .any(|m| matches!(m, SupportedProviderMode::AnthropicProxy))
                .then_some(ProviderMode::Proxy)
                .ok_or_else(|| {
                    LlmError::InvalidRequest("Provider does not support Anthropic token forwarding".to_string())
                });
        }

        if let Some(value) = supported
            .iter()
            .filter_map(|mode| match mode {
                SupportedProviderMode::RouterWithClientKey(name) => ctx.headers().get(name),
                _ => None,
            })
            .next()
        {
            let mut value = value.clone();
            value.set_sensitive(true);
            return Ok(ProviderMode::RouterWithClientApiKey(value));
        }

        supported
            .iter()
            .filter_map(|mode| match mode {
                SupportedProviderMode::RouterWithOwnKey(key) => Some(key),
                _ => None,
            })
            .next()
            .map(|key| {
                let mut value = http::HeaderValue::from_str(key.expose_secret()).expect("Valid secret API key");
                value.set_sensitive(true);
                ProviderMode::RouterWithOwnedApiKey(value)
            })
            .ok_or_else(|| LlmError::AuthenticationFailed("No API key was provided nor configured.".to_string()))
    }
}
