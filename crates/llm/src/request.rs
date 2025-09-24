use axum::http::HeaderMap;
use config::ClientIdentity;
use secrecy::SecretString;

/// Header name for user-provided API keys (BYOK - Bring Your Own Key).
const PROVIDER_API_KEY_HEADER: &str = "X-Provider-API-Key";

/// Runtime context for provider requests.
///
/// This struct carries runtime information that may override provider configuration,
/// such as user-provided API keys for BYOK (Bring Your Own Key) support,
/// client identity information for rate limiting, and incoming request headers
/// for header transformation rules.
#[derive(Debug, Clone, Default)]
pub(crate) struct RequestContext {
    /// User-provided API key that overrides the configured key.
    /// Only used when BYOK is enabled for the provider.
    pub api_key: Option<SecretString>,

    /// Client identity information for rate limiting and access control.
    pub client_identity: Option<ClientIdentity>,

    /// Incoming request headers for header transformation rules.
    pub headers: HeaderMap,
}

/// Extract request context from request headers and client identity.
///
/// Combines runtime information from headers (like BYOK API keys) with
/// client identity information for rate limiting and access control.
pub(super) fn extract_context(headers: &HeaderMap, client_identity: Option<ClientIdentity>) -> RequestContext {
    // Check for BYOK header
    let api_key = headers
        .get(PROVIDER_API_KEY_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(|key| SecretString::from(key.to_string()));

    RequestContext {
        api_key,
        client_identity,
        headers: headers.clone(),
    }
}
