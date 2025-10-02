//! Rate limiting middleware for HTTP requests.

use std::{
    fmt::Display,
    future::Future,
    net::{IpAddr, SocketAddr},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use axum::{body::Body, extract::ConnectInfo};
use context::ClientIdentity;
use http::{Request, Response, StatusCode};
use rate_limit::{RateLimitError, RateLimitManager, RateLimitRequest};
use tower::Layer;

use config::ClientIpConfig;

#[derive(Clone)]
pub struct RateLimitLayer {
    client_ip_config: ClientIpConfig,
    manager: Option<Arc<RateLimitManager>>,
}

impl RateLimitLayer {
    pub fn new(client_ip_config: ClientIpConfig, manager: Option<Arc<RateLimitManager>>) -> Self {
        Self {
            client_ip_config,
            manager,
        }
    }
}

impl<Service> Layer<Service> for RateLimitLayer
where
    Service: Send + Clone,
{
    type Service = RateLimitService<Service>;

    fn layer(&self, next: Service) -> Self::Service {
        RateLimitService {
            next,
            layer: self.clone(),
        }
    }
}

#[derive(Clone)]
pub struct RateLimitService<Service> {
    next: Service,
    layer: RateLimitLayer,
}

impl<Service, ReqBody> tower::Service<Request<ReqBody>> for RateLimitService<Service>
where
    Service: tower::Service<Request<ReqBody>, Response = Response<Body>> + Send + Clone + 'static,
    Service::Future: Send,
    Service::Error: Display + 'static,
    ReqBody: http_body::Body + Send + 'static,
{
    type Response = http::Response<Body>;
    type Error = Service::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Response<Body>, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.next.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let mut next = self.next.clone();

        let Some(manager) = self.layer.manager.clone() else {
            return Box::pin(next.call(req));
        };

        // Extract client IP for IP-based rate limiting
        let ip = extract_client_ip(&self.layer.client_ip_config, &req);

        Box::pin(async move {
            // Get client identity from request extensions (already validated by ClientIdentificationLayer)
            let identity = req.extensions().get::<ClientIdentity>().cloned();

            // Build rate limit request with IP and client identity
            let rate_limit_request = RateLimitRequest::builder().ip(ip).build();

            // Log client identity if present
            if let Some(ref identity) = identity {
                log::debug!(
                    "Rate limiting for client: {} in group: {:?}",
                    identity.client_id,
                    identity.group
                );
            }

            // Check rate limits
            let err = match manager.check_request(&rate_limit_request).await {
                Ok(()) => {
                    // Request allowed, continue to next handler
                    return next.call(req).await;
                }
                Err(err) => err,
            };

            // Log the specific rate limit error for debugging
            log::debug!("Request rejected due to rate limit: {err:?}");

            // Request blocked, return generic error without specific details
            let (status, message) = match &err {
                RateLimitError::Storage(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error"),
                _ => (StatusCode::TOO_MANY_REQUESTS, "Rate limit exceeded"),
            };

            let response = Response::builder()
                .status(status)
                .header("Content-Type", "text/plain")
                .body(Body::from(message))
                .unwrap();

            // No Retry-After headers are sent to maintain consistency with downstream LLM providers
            Ok(response)
        })
    }
}

fn extract_client_ip<B>(config: &ClientIpConfig, req: &Request<B>) -> IpAddr {
    if config.x_real_ip
        && let Some(ip) = req
            .headers()
            .get("x-real-ip")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.trim().parse().ok())
    {
        return ip;
    }

    if let Some(hops) = config.x_forwarded_for_trusted_hops
        && let Some(ip) = req
            .headers()
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split(',').rev().nth(hops))
            .and_then(|s| s.trim().parse().ok())
    {
        return ip;
    }

    req.extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0.ip())
        .expect("Axum always provides the client SocketAddr info if properly configured.")
}
