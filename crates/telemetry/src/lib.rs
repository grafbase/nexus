//! Telemetry library for Nexus
//!
//! Provides OpenTelemetry metrics, tracing, and logging integration.

pub mod logs;
mod metadata;
pub mod metrics;
pub mod tracing;

use config::TelemetryConfig;
use opentelemetry_sdk::metrics::SdkMeterProvider;

// Re-export common OpenTelemetry types for metrics
pub use opentelemetry::{
    KeyValue,
    metrics::{
        Counter, Gauge, Histogram, Meter, ObservableCounter, ObservableGauge, ObservableUpDownCounter, UpDownCounter,
    },
};

/// Guard that ensures proper cleanup of telemetry resources
pub struct TelemetryGuard {
    meter_provider: SdkMeterProvider,
    _tracing_guard: Option<tracing::TracingGuard>,
    _logs_guard: Option<logs::LogsGuard>,
    pub(crate) logs_appender: Option<logs::OtelLogsAppender>,
}

impl TelemetryGuard {
    /// Force flush all pending metrics and traces immediately
    /// Useful for tests to ensure telemetry is exported before assertions
    pub fn force_flush(&self) -> anyhow::Result<()> {
        // Flush metrics
        self.meter_provider
            .force_flush()
            .map_err(|e| anyhow::anyhow!("Failed to flush metrics: {}", e))?;

        // Flush traces if enabled
        if let Some(ref guard) = self._tracing_guard {
            guard.force_flush()?;
        }

        // Flush logs if enabled
        if let Some(ref guard) = self._logs_guard {
            guard.force_flush()?;
        }

        Ok(())
    }

    /// Get the logs appender if logs are enabled
    pub fn logs_appender(&self) -> Option<&logs::OtelLogsAppender> {
        self.logs_appender.as_ref()
    }
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        if let Err(e) = self.meter_provider.shutdown() {
            log::error!("Failed to shutdown meter provider: {e}");
        }
        // Tracing guard will clean up on drop automatically
    }
}

/// Initialize telemetry with the given configuration
///
/// Returns a guard that should be kept alive for the duration of the application.
/// When the guard is dropped, telemetry resources will be cleaned up.
pub async fn init(config: &TelemetryConfig) -> anyhow::Result<TelemetryGuard> {
    log::debug!("Telemetry config: tracing enabled = {}", config.tracing_enabled());
    log::debug!("Telemetry config: logs enabled = {}", config.logs_enabled());

    // Initialize metrics if enabled
    let meter_provider = metrics::init_metrics(config).await?;

    // Initialize tracing if enabled (has exporters configured)
    let tracing_guard = if config.tracing_enabled() {
        log::info!("Tracing is enabled, initializing tracing subsystem");
        Some(tracing::init_tracing(config).await?)
    } else {
        log::debug!("Tracing is disabled (no exporters configured)");
        None
    };

    // Initialize logs if enabled (has exporters configured)
    let (logs_appender, logs_guard) = if config.logs_enabled() {
        log::info!("Logs export is enabled, initializing OpenTelemetry logs");
        let (appender, guard) = logs::init_logs(config).await?;
        (Some(appender), Some(guard))
    } else {
        log::debug!("Logs export is disabled (no exporters configured)");
        (None, None)
    };

    Ok(TelemetryGuard {
        meter_provider,
        _tracing_guard: tracing_guard,
        _logs_guard: logs_guard,
        logs_appender,
    })
}
