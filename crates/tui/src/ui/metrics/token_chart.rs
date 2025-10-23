use std::time::SystemTime;

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Rect},
    style::Style,
    symbols,
    text::{Line, Span},
    widgets::{Axis, Block, Borders, Chart, Dataset, GraphType, LegendPosition, Paragraph},
};

use crate::ui::{TEXT_MUTED, metrics::FlowPalette, snapshots::TokenFlowSnapshot};

pub struct TokenFlowChartRenderer<'a> {
    snapshot: &'a TokenFlowSnapshot,
    palette: &'a FlowPalette,
}

impl<'a> TokenFlowChartRenderer<'a> {
    pub fn new(snapshot: &'a TokenFlowSnapshot, palette: &'a FlowPalette) -> Self {
        Self { snapshot, palette }
    }

    pub fn render(self, frame: &mut Frame<'_>, area: Rect) {
        let chart_data = self.snapshot;

        if chart_data.input_points.is_empty() && chart_data.output_points.is_empty() {
            self.render_empty_chart(frame, area);
            return;
        }

        self.render_populated_chart(frame, area, chart_data);
    }

    fn render_empty_chart(&self, frame: &mut Frame<'_>, area: Rect) {
        let block = self.base_block().title(self.build_title());
        let placeholder = Paragraph::new("No token activity recorded")
            .alignment(Alignment::Center)
            .style(Style::default().fg(TEXT_MUTED))
            .block(block);

        frame.render_widget(placeholder, area);
    }

    fn render_populated_chart(&self, frame: &mut Frame<'_>, area: Rect, chart_data: &TokenFlowSnapshot) {
        let y_max = (chart_data.y_max * 1.1).max(1.0);
        let block = self.base_block().title(self.build_title());
        let datasets = self.create_datasets(chart_data);

        let window_end = chart_data.window_end.unwrap_or_else(SystemTime::now);
        let (x_bounds, x_labels) = super::time_axis_bounds(window_end);
        let y_labels = make_count_labels(y_max);

        let chart = Chart::new(datasets)
            .block(
                block
                    .border_style(Style::default().fg(self.palette.border))
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
            .legend_position(Some(LegendPosition::TopLeft));

        frame.render_widget(chart, area);
    }

    fn base_block(&self) -> Block<'_> {
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.palette.border))
    }

    fn create_datasets(&'a self, chart_data: &'a TokenFlowSnapshot) -> Vec<Dataset<'a>> {
        vec![
            Dataset::default()
                .name("Input")
                .marker(symbols::Marker::Braille)
                .style(Style::default().fg(self.palette.input))
                .graph_type(GraphType::Line)
                .data(&chart_data.input_points),
            Dataset::default()
                .name("Output")
                .marker(symbols::Marker::Braille)
                .style(Style::default().fg(self.palette.output))
                .graph_type(GraphType::Line)
                .data(&chart_data.output_points),
        ]
    }

    fn build_title(&self) -> Line<'static> {
        let prefix = Span::styled("Token Flow  (Σ in ", Style::default().fg(self.palette.title));
        let input_total = Span::styled(
            super::format_count(self.snapshot.total_input_tokens),
            Style::default().fg(self.palette.input),
        );
        let separator = Span::styled(" | Σ out ", Style::default().fg(self.palette.title));
        let output_total = Span::styled(
            super::format_count(self.snapshot.total_output_tokens),
            Style::default().fg(self.palette.output),
        );
        let suffix = Span::styled(")", Style::default().fg(self.palette.title));

        Line::from(vec![prefix, input_total, separator, output_total, suffix])
    }
}

/// Generate Y-axis labels for the token count chart.
fn make_count_labels(max: f64) -> Vec<Line<'static>> {
    let mid = (max / 2.0).round() as u64;
    let max_value = max.round() as u64;
    vec![
        Line::from("0"),
        Line::from(super::format_count(mid)),
        Line::from(super::format_count(max_value)),
    ]
}
