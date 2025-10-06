use std::{
    convert::Infallible,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use axum::{
    Router,
    body::{Body, Bytes},
    extract::State,
    response::{IntoResponse, Response, Sse},
    routing::{MethodFilter, on_service, post},
};
use eventsource_stream::Eventsource as _;
use futures::TryStreamExt as _;
use http_body_util::BodyExt;
use reqwest::Url;
use serde::de::DeserializeOwned;
use tower::Service;

use crate::{
    http_client::http_client,
    protocol::anthropic::{error, messages},
    proxy::utils::headers::{insert_proxied_headers_and_content_accept_into, insert_proxied_headers_into},
    request::RequestContext,
};

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/";

pub fn router(base_path: &str) -> Router<()> {
    let proxy = Proxy(Arc::new(ProxyInner {
        base_path: base_path.to_string(),
        anthropic_base_url: Url::parse(ANTHROPIC_API_URL).expect("Invalid Anthropic API URL"),
        client: http_client(),
    }));

    let mut router = Router::new().route("/v1/messages", post(v1_messages));

    // TODO: Batch messages API.

    let forward = ForwardService::new(proxy.clone());
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
        router = router.route(route, on_service(method, forward.clone()));
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

async fn v1_messages(
    State(proxy): State<Proxy>,
    Extract(ctx, payload, bytes): Extract<messages::Request>,
) -> Result<impl IntoResponse, Response> {
    let mut url = proxy.anthropic_base_url.join("v1/messages").unwrap();
    url.set_query(ctx.parts.uri.query());

    let stream = payload.stream.unwrap_or_default();

    let mut response = insert_proxied_headers_into(proxy.client.post(url), ctx.headers())
        .header(http::header::CONTENT_TYPE, "application/json")
        .header(http::header::CONTENT_LENGTH, bytes.len())
        // We're not modifying the bytes (yet) so we just forward the original bytes.
        .body(bytes)
        .send()
        .await
        .map_err(|err| {
            log::error!("Failed to send request to Anthropic: {err}");
            internal_server_error("Could not connect to upstream API")
        })?;

    let status = response.status();
    let headers = std::mem::take(response.headers_mut());

    if stream {
        let byte_stream = response.bytes_stream();
        let event_stream = byte_stream.eventsource();

        let stream = event_stream.map_ok(|event| {
            let evt: messages::StreamEvent = sonic_rs::from_str(&event.data).unwrap();
            (event, evt)
        });

        let stream = stream.map_ok(|(raw, _)| {
            let event = axum::response::sse::Event::default()
                .data(raw.data)
                .event(raw.event)
                .id(raw.id);
            if let Some(retry) = raw.retry {
                event.retry(retry)
            } else {
                event
            }
        });

        Ok((status, headers, Sse::new(stream)).into_response())
    } else {
        let bytes = response.bytes().await.map_err(|err| {
            log::error!("Failed to read response from Anthropic: {err}");
            internal_server_error("Could not read response from upstream API")
        })?;
        let _response: messages::Response = sonic_rs::from_slice(&bytes).map_err(|err| {
            log::error!("Failed to parse response from Anthropic: {err}",);
            log::debug!("Original message:\n{}", String::from_utf8_lossy(&bytes));
            internal_server_error("Could not parse response from upstream API")
        })?;

        Ok((status, headers, bytes).into_response())
    }
}

#[derive(Clone, Debug)]
struct ForwardService {
    proxy: Proxy,
}

impl ForwardService {
    fn new(proxy: Proxy) -> Self {
        Self { proxy }
    }
}

impl Service<axum::extract::Request> for ForwardService {
    type Response = Response;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: axum::extract::Request) -> Self::Future {
        let proxy = self.proxy.clone();
        Box::pin(async move {
            let path = request
                .uri()
                .path()
                .strip_prefix(&proxy.base_path)
                .expect("Invalid URL, should not be reachable.");
            let mut url = proxy
                .anthropic_base_url
                .join(path.strip_prefix('/').unwrap_or(path))
                .unwrap();
            url.set_query(request.uri().query());

            let response = insert_proxied_headers_and_content_accept_into(proxy.client.get(url), request.headers())
                .body(reqwest::Body::wrap_stream(request.into_data_stream()))
                .send()
                .await;

            let response = match response {
                Ok(response) => http::Response::from(response).into_response(),
                Err(err) => {
                    log::error!("Failed to send request to Anthropic: {err}");
                    internal_server_error("Could not connect to upstream API")
                }
            };

            Ok(response)
        })
    }
}

fn internal_server_error(message: &str) -> Response {
    (
        http::StatusCode::INTERNAL_SERVER_ERROR,
        axum::Json(messages::Response::from(error::Error::api_error(message))),
    )
        .into_response()
}

fn bad_request_error(message: impl Into<String>) -> Response {
    (
        http::StatusCode::BAD_REQUEST,
        axum::Json(messages::Response::from(error::Error::invalid_request_error(message))),
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
