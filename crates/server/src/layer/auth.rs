mod claims;
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
use http::{Request, Response, StatusCode};
use jwt::JwtAuth;

use tower::Layer;

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

        let (parts, body) = req.into_parts();

        Box::pin(async move {
            if layer.jwt.authenticate(&parts).await.is_ok() {
                return next.call(Request::from_parts(parts, body)).await;
            }

            let metadata_endpoint = layer.jwt.metadata_endpoint();
            let header_value = format!("Bearer resource_metadata=\"{metadata_endpoint}\"");

            let response = Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .header("WWW-Authenticate", &header_value)
                .body(Body::empty())
                .unwrap();

            Ok(response)
        })
    }
}
