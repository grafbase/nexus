use std::sync::Arc;

use fastrace::collector::{SpanId, TraceId};
use ratatui::{
    Frame,
    prelude::{Alignment, Constraint, Direction, Layout, Modifier, Rect, Style},
    widgets::{Block, Borders, Cell, List, ListItem, ListState, Paragraph, Row, Table, TableState, Wrap},
};

use crate::ui::snapshots::{TimelineSnapshot, TraceRenderSnapshot, TracesSnapshot};

use super::{
    PANEL_BACKGROUND, PANEL_BORDER_ACTIVE, PANEL_BORDER_DIM, SELECTION_BG, SELECTION_FG, TEXT_ACCENT, TEXT_MUTED,
    TEXT_PRIMARY, TRACE_ROOT_COLOR, contains_point,
};

/// Rendering controller for the traces tab, keeping list/timeline state.
#[derive(Default)]
pub(crate) struct Traces {
    detail_focus: DetailFocus,
    trace_list_inner: Option<Rect>,
    trace_list_state: ListState,
    timeline_inner: Option<Rect>,
    timeline_state: TableState,
    current_span_ids: Vec<SpanId>,
    selected_span: Option<SpanId>,
    selected_trace_id: Option<TraceId>,
}

impl Traces {
    /// Prepare the traces tab for interaction after new data arrives.
    pub(crate) fn activate(&mut self, snapshot: &TracesSnapshot) {
        self.detail_focus = DetailFocus::TraceList;
        self.on_snapshot_changed(snapshot);
    }

    /// Render the traces list, timeline, and attribute panels.
    pub(crate) fn render(&mut self, snapshot: &TracesSnapshot, frame: &mut Frame<'_>, area: Rect) {
        let title = format!("Traces • {}", snapshot.traces.len());
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .title_style(Style::default().fg(TEXT_ACCENT))
            .border_style(Style::default().fg(PANEL_BORDER_DIM))
            .style(Style::default().bg(PANEL_BACKGROUND));
        frame.render_widget(block.clone(), area);

        let inner = block.inner(area);
        if inner.width < 4 || inner.height < 4 {
            return;
        }

        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
            .split(inner);

        self.render_trace_list(snapshot, frame, columns[0]);
        self.render_trace_detail(snapshot, frame, columns[1]);
    }

    /// Refresh selection and caches when the underlying trace set changes.
    pub(crate) fn on_snapshot_changed(&mut self, snapshot: &TracesSnapshot) {
        if snapshot.traces.is_empty() {
            self.trace_list_state.select(None);
            self.selected_trace_id = None;
            self.selected_span = None;
            self.current_span_ids.clear();
            self.timeline_state.select(None);
            return;
        }

        let num_of_traces = snapshot.traces.len();

        let target = match self.trace_list_state.selected() {
            Some(index) if index < num_of_traces => index,
            Some(_) => num_of_traces.saturating_sub(1),
            None => 0,
        };

        self.trace_list_state.select(Some(target));
        self.ensure_trace_selection_visible();

        let new_trace_id = self.selected_trace(snapshot).map(|trace| trace.trace_id);
        self.on_trace_selection_changed(new_trace_id);
    }

    /// Move focus forward through the trace list, timeline, and attribute panes.
    pub(crate) fn focus_next_section(&mut self) {
        self.detail_focus = self.detail_focus.next();
    }

    /// Move focus backward through the trace sub-sections.
    pub(crate) fn focus_previous_section(&mut self) {
        self.detail_focus = self.detail_focus.previous();
    }

    /// Handle keyboard navigation that scrolls the active panel.
    pub(crate) fn handle_vertical_navigation(&mut self, snapshot: &TracesSnapshot, delta: isize) {
        match self.detail_focus {
            DetailFocus::TraceList => self.move_trace_selection(snapshot, delta),
            DetailFocus::Timeline => self.move_timeline_selection(snapshot, delta),
            DetailFocus::Attributes => {}
        }
    }

    /// React to mouse clicks inside the traces tab, forwarding to the timeline
    /// or trace list depending on the coordinates.
    pub(crate) fn handle_mouse_click(&mut self, snapshot: &TracesSnapshot, column: u16, row: u16) {
        if self.handle_timeline_click(snapshot, column, row) {
            return;
        }
        let _ = self.handle_trace_click(snapshot, column, row);
    }

    /// Draw the left-hand trace list with selection highlighting.
    fn render_trace_list(&mut self, snapshot: &TracesSnapshot, frame: &mut Frame<'_>, area: Rect) {
        let mut block = Block::default()
            .title("Traces")
            .borders(Borders::ALL)
            .title_style(Style::default().fg(TEXT_ACCENT))
            .style(Style::default().bg(PANEL_BACKGROUND))
            .border_style(Style::default().fg(PANEL_BORDER_DIM));
        if self.detail_focus == DetailFocus::TraceList {
            block = block.border_style(Style::default().fg(PANEL_BORDER_ACTIVE));
        }
        let inner = block.inner(area);
        self.trace_list_inner = Some(inner);

        if snapshot.traces.is_empty() {
            let placeholder = Paragraph::new("Waiting for traces...")
                .alignment(Alignment::Center)
                .style(Style::default().fg(TEXT_MUTED).bg(PANEL_BACKGROUND))
                .block(block);
            frame.render_widget(placeholder, area);
            return;
        }

        if self.trace_list_state.selected().is_none() {
            self.trace_list_state.select(Some(0));
        }

        self.ensure_trace_selection_visible();

        let items: Vec<ListItem<'static>> = snapshot
            .traces
            .iter()
            .rev()
            .map(|trace| ListItem::new(trace.list_line.clone()))
            .collect();

        let list = List::new(items)
            .block(block)
            .style(Style::default().fg(TEXT_PRIMARY).bg(PANEL_BACKGROUND))
            .highlight_style(self.trace_list_highlight_style())
            .highlight_symbol("> ");

        frame.render_stateful_widget(list, area, &mut self.trace_list_state);
    }

    /// Render the summary, timeline, and attribute panes for the selected trace.
    fn render_trace_detail(&mut self, snapshot: &TracesSnapshot, frame: &mut Frame<'_>, area: Rect) {
        let block = Block::default()
            .title("Trace detail")
            .borders(Borders::ALL)
            .title_style(Style::default().fg(TEXT_ACCENT))
            .border_style(Style::default().fg(PANEL_BORDER_DIM))
            .style(Style::default().bg(PANEL_BACKGROUND));

        frame.render_widget(block.clone(), area);
        let inner = block.inner(area);
        if inner.width < 4 || inner.height < 6 {
            return;
        }

        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(4), Constraint::Min(5), Constraint::Min(4)])
            .split(inner);

        if snapshot.traces.is_empty() {
            self.render_empty_detail_sections(
                frame,
                &sections,
                "Waiting for traces...",
                "Timeline will appear once trace data is available",
                "Attributes populate when spans arrive",
            );
            return;
        }

        let Some(actual_index) = self.selected_trace_actual_index(snapshot) else {
            self.render_empty_detail_sections(
                frame,
                &sections,
                "Select a trace from the list",
                "Timeline will populate after choosing a trace",
                "Select a span to view its attributes",
            );
            return;
        };

        let trace = &snapshot.traces[actual_index];

        let summary = Paragraph::new(trace.summary.as_ref().clone())
            .wrap(Wrap { trim: true })
            .style(Style::default().fg(TEXT_PRIMARY).bg(PANEL_BACKGROUND))
            .block(Block::default().style(Style::default().bg(PANEL_BACKGROUND)));
        frame.render_widget(summary, sections[0]);

        self.prepare_timeline_for_index(snapshot, actual_index);
        self.render_timeline(trace, frame, sections[1]);

        let attribute_rows = self.attribute_rows(trace);
        let mut attr_block = Block::default()
            .title("Attributes")
            .borders(Borders::ALL)
            .title_style(Style::default().fg(TEXT_ACCENT))
            .border_style(Style::default().fg(PANEL_BORDER_DIM))
            .style(Style::default().bg(PANEL_BACKGROUND));
        if self.detail_focus == DetailFocus::Attributes {
            attr_block = attr_block.border_style(Style::default().fg(PANEL_BORDER_ACTIVE));
        }
        if sections[2].height < 3 {
            let placeholder = Paragraph::new("Attributes area too small")
                .alignment(Alignment::Center)
                .block(attr_block);
            frame.render_widget(placeholder, sections[2]);
            return;
        }

        match attribute_rows {
            Some(rows) => {
                let table = Table::new(rows.as_ref().clone(), [Constraint::Length(24), Constraint::Min(10)])
                    .column_spacing(1)
                    .style(Style::default().fg(TEXT_PRIMARY).bg(PANEL_BACKGROUND))
                    .block(attr_block);
                frame.render_widget(table, sections[2]);
            }
            None => {
                let placeholder = Paragraph::new("Select a span to view attributes")
                    .alignment(Alignment::Center)
                    .style(Style::default().fg(TEXT_MUTED).bg(PANEL_BACKGROUND))
                    .block(attr_block);
                frame.render_widget(placeholder, sections[2]);
            }
        }
    }

    fn render_empty_detail_sections(
        &mut self,
        frame: &mut Frame<'_>,
        sections: &[Rect],
        summary_message: &str,
        timeline_message: &str,
        attributes_message: &str,
    ) {
        if sections.len() < 3 {
            return;
        }

        let summary = Paragraph::new(summary_message)
            .alignment(Alignment::Center)
            .style(Style::default().fg(TEXT_MUTED).bg(PANEL_BACKGROUND))
            .block(Block::default().style(Style::default().bg(PANEL_BACKGROUND)));
        frame.render_widget(summary, sections[0]);

        self.render_timeline_placeholder(frame, sections[1], timeline_message);
        self.render_attributes_placeholder(frame, sections[2], attributes_message);
    }

    fn render_timeline_placeholder(&mut self, frame: &mut Frame<'_>, area: Rect, message: &str) {
        let mut block = Block::default()
            .title("Timeline")
            .borders(Borders::ALL)
            .title_style(Style::default().fg(TEXT_ACCENT))
            .border_style(Style::default().fg(PANEL_BORDER_DIM))
            .style(Style::default().bg(PANEL_BACKGROUND));
        if self.detail_focus == DetailFocus::Timeline {
            block = block.border_style(Style::default().fg(PANEL_BORDER_ACTIVE));
        }

        let inner = block.inner(area);
        self.timeline_inner = Some(inner);

        if inner.height < 3 {
            let placeholder = Paragraph::new("Timeline area too small")
                .alignment(Alignment::Center)
                .style(Style::default().fg(TEXT_MUTED).bg(PANEL_BACKGROUND))
                .block(block);
            frame.render_widget(placeholder, area);
            return;
        }

        let placeholder = Paragraph::new(message)
            .alignment(Alignment::Center)
            .style(Style::default().fg(TEXT_MUTED).bg(PANEL_BACKGROUND))
            .block(block);
        frame.render_widget(placeholder, area);
    }

    fn render_attributes_placeholder(&self, frame: &mut Frame<'_>, area: Rect, message: &str) {
        let mut block = Block::default()
            .title("Attributes")
            .borders(Borders::ALL)
            .title_style(Style::default().fg(TEXT_ACCENT))
            .border_style(Style::default().fg(PANEL_BORDER_DIM))
            .style(Style::default().bg(PANEL_BACKGROUND));
        if self.detail_focus == DetailFocus::Attributes {
            block = block.border_style(Style::default().fg(PANEL_BORDER_ACTIVE));
        }

        if area.height < 3 {
            let placeholder = Paragraph::new("Attributes area too small")
                .alignment(Alignment::Center)
                .block(block);
            frame.render_widget(placeholder, area);
            return;
        }

        let placeholder = Paragraph::new(message)
            .alignment(Alignment::Center)
            .style(Style::default().fg(TEXT_MUTED).bg(PANEL_BACKGROUND))
            .block(block);
        frame.render_widget(placeholder, area);
    }

    /// Draw the timeline table for the currently selected trace.
    fn render_timeline(&mut self, trace: &TraceRenderSnapshot, frame: &mut Frame<'_>, area: Rect) {
        let mut block = Block::default()
            .title("Timeline")
            .borders(Borders::ALL)
            .title_style(Style::default().fg(TEXT_ACCENT))
            .border_style(Style::default().fg(PANEL_BORDER_DIM))
            .style(Style::default().bg(PANEL_BACKGROUND));
        if self.detail_focus == DetailFocus::Timeline {
            block = block.border_style(Style::default().fg(PANEL_BORDER_ACTIVE));
        }

        let inner = block.inner(area);
        self.timeline_inner = Some(inner);

        if inner.height < 3 {
            let placeholder = Paragraph::new("Timeline area too small")
                .alignment(Alignment::Center)
                .style(Style::default().fg(TEXT_MUTED).bg(PANEL_BACKGROUND))
                .block(block);
            frame.render_widget(placeholder, area);
            return;
        }

        if trace.timeline.items.is_empty() {
            let placeholder = Paragraph::new("Select a trace with spans")
                .alignment(Alignment::Center)
                .style(Style::default().fg(TEXT_MUTED).bg(PANEL_BACKGROUND))
                .block(block);
            frame.render_widget(placeholder, area);
            return;
        }

        let column_widths = [
            Constraint::Percentage(45),
            Constraint::Percentage(45),
            Constraint::Percentage(10),
        ];
        let (rows, span_ids) = self.build_timeline_rows(&trace.timeline, inner.width);
        self.current_span_ids = span_ids;

        let table = Table::new(rows, column_widths)
            .column_spacing(1)
            .style(Style::default().fg(TEXT_PRIMARY).bg(PANEL_BACKGROUND))
            .block(block)
            .highlight_symbol("> ")
            .row_highlight_style(self.timeline_highlight_style());

        frame.render_stateful_widget(table, area, &mut self.timeline_state);
    }

    /// Style used when the trace list has keyboard focus.
    fn trace_list_highlight_style(&self) -> Style {
        if self.detail_focus == DetailFocus::TraceList {
            Style::default()
                .fg(SELECTION_FG)
                .bg(SELECTION_BG)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(TEXT_ACCENT).bg(SELECTION_BG)
        }
    }

    fn build_timeline_rows(&self, timeline: &TimelineSnapshot, width: u16) -> (Vec<Row<'static>>, Vec<SpanId>) {
        if timeline.items.is_empty() {
            return (
                vec![Row::new(vec![
                    Cell::from("No spans collected"),
                    Cell::from(""),
                    Cell::from(""),
                ])],
                Vec::new(),
            );
        }

        let total_ns = timeline
            .items
            .iter()
            .map(|item| item.offset_ns + item.duration_ns)
            .max()
            .unwrap_or(0);

        let available_width = width as usize;
        let bar_width = available_width.saturating_sub(38).clamp(10, 80);
        let name_width = available_width.saturating_sub(bar_width + 12).max(16);

        let mut span_ids = Vec::with_capacity(timeline.items.len());
        let rows = timeline
            .items
            .iter()
            .map(|item| {
                span_ids.push(item.span_id);
                let indent = "  ".repeat(item.depth);
                let raw_name = format!("{indent}{}", item.name);
                let display_name = truncate_with_ellipsis(&raw_name, name_width);
                let bar = build_timeline_bar(item.offset_ns, item.duration_ns, total_ns, bar_width);
                let duration = format_duration_ns(item.duration_ns);
                let style = if item.depth == 0 {
                    Style::default().fg(TRACE_ROOT_COLOR).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(TEXT_PRIMARY)
                };

                Row::new(vec![Cell::from(display_name), Cell::from(bar), Cell::from(duration)]).style(style)
            })
            .collect();

        (rows, span_ids)
    }

    /// Style that highlights the selected timeline row.
    fn timeline_highlight_style(&self) -> Style {
        if self.detail_focus == DetailFocus::Timeline {
            Style::default()
                .fg(SELECTION_FG)
                .bg(SELECTION_BG)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(TEXT_ACCENT).bg(SELECTION_BG)
        }
    }

    /// Convert cached timeline items into table rows sized for the given width.
    fn attribute_rows(&self, trace: &TraceRenderSnapshot) -> Option<Arc<Vec<Row<'static>>>> {
        let span_id = self.selected_span?;
        trace
            .attributes
            .iter()
            .find(|entry| entry.span_id == span_id)
            .map(|entry| entry.rows.clone())
    }

    /// Scroll the trace list selection by `delta` rows.
    fn move_trace_selection(&mut self, snapshot: &TracesSnapshot, delta: isize) {
        if snapshot.traces.is_empty() {
            self.trace_list_state.select(None);
            self.on_trace_selection_changed(None);
            return;
        }

        let len = snapshot.traces.len() as isize;
        let current = self.trace_list_state.selected().map(|idx| idx as isize).unwrap_or(0);
        let new_index = (current + delta).clamp(0, len - 1) as usize;
        self.trace_list_state.select(Some(new_index));
        self.ensure_trace_selection_visible();
        let new_trace_id = self.selected_trace(snapshot).map(|trace| trace.trace_id);
        self.on_trace_selection_changed(new_trace_id);
    }

    /// Move the timeline selection caret by `delta` rows.
    fn move_timeline_selection(&mut self, snapshot: &TracesSnapshot, delta: isize) {
        if self.selected_trace(snapshot).is_none() {
            self.timeline_state.select(None);
            self.selected_span = None;
            return;
        }

        if self.current_span_ids.is_empty() {
            self.timeline_state.select(None);
            self.selected_span = None;
            return;
        }

        let len = self.current_span_ids.len() as isize;
        let current = self.timeline_state.selected().map(|idx| idx as isize).unwrap_or(0);
        let new_index = (current + delta).clamp(0, len - 1) as usize;
        self.timeline_state.select(Some(new_index));
        if let Some(span_id) = self.current_span_ids.get(new_index) {
            self.selected_span = Some(*span_id);
        }
        self.ensure_timeline_selection_visible();
    }

    /// Adjust the list viewport to keep the selected trace in view.
    fn ensure_trace_selection_visible(&mut self) {
        let Some(inner) = self.trace_list_inner else {
            return;
        };

        let visible_rows = inner.height as usize;
        if visible_rows == 0 {
            return;
        }

        let Some(selected) = self.trace_list_state.selected() else {
            return;
        };

        let offset = self.trace_list_state.offset();

        if selected < offset {
            *self.trace_list_state.offset_mut() = selected;
        } else if selected >= offset + visible_rows {
            let new_offset = selected + 1 - visible_rows;
            *self.trace_list_state.offset_mut() = new_offset;
        }
    }

    /// Maintain the timeline scroll offset so the selected span stays on screen.
    fn ensure_timeline_selection_visible(&mut self) {
        let Some(inner) = self.timeline_inner else {
            return;
        };
        let visible_rows = inner.height as usize;
        if visible_rows == 0 {
            return;
        }
        let Some(selected) = self.timeline_state.selected() else {
            return;
        };

        let offset = self.timeline_state.offset();
        if selected < offset {
            *self.timeline_state.offset_mut() = selected;
        } else if selected >= offset + visible_rows {
            let new_offset = selected + 1 - visible_rows;
            *self.timeline_state.offset_mut() = new_offset;
        }
    }

    /// Return the `TraceEntry` currently under the cursor in the UI.
    fn selected_trace<'a>(&self, snapshot: &'a TracesSnapshot) -> Option<&'a TraceRenderSnapshot> {
        let index = self.trace_list_state.selected()?;
        self.trace_by_display_index(snapshot, index)
    }

    /// Resolve the selected display index into an index inside `app.traces`.
    fn selected_trace_actual_index(&self, snapshot: &TracesSnapshot) -> Option<usize> {
        let index = self.trace_list_state.selected()?;
        if snapshot.traces.is_empty() || index >= snapshot.traces.len() {
            return None;
        }
        Some(snapshot.traces.len() - 1 - index)
    }

    /// Map a trace list row to the underlying `TraceEntry`.
    fn trace_by_display_index<'a>(
        &self,
        snapshot: &'a TracesSnapshot,
        index: usize,
    ) -> Option<&'a TraceRenderSnapshot> {
        if snapshot.traces.is_empty() || index >= snapshot.traces.len() {
            return None;
        }
        let actual_index = snapshot.traces.len() - 1 - index;
        snapshot.traces.get(actual_index)
    }

    /// Load timeline data for the given trace index and reset selection.
    fn prepare_timeline_for_index(&mut self, snapshot: &TracesSnapshot, actual_index: usize) {
        let Some(trace) = snapshot.traces.get(actual_index) else {
            self.timeline_state.select(None);
            self.selected_span = None;
            self.current_span_ids.clear();
            return;
        };

        if trace.timeline.items.is_empty() {
            self.timeline_state.select(None);
            self.selected_span = None;
            self.current_span_ids.clear();
            return;
        }

        let span_ids: Vec<SpanId> = trace.timeline.items.iter().map(|item| item.span_id).collect();
        if let Some(selected_span) = self.selected_span {
            if let Some(index) = span_ids.iter().position(|id| *id == selected_span) {
                self.timeline_state.select(Some(index));
            } else {
                self.timeline_state.select(Some(0));
            }
        } else {
            self.timeline_state.select(Some(0));
        }

        self.current_span_ids = span_ids;

        if let Some(index) = self.timeline_state.selected() {
            self.selected_span = self.current_span_ids.get(index).copied();
        }

        self.ensure_timeline_selection_visible();
    }

    /// Update the cached timeline when the trace selection changes.
    fn on_trace_selection_changed(&mut self, new_trace_id: Option<TraceId>) {
        if self.selected_trace_id != new_trace_id {
            self.selected_trace_id = new_trace_id;
            self.timeline_state.select(None);
            self.current_span_ids.clear();
            self.selected_span = None;
        }
    }

    /// Translate a mouse click in the trace list into a selection change.
    fn handle_trace_click(&mut self, snapshot: &TracesSnapshot, column: u16, row: u16) -> bool {
        let Some(inner) = self.trace_list_inner else {
            return false;
        };

        if !contains_point(inner, column, row) {
            return false;
        }

        let offset = self.trace_list_state.offset();
        let relative_row = row.saturating_sub(inner.y) as usize;
        let index = offset + relative_row;

        if index < snapshot.traces.len() {
            self.trace_list_state.select(Some(index));
            self.ensure_trace_selection_visible();
            let new_trace_id = self.selected_trace(snapshot).map(|trace| trace.trace_id);
            self.on_trace_selection_changed(new_trace_id);
            self.detail_focus = DetailFocus::TraceList;
        }

        true
    }

    /// Handle mouse selection within the timeline table.
    fn handle_timeline_click(&mut self, snapshot: &TracesSnapshot, column: u16, row: u16) -> bool {
        let Some(inner) = self.timeline_inner else {
            return false;
        };

        if !contains_point(inner, column, row) {
            return false;
        }

        if self.selected_trace(snapshot).is_none() {
            return true;
        }

        if self.current_span_ids.is_empty() {
            return true;
        }

        let offset = self.timeline_state.offset();
        let relative_row = row.saturating_sub(inner.y) as usize;
        let index = offset + relative_row;

        if index < self.current_span_ids.len() {
            self.timeline_state.select(Some(index));
            self.selected_span = self.current_span_ids.get(index).copied();
            self.ensure_timeline_selection_visible();
            self.detail_focus = DetailFocus::Timeline;
        }

        true
    }
}

/// Subsections within the traces tab that can receive keyboard focus.
#[derive(Copy, Default, Clone, Eq, PartialEq)]
enum DetailFocus {
    #[default]
    TraceList,
    Timeline,
    Attributes,
}

impl DetailFocus {
    /// Advance to the next focusable region in the traces pane.
    fn next(self) -> Self {
        match self {
            DetailFocus::TraceList => DetailFocus::Timeline,
            DetailFocus::Timeline => DetailFocus::Attributes,
            DetailFocus::Attributes => DetailFocus::TraceList,
        }
    }

    /// Move focus to the previous region in the traces pane.
    fn previous(self) -> Self {
        match self {
            DetailFocus::TraceList => DetailFocus::Attributes,
            DetailFocus::Timeline => DetailFocus::TraceList,
            DetailFocus::Attributes => DetailFocus::Timeline,
        }
    }
}

/// Helper to build a two-column attribute row with consistent styling.
/// Limit the provided text to `max_len`, appending an ellipsis when truncated.
fn truncate_with_ellipsis(text: &str, max_len: usize) -> String {
    if max_len == 0 {
        return String::new();
    }

    let chars = text.chars();
    if chars.clone().count() <= max_len {
        return text.to_string();
    }

    if max_len <= 3 {
        return text.chars().take(max_len).collect();
    }

    let mut truncated: String = text.chars().take(max_len - 3).collect();
    truncated.push_str("...");
    truncated
}

/// Create the ASCII timeline bar for a span based on its offset and duration.
fn build_timeline_bar(offset_ns: u64, duration_ns: u64, total_ns: u64, width: usize) -> String {
    if width == 0 {
        return String::new();
    }

    let total = total_ns.max(1) as f64;
    let start_ratio = offset_ns as f64 / total;
    let length_ratio = duration_ns as f64 / total;

    let mut bar = vec![' '; width];
    let mut start = (start_ratio * width as f64).floor() as usize;
    let mut length = (length_ratio * width as f64).ceil() as usize;

    if start >= width {
        start = width - 1;
    }
    length = length.max(1);
    let end = (start + length).min(width);

    for cell in bar.iter_mut().take(end).skip(start) {
        *cell = '=';
    }

    bar.into_iter().collect()
}

/// Convert a nanosecond duration into a compact human-readable string.
fn format_duration_ns(duration_ns: u64) -> String {
    if duration_ns >= 1_000_000_000 {
        format!("{:.2}s", duration_ns as f64 / 1_000_000_000.0)
    } else if duration_ns >= 1_000_000 {
        format!("{:.2}ms", duration_ns as f64 / 1_000_000.0)
    } else if duration_ns >= 1_000 {
        format!("{:.2}us", duration_ns as f64 / 1_000.0)
    } else {
        format!("{duration_ns}ns")
    }
}
