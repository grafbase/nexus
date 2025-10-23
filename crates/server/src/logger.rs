//! Logger initialization for the server

use jiff::{Zoned, tz::TimeZone};
use logforth::{
    append::{Append, FastraceEvent, Stderr},
    filter::EnvFilter,
    layout::Layout,
};
use std::{fmt::Write, io::IsTerminal, str::FromStr, sync::Once};
use telemetry::{
    logs::OtelLogsAppender,
    tracing::{TraceEvent, TraceExportSender},
};
use tokio::sync::mpsc::error::TrySendError;

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
    let log_filter = log_filter.to_owned();
    INIT.call_once(move || apply_logger(log_filter, otel_appender, None));
}

/// Initialize the logger for TUI mode; replaces stderr output with a channel-based appender.
pub fn init_with_tui(log_filter: &str, otel_appender: Option<OtelLogsAppender>, tui_channel: TraceExportSender) {
    let log_filter = log_filter.to_owned();
    INIT.call_once(move || apply_logger(log_filter, otel_appender, Some(tui_channel)));
}

fn apply_logger(log_filter: String, otel_appender: Option<OtelLogsAppender>, tui_channel: Option<TraceExportSender>) {
    let mut builder = logforth::builder();

    // Add FastraceEvent appender
    let filter_for_fastrace = log_filter.clone();
    builder = builder.dispatch(move |d| {
        let filter = EnvFilter::from_str(&filter_for_fastrace)
            .unwrap_or_else(|_| EnvFilter::from_str("info").expect("default filter should be valid"));

        d.filter(filter).append(FastraceEvent::default())
    });

    // Add OTEL appender if provided
    if let Some(appender) = otel_appender {
        let filter_for_otel = log_filter.clone();
        builder = builder.dispatch(move |d| {
            let filter_str =
                format!("{filter_for_otel},opentelemetry=off,opentelemetry_sdk=off,opentelemetry_otlp=off");

            let filter = EnvFilter::from_str(&filter_str).unwrap_or_else(|_| {
                EnvFilter::from_str("info,opentelemetry=off,opentelemetry_sdk=off,opentelemetry_otlp=off")
                    .expect("default filter should be valid")
            });

            d.filter(filter).append(appender)
        });
    }

    match tui_channel {
        Some(channel) => {
            let filter_for_tui = log_filter.clone();
            builder = builder.dispatch(move |d| {
                let filter = EnvFilter::from_str(&filter_for_tui)
                    .unwrap_or_else(|_| EnvFilter::from_str("info").expect("default filter should be valid"));

                d.filter(filter).append(TuiAppender::new(channel))
            });
        }
        None => {
            let filter_for_stderr = log_filter.clone();
            builder = builder.dispatch(move |d| {
                let filter = EnvFilter::from_str(&filter_for_stderr)
                    .unwrap_or_else(|_| EnvFilter::from_str("info").expect("default filter should be valid"));

                let layout = if std::io::stderr().is_terminal() {
                    UtcLayout::new()
                } else {
                    UtcLayout::new().no_color()
                };

                d.filter(filter).append(Stderr::default().with_layout(layout))
            });
        }
    }

    builder.apply();
}

#[derive(Debug)]
struct TuiAppender {
    channel: TraceExportSender,
}

impl TuiAppender {
    fn new(channel: TraceExportSender) -> Self {
        Self { channel }
    }
}

impl Append for TuiAppender {
    fn append(
        &self,
        record: &log::Record<'_>,
        _diagnostics: &[Box<dyn logforth::diagnostic::Diagnostic>],
    ) -> anyhow::Result<()> {
        let timestamp = Zoned::now()
            .with_time_zone(TimeZone::UTC)
            .strftime("%Y-%m-%dT%H:%M:%S%.6fZ");

        let message = record.args().to_string();

        if let Err(err) = self.channel.try_send(TraceEvent::Log {
            timestamp: timestamp.to_string(),
            level: record.level(),
            message,
        }) {
            match err {
                TrySendError::Full(_) => eprintln!("Dropping log event for TUI: channel full"),
                TrySendError::Closed(_) => {
                    // Receiver has been dropped; ignore without spamming stderr during shutdown.
                }
            }
        }

        Ok(())
    }
}
