use ratatui::{
    Frame,
    prelude::{Line, Rect, Style},
    style::Color,
    widgets::{Block, Borders, Paragraph},
};

use crate::{app::LogLine, ui::snapshots::LogsSnapshot};

use super::{PANEL_BACKGROUND, PANEL_BORDER_DIM, TEXT_ACCENT, TEXT_MUTED, TEXT_PRIMARY, TIMESTAMP_COLOR};

const LOG_ERROR_COLOR: Color = Color::Rgb(255, 118, 189);
const LOG_WARN_COLOR: Color = Color::Rgb(252, 214, 87);
const LOG_INFO_COLOR: Color = Color::Rgb(108, 220, 255);
const LOG_DEBUG_COLOR: Color = Color::Rgb(178, 246, 217);
const LOG_TRACE_COLOR: Color = Color::Rgb(244, 110, 196);

/// Rendering helper for the logs tab.
#[derive(Default)]
pub(crate) struct Logs;

impl Logs {
    /// Draw the log viewer using the latest buffered lines.
    pub(crate) fn render(&self, snapshot: &LogsSnapshot, frame: &mut Frame<'_>, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(PANEL_BORDER_DIM))
            .title("Logs")
            .title_style(Style::default().fg(TEXT_ACCENT))
            .style(Style::default().bg(PANEL_BACKGROUND));

        if area.height <= 2 {
            frame.render_widget(block, area);
            return;
        }

        let mut lines = snapshot.lines.as_ref().clone();

        if lines.is_empty() {
            lines.push(Line::from(ratatui::prelude::Span::styled(
                "Waiting for logs...",
                Style::default().fg(TEXT_MUTED),
            )));
        }

        let paragraph = Paragraph::new(lines)
            .style(Style::default().fg(TEXT_PRIMARY).bg(PANEL_BACKGROUND))
            .block(block);

        frame.render_widget(paragraph, area);
    }
}

impl LogLine {
    /// Convert a log entry into a colored Ratatui line ready for display.
    pub(crate) fn to_line(&self) -> Line<'static> {
        use ratatui::prelude::Span;

        let mut spans = Vec::with_capacity(5);

        spans.push(Span::styled(
            self.timestamp.clone(),
            Style::default().fg(TIMESTAMP_COLOR),
        ));

        spans.push(Span::raw("  "));

        spans.push(Span::styled(
            format!("{:>5}", self.level.to_string()),
            self.level_style(),
        ));

        spans.push(Span::raw("  "));
        spans.push(Span::styled(self.message.clone(), Style::default().fg(TEXT_PRIMARY)));

        Line::from(spans)
    }

    /// Choose a color palette for the log level column.
    fn level_style(&self) -> Style {
        match self.level {
            log::Level::Error => Style::default()
                .fg(LOG_ERROR_COLOR)
                .add_modifier(ratatui::prelude::Modifier::BOLD),
            log::Level::Warn => Style::default()
                .fg(LOG_WARN_COLOR)
                .add_modifier(ratatui::prelude::Modifier::BOLD),
            log::Level::Info => Style::default().fg(LOG_INFO_COLOR),
            log::Level::Debug => Style::default().fg(LOG_DEBUG_COLOR),
            log::Level::Trace => Style::default().fg(LOG_TRACE_COLOR),
        }
    }
}
