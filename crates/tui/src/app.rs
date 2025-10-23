use std::{
    collections::{HashMap, HashSet, VecDeque},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use fastrace::{
    collector::{SpanId, TraceId},
    prelude::SpanRecord,
};
use log::Level;
use telemetry::tracing::{TuiKeyValue, TuiMetricData, TuiMetricNumber, TuiMetricSnapshot, TuiTemporality};

/// Maximum number of trace trees kept in memory at once. Older entries roll off
/// the front of the deque to keep rendering cheap.
pub(crate) const MAX_TRACES: usize = 200;

/// Cap the in-memory log buffer to avoid unbounded growth while still showing
/// a meaningful history.
pub(crate) const MAX_LOG_LINES: usize = 200;
/// Upper bound on how long metrics snapshots are retained in memory.
const MAX_METRICS_RETENTION: Duration = Duration::from_secs(60 * 60 * 24);
/// Hard cap on the number of snapshots kept to avoid runaway growth.
const MAX_METRIC_SNAPSHOTS: usize = 512;
/// Maximum number of sparkline points rendered for latency/token charts.
const MAX_CHART_TIMESLICES: usize = 240;
/// Sliding window duration (10 minutes) used when aggregating metrics for the
/// dashboard. Historical data beyond this window is retained separately.
pub(crate) const METRICS_WINDOW: Duration = Duration::from_secs(10 * 60);

/// Aggregates telemetry data received from the tracing stream.
#[derive(Default, Debug)]
pub(crate) struct App {
    pub(crate) traces: VecDeque<TraceEntry>,
    pub(crate) logs: VecDeque<LogLine>,
    pub(crate) channel_closed: bool,
    pub(crate) shutdown_complete: bool,
    pub(crate) metrics_history: VecDeque<TuiMetricSnapshot>,
    has_received_data: bool,
}

impl App {
    /// Integrate a batch of span records delivered by fastrace and rebuild
    /// derived selections.
    pub(crate) fn push_batch(&mut self, records: Vec<SpanRecord>) {
        if !records.is_empty() {
            self.has_received_data = true;
        }
        for record in records {
            self.insert_record(record);
        }
    }

    /// Attach an incoming span record to an existing trace or create a new
    /// entry the first time we see a given trace id.
    pub(crate) fn insert_record(&mut self, record: SpanRecord) {
        if let Some(existing) = self.traces.iter_mut().find(|trace| trace.trace_id == record.trace_id) {
            existing.add_span(record);
            return;
        }

        if self.traces.len() == MAX_TRACES {
            self.traces.pop_front();
        }

        self.traces.push_back(TraceEntry::from_record(record));
    }

    /// Append a log entry while respecting the fixed buffering limit.
    pub(crate) fn push_log(&mut self, timestamp: String, level: Level, message: String) {
        if self.logs.len() == MAX_LOG_LINES {
            self.logs.pop_front();
        }

        let is_shutdown_confirmation = message.contains("Server shut down gracefully");

        self.logs.push_back(LogLine {
            timestamp,
            level,
            message,
        });
        self.has_received_data = true;

        if is_shutdown_confirmation {
            self.shutdown_complete = true;
        }
    }

    /// Record that the telemetry channel dropped so the UI can surface it.
    pub(crate) fn mark_channel_closed(&mut self) {
        self.channel_closed = true;
    }

    /// Update the cached metrics snapshot.
    pub(crate) fn update_metrics(&mut self, snapshot: TuiMetricSnapshot) {
        self.metrics_history.push_back(snapshot);
        self.prune_metrics_history();
        self.has_received_data = true;
    }

    /// Compute aggregated metrics for the default timeframe.
    pub(crate) fn metrics_dashboard(&self) -> Option<MetricsDashboard> {
        if self.metrics_history.is_empty() {
            return None;
        }

        let now = SystemTime::now();
        let timeframe = METRICS_WINDOW;

        let mut input_totals: HashMap<String, u64> = HashMap::new();
        let mut output_totals: HashMap<String, u64> = HashMap::new();
        let mut previous_input: HashMap<String, u64> = HashMap::new();
        let mut previous_output: HashMap<String, u64> = HashMap::new();
        let mut previous_histograms: HashMap<&'static str, HistogramSnapshotData> = HashMap::new();

        let mut timeslices: Vec<MetricsTimeslice> = Vec::new();
        let mut window_operation_hist: Option<HistogramAccumulator> = None;
        let mut window_ttft_hist: Option<HistogramAccumulator> = None;

        for snapshot in &self.metrics_history {
            let within_timeframe = timeframe.is_zero()
                || match now.duration_since(snapshot.captured_at) {
                    Ok(elapsed) => elapsed <= timeframe,
                    Err(_) => true,
                };
            let operation_metric = telemetry::metrics::GEN_AI_CLIENT_OPERATION_DURATION;
            let ttft_metric = telemetry::metrics::GEN_AI_CLIENT_TIME_TO_FIRST_TOKEN;

            let operation_current = collect_histogram(snapshot, operation_metric);
            let ttft_current = collect_histogram(snapshot, ttft_metric);

            let operation_delta = operation_current
                .as_ref()
                .and_then(|current| histogram_delta(current, previous_histograms.get(operation_metric)));
            let ttft_delta = ttft_current
                .as_ref()
                .and_then(|current| histogram_delta(current, previous_histograms.get(ttft_metric)));

            let operation_delta_acc = operation_delta.as_ref().and_then(|delta| delta.to_accumulator());
            let ttft_delta_acc = ttft_delta.as_ref().and_then(|delta| delta.to_accumulator());

            let input_deltas = snapshot_sum_deltas(
                snapshot,
                telemetry::metrics::GEN_AI_CLIENT_INPUT_TOKEN_USAGE,
                "gen_ai.request.model",
                &mut previous_input,
            );
            let output_deltas = snapshot_sum_deltas(
                snapshot,
                telemetry::metrics::GEN_AI_CLIENT_OUTPUT_TOKEN_USAGE,
                "gen_ai.request.model",
                &mut previous_output,
            );
            if within_timeframe {
                if let Some(acc) = operation_delta_acc.as_ref() {
                    match &mut window_operation_hist {
                        Some(existing) => existing.merge(acc),
                        None => window_operation_hist = Some(acc.clone()),
                    }
                }

                if let Some(acc) = ttft_delta_acc.as_ref() {
                    match &mut window_ttft_hist {
                        Some(existing) => existing.merge(acc),
                        None => window_ttft_hist = Some(acc.clone()),
                    }
                }

                let input_total_delta: u64 = input_deltas.values().copied().sum();
                let output_total_delta: u64 = output_deltas.values().copied().sum();

                let slice_operation_summary = operation_delta_acc.as_ref().and_then(|acc| acc.summary());
                let slice_ttft_summary = ttft_delta_acc.as_ref().and_then(|acc| acc.summary());

                timeslices.push(MetricsTimeslice {
                    timestamp: snapshot.captured_at,
                    operation_hist: operation_delta_acc.clone(),
                    operation_latency: slice_operation_summary,
                    ttft_hist: ttft_delta_acc.clone(),
                    ttft_latency: slice_ttft_summary,
                    input_tokens: input_total_delta,
                    output_tokens: output_total_delta,
                });

                for (model, delta) in input_deltas {
                    if delta > 0 {
                        *input_totals.entry(model).or_default() += delta;
                    }
                }
                for (model, delta) in output_deltas {
                    if delta > 0 {
                        *output_totals.entry(model).or_default() += delta;
                    }
                }
            }

            if let Some(current) = operation_current {
                previous_histograms.insert(operation_metric, current);
            }

            if let Some(current) = ttft_current {
                previous_histograms.insert(ttft_metric, current);
            }
        }

        if let Some(cutoff) = now.checked_sub(METRICS_WINDOW) {
            timeslices.retain(|slice| slice.timestamp >= cutoff);
        }

        if timeslices.is_empty() {
            return None;
        }

        if timeslices.len() > MAX_CHART_TIMESLICES {
            let skip = timeslices.len() - MAX_CHART_TIMESLICES;
            timeslices = timeslices.into_iter().skip(skip).collect();
        }

        let mut window_operation_hist: Option<HistogramAccumulator> = None;
        let mut window_ttft_hist: Option<HistogramAccumulator> = None;

        for slice in &timeslices {
            if let Some(hist) = slice.operation_hist.as_ref() {
                match &mut window_operation_hist {
                    Some(existing) => existing.merge(hist),
                    None => window_operation_hist = Some(hist.clone()),
                }
            }

            if let Some(hist) = slice.ttft_hist.as_ref() {
                match &mut window_ttft_hist {
                    Some(existing) => existing.merge(hist),
                    None => window_ttft_hist = Some(hist.clone()),
                }
            }
        }

        let operation_latency = window_operation_hist.as_ref().and_then(|hist| hist.summary());
        let ttft_latency = window_ttft_hist.as_ref().and_then(|hist| hist.summary());
        let total_input_tokens = input_totals.values().copied().sum();
        let total_output_tokens = output_totals.values().copied().sum();
        let per_model_tokens = combine_token_usage(input_totals, output_totals);

        Some(MetricsDashboard {
            operation_latency,
            ttft_latency,
            timeslices,
            per_model_tokens,
            total_input_tokens,
            total_output_tokens,
        })
    }

    fn prune_metrics_history(&mut self) {
        if let Some(cutoff) = SystemTime::now().checked_sub(MAX_METRICS_RETENTION) {
            while let Some(front) = self.metrics_history.front() {
                if front.captured_at < cutoff {
                    self.metrics_history.pop_front();
                } else {
                    break;
                }
            }
        }

        while self.metrics_history.len() > MAX_METRIC_SNAPSHOTS {
            self.metrics_history.pop_front();
        }
    }

    /// Indicate whether any telemetry has landed yet.
    ///
    /// The UI uses this flag to decide when to transition away from the loading
    /// placeholder so the first rendered frame always contains meaningful data.
    pub(crate) fn has_initialized(&self) -> bool {
        self.has_received_data || !self.traces.is_empty() || !self.logs.is_empty() || !self.metrics_history.is_empty()
    }
}

/// High-level aggregation of metrics presented on the dashboard tab.
#[derive(Debug, Clone)]
pub(crate) struct MetricsDashboard {
    pub operation_latency: Option<LatencySummary>,
    pub ttft_latency: Option<LatencySummary>,
    pub timeslices: Vec<MetricsTimeslice>,
    pub per_model_tokens: Vec<ModelTokenUsage>,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
}

/// Aggregate latency statistics derived from histogram data.
#[derive(Debug, Clone)]
pub(crate) struct LatencySummary {
    pub average_ms: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
}

/// Aggregated token usage for a single model within the metrics window.
#[derive(Debug, Clone)]
pub(crate) struct ModelTokenUsage {
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

/// Per-snapshot metrics sample backing the sparklines.
#[derive(Debug, Clone)]
pub(crate) struct MetricsTimeslice {
    pub timestamp: SystemTime,
    operation_hist: Option<HistogramAccumulator>,
    pub operation_latency: Option<LatencySummary>,
    ttft_hist: Option<HistogramAccumulator>,
    pub ttft_latency: Option<LatencySummary>,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Accumulates histogram buckets while merging multiple delta snapshots.
#[derive(Clone, Debug)]
struct HistogramAccumulator {
    bounds: Vec<f64>,
    bucket_counts: Vec<u64>,
    sum: f64,
    count: u64,
}

/// Raw histogram snapshot as exported by the telemetry pipeline.
#[derive(Clone, Debug)]
struct HistogramSnapshotData {
    bounds: Vec<f64>,
    bucket_counts: Vec<u64>,
    sum: f64,
    count: u64,
    temporality: TuiTemporality,
}

#[cfg(test)]
mod tests {
    use super::*;
    use telemetry::metrics::{GEN_AI_CLIENT_OPERATION_DURATION, GEN_AI_CLIENT_TIME_TO_FIRST_TOKEN};
    use telemetry::tracing::{
        TuiHistogramMetric, TuiHistogramPoint, TuiInstrumentationScope, TuiMetric, TuiMetricSnapshot, TuiScopeMetrics,
        TuiSumMetric, TuiSumPoint,
    };

    #[test]
    fn dashboard_keeps_operation_and_ttft_histograms_distinct() {
        let timestamp = SystemTime::now() - Duration::from_secs(30);

        let histogram_bounds = vec![0.5, 1.0, 2.0];

        let operation_metric = histogram_metric(
            GEN_AI_CLIENT_OPERATION_DURATION,
            histogram_bounds.clone(),
            vec![0, 0, 5, 0],
            7.5,
            timestamp,
        );

        let ttft_metric = histogram_metric(
            GEN_AI_CLIENT_TIME_TO_FIRST_TOKEN,
            histogram_bounds,
            vec![5, 0, 0, 0],
            1.25,
            timestamp,
        );

        let snapshot = TuiMetricSnapshot {
            captured_at: timestamp,
            resource_attributes: Vec::new(),
            scopes: vec![TuiScopeMetrics {
                scope: TuiInstrumentationScope {
                    name: "test".to_string(),
                    version: None,
                    schema_url: None,
                    attributes: Vec::new(),
                },
                metrics: vec![operation_metric, ttft_metric],
            }],
        };

        let mut app = App::default();
        app.update_metrics(snapshot);

        let dashboard = app.metrics_dashboard().expect("dashboard missing");
        let operation = dashboard.operation_latency.expect("operation latency missing");
        let ttft = dashboard.ttft_latency.expect("ttft latency missing");

        assert_approx(operation.average_ms, 1_500.0);
        assert_approx(operation.p50_ms, 1_500.0);
        assert_approx(operation.p95_ms, 1_950.0);
        assert_approx(operation.p99_ms, 1_990.0);

        assert_approx(ttft.average_ms, 250.0);
        assert_approx(ttft.p50_ms, 250.0);
        assert_approx(ttft.p95_ms, 475.0);
        assert_approx(ttft.p99_ms, 495.0);

        assert!(operation.average_ms > ttft.average_ms);
        assert!(operation.p50_ms > ttft.p50_ms);
        assert!(operation.p95_ms > ttft.p95_ms);
        assert!(operation.p99_ms > ttft.p99_ms);
    }

    #[test]
    fn token_usage_records_on_first_snapshot() {
        let timestamp = SystemTime::now();

        let token_metric = sum_metric(
            telemetry::metrics::GEN_AI_CLIENT_INPUT_TOKEN_USAGE,
            "gen_ai.request.model",
            "provider/model",
            64,
            timestamp,
        );

        let snapshot = TuiMetricSnapshot {
            captured_at: timestamp,
            resource_attributes: Vec::new(),
            scopes: vec![TuiScopeMetrics {
                scope: TuiInstrumentationScope {
                    name: "test".to_string(),
                    version: None,
                    schema_url: None,
                    attributes: Vec::new(),
                },
                metrics: vec![token_metric],
            }],
        };

        let mut app = App::default();
        app.update_metrics(snapshot);

        let dashboard = app.metrics_dashboard().expect("dashboard missing");
        assert_eq!(dashboard.total_input_tokens, 64);
        assert_eq!(dashboard.total_output_tokens, 0);
        assert_eq!(dashboard.per_model_tokens.len(), 1);
        let row = &dashboard.per_model_tokens[0];
        assert_eq!(row.model, "provider/model");
        assert_eq!(row.input_tokens, 64);
        assert_eq!(row.output_tokens, 0);
    }

    fn histogram_metric(
        name: &str,
        bounds: Vec<f64>,
        bucket_counts: Vec<u64>,
        sum: f64,
        timestamp: SystemTime,
    ) -> TuiMetric {
        let count: u64 = bucket_counts.iter().sum();
        assert_eq!(bounds.len() + 1, bucket_counts.len());
        assert!(count > 0, "histogram needs samples");

        TuiMetric {
            name: name.to_string(),
            description: String::new(),
            unit: String::new(),
            data: TuiMetricData::Histogram(TuiHistogramMetric {
                start_time: timestamp,
                time: timestamp,
                temporality: TuiTemporality::Delta,
                points: vec![TuiHistogramPoint {
                    attributes: Vec::new(),
                    count,
                    sum: TuiMetricNumber::F64(sum),
                    min: None,
                    max: None,
                    bounds,
                    bucket_counts,
                }],
            }),
        }
    }

    fn sum_metric(name: &str, label_key: &str, label_value: &str, value: u64, timestamp: SystemTime) -> TuiMetric {
        TuiMetric {
            name: name.to_string(),
            description: String::new(),
            unit: String::new(),
            data: TuiMetricData::Sum(TuiSumMetric {
                start_time: timestamp,
                time: timestamp,
                temporality: TuiTemporality::Cumulative,
                is_monotonic: true,
                points: vec![TuiSumPoint {
                    attributes: vec![TuiKeyValue {
                        key: label_key.to_string(),
                        value: label_value.to_string(),
                    }],
                    value: TuiMetricNumber::U64(value),
                }],
            }),
        }
    }

    fn assert_approx(actual: f64, expected: f64) {
        let delta = (actual - expected).abs();
        assert!(delta <= 1.0, "expected {expected} ms, got {actual} ms (delta {delta})");
    }
}

impl HistogramAccumulator {
    fn from_components(bounds: Vec<f64>, bucket_counts: Vec<u64>, sum: f64, count: u64) -> Option<Self> {
        if bounds.len() + 1 != bucket_counts.len() || count == 0 {
            return None;
        }

        Some(Self {
            bounds,
            bucket_counts,
            sum,
            count,
        })
    }

    fn merge(&mut self, other: &HistogramAccumulator) {
        if !bounds_compatible(&self.bounds, &other.bounds) || self.bucket_counts.len() != other.bucket_counts.len() {
            return;
        }

        self.sum += other.sum;
        self.count = self.count.saturating_add(other.count);

        for (dst, src) in self.bucket_counts.iter_mut().zip(other.bucket_counts.iter()) {
            *dst = dst.saturating_add(*src);
        }
    }

    fn quantile(&self, percentile: f64) -> Option<f64> {
        if self.count == 0 || self.bucket_counts.is_empty() {
            return None;
        }

        let clamped = percentile.clamp(0.0, 1.0);
        let target = clamped * self.count as f64;

        let mut cumulative = 0u64;
        let mut previous_cumulative = 0u64;

        for (index, bucket_count) in self.bucket_counts.iter().enumerate() {
            cumulative = cumulative.saturating_add(*bucket_count);

            if cumulative as f64 >= target {
                let lower = if index == 0 {
                    0.0
                } else {
                    self.bounds.get(index - 1).copied().unwrap_or(0.0).max(0.0)
                };

                let upper = if index < self.bounds.len() {
                    self.bounds[index].max(lower)
                } else {
                    self.bounds.last().copied().unwrap_or(lower)
                };

                if *bucket_count == 0 {
                    return Some(upper);
                }

                let offset = target - previous_cumulative as f64;
                let fraction = (offset / *bucket_count as f64).clamp(0.0, 1.0);
                let width = (upper - lower).max(0.0);

                return Some(lower + fraction * width);
            }

            previous_cumulative = cumulative;
        }

        self.bounds.last().copied()
    }

    fn summary(&self) -> Option<LatencySummary> {
        if self.count == 0 {
            return None;
        }

        let average_seconds = (self.sum / self.count as f64).max(0.0);
        let p50_seconds = self.quantile(0.50).unwrap_or(average_seconds);
        let p95_seconds = self.quantile(0.95).unwrap_or(average_seconds);
        let p99_seconds = self.quantile(0.99).unwrap_or(average_seconds);

        Some(LatencySummary {
            average_ms: average_seconds * 1_000.0,
            p50_ms: p50_seconds * 1_000.0,
            p95_ms: p95_seconds * 1_000.0,
            p99_ms: p99_seconds * 1_000.0,
        })
    }
}

/// Return `true` if two histogram bound vectors describe the same bucket layout.
fn bounds_compatible(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(lhs, rhs)| (*lhs - *rhs).abs() <= 1e-9)
}

/// Compute delta sums for a monotonic counter metric and group them by attribute.
fn snapshot_sum_deltas(
    snapshot: &TuiMetricSnapshot,
    metric_name: &str,
    attribute_key: &str,
    previous: &mut HashMap<String, u64>,
) -> HashMap<String, u64> {
    let mut deltas: HashMap<String, u64> = HashMap::new();

    for scope in &snapshot.scopes {
        for metric in &scope.metrics {
            if metric.name != metric_name {
                continue;
            }

            let TuiMetricData::Sum(sum_metric) = &metric.data else {
                continue;
            };

            for point in &sum_metric.points {
                let Some(label) = find_attribute(&point.attributes, attribute_key) else {
                    continue;
                };

                let Some(value) = metric_number_to_u64(&point.value) else {
                    continue;
                };

                let label = label.to_string();
                let delta = match sum_metric.temporality {
                    TuiTemporality::Delta | TuiTemporality::LowMemory => {
                        previous.insert(label.clone(), value);
                        value
                    }
                    TuiTemporality::Cumulative => {
                        let previous_value = previous.insert(label.clone(), value);
                        match previous_value {
                            Some(prev) if value >= prev => value - prev,
                            Some(_) => value,
                            None => value,
                        }
                    }
                };

                if delta > 0 {
                    *deltas.entry(label).or_default() += delta;
                }
            }
        }
    }

    deltas
}

impl HistogramSnapshotData {
    fn to_accumulator(&self) -> Option<HistogramAccumulator> {
        HistogramAccumulator::from_components(self.bounds.clone(), self.bucket_counts.clone(), self.sum, self.count)
    }
}

/// Extract the most recent histogram data for `metric_name` from a snapshot.
fn collect_histogram(snapshot: &TuiMetricSnapshot, metric_name: &str) -> Option<HistogramSnapshotData> {
    let mut aggregate: Option<HistogramSnapshotData> = None;

    for scope in &snapshot.scopes {
        for metric in &scope.metrics {
            if metric.name != metric_name {
                continue;
            }

            let TuiMetricData::Histogram(histogram) = &metric.data else {
                continue;
            };

            for point in &histogram.points {
                if point.count == 0 {
                    continue;
                }

                let sum = metric_number_to_f64(&point.sum).unwrap_or(0.0);

                match aggregate.as_mut() {
                    Some(existing) => {
                        if bounds_compatible(&existing.bounds, &point.bounds)
                            && existing.bucket_counts.len() == point.bucket_counts.len()
                        {
                            existing.sum += sum;
                            existing.count = existing.count.saturating_add(point.count);
                            for (dst, src) in existing.bucket_counts.iter_mut().zip(point.bucket_counts.iter()) {
                                *dst = dst.saturating_add(*src);
                            }
                            existing.temporality = histogram.temporality;
                        }
                    }
                    None => {
                        aggregate = Some(HistogramSnapshotData {
                            bounds: point.bounds.clone(),
                            bucket_counts: point.bucket_counts.clone(),
                            sum,
                            count: point.count,
                            temporality: histogram.temporality,
                        });
                    }
                }
            }
        }
    }

    match aggregate {
        Some(data) if data.count > 0 => Some(data),
        _ => None,
    }
}

/// Calculate the difference between two histogram snapshots, returning the incremental counts.
fn histogram_delta(
    current: &HistogramSnapshotData,
    previous: Option<&HistogramSnapshotData>,
) -> Option<HistogramSnapshotData> {
    match current.temporality {
        TuiTemporality::Delta | TuiTemporality::LowMemory => {
            return Some(current.clone());
        }
        TuiTemporality::Cumulative => {}
    }

    let mut delta_counts = current.bucket_counts.clone();
    let mut delta_sum = current.sum;

    if let Some(previous) = previous
        && bounds_compatible(&current.bounds, &previous.bounds)
        && current.bucket_counts.len() == previous.bucket_counts.len()
    {
        for (dst, src) in delta_counts.iter_mut().zip(previous.bucket_counts.iter()) {
            *dst = dst.saturating_sub(*src);
        }
        delta_sum = (current.sum - previous.sum).max(0.0);
    }

    let delta_count: u64 = delta_counts.iter().copied().sum();

    if delta_count == 0 {
        return None;
    }

    Some(HistogramSnapshotData {
        bounds: current.bounds.clone(),
        bucket_counts: delta_counts,
        sum: delta_sum,
        count: delta_count,
        temporality: TuiTemporality::Delta,
    })
}

/// Merge per-model input and output token totals into a combined structure sorted by usage.
fn combine_token_usage(
    input_tokens: HashMap<String, u64>,
    output_tokens: HashMap<String, u64>,
) -> Vec<ModelTokenUsage> {
    let mut models: HashSet<String> = input_tokens.keys().cloned().collect();
    models.extend(output_tokens.keys().cloned());

    let mut rows: Vec<ModelTokenUsage> = models
        .into_iter()
        .map(|model| {
            let input = input_tokens.get(&model).copied().unwrap_or(0);
            let output = output_tokens.get(&model).copied().unwrap_or(0);
            ModelTokenUsage {
                model,
                input_tokens: input,
                output_tokens: output,
                total_tokens: input + output,
            }
        })
        .filter(|row| row.input_tokens > 0 || row.output_tokens > 0)
        .collect();

    rows.sort_by(|a, b| b.total_tokens.cmp(&a.total_tokens).then_with(|| a.model.cmp(&b.model)));

    rows
}

/// Look up an attribute value by key within an OTLP attribute set.
fn find_attribute<'a>(attributes: &'a [TuiKeyValue], key: &str) -> Option<&'a str> {
    attributes.iter().find(|kv| kv.key == key).map(|kv| kv.value.as_str())
}

/// Convert a metric number to `f64`, discarding negative values.
fn metric_number_to_f64(number: &TuiMetricNumber) -> Option<f64> {
    match number {
        TuiMetricNumber::F64(value) => Some(*value),
        TuiMetricNumber::I64(value) => Some((*value).max(0) as f64),
        TuiMetricNumber::U64(value) => Some(*value as f64),
    }
}

/// Convert a metric number into `u64`, rejecting negative inputs.
fn metric_number_to_u64(number: &TuiMetricNumber) -> Option<u64> {
    match number {
        TuiMetricNumber::F64(value) => {
            if *value >= 0.0 {
                Some(*value as u64)
            } else {
                None
            }
        }
        TuiMetricNumber::I64(value) => {
            if *value >= 0 {
                Some(*value as u64)
            } else {
                None
            }
        }
        TuiMetricNumber::U64(value) => Some(*value),
    }
}

/// Lightweight representation of a single log entry shown in the log pane.
#[derive(Debug)]
pub(crate) struct LogLine {
    pub(crate) timestamp: String,
    pub(crate) level: Level,
    pub(crate) message: String,
}

/// Represents a single trace tree and caches derived information for fast UI
/// rendering.
#[derive(Debug)]
pub(crate) struct TraceEntry {
    pub(crate) trace_id: TraceId,
    pub(crate) spans: HashMap<SpanId, TraceSpan>,
    pub(crate) children: HashMap<SpanId, Vec<SpanId>>,
    pub(crate) start_ns: u64,
    pub(crate) end_ns: u64,
}

impl TraceEntry {
    /// Create a brand-new trace entry from the first span record we observe.
    pub(crate) fn from_record(record: SpanRecord) -> Self {
        let mut entry = Self {
            trace_id: record.trace_id,
            spans: HashMap::new(),
            children: HashMap::new(),
            start_ns: record.begin_time_unix_ns,
            end_ns: record.begin_time_unix_ns.saturating_add(record.duration_ns),
        };
        entry.insert_span(record);
        entry
    }

    /// Merge another span into the trace while expanding the time bounds.
    pub(crate) fn add_span(&mut self, record: SpanRecord) {
        self.start_ns = self.start_ns.min(record.begin_time_unix_ns);
        self.end_ns = self
            .end_ns
            .max(record.begin_time_unix_ns.saturating_add(record.duration_ns));
        self.insert_span(record);
    }

    /// Insert a span into the internal lookup tables and bind it to its parent.
    pub(crate) fn insert_span(&mut self, record: SpanRecord) {
        let span_id = record.span_id;
        let parent_id = record.parent_id;
        let properties = record
            .properties
            .into_iter()
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect();
        let span = TraceSpan {
            span_id,
            parent_id,
            name: record.name.into_owned(),
            begin_time_unix_ns: record.begin_time_unix_ns,
            duration_ns: record.duration_ns,
            properties,
        };

        self.spans.insert(span_id, span);

        if parent_id != SpanId::default() {
            let children = self.children.entry(parent_id).or_default();
            if !children.contains(&span_id) {
                children.push(span_id);
            }
        }

        self.children.entry(span_id).or_default();
    }

    /// Flatten the trace hierarchy into a vector ready for rendering.
    pub(crate) fn timeline_items(&self) -> Vec<TimelineItem> {
        let mut items = Vec::new();
        for root_id in self.root_ids() {
            self.collect_timeline_items(root_id, 0, &mut items);
        }
        items
    }

    /// Recursively push spans into the flattened timeline vector.
    pub(crate) fn collect_timeline_items(&self, span_id: SpanId, depth: usize, items: &mut Vec<TimelineItem>) {
        let Some(span) = self.spans.get(&span_id) else {
            return;
        };

        let offset_ns = span.begin_time_unix_ns.saturating_sub(self.start_ns);
        items.push(TimelineItem {
            span_id,
            depth,
            offset_ns,
            duration_ns: span.duration_ns,
            name: span.name.clone(),
        });

        let mut children = self.children.get(&span_id).cloned().unwrap_or_default();
        children.sort_by_key(|child_id| {
            self.spans
                .get(child_id)
                .map(|child| child.begin_time_unix_ns)
                .unwrap_or(u64::MAX)
        });

        for child_id in children {
            self.collect_timeline_items(child_id, depth + 1, items);
        }
    }

    /// Convert the trace start nanoseconds into a `SystemTime`.
    pub(crate) fn start_time(&self) -> SystemTime {
        UNIX_EPOCH + Duration::from_nanos(self.start_ns)
    }

    /// Total time covered by the trace, in nanoseconds.
    pub(crate) fn total_duration_ns(&self) -> u64 {
        self.end_ns.saturating_sub(self.start_ns)
    }

    /// Identify the span best suited to act as the root label.
    pub(crate) fn root_span(&self) -> Option<&TraceSpan> {
        let roots = self.root_ids();
        roots
            .first()
            .and_then(|id| self.spans.get(id))
            .or_else(|| self.spans.values().min_by_key(|span| span.begin_time_unix_ns))
    }

    /// Collect all span ids that do not have parents within this trace.
    pub(crate) fn root_ids(&self) -> Vec<SpanId> {
        if self.spans.is_empty() {
            return Vec::new();
        }

        let mut child_ids = HashSet::new();
        for children in self.children.values() {
            for child in children {
                child_ids.insert(*child);
            }
        }

        let mut roots = Vec::new();
        for (&span_id, span) in &self.spans {
            if child_ids.contains(&span_id)
                && span.parent_id != SpanId::default()
                && self.spans.contains_key(&span.parent_id)
            {
                continue;
            }
            if span.parent_id == SpanId::default() || !self.spans.contains_key(&span.parent_id) {
                roots.push(span_id);
            }
        }

        roots.sort_by_key(|id| self.spans[id].begin_time_unix_ns);
        roots
    }
}

/// Concrete span metadata cached inside a `TraceEntry`.
#[derive(Debug)]
pub(crate) struct TraceSpan {
    pub(crate) span_id: SpanId,
    pub(crate) parent_id: SpanId,
    pub(crate) name: String,
    pub(crate) begin_time_unix_ns: u64,
    pub(crate) duration_ns: u64,
    pub(crate) properties: Vec<(String, String)>,
}

/// Flattened representation of a span used by the timeline table.
pub(crate) struct TimelineItem {
    pub(crate) span_id: SpanId,
    pub(crate) depth: usize,
    pub(crate) offset_ns: u64,
    pub(crate) duration_ns: u64,
    pub(crate) name: String,
}
