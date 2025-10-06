use std::{convert::Infallible, task::Poll};

use axum::response::{IntoResponse as _, Response};
use futures::future::BoxFuture;
use http_body_util::BodyExt as _;
use tower::Service;

use crate::proxy::utils::headers::insert_proxied_headers_and_content_accept_into;

#[derive(Clone, Debug)]
pub struct Forward(pub super::Proxy);

impl Service<axum::extract::Request> for Forward {
    type Response = Response;
    type Error = Infallible;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: axum::extract::Request) -> Self::Future {
        let path = request
            .uri()
            .path()
            .strip_prefix(&self.0.base_path)
            .expect("Invalid URL, should not be reachable.");
        let mut url = self
            .0
            .anthropic_base_url
            .join(path.strip_prefix('/').unwrap_or(path))
            .unwrap();
        url.set_query(request.uri().query());

        let fut = insert_proxied_headers_and_content_accept_into(self.0.client.get(url), request.headers())
            .body(reqwest::Body::wrap_stream(request.into_data_stream()))
            .send();

        Box::pin(async move {
            let response = match fut.await {
                Ok(response) => http::Response::from(response).into_response(),
                Err(err) => {
                    log::error!("Failed to send request to Anthropic: {err}");
                    super::internal_server_error("Could not connect to Anthropic API")
                }
            };

            Ok(response)
        })
    }
}
