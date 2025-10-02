//! Client identification middleware for access control.
//!
//! This middleware extracts and validates client identity based on JWT claims or HTTP headers.
//! It runs before rate limiting to ensure unauthorized users are rejected immediately.

use std::{
    fmt::Display,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use axum::body::Body;
use config::ClientIdentificationConfig;
use http::{Request, Response, StatusCode};
use tower::Layer;

use crate::client_id::{ClientIdentificationError, extract_client_identity};

#[derive(Clone)]
pub struct ClientIdentificationLayer(Arc<ClientIdentificationConfig>);

impl ClientIdentificationLayer {
    pub fn new(config: ClientIdentificationConfig) -> Self {
        Self(Arc::new(config))
    }
}

impl<Service> Layer<Service> for ClientIdentificationLayer
where
    Service: Send + Clone,
{
    type Service = ClientIdentificationService<Service>;

    fn layer(&self, next: Service) -> Self::Service {
        ClientIdentificationService {
            next,
            config: self.0.clone(),
        }
    }
}

#[derive(Clone)]
pub struct ClientIdentificationService<Service> {
    next: Service,
    config: Arc<ClientIdentificationConfig>,
}

impl<Service, ReqBody> tower::Service<Request<ReqBody>> for ClientIdentificationService<Service>
where
    Service: tower::Service<Request<ReqBody>, Response = Response<Body>> + Send + Clone + 'static,
    Service::Future: Send,
    Service::Error: Display + 'static,
    ReqBody: http_body::Body + Send + 'static,
{
    type Response = Response<Body>;
    type Error = Service::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Response<Body>, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.next.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let mut next = self.next.clone();
        let config = self.config.clone();

        Box::pin(async move {
            // Extract and validate client identity
            match extract_client_identity(&req, &config) {
                Ok(Some(identity)) => {
                    // Valid identity - store it and continue
                    let (mut parts, body) = req.into_parts();
                    parts.extensions.insert(identity);
                    let req = Request::from_parts(parts, body);
                    next.call(req).await
                }
                Ok(None) => {
                    // Client identification not enabled or not configured - continue
                    next.call(req).await
                }
                Err(ClientIdentificationError::UnauthorizedGroup { group, allowed_groups }) => {
                    // Client is in an invalid group - this is a bad request, not authentication
                    log::warn!(
                        "Invalid group: client attempted to use group '{}', allowed: {:?}",
                        group,
                        allowed_groups
                    );

                    // Return 400 Bad Request for invalid group
                    let response = Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .header("Content-Type", "application/json")
                        .body(Body::from(
                            r#"{"error":"invalid_group","error_description":"The specified group is not valid"}"#,
                        ))
                        .unwrap();

                    Ok(response)
                }
                Err(ClientIdentificationError::MissingIdentification) => {
                    // Client identification is required but missing - this is a bad request
                    log::debug!("Bad request: client identification required but not provided");

                    // Return 400 Bad Request for missing required client ID
                    let response = Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .header("Content-Type", "application/json")
                        .body(Body::from(
                            r#"{"error":"missing_client_id","error_description":"Client identification is required"}"#,
                        ))
                        .unwrap();

                    Ok(response)
                }
            }
        })
    }
}
