use std::{
    fmt::Display,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use super::error::AuthError;
use super::jwt::JwtAuth;
use axum::body::Body;
use config::OauthConfig;
use context::Authentication;
use http::{HeaderValue, Request, Response, StatusCode};
use serde::Serialize;

use tower::Layer;

#[derive(Clone)]
pub struct AuthLayer(Arc<AuthLayerInner>);

struct AuthLayerInner {
    jwt: Option<JwtAuth>,
}

impl AuthLayer {
    pub fn new(config: Option<OauthConfig>) -> Self {
        let jwt = config.map(JwtAuth::new);
        Self(Arc::new(AuthLayerInner { jwt }))
    }
}

impl<Service> Layer<Service> for AuthLayer
where
    Service: Send + Clone,
{
    type Service = AuthService<Service>;

    fn layer(&self, next: Service) -> Self::Service {
        AuthService {
            next,
            layer: self.0.clone(),
        }
    }
}

#[derive(Clone)]
pub struct AuthService<Service> {
    next: Service,
    layer: Arc<AuthLayerInner>,
}

impl<Service, ReqBody> tower::Service<Request<ReqBody>> for AuthService<Service>
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
        let layer = self.layer.clone();

        Box::pin(async move {
            let Some(jwt) = layer.jwt.as_ref() else {
                return next.call(req).await;
            };

            let (mut parts, body) = req.into_parts();

            match jwt.authenticate(&parts).await {
                Ok(token) => {
                    // Inject both the token string and validated token into request extensions
                    parts.extensions.insert(Authentication {
                        nexus: Some(token),
                        anthropic: None,
                    });
                    next.call(Request::from_parts(parts, body)).await
                }
                Err(auth_error) => {
                    let metadata_endpoint = jwt.metadata_endpoint();
                    let header_value = format!("Bearer resource_metadata=\"{metadata_endpoint}\"");

                    // Use HeaderValue for proper validation and to prevent header injection
                    let www_authenticate_value = match HeaderValue::from_str(&header_value) {
                        Ok(value) => value,
                        Err(_) => {
                            // If header value is invalid, use a safe fallback
                            HeaderValue::from_static("Bearer")
                        }
                    };

                    #[derive(Serialize)]
                    struct Content {
                        error: &'static str,
                    }

                    let (status_code, error) = match auth_error {
                        AuthError::Unauthorized => (StatusCode::UNAUTHORIZED, "Unauthorized"),
                        AuthError::Internal => (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error"),
                    };

                    let response = Response::builder()
                        .status(status_code)
                        .header(http::header::WWW_AUTHENTICATE, www_authenticate_value)
                        .header(http::header::CONTENT_TYPE, "application/json")
                        .body(Body::from(serde_json::to_vec(&Content { error }).unwrap()))
                        .unwrap();

                    Ok(response)
                }
            }
        })
    }
}
