use std::{
    fmt::Display,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use axum::body::Body;
use config::OauthConfig;
use context::Authentication;
use http::{HeaderValue, Request, Response, StatusCode};
use serde::Serialize;

use tower::Layer;

use crate::auth::{NativeProviderAuthentication, error::AuthError, jwt::NexusOAuth};

pub struct AuthLayer<NativeProviderOAuth = ()>(Arc<AuthLayerInner<NativeProviderOAuth>>);

impl<A> Clone for AuthLayer<A> {
    fn clone(&self) -> Self {
        AuthLayer(self.0.clone())
    }
}

struct AuthLayerInner<NativeProviderOAuth> {
    nexus_oauth: Option<NexusOAuth>,
    native_provider_oauth: Option<NativeProviderOAuth>,
}

impl AuthLayer {
    pub fn new(config: Option<OauthConfig>) -> Self {
        let nexus_oauth = config.map(NexusOAuth::new);
        Self(Arc::new(AuthLayerInner {
            nexus_oauth,
            native_provider_oauth: None,
        }))
    }

    pub fn new_with_native_provider<NativeProviderOAuth>(
        config: Option<OauthConfig>,
        native_provider_oauth: NativeProviderOAuth,
    ) -> AuthLayer<NativeProviderOAuth> {
        let nexus_oauth = config.map(NexusOAuth::new);
        AuthLayer(Arc::new(AuthLayerInner {
            nexus_oauth,
            native_provider_oauth: Some(native_provider_oauth),
        }))
    }
}

impl<Service, NativeProviderOAuth> Layer<Service> for AuthLayer<NativeProviderOAuth>
where
    Service: Send + Clone,
{
    type Service = AuthService<Service, NativeProviderOAuth>;

    fn layer(&self, next: Service) -> Self::Service {
        AuthService {
            next,
            layer: self.0.clone(),
        }
    }
}

pub struct AuthService<Service, NativeProviderOAuth = ()> {
    next: Service,
    layer: Arc<AuthLayerInner<NativeProviderOAuth>>,
}

impl<A, S: Clone> Clone for AuthService<S, A> {
    fn clone(&self) -> Self {
        AuthService {
            next: self.next.clone(),
            layer: self.layer.clone(),
        }
    }
}

impl<NativeProviderOAuth, Service, ReqBody> tower::Service<Request<ReqBody>>
    for AuthService<Service, NativeProviderOAuth>
where
    Service: tower::Service<Request<ReqBody>, Response = Response<Body>> + Send + Clone + 'static,
    Service::Future: Send,
    Service::Error: Display + 'static,
    ReqBody: http_body::Body + Send + 'static,
    NativeProviderOAuth: NativeProviderAuthentication + Send + Sync + 'static,
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
            let (mut parts, body) = req.into_parts();

            let Some(nexus_oauth) = layer.nexus_oauth.as_ref() else {
                let auth = layer
                    .native_provider_oauth
                    .as_ref()
                    .map(|native_auth| native_auth.authenticate(&parts))
                    .unwrap_or_default();
                parts.extensions.insert(auth);
                return next.call(Request::from_parts(parts, body)).await;
            };

            if let Some(native_provider_oauth) = &layer.native_provider_oauth
                && let Some(value) = parts.headers.get(http::header::PROXY_AUTHORIZATION)
            {
                match nexus_oauth.authenticate(value).await {
                    Ok(token) => {
                        let auth = native_provider_oauth.authenticate(&parts);
                        parts.extensions.insert(Authentication {
                            nexus: Some(token),
                            ..auth
                        });

                        next.call(Request::from_parts(parts, body)).await
                    }
                    Err(err) => Ok(error_response(nexus_oauth, err)),
                }
            } else if let Some(value) = parts.headers.get(http::header::AUTHORIZATION) {
                match nexus_oauth.authenticate(value).await {
                    Ok(token) => {
                        parts.extensions.insert(Authentication {
                            nexus: Some(token),
                            ..Default::default()
                        });

                        next.call(Request::from_parts(parts, body)).await
                    }
                    Err(err) => Ok(error_response(nexus_oauth, err)),
                }
            } else {
                Ok(error_response(nexus_oauth, AuthError::InvalidToken("missing token")))
            }
        })
    }
}

fn error_response(nexus_oauth: &NexusOAuth, err: AuthError) -> http::Response<Body> {
    let metadata_endpoint = nexus_oauth.metadata_endpoint();
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

    let (status_code, error_response) = match err {
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

    Response::builder()
        .status(status_code)
        .header("WWW-Authenticate", www_authenticate_value)
        .header("Content-Type", "application/json")
        .body(Body::from(error_response.to_json()))
        .unwrap()
}
