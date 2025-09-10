//! Telemetry testing utilities for ClickHouse metrics queries

use clickhouse::Row;
use serde::Deserialize;
use std::time::Duration;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10); // Increased to handle slow OTLP export
const RETRY_INTERVAL: Duration = Duration::from_millis(10); // Check more frequently

/// Helper to create a ClickHouse client for metrics queries
pub async fn create_clickhouse_client() -> clickhouse::Client {
    clickhouse::Client::default()
        .with_url("http://localhost:8123")
        .with_database("otel")
        .with_user("grafbase")
        .with_password("grafbase")
}

/// Generate a unique service name for test isolation
pub fn unique_service_name(test_name: &str) -> String {
    let uuid = uuid::Uuid::new_v4();
    format!("nexus-test-{}-{}", test_name, uuid)
}

/// Wait for metrics that match a condition.
/// This is the generic function that retries until the condition is met or times out.
/// If the query fails (e.g., deserialization error), it returns immediately with an error.
pub async fn wait_for_metrics_matching<T, F>(
    clickhouse: &clickhouse::Client,
    query: impl Into<String>,
    condition: F,
) -> anyhow::Result<Vec<T>>
where
    T: Row + for<'a> Deserialize<'a>,
    F: Fn(&[T]) -> bool,
{
    let query = query.into();
    let start = std::time::Instant::now();

    tokio::time::timeout(DEFAULT_TIMEOUT, async {
        let mut attempt = 0;

        loop {
            // Execute query and handle errors explicitly
            let rows = match clickhouse.query(&query).fetch_all::<T>().await {
                Ok(rows) => rows,
                Err(e) => {
                    // If query fails (e.g., wrong schema, deserialization error), fail immediately
                    return Err(anyhow::anyhow!("Query failed: {}", e));
                }
            };

            if condition(&rows) {
                let elapsed = start.elapsed();
                if elapsed.as_millis() > 100 {
                    eprintln!(
                        "DEBUG: Found metrics after {}ms ({} attempts)",
                        elapsed.as_millis(),
                        attempt
                    );
                }
                return Ok(rows);
            }

            attempt += 1;
            if attempt % 50 == 0 {
                eprintln!(
                    "Still waiting for metrics (attempt {} - {}ms elapsed)",
                    attempt,
                    attempt * 10
                );
            }

            tokio::time::sleep(RETRY_INTERVAL).await;
        }
    })
    .await
    .map_err(|_| anyhow::anyhow!("Timeout waiting for metrics after {:?}", DEFAULT_TIMEOUT))?
}
