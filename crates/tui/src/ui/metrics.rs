mod latency_chart;
mod model_table;
mod palette;
mod token_chart;

use std::time::SystemTime;

use crate::app::METRICS_WINDOW;

use crate::ui::{
    metrics::{
        latency_chart::LatencyChartRenderer, model_table::ModelTableRenderer, token_chart::TokenFlowChartRenderer,
    },
    snapshots::MetricsSnapshot,
};
use palette::{FlowPalette, LatencyPalette, ModelPalette, PaletteBundle};
use ratatui::{
    Frame,
    prelude::{Constraint, Direction, Layout, Line, Margin, Rect, Style},
    widgets::Block,
};
use time::OffsetDateTime;

use super::{PANEL_BACKGROUND, TRACE_TIMESTAMP_FORMAT};

/// Rendering helper for the metrics tab.
#[derive(Default)]
pub(crate) struct Metrics;

impl Metrics {
    /// Draw the metrics dashboard or a placeholder when no data is available.
    pub(crate) fn render(&self, snapshot: &MetricsSnapshot, frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(Block::default().style(Style::default().bg(PANEL_BACKGROUND)), area);

        let palettes = PaletteBundle::default();

        let inner = area.inner(Margin {
            horizontal: 0,
            vertical: 0,
        });

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(inner);

        let trend_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(layout[0]);

        let op_latencies =
            LatencyChartRenderer::new("Operation Duration", &snapshot.operation_latency, palettes.operation);

        op_latencies.render(frame, trend_layout[0]);

        let ttft_latencies = LatencyChartRenderer::new("Time to First Token", &snapshot.ttft_latency, palettes.ttft);

        ttft_latencies.render(frame, trend_layout[1]);

        let lower_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(layout[1]);

        let token_flow_chart = TokenFlowChartRenderer::new(&snapshot.token_flow, palettes.flow);
        token_flow_chart.render(frame, lower_layout[0]);

        let model_table_renderer = ModelTableRenderer::new(&snapshot.model_table, palettes.model);
        model_table_renderer.render(frame, lower_layout[1]);
    }
}

/// Compute the number of seconds between two instants as a float for charting.
pub(crate) fn seconds_since(start: SystemTime, end: SystemTime) -> f64 {
    match end.duration_since(start) {
        Ok(duration) => duration.as_secs_f64(),
        Err(_) => 0.0,
    }
}

/// Build X-axis bounds and labels capped to the metrics window.
fn time_axis_bounds(window_end: SystemTime) -> ([f64; 2], Vec<Line<'static>>) {
    let window_start = window_end.checked_sub(METRICS_WINDOW).unwrap_or(window_end);
    let duration = METRICS_WINDOW.as_secs_f64().max(1.0);

    let start_label = format_time_label(window_start);
    let end_label = format_time_label(window_end);

    let labels = vec![Line::from(start_label), Line::from(""), Line::from(end_label)];
    ([0.0, duration], labels)
}

/// Format a timestamp for display on the charts' X-axis.
fn format_time_label(timestamp: SystemTime) -> String {
    OffsetDateTime::from(timestamp)
        .format(&TRACE_TIMESTAMP_FORMAT)
        .unwrap_or_else(|_| "".to_string())
}

/// Pretty-print large counts using unit suffixes.
pub(crate) fn format_count(value: u64) -> String {
    match value {
        0..=999 => value.to_string(),
        1_000..=999_999 => format!("{:.1}k", value as f64 / 1000.0),
        1_000_000..=999_999_999 => format!("{:.1}M", value as f64 / 1_000_000.0),
        _ => format!("{:.1}B", value as f64 / 1_000_000_000.0),
    }
}
