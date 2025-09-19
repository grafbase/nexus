//! Metrics initialization and management

mod names;
mod recorder;

pub use names::*;
pub use recorder::Recorder;

use anyhow::Context;
use config::{OtlpProtocol, TelemetryConfig};
use opentelemetry::metrics::Meter;
use opentelemetry_otlp::{MetricExporter, WithExportConfig};
use opentelemetry_sdk::{
    Resource,
    metrics::{PeriodicReader, SdkMeterProvider},
};

use crate::metadata;

const METER_NAME: &str = "nexus";

/// Get the global meter for recording metrics
pub fn meter() -> Meter {
    opentelemetry::global::meter(METER_NAME)
}

/// Initialize the metrics subsystem
pub(crate) async fn init_metrics(config: &TelemetryConfig) -> anyhow::Result<SdkMeterProvider> {
    let meter_provider = create_otlp_meter_provider(config).await?;

    // Set as global meter provider
    opentelemetry::global::set_meter_provider(meter_provider.clone());

    log::info!(
        "Telemetry metrics initialized for service '{}'",
        config.service_name().unwrap_or("nexus")
    );

    Ok(meter_provider)
}

/// Create an OTLP meter provider
async fn create_otlp_meter_provider(telemetry_config: &TelemetryConfig) -> anyhow::Result<SdkMeterProvider> {
    let Some(exporter_config) = telemetry_config.metrics_otlp_config() else {
        log::debug!("No metrics exporters configured or enabled, metrics will not be exported");
        return Ok(create_noop_meter_provider());
    };

    log::debug!(
        "Initializing OTLP metrics exporter to {} via {:?}",
        exporter_config.endpoint,
        exporter_config.protocol
    );

    // Build resource with service name
    let mut builder = Resource::builder();

    if let Some(service_name) = telemetry_config.service_name() {
        builder = builder.with_service_name(service_name.to_string());
    }

    // Add custom resource attributes
    for (key, value) in telemetry_config.resource_attributes() {
        use opentelemetry::{Key, KeyValue, Value};
        builder = builder.with_attribute(KeyValue::new(Key::from(key.clone()), Value::from(value.clone())));
    }

    let resource = builder.build();

    // Create the OTLP exporter based on protocol
    let exporter = match exporter_config.protocol {
        OtlpProtocol::Grpc => {
            use opentelemetry_otlp::WithTonicConfig;

            let mut builder = MetricExporter::builder()
                .with_tonic()
                .with_endpoint(exporter_config.endpoint.as_str())
                .with_timeout(exporter_config.timeout);

            let metadata = metadata::build_metadata(exporter_config)?;
            builder = builder.with_metadata(metadata);

            builder.build().context("Failed to create gRPC OTLP metric exporter")?
        }
        OtlpProtocol::Http => {
            use opentelemetry_otlp::WithHttpConfig;

            let mut builder = MetricExporter::builder()
                .with_http()
                .with_endpoint(exporter_config.endpoint.as_str())
                .with_timeout(exporter_config.timeout);

            let headers = metadata::build_http_headers(exporter_config)?;
            builder = builder.with_headers(headers);

            builder.build().context("Failed to create HTTP OTLP metric exporter")?
        }
    };

    // Create a periodic reader with the configured batch settings
    let reader = PeriodicReader::builder(exporter)
        .with_interval(exporter_config.batch_export.scheduled_delay)
        .build();

    // Build the meter provider with resource
    let provider = SdkMeterProvider::builder()
        .with_resource(resource)
        .with_reader(reader)
        .build();

    log::debug!(
        "OTLP metrics exporter initialized to {} via {:?}",
        exporter_config.endpoint,
        exporter_config.protocol
    );

    Ok(provider)
}

/// Create a no-op meter provider (metrics are recorded but not exported)
fn create_noop_meter_provider() -> SdkMeterProvider {
    SdkMeterProvider::builder().build()
}
