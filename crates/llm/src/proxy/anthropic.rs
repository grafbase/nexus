mod forward;
mod v1_messages;

use std::sync::Arc;

use axum::{
    Router,
    body::{Body, Bytes},
    response::{IntoResponse, Response},
    routing::{MethodFilter, on_service, post},
};
use reqwest::Url;
use serde::de::DeserializeOwned;
use tower::Service as _;

use crate::{http_client::http_client, protocol::anthropic as protocol, request::RequestContext};

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/";

type ProxyError = (http::StatusCode, protocol::error::Error);

pub fn router(base_path: &str) -> Router<()> {
    let proxy = Proxy(Arc::new(ProxyInner {
        base_path: base_path.to_string(),
        anthropic_base_url: Url::parse(ANTHROPIC_API_URL).expect("Invalid Anthropic API URL"),
        client: http_client(),
    }));

    let mut router = Router::new().route(
        "/v1/messages",
        post({
            let mut handler = v1_messages::Handler(proxy.clone());
            move |req| handler.call(req)
        }),
    );

    // TODO: Batch messages API.

    for (method, route) in [
        // Messages count tokens (doesn't involve any LLM)
        (MethodFilter::POST, "/v1/messages/count_tokens"),
        // Models
        (MethodFilter::GET, "/v1/models"),
        (MethodFilter::GET, "/v1/models/{model_id}"),
        // Files
        (MethodFilter::POST, "/v1/files"),
        (MethodFilter::GET, "/v1/files"),
        (MethodFilter::GET, "/v1/files/{file_id}"),
        (MethodFilter::GET, "/v1/files/{file_id}/content"),
        (MethodFilter::DELETE, "/v1/files/{file_id}"),
    ] {
        router = router.route(route, on_service(method, proxy.clone()));
    }

    Router::new().nest(base_path, router.with_state(proxy))
}

#[derive(Clone, Debug)]
struct Proxy(Arc<ProxyInner>);

impl std::ops::Deref for Proxy {
    type Target = ProxyInner;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug)]
pub struct ProxyInner {
    base_path: String,
    anthropic_base_url: Url,
    client: reqwest::Client,
}

fn internal_server_error(message: &str) -> Response {
    (
        http::StatusCode::INTERNAL_SERVER_ERROR,
        axum::Json(protocol::messages::Response::from(protocol::error::Error::api_error(
            message,
        ))),
    )
        .into_response()
}

fn bad_request_error(message: impl Into<String>) -> Response {
    (
        http::StatusCode::BAD_REQUEST,
        axum::Json(protocol::messages::Response::from(
            protocol::error::Error::invalid_request_error(message),
        )),
    )
        .into_response()
}

// It's very easy to inadvertently clone the headers and other parts of the request with axum extractors and
// it also hides the implicit body limit.
struct Extract<T>(pub RequestContext, pub T, pub Bytes);

// TODO: Should come from the state which should expose the config.
const BODY_LIMIT_BYTES: usize = 32 << 20; // 32 MiB like Anthropic

impl<S, T: DeserializeOwned> axum::extract::FromRequest<S> for Extract<T>
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request(request: http::Request<Body>, _state: &S) -> Result<Self, Self::Rejection> {
        let (mut parts, body) = request.into_parts();

        static APPLICATION_JSON: http::HeaderValue = http::HeaderValue::from_static("application/json");
        if parts
            .headers
            .get(http::header::CONTENT_TYPE)
            .is_none_or(|value| value != APPLICATION_JSON)
        {
            return Err(bad_request_error(
                "Unsupported Content-Type, expected: 'Content-Type: application/json'",
            ));
        }

        let bytes = axum::body::to_bytes(body, BODY_LIMIT_BYTES).await.map_err(|err| {
            let source = std::error::Error::source(&err).unwrap();
            if source.is::<http_body_util::LengthLimitError>() {
                bad_request_error(format!(
                    "Request body is too large, limit is {} bytes",
                    BODY_LIMIT_BYTES
                ))
            } else {
                bad_request_error(format!("Failed to read request body: {err}"))
            }
        })?;

        let body = sonic_rs::from_slice(&bytes)
            .map_err(|err| bad_request_error(format!("Failed to parse request body: {}", err)))?;

        let ctx = RequestContext {
            api_key: None,
            client_identity: parts.extensions.remove(),
            authentication: parts
                .extensions
                .remove()
                .expect("Authentication must be provided by a parent layer."),
            parts,
        };

        Ok(Extract(ctx, body, bytes))
    }
}
