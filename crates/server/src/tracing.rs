//! HTTP tracing middleware
//!
//! Creates distributed traces for all HTTP requests following OpenTelemetry semantic conventions.

use axum::{body::Body, extract::MatchedPath};
use config::ClientIdentity;
use fastrace::future::FutureExt;
use fastrace::{
    Span,
    collector::{SpanId, TraceId},
    prelude::{LocalSpan, SpanContext},
};
use http::{HeaderMap, Request, Response};
use std::{
    fmt::Display,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use tower::Layer;

/// Layer for HTTP tracing
#[derive(Clone)]
pub struct TracingLayer;

impl TracingLayer {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TracingLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl<Service> Layer<Service> for TracingLayer
where
    Service: Send + Clone,
{
    type Service = TracingService<Service>;

    fn layer(&self, next: Service) -> Self::Service {
        TracingService { next }
    }
}

/// Service that creates traces for HTTP requests
#[derive(Clone)]
pub struct TracingService<Service> {
    next: Service,
}

impl<Service, ReqBody> tower::Service<Request<ReqBody>> for TracingService<Service>
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

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        let path = req
            .extensions()
            .get::<MatchedPath>()
            .map(|matched_path| matched_path.as_str().to_owned())
            .unwrap_or_else(|| req.uri().path().to_owned());

        let method = req.method().to_string();
        let uri = req.uri().to_string();
        let scheme = req.uri().scheme_str().unwrap_or("http").to_string();

        // Extract host header
        let host = req
            .headers()
            .get("host")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());

        // Extract trace context from headers
        let span_context = extract_trace_context(req.headers());

        // Create span name
        let span_name = format!("{} {}", method, path);

        // Create or continue span based on extracted context
        let parent = span_context.unwrap_or_else(SpanContext::random);

        log::debug!("Creating root span '{}' with parent context", span_name);

        // Clone the service for the async block
        let mut next = self.next.clone();

        // Create the root span with properties
        let root = Span::root(span_name.clone(), parent);

        // Store the trace context in request extensions so downstream services can access it
        // This is needed because some service layers (like StreamableHttpService) spawn new tasks
        // which lose the thread-local span context
        // Unfortunately, MCP spans will be siblings rather than children due to this limitation
        req.extensions_mut().insert(parent);

        // Add span attributes following OpenTelemetry semantic conventions
        root.add_property(|| ("http.request.method", method.clone()));
        root.add_property(|| ("http.route", path.clone()));
        root.add_property(|| ("url.full", uri.clone()));
        root.add_property(|| ("url.scheme", scheme.clone()));

        if let Some(host) = host.clone() {
            root.add_property(|| ("server.address", host));
        }

        // Add client identity if present (extracted by ClientIdentificationLayer middleware)
        if let Some(client_identity) = req.extensions().get::<ClientIdentity>() {
            root.add_property(|| ("client.id", client_identity.client_id.clone()));

            if let Some(ref group) = client_identity.group {
                root.add_property(|| ("client.group", group.clone()));
            }
        }

        log::debug!("Created root span '{}' with parent context", span_name);

        // Create the future and wrap it with the span
        let fut = async move {
            log::debug!("Executing request within tracing span for {}", span_name);

            let response = next.call(req).await?;

            // Add response attributes using LocalSpan
            let status = response.status();
            LocalSpan::add_property(|| ("http.response.status_code", status.as_u16().to_string()));

            // Set error status if response indicates an error
            if status.is_client_error() || status.is_server_error() {
                LocalSpan::add_property(|| ("error", "true"));
            }

            log::debug!("Completed request for {}, span will be submitted", span_name);

            Ok(response)
        };

        // Wrap the future with the span using in_span
        Box::pin(fut.in_span(root))
    }
}

/// Extract trace context from HTTP headers
fn extract_trace_context(headers: &HeaderMap) -> Option<SpanContext> {
    // Try W3C Trace Context first (most common)
    if let Some(traceparent) = headers.get("traceparent")
        && let Ok(traceparent_str) = traceparent.to_str()
        && let Some(context) = parse_traceparent(traceparent_str)
    {
        return Some(context);
    }

    // Try AWS X-Ray format
    // Format: X-Amzn-Trace-Id: Root=1-5759e988-bd862e3fe1be46a994272793;Parent=53995c3f42cd8ad8;Sampled=1
    if let Some(xray_header) = headers.get("x-amzn-trace-id")
        && let Ok(xray_str) = xray_header.to_str()
        && let Some(context) = parse_xray_trace_id(xray_str)
    {
        return Some(context);
    }

    // Note: Baggage doesn't carry trace context, only additional metadata
    // Jaeger would be added here if needed

    None
}

/// Parse W3C traceparent header
/// Format: version-trace_id-parent_id-trace_flags
/// Example: 00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01
fn parse_traceparent(traceparent: &str) -> Option<SpanContext> {
    // Fastrace has built-in W3C traceparent support
    SpanContext::decode_w3c_traceparent(traceparent)
}

/// Parse AWS X-Ray trace ID header
/// Format: X-Amzn-Trace-Id: Root=1-5759e988-bd862e3fe1be46a994272793;Parent=53995c3f42cd8ad8;Sampled=1
fn parse_xray_trace_id(xray_str: &str) -> Option<SpanContext> {
    let mut trace_id = None;
    let mut parent_id = None;

    // Parse the key-value pairs
    for part in xray_str.split(';') {
        let part = part.trim();
        if let Some((key, value)) = part.split_once('=') {
            match key {
                "Root" => {
                    // Root format: 1-5759e988-bd862e3fe1be46a994272793
                    // Version: 1 (currently always 1)
                    // Timestamp: 5759e988 (8 hex chars, unix seconds)
                    // Random: bd862e3fe1be46a994272793 (24 hex chars)

                    let parts: Vec<&str> = value.split('-').collect();
                    if parts.len() == 3 && parts[0] == "1" {
                        // Combine timestamp and random parts into a single 128-bit ID
                        // This preserves the X-Ray structure for proper backend handling
                        let trace_id_str = format!("{}{}", parts[1], parts[2]);
                        if trace_id_str.len() == 32
                            && let Ok(id) = u128::from_str_radix(&trace_id_str, 16)
                        {
                            trace_id = Some(id);
                        }
                    }
                }
                "Parent" => {
                    // Parent is 16 hex chars (64-bit)
                    if let Ok(id) = u64::from_str_radix(value, 16) {
                        parent_id = Some(id);
                    }
                }
                _ => {} // Ignore other fields like Sampled
            }
        }
    }

    // Create SpanContext if we have both trace and parent IDs
    match (trace_id, parent_id) {
        (Some(tid), Some(pid)) => {
            // Create a SpanContext with the extracted IDs
            // Note: fastrace doesn't have direct X-Ray support, so we create a context manually
            Some(SpanContext::new(TraceId(tid), SpanId(pid)))
        }
        _ => None,
    }
}
