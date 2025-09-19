//! Distributed tracing implementation using fastrace with OpenTelemetry export

use anyhow::Context;
use config::TelemetryConfig;
use fastrace::collector::Config as CollectorConfig;
use fastrace_opentelemetry::OpenTelemetryReporter;
use opentelemetry::{InstrumentationScope, KeyValue};
use opentelemetry_otlp::{SpanExporter, WithExportConfig};
use opentelemetry_sdk::Resource;
use std::borrow::Cow;
use std::time::Duration;

use crate::metadata;

/// Guard that ensures proper cleanup of tracing resources
pub struct TracingGuard;

impl TracingGuard {
    /// Force flush all pending traces immediately
    /// Useful for tests to ensure traces are exported before assertions
    pub fn force_flush(&self) -> anyhow::Result<()> {
        fastrace::flush();
        Ok(())
    }
}

impl Drop for TracingGuard {
    fn drop(&mut self) {
        fastrace::flush();
    }
}

/// Initialize distributed tracing with fastrace and OpenTelemetry export
pub async fn init_tracing(config: &TelemetryConfig) -> anyhow::Result<TracingGuard> {
    log::info!("init_tracing called");
    let tracing_config = config.tracing();

    // Only initialize if tracing is enabled (has exporters configured)
    if !config.tracing_enabled() {
        log::debug!("Tracing is disabled (no exporters configured)");
        return Ok(TracingGuard);
    }

    log::info!("Tracing is enabled, checking for OTLP exporter configuration");

    // Check if we have OTLP export configured
    let Some(otlp_config) = config.traces_otlp_config() else {
        log::debug!(
            "No OTLP exporter configured for traces, using console reporter. Global exporters OTLP enabled: {}",
            config.global_exporters().otlp.enabled
        );

        return Ok(TracingGuard);
    };

    log::debug!("Initializing tracing with OTLP export to {}", otlp_config.endpoint);
    log::debug!(
        "Tracing configuration: sampling={}, parent_based={}",
        tracing_config.sampling,
        tracing_config.parent_based_sampler
    );

    let service_name = config.service_name().unwrap_or("nexus").to_string();
    let mut resource_attributes = vec![KeyValue::new("service.name", service_name)];

    for (key, value) in config.resource_attributes() {
        resource_attributes.push(KeyValue::new(key.clone(), value.clone()));
    }

    let resource = Resource::builder_empty().with_attributes(resource_attributes).build();

    log::debug!(
        "Creating OTLP span exporter with endpoint: {}, protocol: {:?}",
        otlp_config.endpoint,
        otlp_config.protocol
    );

    let exporter = match otlp_config.protocol {
        config::OtlpProtocol::Grpc => {
            use opentelemetry_otlp::WithTonicConfig;

            let mut builder = SpanExporter::builder()
                .with_tonic()
                .with_endpoint(otlp_config.endpoint.to_string())
                .with_timeout(otlp_config.timeout);

            let metadata = metadata::build_metadata(otlp_config)?;
            builder = builder.with_metadata(metadata);

            builder.build().context("Failed to build gRPC OTLP span exporter")?
        }
        config::OtlpProtocol::Http => {
            use opentelemetry_otlp::WithHttpConfig;

            let mut builder = SpanExporter::builder()
                .with_http()
                .with_endpoint(otlp_config.endpoint.to_string())
                .with_timeout(otlp_config.timeout);

            let headers = metadata::build_http_headers(otlp_config)?;
            builder = builder.with_headers(headers);

            builder.build().context("Failed to build HTTP OTLP span exporter")?
        }
    };

    log::debug!("OTLP span exporter created successfully");

    let instrumentation_scope = InstrumentationScope::builder("nexus")
        .with_version(env!("CARGO_PKG_VERSION"))
        .build();

    let otel_reporter = OpenTelemetryReporter::new(exporter, Cow::Owned(resource), instrumentation_scope);

    let report_interval = Duration::from_millis(otlp_config.batch_export.scheduled_delay.as_millis() as u64);
    let collector_config = CollectorConfig::default().report_interval(report_interval);

    fastrace::set_reporter(otel_reporter, collector_config);

    // Note: Trace context propagation from incoming requests is handled at the HTTP middleware level
    // We don't need OpenTelemetry propagators since we're not making outgoing traced requests

    log::debug!(
        "Tracing subsystem initialized successfully with service name: {}",
        config.service_name().unwrap_or("nexus")
    );

    Ok(TracingGuard)
}
