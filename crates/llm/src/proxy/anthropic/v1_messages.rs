use std::{convert::Infallible, task::Poll};

use axum::{
    body::Bytes,
    response::{IntoResponse, Sse},
};
use eventsource_stream::Eventsource as _;
use futures::{FutureExt as _, StreamExt as _, TryStreamExt as _, future::BoxFuture, stream::BoxStream};
use http::StatusCode;
use tower::Service;

use crate::{
    protocol::anthropic::{self as protocol, error::Error},
    proxy::{anthropic::Proxy, utils::headers::insert_proxied_headers_into},
    request::RequestContext,
};

use super::Extract;

#[derive(Clone)]
pub(super) struct Handler<S>(pub S);

impl<S> Service<Extract<protocol::messages::Request>> for Handler<S>
where
    S: Service<Request, Response = Response, Error = super::ProxyError, Future: Send + 'static>,
    S: Service<StreamRequest, Response = StreamResponse, Error = super::ProxyError, Future: Send + 'static>,
{
    type Response = axum::response::Response;
    type Error = Infallible;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
    fn call(&mut self, Extract(ctx, payload, bytes): Extract<protocol::messages::Request>) -> Self::Future {
        if payload.stream.unwrap_or_default() {
            self.0
                .call(StreamRequest { ctx, payload, bytes })
                .map(|result| match result {
                    Ok(StreamResponse {
                        status_code,
                        headers,
                        event_stream,
                        ..
                    }) => {
                        let event_stream = event_stream.map(|result: Event| match result {
                            Ok((event, _)) => {
                                let out = axum::response::sse::Event::default()
                                    .data(event.data)
                                    .event(event.event)
                                    .id(event.id);
                                Result::<_, Infallible>::Ok(if let Some(retry) = event.retry {
                                    out.retry(retry)
                                } else {
                                    out
                                })
                            }
                            Err(error) => Ok(axum::response::sse::Event::default()
                                .event("error")
                                .json_data(&protocol::messages::StreamEvent::Error { error })
                                .unwrap()),
                        });
                        Ok((status_code, headers, Sse::new(event_stream)).into_response())
                    }
                    Err((status_code, error)) => {
                        Ok((status_code, axum::Json(protocol::messages::Response::from(error))).into_response())
                    }
                })
                .boxed()
        } else {
            self.0
                .call(Request { ctx, payload, bytes })
                .map(|result| match result {
                    Ok(Response {
                        status_code,
                        headers,
                        bytes,
                        ..
                    }) => Ok((status_code, headers, bytes).into_response()),
                    Err((status_code, error)) => {
                        Ok((status_code, axum::Json(protocol::messages::Response::from(error))).into_response())
                    }
                })
                .boxed()
        }
    }
}

pub struct Request {
    pub ctx: RequestContext,
    pub payload: protocol::messages::Request,
    pub bytes: Bytes,
}

pub struct Response {
    #[allow(dead_code)]
    pub request: Request,
    pub status_code: http::StatusCode,
    pub headers: http::HeaderMap,
    pub bytes: Bytes,
    pub payload: Option<protocol::messages::Response>,
}

impl Service<Request> for super::Proxy {
    type Response = Response;

    type Error = super::ProxyError;

    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request) -> Self::Future {
        debug_assert!(
            !request.payload.stream.unwrap_or_default(),
            "Stream request should use a different service."
        );

        let proxy = self.clone();
        Box::pin(async move {
            let mut response = forward(&proxy, &request.ctx, request.bytes.clone()).await?;

            let status_code = response.status();
            let headers = std::mem::take(response.headers_mut());

            let bytes = response.bytes().await.map_err(|err| {
                log::error!("Failed to read response from Anthropic: {err}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Error::api_error("Could not read response from upstream API"),
                )
            })?;

            let mut response = Response {
                request,
                status_code,
                headers,
                bytes,
                payload: None,
            };

            match sonic_rs::from_slice(&response.bytes) {
                Ok(payload) => {
                    response.payload = payload;
                }
                Err(err) => {
                    // If not a server error, we don't know how to interpret the result.
                    if !response.status_code.is_server_error() {
                        log::error!("Failed to parse response from Anthropic: {err}",);
                        log::debug!("Original message:\n{}", String::from_utf8_lossy(&response.bytes));
                        return Err((
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Error::api_error("Could not parse response from Anthropic API"),
                        ));
                    }
                }
            }

            Ok(response)
        })
    }
}

pub struct StreamRequest {
    pub ctx: RequestContext,
    pub payload: protocol::messages::Request,
    pub bytes: Bytes,
}

pub type Event = Result<(eventsource_stream::Event, protocol::messages::StreamEvent), Error>;

pub struct StreamResponse {
    #[allow(dead_code)]
    pub request: StreamRequest,
    pub status_code: http::StatusCode,
    pub headers: http::HeaderMap,
    pub event_stream: BoxStream<'static, Event>,
}

impl Service<StreamRequest> for super::Proxy {
    type Response = StreamResponse;

    type Error = super::ProxyError;

    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: StreamRequest) -> Self::Future {
        debug_assert!(request.payload.stream.unwrap_or_default(), "Must be a stream request");

        let proxy = self.clone();
        Box::pin(async move {
            let mut response = forward(&proxy, &request.ctx, request.bytes.clone()).await?;

            let status_code = response.status();
            let headers = std::mem::take(response.headers_mut());
            let event_stream = response
                .bytes_stream()
                .eventsource()
                .map_err(|err| {
                    log::error!("Failed to parse stream event from Anthropic: {err}");
                    Error::api_error("Invalid response from Anthropic API: ")
                })
                .and_then(
                    async |event| match sonic_rs::from_str::<protocol::messages::StreamEvent>(&event.data) {
                        Ok(payload) => Ok((event, payload)),
                        Err(err) => {
                            log::error!("Failed to parse stream event from Anthropic: {err}");
                            log::debug!("Original message:\n{}", event.data);
                            Err(Error::api_error("Could not parse stream event from Anthropic API"))
                        }
                    },
                )
                .boxed();

            Ok(StreamResponse {
                request,
                status_code,
                headers,
                event_stream,
            })
        })
    }
}

async fn forward(proxy: &Proxy, ctx: &RequestContext, bytes: Bytes) -> Result<reqwest::Response, (StatusCode, Error)> {
    let mut url = proxy.anthropic_base_url.join("v1/messages").unwrap();
    url.set_query(ctx.parts.uri.query());

    insert_proxied_headers_into(proxy.client.post(url), ctx.headers())
        .header(http::header::CONTENT_TYPE, "application/json")
        .header(http::header::CONTENT_LENGTH, bytes.len())
        // We're not modifying the bytes (yet) so we just forward the original bytes.
        .body(bytes)
        .send()
        .await
        .map_err(|err| {
            log::error!("Failed to send request to Anthropic: {err}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Error::api_error("Could not connect to Anthropic API"),
            )
        })
}
