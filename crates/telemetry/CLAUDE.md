# Telemetry Crate

This crate provides OpenTelemetry integration for Nexus, handling metrics collection and export.

## Purpose

- Initialize OpenTelemetry SDK with OTLP exporters
- Provide duration recording via the `Recorder` type
- Manage telemetry lifecycle with proper shutdown via `TelemetryGuard`
- Export metrics to configured backends (e.g., OTLP collectors)

## Structure

```
telemetry/
├── lib.rs              # Public API and convenience functions
├── metrics.rs          # Metrics initialization and OTLP setup
└── metrics/
    ├── names.rs        # Standard metric name constants
    └── recorder.rs     # Duration recorder for timing operations
```

## Metric Names

All metric names follow OpenTelemetry semantic conventions and are defined as constants in `metrics::names`:

- `HTTP_SERVER_REQUEST_DURATION` - HTTP server request duration histogram (milliseconds)
  - Automatically provides count, sum, and distribution

## Usage Patterns

### Using the Recorder for Duration Tracking

The `Recorder` provides a convenient way to measure operation durations:

```rust
use telemetry::metrics::Recorder;

async fn handle_request() -> Result<Response, Error> {
    let mut recorder = Recorder::new("request.duration");
    recorder.push_attribute("method", "GET");
    recorder.push_attribute("endpoint", "/api/users");

    let response = process().await?;

    recorder.push_attribute("status", response.status());
    recorder.record(); // Records elapsed time in milliseconds

    Ok(response)
}
```

### Initialization

```rust
// Initialize at startup, keep guard alive for app lifetime
let _guard = telemetry::init(&config, None).await?;
// Metrics are now available globally
// Guard ensures proper shutdown when dropped
```

## Key Design Decisions

- **Global Meter**: Uses a single "nexus" meter for all metrics
- **Recorder-based**: Uses the `Recorder` type for duration tracking with explicit recording
- **No Static Metrics**: Avoids static initialization issues
- **Histogram-based**: The Recorder uses histograms internally to provide count, sum, and distribution
