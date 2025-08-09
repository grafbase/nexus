use axum::http::HeaderMap;
use secrecy::SecretString;

/// Header name for user-provided API keys (BYOK - Bring Your Own Key).
const PROVIDER_API_KEY_HEADER: &str = "X-Provider-API-Key";

/// Runtime context for provider requests.
///
/// This struct carries runtime information that may override provider configuration,
/// such as user-provided API keys for BYOK (Bring Your Own Key) support.
#[derive(Debug, Clone, Default)]
pub(crate) struct RequestContext {
    /// User-provided API key that overrides the configured key.
    /// Only used when BYOK is enabled for the provider.
    pub api_key: Option<SecretString>,
}

/// Extract request context from request headers.
///
/// Looks for the X-Provider-API-Key header which contains the user's API key
/// for BYOK (Bring Your Own Key) support.
pub(super) fn extract_context(headers: &HeaderMap) -> RequestContext {
    // Check for BYOK header
    let api_key = headers
        .get(PROVIDER_API_KEY_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(|key| SecretString::from(key.to_string()));

    RequestContext { api_key }
}
