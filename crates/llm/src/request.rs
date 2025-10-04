use axum::{body::Body, http::HeaderMap, response::IntoResponse as _};
use context::{Authentication, ClientIdentity};
use secrecy::SecretString;
use serde::de::DeserializeOwned;

/// Header name for user-provided API keys (BYOK - Bring Your Own Key).
const PROVIDER_API_KEY_HEADER: &str = "X-Provider-API-Key";

/// Runtime context for provider requests.
///
/// This struct carries runtime information that may override provider configuration,
/// such as user-provided API keys for BYOK (Bring Your Own Key) support,
/// client identity information for rate limiting, and incoming request headers
/// for header transformation rules.
#[derive(Debug, Clone)]
pub(crate) struct RequestContext {
    pub parts: http::request::Parts,

    /// User-provided API key that overrides the configured key.
    /// Only used when BYOK is enabled for the provider.
    pub api_key: Option<SecretString>,

    /// Client identity information for rate limiting and access control.
    pub client_identity: Option<ClientIdentity>,

    #[allow(dead_code)]
    pub authentication: Authentication,
}

impl RequestContext {
    pub fn headers(&self) -> &HeaderMap {
        &self.parts.headers
    }
}

// It's very easy to inadvertently clone the headers and other parts of the request with axum extractors and
// it also hides the implicit body limit.
pub struct ExtractPayload<T>(pub RequestContext, pub T);

// TODO: Should come from the state which should expose the config.
const BODY_LIMIT_BYTES: usize = 32 << 20; // 32 MiB like Anthropic

impl<S, T: DeserializeOwned> axum::extract::FromRequest<S> for ExtractPayload<T>
where
    S: Send + Sync,
{
    type Rejection = axum::response::Response;

    async fn from_request(request: http::Request<Body>, _state: &S) -> Result<Self, Self::Rejection> {
        let (mut parts, body) = request.into_parts();

        static APPLICATION_JSON: http::HeaderValue = http::HeaderValue::from_static("application/json");
        if parts
            .headers
            .get(http::header::CONTENT_TYPE)
            .is_none_or(|value| value != APPLICATION_JSON)
        {
            return Err((
                axum::http::StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Unsupported Content-Type, expected: 'Content-Type: application/json'",
            )
                .into_response());
        }

        let bytes = axum::body::to_bytes(body, BODY_LIMIT_BYTES).await.map_err(|err| {
            let source = std::error::Error::source(&err).unwrap();
            if source.is::<http_body_util::LengthLimitError>() {
                (
                    axum::http::StatusCode::PAYLOAD_TOO_LARGE,
                    format!("Request body is too large, limit is {} bytes", BODY_LIMIT_BYTES),
                )
            } else {
                (
                    axum::http::StatusCode::BAD_REQUEST,
                    format!("Failed to read request body: {}", err),
                )
            }
            .into_response()
        })?;

        let body = match sonic_rs::from_slice::<T>(&bytes) {
            Ok(body) => body,
            Err(e) => {
                return Err((
                    axum::http::StatusCode::BAD_REQUEST,
                    format!("Failed to parse request body: {}", e),
                )
                    .into_response());
            }
        };

        let ctx = RequestContext {
            api_key: parts
                .headers
                .get(PROVIDER_API_KEY_HEADER)
                .and_then(|value| value.to_str().map(str::to_string).ok())
                .map(SecretString::from),
            client_identity: parts.extensions.remove(),
            authentication: parts
                .extensions
                .remove()
                .expect("Authentication must be provided by a parent layer."),
            parts,
        };

        Ok(ExtractPayload(ctx, body))
    }
}
