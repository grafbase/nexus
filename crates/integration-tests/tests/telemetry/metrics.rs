//! Tests for telemetry metrics collection

use clickhouse::Row;
use serde::{Deserialize, Serialize};

mod llm;
mod mcp;
mod redis;

/// Row structure for histogram metrics in ClickHouse
#[derive(Row, Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct HistogramMetricRow {
    pub metric_name: String,
    pub attributes: Vec<(String, String)>,
    #[serde(alias = "Count")]
    pub count: u64,
}

/// Row structure for sum/counter metrics in ClickHouse
#[derive(Row, Deserialize, Debug, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct SumMetricRow {
    pub metric_name: String,
    pub attributes: Vec<(String, String)>,
    pub value: f64,
}
