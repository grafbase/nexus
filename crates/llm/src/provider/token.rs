use secrecy::SecretString;

use crate::{error::LlmError, request::RequestContext};

/// Generic helper to get API key based on token forwarding configuration.
///
/// This function implements the token forwarding logic consistently across all providers:
/// 1. If token forwarding is enabled and context has a key, use it
/// 2. Otherwise, use the configured fallback key if available
/// 3. Return error if no key is available
pub(super) fn get<'a>(
    forward_token_enabled: bool,
    configured_key: &'a Option<SecretString>,
    context: &'a RequestContext,
) -> crate::Result<&'a SecretString> {
    // Check if token forwarding is enabled
    if forward_token_enabled {
        // First try user-provided key
        if let Some(api_key) = &context.api_key {
            return Ok(api_key);
        }

        // Fall back to configured key
        if let Some(api_key) = configured_key.as_ref() {
            return Ok(api_key);
        }

        // No key available
        Err(LlmError::AuthenticationFailed(
            "Token forwarding is enabled but no API key was provided".to_string(),
        ))
    } else if let Some(api_key) = configured_key {
        // Token forwarding disabled - use configured key only
        Ok(api_key)
    } else {
        // No configured key
        Err(LlmError::AuthenticationFailed("No API key configured".to_string()))
    }
}
