use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
};

use crate::ui::{PANEL_BACKGROUND, TEXT_MUTED, TEXT_PRIMARY, metrics::ModelPalette, snapshots::ModelTableSnapshot};

/// A renderer for displaying model token usage data in a table format.
///
/// This renderer creates a styled table showing token usage statistics for different models,
/// including input tokens, output tokens, and total tokens. It uses the ratatui library
/// for terminal UI rendering.
pub struct ModelTableRenderer<'a> {
    /// The model token usage data to display in rows
    rows: &'a ModelTableSnapshot,
    /// Color palette for styling the table
    palette: &'a ModelPalette,
}

impl<'a> ModelTableRenderer<'a> {
    /// Creates a new ModelTableRenderer with the given data and styling palette.
    pub fn new(rows: &'a ModelTableSnapshot, palette: &'a ModelPalette) -> Self {
        Self { rows, palette }
    }

    /// Renders the model table to the given frame and area.
    pub fn render(self, frame: &mut Frame<'_>, area: Rect) {
        if self.rows.rows.is_empty() {
            self.render_placeholder(frame, area);
            return;
        }

        let table = self.build_table();
        frame.render_widget(table, area);
    }

    /// Renders a placeholder message when no token usage data is available.
    fn render_placeholder(&self, frame: &mut Frame<'_>, area: Rect) {
        let placeholder = Paragraph::new("No token usage recorded")
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(self.palette.border))
                    .title("Top Models")
                    .title_style(Style::default().fg(self.palette.title))
                    .style(Style::default().bg(PANEL_BACKGROUND)),
            )
            .alignment(Alignment::Center)
            .style(Style::default().fg(TEXT_MUTED).bg(PANEL_BACKGROUND));

        frame.render_widget(placeholder, area);
    }

    /// Builds the complete table widget with headers, rows, and styling.
    fn build_table(&'a self) -> Table<'a> {
        let header = self.build_header();
        let rows = self.build_rows();

        Table::new(
            rows,
            [
                Constraint::Percentage(55),
                Constraint::Percentage(15),
                Constraint::Percentage(15),
                Constraint::Percentage(15),
            ],
        )
        .column_spacing(1)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(self.palette.border))
                .title("Top Models (total tokens)")
                .title_style(Style::default().fg(self.palette.title))
                .style(Style::default().bg(PANEL_BACKGROUND)),
        )
        .style(Style::default().fg(TEXT_PRIMARY).bg(PANEL_BACKGROUND))
    }

    /// Builds the header row for the table.
    ///
    /// # Returns
    ///
    /// A styled Row containing the column headers
    fn build_header(&'a self) -> Row<'a> {
        Row::new(vec!["Model", "Input", "Output", "Total"]).style(
            Style::default()
                .fg(self.palette.title)
                .add_modifier(Modifier::BOLD)
                .bg(PANEL_BACKGROUND),
        )
    }

    /// Builds the data rows for the table from the model token usage data.
    ///
    /// Takes up to 8 rows of data and formats them for display with appropriate styling.
    ///
    /// # Returns
    ///
    /// A vector of styled Row widgets containing the model data
    fn build_rows(&'a self) -> Vec<Row<'a>> {
        self.rows
            .rows
            .iter()
            .take(8)
            .map(|model| {
                Row::new(vec![
                    Cell::from(shorten_label(&model.model, 28)).style(Style::default().fg(self.palette.label)),
                    Cell::from(super::format_count(model.input_tokens)).style(Style::default().fg(self.palette.label)),
                    Cell::from(super::format_count(model.output_tokens)).style(Style::default().fg(self.palette.label)),
                    Cell::from(super::format_count(model.total_tokens)).style(Style::default().fg(self.palette.label)),
                ])
            })
            .collect::<Vec<_>>()
    }
}

/// Trim model names to fit within the token table column width.
fn shorten_label(label: &str, max_len: usize) -> String {
    let char_count = label.chars().count();
    if char_count <= max_len {
        return label.to_string();
    }

    let mut result = String::new();
    for (idx, ch) in label.chars().enumerate() {
        if idx + 1 >= max_len {
            break;
        }
        result.push(ch);
    }

    result.push('…');
    result
}
