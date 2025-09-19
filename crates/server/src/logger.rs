//! Logger initialization for the server

use jiff::{Zoned, tz::TimeZone};
use logforth::{
    append::{FastraceEvent, Stderr},
    filter::EnvFilter,
    layout::Layout,
};
use std::{fmt::Write, io::IsTerminal, str::FromStr, sync::Once};
use telemetry::logs::OtelLogsAppender;

static INIT: Once = Once::new();

/// Custom layout that formats timestamps in UTC
#[derive(Debug)]
struct UtcLayout {
    no_color: bool,
}

impl UtcLayout {
    fn new() -> Self {
        Self { no_color: false }
    }

    fn no_color(mut self) -> Self {
        self.no_color = true;
        self
    }
}

impl Layout for UtcLayout {
    fn format(
        &self,
        record: &log::Record<'_>,
        _diagnostics: &[Box<dyn logforth::diagnostic::Diagnostic>],
    ) -> anyhow::Result<Vec<u8>> {
        let mut output = String::new();

        // Get current time in UTC
        let now = Zoned::now().with_time_zone(TimeZone::UTC);

        // Format timestamp with Z suffix to indicate UTC
        write!(output, "{} ", now.strftime("%Y-%m-%dT%H:%M:%S%.6fZ"))?;

        // Add log level with colors
        let level_str = if self.no_color {
            format!("{:>5}", record.level())
        } else {
            match record.level() {
                log::Level::Error => format!("\x1b[31m{:>5}\x1b[0m", record.level()),
                log::Level::Warn => format!("\x1b[33m{:>5}\x1b[0m", record.level()),
                log::Level::Info => format!("\x1b[32m{:>5}\x1b[0m", record.level()),
                log::Level::Debug => format!("\x1b[34m{:>5}\x1b[0m", record.level()),
                log::Level::Trace => format!("\x1b[35m{:>5}\x1b[0m", record.level()),
            }
        };

        write!(output, "{}  ", level_str)?;

        // Add the message
        write!(output, "{}", record.args())?;

        Ok(output.into_bytes())
    }
}

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

        // Add stderr appender for local logging with UTC timestamps
        builder = builder.dispatch(|d| {
            let filter = EnvFilter::from_str(log_filter)
                .unwrap_or_else(|_| EnvFilter::from_str("info").expect("default filter should be valid"));

            // Detect if stderr is a TTY to determine if colors should be used
            let layout = if std::io::stderr().is_terminal() {
                UtcLayout::new()
            } else {
                UtcLayout::new().no_color()
            };

            d.filter(filter).append(Stderr::default().with_layout(layout))
        });

        builder.apply();
    });
}
