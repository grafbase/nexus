//! Logger initialization for the server

use logforth::{
    append::{FastraceEvent, Stderr},
    filter::EnvFilter,
};
use std::{str::FromStr, sync::Once};
use telemetry::logs::OtelLogsAppender;

static INIT: Once = Once::new();

/// Initialize the logger with optional OTEL appender
/// The log_filter should be a string like "info" or "server=debug,mcp=debug"
pub fn init(log_filter: &str, otel_appender: Option<OtelLogsAppender>) {
    INIT.call_once(|| {
        // Create filters for each appender
        let mut builder = logforth::builder();

        // Add FastraceEvent appender
        builder = builder.dispatch(|d| {
            let filter = EnvFilter::from_str(log_filter)
                .unwrap_or_else(|_| EnvFilter::from_str("info").expect("default filter should be valid"));

            d.filter(filter).append(FastraceEvent::default())
        });

        // Add OTEL appender if provided
        if let Some(appender) = otel_appender {
            builder = builder.dispatch(|d| {
                // For OTEL appender, exclude opentelemetry crates to prevent recursion
                let filter_str =
                    format!("{log_filter},opentelemetry=off,opentelemetry_sdk=off,opentelemetry_otlp=off",);

                let filter = EnvFilter::from_str(&filter_str).unwrap_or_else(|_| {
                    EnvFilter::from_str("info,opentelemetry=off,opentelemetry_sdk=off,opentelemetry_otlp=off")
                        .expect("default filter should be valid")
                });

                d.filter(filter).append(appender)
            });
        }

        // Add stderr appender for local logging
        builder = builder.dispatch(|d| {
            let filter = EnvFilter::from_str(log_filter)
                .unwrap_or_else(|_| EnvFilter::from_str("info").expect("default filter should be valid"));

            d.filter(filter).append(Stderr::default())
        });

        builder.apply();
    });
}
