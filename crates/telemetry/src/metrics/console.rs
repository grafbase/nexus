#![allow(dead_code)]

use std::{
    cmp::Ordering,
    time::{Duration, SystemTime},
};

use opentelemetry::{KeyValue, Value};
use opentelemetry_sdk::{
    error::OTelSdkResult,
    metrics::{
        Temporality,
        data::{self, ResourceMetrics},
        exporter::PushMetricExporter,
    },
};

use crate::tracing::{
    TraceEvent, TraceExportSender, TuiExponentialBucket, TuiExponentialHistogramMetric, TuiExponentialHistogramPoint,
    TuiGaugeMetric, TuiGaugePoint, TuiHistogramMetric, TuiHistogramPoint, TuiInstrumentationScope, TuiKeyValue,
    TuiMetric, TuiMetricData, TuiMetricNumber, TuiMetricSnapshot, TuiScopeMetrics, TuiSumMetric, TuiSumPoint,
    TuiTemporality,
};
use tokio::sync::mpsc::error::TrySendError;

pub struct TuiMetricsExporter {
    channel: TraceExportSender,
}

impl TuiMetricsExporter {
    pub fn new(channel: TraceExportSender) -> Self {
        Self { channel }
    }
}

impl PushMetricExporter for TuiMetricsExporter {
    async fn export(&self, metrics: &ResourceMetrics) -> OTelSdkResult {
        let snapshot = build_snapshot(metrics);

        if let Err(err) = self.channel.try_send(TraceEvent::Metrics(snapshot)) {
            match err {
                TrySendError::Full(_) => log::warn!("Metrics channel full; dropping snapshot"),
                TrySendError::Closed(_) => {
                    // Receiver has been dropped, likely because the TUI shut down; ignore.
                }
            }
        }

        Ok(())
    }

    fn force_flush(&self) -> OTelSdkResult {
        // Nothing buffered locally, so there is nothing to flush.
        Ok(())
    }

    fn shutdown_with_timeout(&self, timeout: Duration) -> OTelSdkResult {
        let _ = timeout;
        // No background tasks or buffers to drain; behave as a no-op shutdown.
        self.force_flush()
    }

    fn temporality(&self) -> Temporality {
        Temporality::Cumulative
    }
}

fn build_snapshot(metrics: &ResourceMetrics) -> TuiMetricSnapshot {
    let mut scopes: Vec<TuiScopeMetrics> = metrics.scope_metrics().map(convert_scope_metrics).collect();

    scopes.sort_by(|a, b| a.scope.name.cmp(&b.scope.name));

    let mut resource_attributes = metrics
        .resource()
        .iter()
        .map(|(key, value)| convert_pair(key.to_string(), value))
        .collect::<Vec<_>>();

    resource_attributes.sort();

    TuiMetricSnapshot {
        captured_at: SystemTime::now(),
        resource_attributes,
        scopes,
    }
}

fn convert_scope_metrics(scope_metrics: &data::ScopeMetrics) -> TuiScopeMetrics {
    let scope = scope_metrics.scope();

    let mut attributes = scope
        .attributes()
        .map(|kv| convert_pair(kv.key.to_string(), &kv.value))
        .collect::<Vec<_>>();
    attributes.sort();

    let mut metrics = scope_metrics.metrics().map(convert_metric).collect::<Vec<_>>();
    metrics.sort_by(metric_cmp);

    TuiScopeMetrics {
        scope: TuiInstrumentationScope {
            name: scope.name().to_string(),
            version: scope.version().map(|v| v.to_string()),
            schema_url: scope.schema_url().map(|url| url.to_string()),
            attributes,
        },
        metrics,
    }
}

fn metric_cmp(a: &TuiMetric, b: &TuiMetric) -> Ordering {
    a.name.cmp(&b.name).then_with(|| a.unit.cmp(&b.unit))
}

fn convert_metric(metric: &data::Metric) -> TuiMetric {
    let data = match metric.data() {
        data::AggregatedMetrics::F64(inner) => convert_metric_data_f64(inner),
        data::AggregatedMetrics::I64(inner) => convert_metric_data_i64(inner),
        data::AggregatedMetrics::U64(inner) => convert_metric_data_u64(inner),
    };

    TuiMetric {
        name: metric.name().to_string(),
        description: metric.description().to_string(),
        unit: metric.unit().to_string(),
        data,
    }
}

fn convert_metric_data_f64(data: &data::MetricData<f64>) -> TuiMetricData {
    match data {
        data::MetricData::Gauge(gauge) => TuiMetricData::Gauge(convert_gauge(gauge, TuiMetricNumber::from)),
        data::MetricData::Sum(sum) => TuiMetricData::Sum(convert_sum(sum, TuiMetricNumber::from)),
        data::MetricData::Histogram(histogram) => {
            TuiMetricData::Histogram(convert_histogram(histogram, TuiMetricNumber::from))
        }
        data::MetricData::ExponentialHistogram(histogram) => {
            TuiMetricData::ExponentialHistogram(convert_exponential_histogram(histogram, TuiMetricNumber::from))
        }
    }
}

fn convert_metric_data_i64(data: &data::MetricData<i64>) -> TuiMetricData {
    match data {
        data::MetricData::Gauge(gauge) => TuiMetricData::Gauge(convert_gauge(gauge, TuiMetricNumber::from)),
        data::MetricData::Sum(sum) => TuiMetricData::Sum(convert_sum(sum, TuiMetricNumber::from)),
        data::MetricData::Histogram(histogram) => {
            TuiMetricData::Histogram(convert_histogram(histogram, TuiMetricNumber::from))
        }
        data::MetricData::ExponentialHistogram(histogram) => {
            TuiMetricData::ExponentialHistogram(convert_exponential_histogram(histogram, TuiMetricNumber::from))
        }
    }
}

fn convert_metric_data_u64(data: &data::MetricData<u64>) -> TuiMetricData {
    match data {
        data::MetricData::Gauge(gauge) => TuiMetricData::Gauge(convert_gauge(gauge, TuiMetricNumber::from)),
        data::MetricData::Sum(sum) => TuiMetricData::Sum(convert_sum(sum, TuiMetricNumber::from)),
        data::MetricData::Histogram(histogram) => {
            TuiMetricData::Histogram(convert_histogram(histogram, TuiMetricNumber::from))
        }
        data::MetricData::ExponentialHistogram(histogram) => {
            TuiMetricData::ExponentialHistogram(convert_exponential_histogram(histogram, TuiMetricNumber::from))
        }
    }
}

fn convert_gauge<T: Copy>(gauge: &data::Gauge<T>, to_number: impl Fn(T) -> TuiMetricNumber + Copy) -> TuiGaugeMetric {
    let mut points = gauge
        .data_points()
        .map(|point| TuiGaugePoint {
            attributes: convert_attributes(point.attributes()),
            value: to_number(point.value()),
        })
        .collect::<Vec<_>>();
    points.sort_by(point_cmp);

    TuiGaugeMetric {
        start_time: gauge.start_time(),
        time: gauge.time(),
        points,
    }
}

fn convert_sum<T: Copy>(sum: &data::Sum<T>, to_number: impl Fn(T) -> TuiMetricNumber + Copy) -> TuiSumMetric {
    let mut points = sum
        .data_points()
        .map(|point| TuiSumPoint {
            attributes: convert_attributes(point.attributes()),
            value: to_number(point.value()),
        })
        .collect::<Vec<_>>();
    points.sort_by(point_cmp);

    TuiSumMetric {
        start_time: sum.start_time(),
        time: sum.time(),
        temporality: convert_temporality(sum.temporality()),
        is_monotonic: sum.is_monotonic(),
        points,
    }
}

fn convert_histogram<T: Copy>(
    histogram: &data::Histogram<T>,
    to_number: impl Fn(T) -> TuiMetricNumber + Copy,
) -> TuiHistogramMetric {
    let mut points = histogram
        .data_points()
        .map(|point| TuiHistogramPoint {
            attributes: convert_attributes(point.attributes()),
            count: point.count(),
            sum: to_number(point.sum()),
            min: point.min().map(to_number),
            max: point.max().map(to_number),
            bounds: point.bounds().collect(),
            bucket_counts: point.bucket_counts().collect(),
        })
        .collect::<Vec<_>>();
    points.sort_by(point_cmp);

    TuiHistogramMetric {
        start_time: histogram.start_time(),
        time: histogram.time(),
        temporality: convert_temporality(histogram.temporality()),
        points,
    }
}

fn convert_exponential_histogram<T: Copy>(
    histogram: &data::ExponentialHistogram<T>,
    to_number: impl Fn(T) -> TuiMetricNumber + Copy,
) -> TuiExponentialHistogramMetric {
    let mut points = histogram
        .data_points()
        .map(|point| TuiExponentialHistogramPoint {
            attributes: convert_attributes(point.attributes()),
            count: point.count(),
            sum: to_number(point.sum()),
            min: point.min().map(to_number),
            max: point.max().map(to_number),
            scale: point.scale(),
            zero_count: point.zero_count(),
            zero_threshold: point.zero_threshold(),
            positive: convert_exponential_bucket(point.positive_bucket()),
            negative: convert_exponential_bucket(point.negative_bucket()),
        })
        .collect::<Vec<_>>();
    points.sort_by(point_cmp);

    TuiExponentialHistogramMetric {
        start_time: histogram.start_time(),
        time: histogram.time(),
        temporality: convert_temporality(histogram.temporality()),
        points,
    }
}

fn convert_exponential_bucket(bucket: &data::ExponentialBucket) -> TuiExponentialBucket {
    TuiExponentialBucket {
        offset: bucket.offset(),
        counts: bucket.counts().collect(),
    }
}

fn convert_attributes<'a, I>(iter: I) -> Vec<TuiKeyValue>
where
    I: Iterator<Item = &'a KeyValue>,
{
    let mut attributes = iter
        .map(|kv| convert_pair(kv.key.to_string(), &kv.value))
        .collect::<Vec<_>>();
    attributes.sort();
    attributes
}

fn convert_pair(key: String, value: &Value) -> TuiKeyValue {
    TuiKeyValue {
        key,
        value: format_value(value),
    }
}

fn format_value(value: &Value) -> String {
    match value {
        Value::Bool(v) => v.to_string(),
        Value::I64(v) => v.to_string(),
        Value::F64(v) => v.to_string(),
        Value::String(v) => v.to_string(),
        Value::Array(arr) => arr.to_string(),
        _ => value.to_string(),
    }
}

fn convert_temporality(value: Temporality) -> TuiTemporality {
    match value {
        Temporality::Cumulative => TuiTemporality::Cumulative,
        Temporality::Delta => TuiTemporality::Delta,
        Temporality::LowMemory => TuiTemporality::LowMemory,
        _ => TuiTemporality::Cumulative,
    }
}

fn point_cmp<A>(a: &A, b: &A) -> Ordering
where
    A: PointAttributes,
{
    a.point_attributes()
        .cmp(b.point_attributes())
        .then_with(|| a.point_time_hint().cmp(&b.point_time_hint()))
}

trait PointAttributes {
    fn point_attributes(&self) -> &Vec<TuiKeyValue>;
    fn point_time_hint(&self) -> u64;
}

impl PointAttributes for TuiGaugePoint {
    fn point_attributes(&self) -> &Vec<TuiKeyValue> {
        &self.attributes
    }

    fn point_time_hint(&self) -> u64 {
        0
    }
}

impl PointAttributes for TuiSumPoint {
    fn point_attributes(&self) -> &Vec<TuiKeyValue> {
        &self.attributes
    }

    fn point_time_hint(&self) -> u64 {
        0
    }
}

impl PointAttributes for TuiHistogramPoint {
    fn point_attributes(&self) -> &Vec<TuiKeyValue> {
        &self.attributes
    }

    fn point_time_hint(&self) -> u64 {
        self.count
    }
}

impl PointAttributes for TuiExponentialHistogramPoint {
    fn point_attributes(&self) -> &Vec<TuiKeyValue> {
        &self.attributes
    }

    fn point_time_hint(&self) -> u64 {
        self.count as u64
    }
}
