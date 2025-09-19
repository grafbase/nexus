//! OpenTelemetry logs integration with logforth

use std::{sync::Arc, time::SystemTime};

use anyhow::Result;
use config::{OtlpProtocol, TelemetryConfig};
use fastrace::prelude::*;
use log::{Level, Record};
use logforth::{append::Append, diagnostic::Diagnostic};
use opentelemetry::{
    InstrumentationScope, KeyValue,
    logs::{LogRecord, Logger, LoggerProvider, Severity},
    trace::{SpanId, TraceId},
};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{
    Resource,
    logs::{BatchLogProcessor, LoggerProviderBuilder, SdkLoggerProvider},
};

use crate::metadata;

/// Guard that ensures proper cleanup of logs resources
pub struct LogsGuard {
    provider: SdkLoggerProvider,
}

impl LogsGuard {
    /// Force flush all pending logs immediately
    pub fn force_flush(&self) -> Result<()> {
        self.provider
            .force_flush()
            .map_err(|errs| anyhow::anyhow!("Failed to flush logs: {:?}", errs))
    }
}

impl Drop for LogsGuard {
    fn drop(&mut self) {
        if let Err(e) = self.provider.shutdown() {
            log::error!("Failed to shutdown logs provider: {e}");
        }
    }
}

/// OpenTelemetry logs appender for logforth
#[derive(Clone)]
pub struct OtelLogsAppender {
    provider: Arc<SdkLoggerProvider>,
    scope: InstrumentationScope,
}

impl std::fmt::Debug for OtelLogsAppender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OtelLogsAppender")
            .field("scope", &self.scope.name())
            .finish()
    }
}

impl OtelLogsAppender {
    fn new(provider: SdkLoggerProvider, service_name: String) -> Self {
        let scope = InstrumentationScope::builder(service_name).build();

        Self {
            provider: Arc::new(provider),
            scope,
        }
    }

    fn map_level(level: Level) -> Severity {
        match level {
            Level::Error => Severity::Error,
            Level::Warn => Severity::Warn,
            Level::Info => Severity::Info,
            Level::Debug => Severity::Debug,
            Level::Trace => Severity::Trace,
        }
    }
}

impl Append for OtelLogsAppender {
    fn append(&self, record: &Record<'_>, _diagnostics: &[Box<dyn Diagnostic>]) -> anyhow::Result<()> {
        // Get the current span context if available
        let (trace_id, span_id) = if let Some(span) = SpanContext::current_local_parent() {
            let trace_id = span.trace_id;
            let span_id = span.span_id;

            (
                TraceId::from_bytes(trace_id.0.to_be_bytes()),
                SpanId::from_bytes(span_id.0.to_be_bytes()),
            )
        } else {
            (TraceId::INVALID, SpanId::INVALID)
        };

        // Get a logger from the provider
        let logger = self.provider.logger_with_scope(self.scope.clone());

        // Create the OpenTelemetry log record
        let mut log_record = logger.create_log_record();

        // Set the observed timestamp explicitly to current UTC time
        log_record.set_observed_timestamp(SystemTime::now());

        // Set basic fields
        log_record.set_severity_number(Self::map_level(record.level()));
        log_record.set_severity_text(record.level().as_str());
        log_record.set_body(record.args().to_string().into());

        // Set trace context if available
        if trace_id != TraceId::INVALID {
            log_record.set_trace_context(trace_id, span_id, None);
        }

        // Add source location attributes
        let mut attributes = Vec::new();

        if let Some(module) = record.module_path() {
            attributes.push(("code.namespace", module.to_string()));
        }

        if let Some(file) = record.file() {
            attributes.push(("code.filepath", file.to_string()));

            if let Some(line) = record.line() {
                attributes.push(("code.lineno", line.to_string()));
            }
        }

        if !attributes.is_empty() {
            log_record.add_attributes(attributes);
        }

        // Emit the log record
        logger.emit(log_record);

        Ok(())
    }

    fn flush(&self) -> anyhow::Result<()> {
        // Force flush is handled by the provider
        self.provider
            .force_flush()
            .map_err(|errs| anyhow::anyhow!("Failed to flush logs: {:?}", errs))
    }
}

/// Initialize OpenTelemetry logs
pub async fn init_logs(config: &TelemetryConfig) -> Result<(OtelLogsAppender, LogsGuard)> {
    let otlp_config = config
        .logs_otlp_config()
        .ok_or_else(|| anyhow::anyhow!("OTLP exporter not configured for logs"))?;

    // Build the resource with service name and custom attributes
    let mut resource_builder = Resource::builder();

    resource_builder = resource_builder.with_attribute(KeyValue::new(
        opentelemetry_semantic_conventions::resource::SERVICE_NAME,
        config.service_name().unwrap_or("nexus").to_string(),
    ));

    for (key, value) in config.resource_attributes() {
        resource_builder = resource_builder.with_attribute(KeyValue::new(key.clone(), value.clone()));
    }

    let resource = resource_builder.build();

    // Create OTLP exporter
    let exporter = match otlp_config.protocol {
        OtlpProtocol::Grpc => {
            use opentelemetry_otlp::WithTonicConfig;

            let mut builder = opentelemetry_otlp::LogExporter::builder()
                .with_tonic()
                .with_endpoint(otlp_config.endpoint.to_string())
                .with_timeout(otlp_config.timeout);

            let metadata = metadata::build_metadata(otlp_config)?;
            builder = builder.with_metadata(metadata);

            builder.build()?
        }
        OtlpProtocol::Http => {
            use opentelemetry_otlp::WithHttpConfig;

            let mut builder = opentelemetry_otlp::LogExporter::builder()
                .with_http()
                .with_endpoint(otlp_config.endpoint.to_string())
                .with_timeout(otlp_config.timeout);

            let headers = metadata::build_http_headers(otlp_config)?;
            builder = builder.with_headers(headers);

            builder.build()?
        }
    };

    // Create batch processor with the exporter
    let batch_processor = BatchLogProcessor::builder(exporter).build();

    // Create the logger provider
    let provider = LoggerProviderBuilder::default()
        .with_resource(resource)
        .with_log_processor(batch_processor)
        .build();

    let service_name = config.service_name().unwrap_or("nexus").to_string();
    let appender = OtelLogsAppender::new(provider.clone(), service_name);
    let guard = LogsGuard { provider };

    log::debug!(
        "OTLP logs exporter initialized to {} via {:?}",
        otlp_config.endpoint,
        otlp_config.protocol
    );

    Ok((appender, guard))
}
