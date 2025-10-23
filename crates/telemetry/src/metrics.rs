//! Metrics initialization and management

mod console;
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
    metrics::{Aggregation, Instrument, InstrumentKind, PeriodicReader, SdkMeterProvider, Stream},
};
use std::time::Duration;

use crate::{metadata, tracing::TraceExportSender};

const METER_NAME: &str = "nexus";

/// Get the global meter for recording metrics
pub fn meter() -> Meter {
    opentelemetry::global::meter(METER_NAME)
}

/// Initialize the metrics subsystem
pub(crate) async fn init_metrics(
    config: &TelemetryConfig,
    tui_sender: Option<TraceExportSender>,
) -> anyhow::Result<SdkMeterProvider> {
    let meter_provider = create_meter_provider(config, tui_sender).await?;

    // Set as global meter provider
    opentelemetry::global::set_meter_provider(meter_provider.clone());

    log::info!(
        "Telemetry metrics initialized for service '{}'",
        config.service_name().unwrap_or("nexus")
    );

    Ok(meter_provider)
}

/// Create an OTLP meter provider
async fn create_meter_provider(
    telemetry_config: &TelemetryConfig,
    tui_sender: Option<TraceExportSender>,
) -> anyhow::Result<SdkMeterProvider> {
    let resource = build_resource(telemetry_config);
    let mut builder = SdkMeterProvider::builder().with_resource(resource);

    builder = builder.with_view(llm_latency_view());
    let mut exporter_configured = false;
    let mut interval_hint: Option<Duration> = None;

    if let Some(exporter_config) = telemetry_config.metrics_otlp_config() {
        log::debug!(
            "Initializing OTLP metrics exporter to {} via {:?}",
            exporter_config.endpoint,
            exporter_config.protocol
        );

        let exporter: MetricExporter = match exporter_config.protocol {
            OtlpProtocol::Grpc => {
                use opentelemetry_otlp::WithTonicConfig;

                let mut builder = MetricExporter::builder()
                    .with_tonic()
                    .with_endpoint(exporter_config.endpoint.as_str())
                    .with_timeout(exporter_config.timeout);

                if let Some(tls_config) = metadata::build_tls_config(exporter_config)? {
                    builder = builder.with_tls_config(tls_config);
                }

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

        let interval = exporter_config.batch_export.scheduled_delay;
        interval_hint = Some(interval);

        let reader = PeriodicReader::builder(exporter).with_interval(interval).build();
        builder = builder.with_reader(reader);
        exporter_configured = true;

        log::debug!(
            "OTLP metrics exporter initialized to {} via {:?}",
            exporter_config.endpoint,
            exporter_config.protocol
        );
    }

    if let Some(sender) = tui_sender {
        let interval = interval_hint.unwrap_or_else(|| Duration::from_secs(5));
        let reader = PeriodicReader::builder(console::TuiMetricsExporter::new(sender))
            .with_interval(interval)
            .build();
        builder = builder.with_reader(reader);
        exporter_configured = true;
        log::debug!("TUI metrics exporter enabled with interval {:?}", interval);
    }

    if !exporter_configured {
        log::debug!("No metrics exporters configured or enabled, metrics will not be exported");
    }

    Ok(builder.build())
}

fn llm_latency_view() -> impl Fn(&Instrument) -> Option<Stream> + Send + Sync + 'static {
    move |instrument: &Instrument| {
        if instrument.kind() != InstrumentKind::Histogram {
            return None;
        }

        let name = instrument.name();
        if name != GEN_AI_CLIENT_OPERATION_DURATION && name != GEN_AI_CLIENT_TIME_TO_FIRST_TOKEN {
            return None;
        }

        let aggregation = Aggregation::ExplicitBucketHistogram {
            boundaries: llm_latency_buckets(),
            record_min_max: false,
        };

        Stream::builder().with_aggregation(aggregation).build().ok()
    }
}

fn llm_latency_buckets() -> Vec<f64> {
    vec![
        0.01, 0.02, 0.03, 0.05, 0.075, 0.1, 0.15, 0.2, 0.3, 0.4, 0.5, 0.65, 0.8, 1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0,
        4.0, 5.0,
    ]
}

fn build_resource(telemetry_config: &TelemetryConfig) -> Resource {
    let mut builder = Resource::builder();

    if let Some(service_name) = telemetry_config.service_name() {
        builder = builder.with_service_name(service_name.to_string());
    }

    for (key, value) in telemetry_config.resource_attributes() {
        use opentelemetry::{Key, KeyValue, Value};
        builder = builder.with_attribute(KeyValue::new(Key::from(key.clone()), Value::from(value.clone())));
    }

    builder.build()
}
