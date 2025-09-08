//! HTTP tracing middleware
//!
//! Creates distributed traces for all HTTP requests following OpenTelemetry semantic conventions.

use axum::{body::Body, extract::MatchedPath};
use config::{ClientIdentity, TelemetryConfig};
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
    sync::Arc,
    task::{Context, Poll},
};
use tower::Layer;

/// Layer for HTTP tracing
#[derive(Clone)]
pub struct TracingLayer {
    telemetry_config: Option<Arc<TelemetryConfig>>,
}

impl TracingLayer {
    pub fn new() -> Self {
        Self { telemetry_config: None }
    }

    pub fn with_config(telemetry_config: Arc<TelemetryConfig>) -> Self {
        Self {
            telemetry_config: Some(telemetry_config),
        }
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
        TracingService {
            next,
            telemetry_config: self.telemetry_config.clone(),
        }
    }
}

/// Service that creates traces for HTTP requests
#[derive(Clone)]
pub struct TracingService<Service> {
    next: Service,
    telemetry_config: Option<Arc<TelemetryConfig>>,
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

        // Extract trace context and sampling decision from headers
        let (span_context, parent_sampled) = extract_trace_context(req.headers());

        // Create span name
        let span_name = format!("{} {}", method, path);

        // Determine if we should sample this trace
        let should_sample = should_sample_trace(parent_sampled, self.telemetry_config.as_ref().map(|c| c.as_ref()));

        log::debug!(
            "Sampling decision: should_sample={}, parent_sampled={:?}, config_present={}",
            should_sample,
            parent_sampled,
            self.telemetry_config.is_some()
        );

        // Create or continue span based on extracted context
        // If we have a parent context but decided to sample when parent said not to,
        // we need to preserve the trace ID but create a new context to ensure export
        let parent = if should_sample && parent_sampled == Some(false) {
            if let Some(original_context) = span_context {
                // Parent said don't sample, but we decided to sample anyway
                // We need to preserve the trace ID for testing but ensure the trace is exported
                log::debug!(
                    "Overriding parent's sampled=0 decision. Original trace_id: {:?}",
                    original_context.trace_id
                );
                // Keep the same trace ID but create a new span ID to ensure export
                SpanContext::new(original_context.trace_id, SpanId(rand::random::<u64>()))
            } else {
                SpanContext::random()
            }
        } else {
            span_context.unwrap_or_else(SpanContext::random)
        };

        // Clone the service for the async block
        let mut next = self.next.clone();

        // Only create spans if we should sample
        if !should_sample {
            // If not sampling, just pass through without creating a span
            let fut = async move { next.call(req).await };
            return Box::pin(fut);
        }

        log::debug!(
            "Creating root span '{}' with parent context (sampled). Parent trace_id: {:?}",
            span_name,
            parent.trace_id
        );

        // Create the root span with the parent context
        // This will use the trace ID from the parent if it came from a W3C traceparent header
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

/// Extract trace context and sampling decision from HTTP headers
/// Returns (SpanContext, parent_sampled)
fn extract_trace_context(headers: &HeaderMap) -> (Option<SpanContext>, Option<bool>) {
    // Try W3C Trace Context first (most common)
    if let Some(traceparent) = headers.get("traceparent")
        && let Ok(traceparent_str) = traceparent.to_str()
    {
        let (context, sampled) = parse_traceparent_with_sampling(traceparent_str);
        if let Some(ctx) = context {
            return (Some(ctx), sampled);
        }
    }

    // Try AWS X-Ray format
    // Format: X-Amzn-Trace-Id: Root=1-5759e988-bd862e3fe1be46a994272793;Parent=53995c3f42cd8ad8;Sampled=1
    if let Some(xray_header) = headers.get("x-amzn-trace-id")
        && let Ok(xray_str) = xray_header.to_str()
    {
        let (context, sampled) = parse_xray_trace_id_with_sampling(xray_str);
        if let Some(ctx) = context {
            return (Some(ctx), sampled);
        }
    }

    // Note: Baggage doesn't carry trace context, only additional metadata
    // Jaeger would be added here if needed

    (None, None)
}

/// Parse W3C traceparent header with sampling flag
/// Format: version-trace_id-parent_id-trace_flags
/// Example: 00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01
/// Returns (SpanContext, sampled flag)
fn parse_traceparent_with_sampling(traceparent: &str) -> (Option<SpanContext>, Option<bool>) {
    // Parse the traceparent to extract sampling flag
    let parts: Vec<&str> = traceparent.split('-').collect();
    if parts.len() == 4 {
        // Extract the trace flags (last part)
        if let Ok(flags) = u8::from_str_radix(parts[3], 16) {
            // Bit 0 is the sampled flag
            let sampled = (flags & 0x01) == 0x01;

            // Use fastrace's built-in parser for the context
            let context = SpanContext::decode_w3c_traceparent(traceparent);
            return (context, Some(sampled));
        }
    }

    // Fallback to just parsing context without sampling info
    (SpanContext::decode_w3c_traceparent(traceparent), None)
}

/// Parse AWS X-Ray trace ID header with sampling flag
/// Format: X-Amzn-Trace-Id: Root=1-5759e988-bd862e3fe1be46a994272793;Parent=53995c3f42cd8ad8;Sampled=1
/// Returns (SpanContext, sampled flag)
fn parse_xray_trace_id_with_sampling(xray_str: &str) -> (Option<SpanContext>, Option<bool>) {
    let mut trace_id = None;
    let mut parent_id = None;
    let mut sampled = None;

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
                "Sampled" => {
                    // Sampled is "0" or "1"
                    sampled = Some(value == "1");
                }
                _ => {} // Ignore other fields
            }
        }
    }

    // Create SpanContext if we have both trace and parent IDs
    let context = match (trace_id, parent_id) {
        (Some(tid), Some(pid)) => {
            // Create a SpanContext with the extracted IDs
            // Note: fastrace doesn't have direct X-Ray support, so we create a context manually
            Some(SpanContext::new(TraceId(tid), SpanId(pid)))
        }
        _ => None,
    };

    (context, sampled)
}

/// Determine if a trace should be sampled based on parent sampling and configuration
fn should_sample_trace(parent_sampled: Option<bool>, telemetry_config: Option<&TelemetryConfig>) -> bool {
    // If telemetry is not configured or disabled, don't sample
    let Some(config) = telemetry_config else {
        log::debug!("No telemetry config, not sampling");
        return false;
    };

    if !config.tracing_enabled() {
        log::debug!("Tracing not enabled, not sampling");
        return false;
    }

    let tracing_config = config.tracing();

    log::debug!(
        "Sampling config: parent_based_sampler={}, sampling_rate={}, parent_sampled={:?}",
        tracing_config.parent_based_sampler,
        tracing_config.sampling,
        parent_sampled
    );

    // If parent_based_sampler is enabled and we have a parent sampling decision
    if tracing_config.parent_based_sampler {
        if let Some(sampled) = parent_sampled {
            // Respect the parent's sampling decision
            log::debug!("Using parent-based sampling, parent sampled={}", sampled);
            return sampled;
        }
        // No parent, fall through to ratio-based sampling
        log::debug!("Parent-based sampler enabled but no parent, using ratio");
    }

    // Apply ratio-based sampling
    // Note: This is a simple implementation. In production, you might want
    // to use a deterministic sampling based on trace ID for consistency
    use rand::Rng;
    let sample_rate = tracing_config.sampling;

    // If sampling rate is 0, don't sample
    if sample_rate <= 0.0 {
        log::debug!("Sampling rate is 0, not sampling");
        return false;
    }

    // If sampling rate is 1 or higher, always sample
    if sample_rate >= 1.0 {
        log::debug!("Sampling rate is 1.0, always sampling");
        return true;
    }

    // Random sampling based on rate
    let sampled = rand::rng().random_bool(sample_rate);
    log::debug!("Random sampling with rate {}, sampled={}", sample_rate, sampled);
    sampled
}
