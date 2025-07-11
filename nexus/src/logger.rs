use logforth::{
    append::{FastraceEvent, Stdout},
    diagnostic::FastraceDiagnostic,
    layout::{JsonLayout, TextLayout},
};

use crate::args::{Args, LogStyle};

pub(super) fn init(args: &Args) {
    logforth::builder()
        .dispatch(|d| d.filter(args.log_level.env_filter()).append(FastraceEvent::default()))
        .dispatch(|d| {
            let d = d
                .diagnostic(FastraceDiagnostic::default())
                .filter(args.log_level.env_filter());

            match args.log_style {
                LogStyle::Color => d.append(Stdout::default().with_layout(TextLayout::default())),
                LogStyle::Text => d.append(Stdout::default().with_layout(TextLayout::default().no_color())),
                LogStyle::Json => d.append(Stdout::default().with_layout(JsonLayout::default())),
            }
        })
        .apply();
}
