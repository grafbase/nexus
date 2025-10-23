use std::{sync::Arc, time::SystemTime};

use fastrace::prelude::SpanId;
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Row},
};
use telemetry::tracing::{TraceEvent, TraceExportReceiver};
use time::OffsetDateTime;
use tokio::sync::{mpsc::error::TryRecvError, watch};

use crate::{
    ATTRIBUTE_ROW_LIMIT,
    app::{self, App, LatencySummary, MetricsDashboard, MetricsTimeslice, TraceEntry},
    ui,
};

/// Tracks epoch counters and dirty flags for each UI channel.
#[derive(Default)]
struct UiState {
    status: ChannelState,
    logs: ChannelState,
    metrics: ChannelState,
    traces: ChannelState,
}

/// Tracks a single channel's epoch counter and dirty flag.
#[derive(Default)]
struct ChannelState {
    epoch: u64,
    dirty: bool,
}

/// Orchestrates the processing of telemetry events and manages UI state updates.
///
/// The orchestrator receives trace events from a telemetry export receiver and processes
/// them to update various UI snapshots (status, logs, metrics, traces). It manages
/// dirty state tracking to efficiently update only changed components and handles
/// channel lifecycle events.
pub struct Orchestrator {
    /// Receiver for incoming trace events from telemetry export
    pub receiver: TraceExportReceiver,
    /// Sender for UI status updates
    pub status_tx: watch::Sender<ui::UiStatus>,
    /// Sender for logs snapshot updates
    pub logs_tx: watch::Sender<ui::LogsSnapshot>,
    /// Sender for metrics snapshot updates
    pub metrics_tx: watch::Sender<ui::MetricsSnapshot>,
    /// Sender for traces snapshot updates
    pub traces_tx: watch::Sender<ui::TracesSnapshot>,
}

impl Orchestrator {
    /// Runs the orchestrator event loop.
    ///
    /// This method processes incoming trace events, updates the internal application state,
    /// and sends UI snapshot updates when data changes. The loop continues until the
    /// telemetry channel is closed and all UI channels are closed.
    ///
    /// The orchestrator uses a dirty flag system to track which UI components need updates,
    /// minimizing unnecessary work when data hasn't changed.
    pub fn run(mut self) {
        let mut app = App::default();

        let mut ui_state = UiState::default();
        ui_state.status.dirty = true;

        let mut channel_open = true;

        while channel_open {
            // Process incoming events
            channel_open = self.process_events(&mut app, &mut ui_state, channel_open);

            // Check for app shutdown
            if app.shutdown_complete {
                channel_open = false;
            }

            // Send UI updates
            self.send_ui_updates(&app, &mut ui_state, true);

            if !channel_open {
                break;
            }

            // Check if all UI channels are closed
            if self.all_ui_channels_closed() {
                return;
            }
        }

        // Send final updates
        self.send_ui_updates(&app, &mut ui_state, false);
    }

    /// Processes all available events from the receiver
    fn process_events(&mut self, app: &mut App, ui_state: &mut UiState, mut channel_open: bool) -> bool {
        // First blocking receive
        match self.receiver.blocking_recv() {
            Some(event) => {
                self.handle_event(app, event, ui_state);
            }
            None => {
                app.mark_channel_closed();
                ui_state.status.dirty = true;
                channel_open = false;
            }
        }

        // Then drain all available events
        while channel_open {
            match self.receiver.try_recv() {
                Ok(event) => {
                    self.handle_event(app, event, ui_state);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    app.mark_channel_closed();
                    ui_state.status.dirty = true;
                    channel_open = false;
                    break;
                }
            }
        }

        channel_open
    }

    /// Handles a single trace event
    fn handle_event(&mut self, app: &mut App, event: TraceEvent, ui_state: &mut UiState) {
        match event {
            TraceEvent::Spans(batch) => {
                app.push_batch(batch);
                ui_state.traces.dirty = true;
                ui_state.status.dirty = true;
            }
            TraceEvent::Metrics(snapshot) => {
                app.update_metrics(snapshot);
                ui_state.metrics.dirty = true;
                ui_state.status.dirty = true;
            }
            TraceEvent::Log {
                timestamp,
                level,
                message,
            } => {
                app.push_log(timestamp, level, message);
                ui_state.logs.dirty = true;
                ui_state.status.dirty = true;
            }
        }
    }

    /// Sends UI updates for all dirty snapshots.
    ///
    /// When `clear_dirty` is true we reset the dirty flag after publishing; when false we leave it
    /// untouched so callers can inspect or reuse the flag state after the call.
    fn send_ui_updates(&mut self, app: &App, ui_state: &mut UiState, clear_dirty: bool) {
        if ui_state.status.dirty {
            ui_state.status.epoch = ui_state.status.epoch.saturating_add(1);

            let snapshot = build_status_snapshot(app, ui_state.status.epoch);
            let _ = self.status_tx.send(snapshot);

            if clear_dirty {
                ui_state.status.dirty = false;
            }
        }

        if ui_state.logs.dirty {
            ui_state.logs.epoch = ui_state.logs.epoch.saturating_add(1);

            let snapshot = build_logs_snapshot(app, ui_state.logs.epoch);
            let _ = self.logs_tx.send(snapshot);

            if clear_dirty {
                ui_state.logs.dirty = false;
            }
        }

        if ui_state.metrics.dirty {
            ui_state.metrics.epoch = ui_state.metrics.epoch.saturating_add(1);

            let snapshot = build_metrics_snapshot(app, ui_state.metrics.epoch);
            let _ = self.metrics_tx.send(snapshot);

            if clear_dirty {
                ui_state.metrics.dirty = false;
            }
        }

        if ui_state.traces.dirty {
            ui_state.traces.epoch = ui_state.traces.epoch.saturating_add(1);

            let snapshot = build_traces_snapshot(app, ui_state.traces.epoch);
            let _ = self.traces_tx.send(snapshot);

            if clear_dirty {
                ui_state.traces.dirty = false;
            }
        }
    }

    /// Checks if all UI channels are closed
    fn all_ui_channels_closed(&self) -> bool {
        self.status_tx.is_closed()
            && self.logs_tx.is_closed()
            && self.metrics_tx.is_closed()
            && self.traces_tx.is_closed()
    }
}

/// Builds a status snapshot from the current application state.
fn build_status_snapshot(app: &App, epoch: u64) -> ui::UiStatus {
    ui::UiStatus {
        epoch,
        has_initialized: app.has_initialized(),
        channel_closed: app.channel_closed,
        shutdown_complete: app.shutdown_complete,
    }
}

/// Builds a logs snapshot from the current application state.
fn build_logs_snapshot(app: &App, epoch: u64) -> ui::LogsSnapshot {
    let lines: Vec<_> = app.logs.iter().map(|line| line.to_line()).collect();

    ui::LogsSnapshot {
        epoch,
        lines: Arc::new(lines),
    }
}

/// Builds a metrics snapshot from the current application state.
///
/// If no metrics dashboard is available, returns a default snapshot with the current epoch.
fn build_metrics_snapshot(app: &App, epoch: u64) -> ui::MetricsSnapshot {
    let Some(dashboard) = app.metrics_dashboard() else {
        return ui::MetricsSnapshot {
            epoch,
            ..Default::default()
        };
    };

    let operation_latency =
        build_latency_chart_snapshot(&dashboard.timeslices, dashboard.operation_latency.clone(), |slice| {
            slice.operation_latency.as_ref()
        });

    let ttft_latency = build_latency_chart_snapshot(&dashboard.timeslices, dashboard.ttft_latency.clone(), |slice| {
        slice.ttft_latency.as_ref()
    });

    let token_flow = build_token_flow_snapshot(&dashboard);
    let model_table = build_model_table_snapshot(&dashboard);

    ui::MetricsSnapshot {
        epoch,
        operation_latency,
        ttft_latency,
        token_flow,
        model_table,
    }
}

/// Builds a latency chart snapshot from timeslices and a summary.
///
/// The accessor function is used to extract latency data from each timeslice.
/// Returns a snapshot with time series data points and maximum Y value for charting.
fn build_latency_chart_snapshot(
    timeslices: &[MetricsTimeslice],
    summary: Option<LatencySummary>,
    accessor: impl Fn(&MetricsTimeslice) -> Option<&LatencySummary>,
) -> ui::LatencyChartSnapshot {
    let mut snapshot = ui::LatencyChartSnapshot {
        summary,
        ..Default::default()
    };

    let Some(window_end) = timeslices.last().map(|slice| slice.timestamp) else {
        return snapshot;
    };

    let window_start = window_end.checked_sub(app::METRICS_WINDOW).unwrap_or(window_end);

    let mut average_points = Vec::new();
    let mut y_max = 0.0_f64;

    for slice in timeslices {
        if let Some(latency) = accessor(slice) {
            let x = ui::seconds_since(window_start, slice.timestamp);
            average_points.push((x, latency.average_ms));
            y_max = y_max.max(latency.average_ms);
        }
    }

    snapshot.window_end = Some(window_end);
    snapshot.average_points = Arc::new(average_points);
    snapshot.y_max = y_max;

    snapshot
}

/// Builds a token flow snapshot from the metrics dashboard.
///
/// Creates time series data for input and output token counts over the metrics window.
fn build_token_flow_snapshot(dashboard: &MetricsDashboard) -> ui::TokenFlowSnapshot {
    let mut snapshot = ui::TokenFlowSnapshot {
        total_input_tokens: dashboard.total_input_tokens,
        total_output_tokens: dashboard.total_output_tokens,
        ..Default::default()
    };

    if let Some(window_end) = dashboard.timeslices.last().map(|slice| slice.timestamp) {
        let window_start = window_end.checked_sub(app::METRICS_WINDOW).unwrap_or(window_end);

        let mut input_points = Vec::with_capacity(dashboard.timeslices.len());
        let mut output_points = Vec::with_capacity(dashboard.timeslices.len());
        let mut y_max = 0.0_f64;

        for slice in &dashboard.timeslices {
            let x = ui::seconds_since(window_start, slice.timestamp);
            input_points.push((x, slice.input_tokens as f64));
            output_points.push((x, slice.output_tokens as f64));
            y_max = y_max.max(slice.input_tokens as f64).max(slice.output_tokens as f64);
        }

        snapshot.window_end = Some(window_end);
        snapshot.input_points = Arc::new(input_points);
        snapshot.output_points = Arc::new(output_points);
        snapshot.y_max = y_max;
    }

    snapshot
}

/// Builds a model table snapshot from the metrics dashboard.
///
/// Transforms per-model token statistics into a format suitable for table display.
fn build_model_table_snapshot(dashboard: &MetricsDashboard) -> ui::ModelTableSnapshot {
    let rows = dashboard
        .per_model_tokens
        .iter()
        .map(|model| ui::ModelRowSnapshot {
            model: model.model.clone(),
            input_tokens: model.input_tokens,
            output_tokens: model.output_tokens,
            total_tokens: model.total_tokens,
        })
        .collect();

    ui::ModelTableSnapshot { rows: Arc::new(rows) }
}

/// Builds a traces snapshot from the current application state.
///
/// Processes all traces in the application and creates render snapshots for UI display.
fn build_traces_snapshot(app: &App, epoch: u64) -> ui::TracesSnapshot {
    let traces = app.traces.iter().map(build_trace_render_snapshot).collect();

    ui::TracesSnapshot {
        epoch,
        traces: Arc::new(traces),
    }
}

/// Builds a complete render snapshot for a single trace.
///
/// Includes list line representation, summary lines, timeline, and attributes.
fn build_trace_render_snapshot(trace: &TraceEntry) -> ui::TraceRenderSnapshot {
    let list_line = build_trace_list_line(trace);
    let summary = Arc::new(build_trace_summary_lines(trace));
    let timeline = build_timeline_snapshot(trace);
    let attributes = build_attributes_snapshot(trace);

    ui::TraceRenderSnapshot {
        trace_id: trace.trace_id,
        list_line,
        summary,
        timeline,
        attributes: Arc::new(attributes),
    }
}

/// Builds a timeline snapshot for a trace.
///
/// Converts trace timeline items into a format suitable for timeline visualization.
fn build_timeline_snapshot(trace: &TraceEntry) -> ui::TimelineSnapshot {
    let items = trace
        .timeline_items()
        .into_iter()
        .map(|item| ui::TimelineItemSnapshot {
            span_id: item.span_id,
            depth: item.depth,
            offset_ns: item.offset_ns,
            duration_ns: item.duration_ns,
            name: item.name,
        })
        .collect();

    ui::TimelineSnapshot { items: Arc::new(items) }
}

/// Builds attributes snapshots for all spans in a trace.
///
/// Creates attribute row data for each span that can be displayed in the UI.
fn build_attributes_snapshot(trace: &TraceEntry) -> Vec<ui::SpanAttributesSnapshot> {
    let mut entries = Vec::new();
    for (&span_id, _span) in trace.spans.iter() {
        if let Some(rows) = build_attribute_rows(trace, span_id) {
            entries.push(ui::SpanAttributesSnapshot {
                span_id,
                rows: Arc::new(rows),
            });
        }
    }
    entries
}

/// Builds attribute table rows for a specific span.
///
/// Includes span metadata (name, ID, parent, timing) and custom properties,
/// limited by `ATTRIBUTE_ROW_LIMIT` to prevent excessive display.
fn build_attribute_rows(trace: &TraceEntry, span_id: SpanId) -> Option<Vec<Row<'static>>> {
    let span = trace.spans.get(&span_id)?;

    let mut rows = Vec::new();

    rows.push(attribute_row("Name", span.name.clone()));
    rows.push(attribute_row("Span ID", span.span_id.to_string()));

    if span.parent_id != SpanId::default() {
        rows.push(attribute_row("Parent ID", span.parent_id.to_string()));
    }

    let offset_ns = span.begin_time_unix_ns.saturating_sub(trace.start_ns);
    rows.push(attribute_row("Offset", format_duration_ns(offset_ns)));
    rows.push(attribute_row("Duration", format_duration_ns(span.duration_ns)));

    let mut props = span.properties.clone();
    props.sort_by(|(a, _), (b, _)| a.cmp(b));

    let mut remaining = ATTRIBUTE_ROW_LIMIT.saturating_sub(rows.len());
    for (key, value) in props.into_iter() {
        if remaining == 0 {
            break;
        }
        rows.push(attribute_row(key, value));
        remaining -= 1;
    }

    Some(rows)
}

/// Builds a formatted line for displaying a trace in a list view.
///
/// Includes timestamp, duration, root span name, trace ID, and span count
/// with appropriate styling and truncation for consistent display width.
fn build_trace_list_line(trace: &TraceEntry) -> Line<'static> {
    const DURATION_COLUMN_WIDTH: usize = 9;

    let duration = format_duration_ns(trace.total_duration_ns());
    let trace_id_hex = trace.trace_id.to_string();
    let short_id = trace_id_hex.get(..8).unwrap_or(&trace_id_hex);
    let span_count = trace.spans.len();

    let root_name = trace
        .root_span()
        .map(|span| span.name.as_str())
        .unwrap_or("(unknown root)");

    let display_name = truncate_with_ellipsis(root_name, 32);
    let timestamp = format_trace_timestamp(trace.start_time());

    let mut cells = Vec::new();

    cells.push(Span::styled(
        format!("[{timestamp}]"),
        Style::default().fg(ui::TIMESTAMP_COLOR),
    ));

    cells.push(Span::raw(" "));

    let padded_duration = format!("{duration:>DURATION_COLUMN_WIDTH$}");

    cells.push(Span::styled(
        padded_duration,
        Style::default().fg(ui::TRACE_CHILD_COLOR),
    ));

    cells.push(Span::raw("  "));

    cells.push(Span::styled(
        display_name,
        Style::default().fg(ui::TEXT_PRIMARY).add_modifier(Modifier::BOLD),
    ));

    cells.push(Span::raw("  "));
    cells.push(Span::styled(short_id.to_string(), Style::default().fg(ui::TEXT_MUTED)));

    cells.push(Span::styled(
        format!("  • {span_count} spans"),
        Style::default().fg(ui::TEXT_MUTED),
    ));

    Line::from(cells)
}

/// Builds summary lines for displaying trace details.
///
/// Returns a vector of formatted lines containing trace ID, timing information,
/// and root span details for the trace detail view.
fn build_trace_summary_lines(trace: &TraceEntry) -> Vec<Line<'static>> {
    let duration = format_duration_ns(trace.total_duration_ns());
    let root_name = trace
        .root_span()
        .map(|span| span.name.as_str())
        .unwrap_or("(unknown root)");
    let timestamp = format_trace_timestamp(trace.start_time());

    vec![
        Line::from(format!("Trace {}", trace.trace_id)),
        Line::from(format!(
            "Started at {timestamp} • Duration {duration} • Spans {}",
            trace.spans.len()
        )),
        Line::from(format!("Root span: {root_name}")),
    ]
}

/// Creates a styled table row for displaying key-value attribute pairs.
///
/// The key is styled with accent color and the value with primary text color.
fn attribute_row<K, V>(key: K, value: V) -> Row<'static>
where
    K: Into<String>,
    V: Into<String>,
{
    let key_cell = Cell::from(key.into()).style(Style::default().fg(ui::TEXT_ACCENT));
    let value_cell = Cell::from(value.into()).style(Style::default().fg(ui::TEXT_PRIMARY));

    Row::new(vec![key_cell, value_cell])
}

/// Truncates text to a maximum length, adding ellipsis if truncated.
///
/// If the text fits within max_len, returns the original text.
/// If max_len is 3 or less, truncates without ellipsis.
/// Otherwise, truncates to max_len-3 characters and adds "...".
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

/// Formats a duration in nanoseconds into a human-readable string.
///
/// Automatically selects the most appropriate unit (s, ms, us, ns) based on magnitude.
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

/// Formats a SystemTime as a trace timestamp string.
///
/// Converts the time to the UI timestamp format. Returns a fallback string
/// if the time cannot be converted or formatted.
fn format_trace_timestamp(time: SystemTime) -> String {
    const FALLBACK: &str = "--:--:--";

    let Ok(duration) = time.duration_since(SystemTime::UNIX_EPOCH) else {
        return FALLBACK.to_string();
    };

    let seconds = i128::from(duration.as_secs());
    let nanos = i128::from(duration.subsec_nanos());
    let total_nanos = seconds * 1_000_000_000 + nanos;

    let Ok(datetime) = OffsetDateTime::from_unix_timestamp_nanos(total_nanos) else {
        return FALLBACK.to_string();
    };

    datetime
        .format(ui::TRACE_TIMESTAMP_FORMAT)
        .unwrap_or_else(|_| FALLBACK.to_string())
}
