use std::sync::Arc;
use std::time::SystemTime;

use fastrace::collector::{SpanId, TraceId};
use ratatui::text::Line;
use ratatui::widgets::Row;

use crate::app::LatencySummary;

/// Tracks high-level readiness for the UI and flags fatal channel conditions.
#[derive(Clone, Debug, Default)]
pub(crate) struct UiStatus {
    pub(crate) epoch: u64,
    pub(crate) has_initialized: bool,
    pub(crate) channel_closed: bool,
    pub(crate) shutdown_complete: bool,
}

/// Render-ready log lines for the logs tab.
#[derive(Clone, Default, Debug)]
pub(crate) struct LogsSnapshot {
    pub(crate) epoch: u64,
    pub(crate) lines: Arc<Vec<Line<'static>>>,
}

/// Pre-computed material for latency graphs.
#[derive(Clone, Debug, Default)]
pub(crate) struct LatencyChartSnapshot {
    pub(crate) window_end: Option<SystemTime>,
    pub(crate) average_points: Arc<Vec<(f64, f64)>>,
    pub(crate) summary: Option<LatencySummary>,
    pub(crate) y_max: f64,
}

/// Prepared data for token flow charts.
#[derive(Clone, Debug, Default)]
pub(crate) struct TokenFlowSnapshot {
    pub(crate) window_end: Option<SystemTime>,
    pub(crate) input_points: Arc<Vec<(f64, f64)>>,
    pub(crate) output_points: Arc<Vec<(f64, f64)>>,
    pub(crate) total_input_tokens: u64,
    pub(crate) total_output_tokens: u64,
    pub(crate) y_max: f64,
}

/// Aggregated token usage per model for the metrics table.
#[derive(Clone, Debug, Default)]
pub(crate) struct ModelTableSnapshot {
    pub(crate) rows: Arc<Vec<ModelRowSnapshot>>,
}

/// Single table row for model usage.
#[derive(Clone, Debug)]
pub(crate) struct ModelRowSnapshot {
    pub(crate) model: String,
    pub(crate) input_tokens: u64,
    pub(crate) output_tokens: u64,
    pub(crate) total_tokens: u64,
}

/// All render-ready pieces needed by the metrics tab.
#[derive(Clone, Debug, Default)]
pub(crate) struct MetricsSnapshot {
    pub(crate) epoch: u64,
    pub(crate) operation_latency: LatencyChartSnapshot,
    pub(crate) ttft_latency: LatencyChartSnapshot,
    pub(crate) token_flow: TokenFlowSnapshot,
    pub(crate) model_table: ModelTableSnapshot,
}

/// Cached trace panes for quick rendering.
#[derive(Clone, Debug, Default)]
pub(crate) struct TracesSnapshot {
    pub(crate) epoch: u64,
    pub(crate) traces: Arc<Vec<TraceRenderSnapshot>>,
}

/// Pre-rendered data for a single trace entry.
#[derive(Clone, Debug)]
pub(crate) struct TraceRenderSnapshot {
    pub(crate) trace_id: TraceId,
    pub(crate) list_line: Line<'static>,
    pub(crate) summary: Arc<Vec<Line<'static>>>,
    pub(crate) timeline: TimelineSnapshot,
    pub(crate) attributes: Arc<Vec<SpanAttributesSnapshot>>,
}

/// Timeline rows + metadata for a trace.
#[derive(Clone, Debug, Default)]
pub(crate) struct TimelineSnapshot {
    pub(crate) items: Arc<Vec<TimelineItemSnapshot>>,
}

/// Minimal span information needed to draw the timeline table.
#[derive(Clone, Debug)]
pub(crate) struct TimelineItemSnapshot {
    pub(crate) span_id: SpanId,
    pub(crate) depth: usize,
    pub(crate) offset_ns: u64,
    pub(crate) duration_ns: u64,
    pub(crate) name: String,
}

/// Attribute table precomputed for a span.
#[derive(Clone, Debug)]
pub(crate) struct SpanAttributesSnapshot {
    pub(crate) span_id: SpanId,
    pub(crate) rows: Arc<Vec<Row<'static>>>,
}
