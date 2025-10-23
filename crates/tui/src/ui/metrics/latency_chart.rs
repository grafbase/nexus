use std::time::SystemTime;

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Rect},
    style::Style,
    symbols,
    text::Line,
    widgets::{Axis, Block, Borders, Chart, Dataset, GraphType, LegendPosition, Paragraph},
};

use crate::ui::{
    TEXT_MUTED,
    metrics::{LatencyPalette, time_axis_bounds},
    snapshots::LatencyChartSnapshot,
};

/// A renderer for displaying latency metrics as a time-series chart.
///
/// The chart plots the rolling average latency over time using the ratatui
/// charting widgets, while the title surfaces aggregate percentile statistics
/// for the full window.
pub struct LatencyChartRenderer<'a> {
    /// The title displayed at the top of the chart
    title: &'static str,
    /// Precomputed latency data for rendering.
    snapshot: &'a LatencyChartSnapshot,
    /// Color palette for styling the chart components
    palette: &'a LatencyPalette,
}

impl<'a> LatencyChartRenderer<'a> {
    /// Creates a new latency chart renderer.
    pub fn new(title: &'static str, snapshot: &'a LatencyChartSnapshot, palette: &'a LatencyPalette) -> Self {
        Self {
            title,
            snapshot,
            palette,
        }
    }

    /// Renders the latency chart to the given frame area.
    pub fn render(self, frame: &mut Frame<'_>, area: Rect) {
        if self.snapshot.window_end.is_none() {
            self.render_empty_chart(frame, area);
            return;
        }

        let chart = self.build_chart();
        frame.render_widget(chart, area);
    }

    /// Renders a placeholder when no data is available
    fn render_empty_chart(&self, frame: &mut Frame<'_>, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.palette.border))
            .title(format!("{}  (no samples yet)", self.title))
            .title_style(Style::default().fg(self.palette.title));

        let placeholder = Paragraph::new("No samples in timeframe")
            .alignment(Alignment::Center)
            .style(Style::default().fg(TEXT_MUTED));

        frame.render_widget(placeholder.block(block), area);
    }

    /// Builds the complete chart widget from chart data
    fn build_chart(&'a self) -> Chart<'a> {
        let mut y_max = self.snapshot.y_max;
        y_max = (y_max * 1.15).max(1.0);

        let block_title = self.build_title();
        let window_end = self.snapshot.window_end.unwrap_or_else(SystemTime::now);
        let (x_bounds, x_labels) = time_axis_bounds(window_end);
        let y_labels = make_latency_labels(y_max);

        let datasets = self.build_datasets();

        Chart::new(datasets)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(self.palette.border))
                    .title(block_title)
                    .title_style(Style::default().fg(self.palette.title)),
            )
            .x_axis(
                Axis::default()
                    .bounds(x_bounds)
                    .labels(x_labels)
                    .style(Style::default().fg(self.palette.axis)),
            )
            .y_axis(
                Axis::default()
                    .bounds([0.0, y_max])
                    .labels(y_labels)
                    .style(Style::default().fg(self.palette.axis)),
            )
            .hidden_legend_constraints((Constraint::Ratio(1, 3), Constraint::Percentage(40)))
            .legend_position(Some(LegendPosition::TopLeft))
    }

    /// Builds the chart title with optional summary statistics
    fn build_title(&self) -> String {
        match &self.snapshot.summary {
            Some(summary) => {
                let title = self.title;
                let p50 = format_latency(summary.p50_ms);
                let p95 = format_latency(summary.p95_ms);
                let p99 = format_latency(summary.p99_ms);

                format!("{title}  (P50 {p50} | P95 {p95} | P99 {p99})")
            }
            None => self.title.to_string(),
        }
    }

    /// Creates the datasets for the chart from chart data
    fn build_datasets(&'a self) -> Vec<Dataset<'a>> {
        vec![
            Dataset::default()
                .name("Average")
                .marker(symbols::Marker::Braille)
                .style(Style::default().fg(self.palette.series[0]))
                .graph_type(GraphType::Line)
                .data(&self.snapshot.average_points),
        ]
    }
}

/// Generate human-friendly Y-axis labels for latency graphs.
fn make_latency_labels(max: f64) -> Vec<Line<'static>> {
    let mid = (max / 2.0).max(1.0);
    vec![
        Line::from("0 ms"),
        Line::from(format!("{:.0} ms", mid)),
        Line::from(format!("{:.0} ms", max)),
    ]
}

/// Display milliseconds as a human-readable value with automatic units.
fn format_latency(ms: f64) -> String {
    if ms >= 1000.0 {
        format!("{:.2} s", ms / 1000.0)
    } else if ms >= 1.0 {
        format!("{:.0} ms", ms)
    } else {
        format!("{:.2} ms", ms)
    }
}
