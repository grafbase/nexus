use std::{borrow::Cow, convert::Infallible, task::Poll};

use axum::{
    body::Bytes,
    response::{IntoResponse, Sse},
};
use eventsource_stream::Eventsource as _;
use futures::{
    FutureExt as _, StreamExt as _, TryFutureExt as _, TryStreamExt as _, future::BoxFuture, stream::BoxStream,
};
use http::StatusCode;
use tower::Service;

use crate::{
    protocol::anthropic::{self as protocol, error::Error},
    proxy::{anthropic::Proxy, utils::headers::insert_proxied_headers_into},
    request::RequestContext,
    telemetry::chat,
};

use super::Extract;

pub(super) struct Axum;

impl<S> tower::Layer<S> for Axum {
    type Service = AxumHandler<S>;
    fn layer(&self, service: S) -> Self::Service {
        AxumHandler(service)
    }
}

#[derive(Clone)]
pub(super) struct AxumHandler<S>(pub S);

impl<S> Service<Extract<protocol::messages::Request>> for AxumHandler<S>
where
    S: Service<Request, Response = Response, Error = super::ProxyError, Future: Send + 'static>,
{
    type Response = axum::response::Response;
    type Error = super::ProxyError;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.0.poll_ready(cx)
    }

    fn call(&mut self, Extract(ctx, payload, bytes): Extract<protocol::messages::Request>) -> Self::Future {
        self.0
            .call(Request { ctx, payload, bytes })
            .map_ok(|response| match response {
                Response::Complete(CompleteResponse {
                    status_code,
                    headers,
                    bytes,
                    ..
                }) => (status_code, headers, bytes).into_response(),
                Response::Stream(StreamResponse {
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
                    (status_code, headers, Sse::new(event_stream)).into_response()
                }
            })
            .boxed()
    }
}

pub struct Request {
    pub ctx: RequestContext,
    pub payload: protocol::messages::Request,
    pub bytes: Bytes,
}

impl chat::Request for Request {
    fn ctx(&self) -> &RequestContext {
        &self.ctx
    }
    fn model(&self) -> &str {
        &self.payload.model
    }
    fn max_tokens(&self) -> Option<u32> {
        Some(self.payload.max_tokens)
    }
    fn temperature(&self) -> Option<f32> {
        self.payload.temperature
    }
    fn provider_name(&self) -> Cow<'static, str> {
        "anthropic".into()
    }
}

pub enum Response {
    Complete(CompleteResponse),
    Stream(StreamResponse),
}

pub struct CompleteResponse {
    #[allow(dead_code)]
    pub request: Request,
    pub status_code: http::StatusCode,
    pub headers: http::HeaderMap,
    pub bytes: Bytes,
    pub payload: Option<protocol::messages::Response>,
}

pub type Event = Result<(eventsource_stream::Event, protocol::messages::StreamEvent), Error>;

pub struct StreamResponse {
    #[allow(dead_code)]
    pub request: Request,
    pub status_code: http::StatusCode,
    pub headers: http::HeaderMap,
    pub event_stream: BoxStream<'static, Event>,
}

impl chat::Response for Response {
    type Complete = CompleteResponse;
    type Stream = StreamResponse;
    fn as_message_or_stream_mut(
        &mut self,
    ) -> Result<&mut <Self as chat::Response>::Complete, &mut <Self as chat::Response>::Stream> {
        match self {
            Response::Complete(c) => Ok(c),
            Response::Stream(s) => Err(s),
        }
    }
}

impl CompleteResponse {
    fn message_payload(&self) -> Option<&protocol::messages::MessageResponse> {
        self.payload.as_ref().and_then(|p| p.as_message())
    }
}

impl chat::Message for CompleteResponse {
    fn error_type(&self) -> Option<&str> {
        match &self.payload {
            Some(protocol::messages::Response::Error(error)) => Some(error.error.r#type.as_str()),
            None => Some(protocol::error::ERROR_TYPE_API),
            _ => None,
        }
    }

    fn id(&self) -> Option<&str> {
        self.message_payload().map(|p| p.id.as_str())
    }

    fn model(&self) -> Option<&str> {
        self.message_payload().map(|p| p.model.as_str())
    }
    fn tokens(&self) -> Option<chat::Tokens> {
        self.message_payload().map(|p| chat::Tokens {
            input: p.usage.input_tokens,
            output: p.usage.output_tokens,
        })
    }
    fn finish_reasons(&self) -> Option<String> {
        self.message_payload().and_then(|p| {
            p.stop_reason
                .as_ref()
                .map(|reason| serde_json::to_string(&[reason]).unwrap())
        })
    }
}

impl chat::StreamResponse for StreamResponse {
    type Event = self::Event;
    fn error_type(&self) -> Option<&str> {
        None
    }
    fn wrap_event_stream(
        &mut self,
        f: impl FnOnce(BoxStream<'static, Self::Event>) -> BoxStream<'static, Self::Event>,
    ) {
        self.event_stream = f(std::mem::replace(
            &mut self.event_stream,
            Box::pin(futures::stream::empty()),
        ));
    }
}

impl chat::Message for Event {
    fn error_type(&self) -> Option<&str> {
        match self {
            Ok((_, protocol::messages::StreamEvent::Error { error })) => Some(error.r#type.as_str()),
            Err(error) => Some(error.r#type.as_str()),
            _ => None,
        }
    }

    fn model(&self) -> Option<&str> {
        match self {
            Ok((_, protocol::messages::StreamEvent::MessageStart(msg))) => Some(msg.model.as_str()),
            _ => None,
        }
    }

    fn id(&self) -> Option<&str> {
        match self {
            Ok((_, protocol::messages::StreamEvent::MessageStart(msg))) => Some(msg.id.as_str()),
            _ => None,
        }
    }

    fn tokens(&self) -> Option<chat::Tokens> {
        match self {
            Ok((_, event)) => match event {
                protocol::messages::StreamEvent::MessageStart(msg) => Some(&msg.usage),
                protocol::messages::StreamEvent::MessageDelta(msg) => msg.usage.as_ref(),
                _ => None,
            },
            _ => None,
        }
        .map(|usage| chat::Tokens {
            input: usage.input_tokens.unwrap_or_default(),
            output: usage.output_tokens.unwrap_or_default(),
        })
    }

    fn finish_reasons(&self) -> Option<String> {
        match self {
            Ok((_, event)) => match event {
                protocol::messages::StreamEvent::MessageStart(msg) => msg.stop_reason.as_ref(),
                protocol::messages::StreamEvent::MessageDelta(msg) => msg.delta.stop_reason.as_ref(),
                _ => None,
            },
            _ => None,
        }
        .map(|reason| serde_json::to_string(&[reason]).unwrap())
    }
}

impl Service<Request> for super::Proxy {
    type Response = Response;

    type Error = super::ProxyError;

    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request) -> Self::Future {
        if request.payload.stream.unwrap_or_default() {
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

                Ok(Response::Stream(StreamResponse {
                    request,
                    status_code,
                    headers,
                    event_stream,
                }))
            })
        } else {
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

                let mut response = CompleteResponse {
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

                Ok(Response::Complete(response))
            })
        }
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
