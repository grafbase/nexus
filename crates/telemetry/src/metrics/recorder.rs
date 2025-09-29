use std::time::Instant;

use opentelemetry::{Key, KeyValue, Value, metrics::Histogram};

/// A timer that records elapsed time to a histogram on drop.
///
/// This struct provides automatic duration tracking for operations. When created,
/// it captures the current time, and when dropped, it calculates the elapsed
/// duration and records it to the specified histogram metric.
///
/// # Examples
///
/// ```rust
/// use telemetry::metrics::Recorder;
///
/// async fn process_request() -> Result<Response, Error> {
///     let mut recorder = Recorder::new("request.duration");
///     recorder.push_attribute("endpoint", "/api/users");
///
///     // Perform operation - recorder automatically records when .record() is called
///     do_work().await
/// }
/// ```
///
/// # Recording
///
/// Call the `record()` method to record the elapsed time to the histogram.
/// The recorder consumes itself when recording.
pub struct Recorder {
    start: Instant,
    histogram: Histogram<f64>,
    attributes: Vec<KeyValue>,
}

impl Recorder {
    /// Creates a new recorder for the specified metric.
    ///
    /// The recorder starts timing immediately upon creation and will record the elapsed
    /// time in milliseconds when `record()` is called.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the histogram metric to record to
    ///
    /// # Examples
    ///
    /// ```rust
    /// let recorder = Recorder::new("http.server.request.duration");
    /// ```
    pub fn new(name: &'static str) -> Self {
        Self {
            start: Instant::now(),
            histogram: super::meter().f64_histogram(name).with_unit("s").build(),
            attributes: Vec::new(),
        }
    }

    /// Adds an attribute to be recorded with the metric.
    ///
    /// Attributes provide additional context for the metric and enable
    /// filtering and grouping in observability tools.
    ///
    /// # Arguments
    ///
    /// * `key` - The attribute key
    /// * `value` - The attribute value
    ///
    /// # Examples
    ///
    /// ```rust
    /// let mut recorder = Recorder::new("operation.duration");
    /// recorder.push_attribute("operation", "create_user");
    /// recorder.push_attribute("status", 200);
    /// recorder.record();
    /// ```
    pub fn push_attribute<K, V>(&mut self, key: K, value: V)
    where
        K: Into<Key>,
        V: Into<Value>,
    {
        self.attributes.push(KeyValue::new(key, value));
    }

    /// Records the elapsed time to the histogram.
    pub fn record(self) {
        self.histogram
            .record(self.start.elapsed().as_secs_f64(), &self.attributes);
    }
}
