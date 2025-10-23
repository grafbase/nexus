use std::time::SystemTime;

use fastrace::{collector::Reporter, prelude::SpanRecord};
use log::Level;
use tokio::sync::mpsc::{Receiver, Sender, error::TrySendError};

/// Channel sender for exporting trace events to external consumers.
pub type TraceExportSender = Sender<TraceEvent>;
/// Channel receiver for exporting trace events to external consumers.
pub type TraceExportReceiver = Receiver<TraceEvent>;

/// Key/value pair used in the TUI metrics snapshot.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TuiKeyValue {
    pub key: String,
    pub value: String,
}

/// Represents the instrumentation scope associated with a batch of metrics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TuiInstrumentationScope {
    pub name: String,
    pub version: Option<String>,
    pub schema_url: Option<String>,
    pub attributes: Vec<TuiKeyValue>,
}

/// Snapshot of metrics grouped by instrumentation scope.
#[derive(Debug, Clone, PartialEq)]
pub struct TuiScopeMetrics {
    pub scope: TuiInstrumentationScope,
    pub metrics: Vec<TuiMetric>,
}

/// Number value preserved from an OpenTelemetry metric data point.
#[derive(Debug, Clone, PartialEq)]
pub enum TuiMetricNumber {
    F64(f64),
    I64(i64),
    U64(u64),
}

impl From<f64> for TuiMetricNumber {
    fn from(value: f64) -> Self {
        Self::F64(value)
    }
}

impl From<i64> for TuiMetricNumber {
    fn from(value: i64) -> Self {
        Self::I64(value)
    }
}

impl From<u64> for TuiMetricNumber {
    fn from(value: u64) -> Self {
        Self::U64(value)
    }
}

/// Temporality mode for metric aggregations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TuiTemporality {
    Cumulative,
    Delta,
    LowMemory,
}

/// A single gauge data point.
#[derive(Debug, Clone, PartialEq)]
pub struct TuiGaugePoint {
    pub attributes: Vec<TuiKeyValue>,
    pub value: TuiMetricNumber,
}

/// Gauge metric snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct TuiGaugeMetric {
    pub start_time: Option<SystemTime>,
    pub time: SystemTime,
    pub points: Vec<TuiGaugePoint>,
}

/// A single sum data point.
#[derive(Debug, Clone, PartialEq)]
pub struct TuiSumPoint {
    pub attributes: Vec<TuiKeyValue>,
    pub value: TuiMetricNumber,
}

/// Sum metric snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct TuiSumMetric {
    pub start_time: SystemTime,
    pub time: SystemTime,
    pub temporality: TuiTemporality,
    pub is_monotonic: bool,
    pub points: Vec<TuiSumPoint>,
}

/// Histogram data point.
#[derive(Debug, Clone, PartialEq)]
pub struct TuiHistogramPoint {
    pub attributes: Vec<TuiKeyValue>,
    pub count: u64,
    pub sum: TuiMetricNumber,
    pub min: Option<TuiMetricNumber>,
    pub max: Option<TuiMetricNumber>,
    pub bounds: Vec<f64>,
    pub bucket_counts: Vec<u64>,
}

/// Histogram metric snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct TuiHistogramMetric {
    pub start_time: SystemTime,
    pub time: SystemTime,
    pub temporality: TuiTemporality,
    pub points: Vec<TuiHistogramPoint>,
}

/// Exponential histogram bucket summary.
#[derive(Debug, Clone, PartialEq)]
pub struct TuiExponentialBucket {
    pub offset: i32,
    pub counts: Vec<u64>,
}

/// Exponential histogram data point.
#[derive(Debug, Clone, PartialEq)]
pub struct TuiExponentialHistogramPoint {
    pub attributes: Vec<TuiKeyValue>,
    pub count: usize,
    pub sum: TuiMetricNumber,
    pub min: Option<TuiMetricNumber>,
    pub max: Option<TuiMetricNumber>,
    pub scale: i8,
    pub zero_count: u64,
    pub zero_threshold: f64,
    pub positive: TuiExponentialBucket,
    pub negative: TuiExponentialBucket,
}

/// Exponential histogram metric snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct TuiExponentialHistogramMetric {
    pub start_time: SystemTime,
    pub time: SystemTime,
    pub temporality: TuiTemporality,
    pub points: Vec<TuiExponentialHistogramPoint>,
}

/// Snapshot of a single metric exported through the TUI channel.
#[derive(Debug, Clone, PartialEq)]
pub struct TuiMetric {
    pub name: String,
    pub description: String,
    pub unit: String,
    pub data: TuiMetricData,
}

/// Metric data enumeration for the TUI snapshot.
#[derive(Debug, Clone, PartialEq)]
pub enum TuiMetricData {
    Gauge(TuiGaugeMetric),
    Sum(TuiSumMetric),
    Histogram(TuiHistogramMetric),
    ExponentialHistogram(TuiExponentialHistogramMetric),
}

/// Snapshot of the metrics batch exported to the TUI.
#[derive(Debug, Clone, PartialEq)]
pub struct TuiMetricSnapshot {
    pub captured_at: SystemTime,
    pub resource_attributes: Vec<TuiKeyValue>,
    pub scopes: Vec<TuiScopeMetrics>,
}

/// Events forwarded to the TUI.
#[derive(Debug)]
pub enum TraceEvent {
    /// A batch of spans ready for display.
    Spans(Vec<SpanRecord>),
    /// A human-readable log message with level metadata.
    Log {
        timestamp: String,
        level: Level,
        message: String,
    },
    /// Structured metrics captured during the latest export.
    Metrics(TuiMetricSnapshot),
}

pub struct TuiReporter {
    channel: TraceExportSender,
}

impl TuiReporter {
    /// Create a new reporter that forwards spans to the provided channel.
    pub fn new(channel: TraceExportSender) -> Self {
        Self { channel }
    }
}

impl Reporter for TuiReporter {
    fn report(&mut self, spans: Vec<SpanRecord>) {
        if spans.is_empty() {
            return;
        }

        if let Err(err) = self.channel.try_send(TraceEvent::Spans(spans)) {
            match err {
                TrySendError::Full(_) => log::warn!("Span export channel full; dropping span batch"),
                TrySendError::Closed(_) => {
                    // Receiver has been dropped, typically because the TUI shut down; ignore.
                }
            }
        }
    }
}
