use std::fmt::Write;

use jiff::{Zoned, tz::TimeZone};
use log::Record;
use logforth::{
    append::{FastraceEvent, Stdout},
    diagnostic::FastraceDiagnostic,
    layout::{JsonLayout, Layout},
};

use crate::args::{Args, LogStyle};

#[derive(Debug, Clone)]
struct CustomTextLayout {
    no_color: bool,
}

impl CustomTextLayout {
    fn new() -> Self {
        Self { no_color: false }
    }

    fn no_color(mut self) -> Self {
        self.no_color = true;
        self
    }
}

impl Layout for CustomTextLayout {
    fn format(
        &self,
        record: &Record<'_>,
        _diagnostics: &[Box<dyn logforth::diagnostic::Diagnostic>],
    ) -> anyhow::Result<Vec<u8>> {
        let mut output = String::new();
        let now = Zoned::now().with_time_zone(TimeZone::UTC);

        write!(output, "{} ", now.strftime("%Y-%m-%dT%H:%M:%S%.6fZ"))?;

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

        write!(output, "{level_str}  ")?;
        write!(output, "{}", record.args())?;

        Ok(output.into_bytes())
    }
}

pub(super) fn init(args: &Args) {
    logforth::builder()
        .dispatch(|d| d.filter(args.log_level.env_filter()).append(FastraceEvent::default()))
        .dispatch(|d| {
            let d = d
                .diagnostic(FastraceDiagnostic::default())
                .filter(args.log_level.env_filter());

            match args.log_style {
                LogStyle::Color => d.append(Stdout::default().with_layout(CustomTextLayout::new())),
                LogStyle::Text => d.append(Stdout::default().with_layout(CustomTextLayout::new().no_color())),
                LogStyle::Json => d.append(Stdout::default().with_layout(JsonLayout::default())),
            }
        })
        .apply();
}
