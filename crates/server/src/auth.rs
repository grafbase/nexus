mod claims;
mod error;
mod jwks;
mod jwt;

use std::{
    fmt::Display,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use axum::body::Body;
use config::OauthConfig;
use error::AuthError;
use http::{HeaderValue, Request, Response, StatusCode};
use jwt::JwtAuth;
use serde::Serialize;

use tower::Layer;

type AuthResult<T> = Result<T, AuthError>;

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_description: Option<String>,
}

impl ErrorResponse {
    fn new(error: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            error_description: None,
        }
    }

    fn with_description(error: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            error_description: Some(description.into()),
        }
    }

    fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| r#"{"error":"internal_error"}"#.to_string())
    }
}

#[derive(Clone)]
pub struct AuthLayer(Arc<AuthLayerInner>);

struct AuthLayerInner {
    jwt: JwtAuth,
}

impl AuthLayer {
    pub fn new(config: OauthConfig) -> Self {
        let jwt = JwtAuth::new(config);
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

        let (mut parts, body) = req.into_parts();

        Box::pin(async move {
            match layer.jwt.authenticate(&parts).await {
                Ok(token) => {
                    // Inject token into request extensions
                    parts.extensions.insert(token);
                    next.call(Request::from_parts(parts, body)).await
                }
                Err(auth_error) => {
                    let metadata_endpoint = layer.jwt.metadata_endpoint();
                    let header_value = format!("Bearer resource_metadata=\"{metadata_endpoint}\"");

                    // Use HeaderValue for proper validation and to prevent header injection
                    let www_authenticate_value = match HeaderValue::from_str(&header_value) {
                        Ok(value) => value,
                        Err(_) => {
                            // If header value is invalid, use a safe fallback
                            HeaderValue::from_static("Bearer")
                        }
                    };

                    let (status_code, error_response) = match auth_error {
                        AuthError::Unauthorized => (StatusCode::UNAUTHORIZED, ErrorResponse::new("unauthorized")),
                        AuthError::Internal => (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            ErrorResponse::with_description("internal_server_error", "An internal error occurred"),
                        ),
                        AuthError::InvalidToken(msg) => (
                            StatusCode::UNAUTHORIZED,
                            ErrorResponse::with_description("invalid_token", msg),
                        ),
                    };

                    let response = Response::builder()
                        .status(status_code)
                        .header("WWW-Authenticate", www_authenticate_value)
                        .header("Content-Type", "application/json")
                        .body(Body::from(error_response.to_json()))
                        .unwrap();

                    Ok(response)
                }
            }
        })
    }
}
