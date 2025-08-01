# OpenTelemetry Logs Configuration for Nexus

This document contains the OpenTelemetry logs configuration design for integrating with logforth and fastrace-opentelemetry.

## TOML Configuration

```toml
# OpenTelemetry Logs Configuration for Nexus
[server.telemetry]
# Enable telemetry
enabled = true

# Service identification
service_name = "nexus"
service_version = "{{ env.SERVICE_VERSION }}"
service_namespace = "mcp-routing"

# Resource attributes for all telemetry data
[server.telemetry.resource]
# Additional attributes that will be attached to all logs
[server.telemetry.resource.attributes]
"deployment.environment" = "{{ env.ENVIRONMENT }}"
"host.name" = "{{ env.HOSTNAME }}"
"service.instance.id" = "{{ env.INSTANCE_ID }}"

# OpenTelemetry Logs specific configuration
[server.telemetry.logs]
# Enable OTLP log export
enabled = true

# Log level threshold for OTLP export
# Only logs at this level and above will be exported
# Options: "trace", "debug", "info", "warn", "error"
export_level = "info"

# OTLP Exporter configuration
[server.telemetry.logs.exporter]
# OTLP endpoint for logs
endpoint = "{{ env.OTEL_EXPORTER_OTLP_LOGS_ENDPOINT }}"

# Protocol: "grpc" or "http"
protocol = "grpc"

# Timeout for export operations
timeout = "10s"

# Headers for authentication/routing
[server.telemetry.logs.exporter.headers]
"api-key" = "{{ env.OTEL_API_KEY }}"
"x-tenant-id" = "{{ env.TENANT_ID }}"

# TLS configuration
[server.telemetry.logs.exporter.tls]
enabled = true
ca_cert = "{{ env.OTEL_CA_CERT_PATH }}"
# For mTLS
client_cert = "{{ env.OTEL_CLIENT_CERT_PATH }}"
client_key = "{{ env.OTEL_CLIENT_KEY_PATH }}"
# Dangerous: only for development
insecure_skip_verify = false

# Batch processor configuration
[server.telemetry.logs.batch]
# Maximum number of logs to queue
max_queue_size = 2048

# Maximum number of logs to export in a single batch
max_export_batch_size = 512

# Time to wait before exporting a batch (even if not full)
scheduled_delay = "1s"

# Maximum time allowed for exporting a batch
export_timeout = "30s"

# Log enhancement configuration
[server.telemetry.logs.enhancement]
# Include trace context in logs (for correlation)
include_trace_context = true

# Include span context fields
include_span_id = true
include_trace_id = true
include_trace_flags = true

# Add source location information
include_source_location = true

# Fields to include from log records
include_fields = ["target", "module_path", "file", "line"]

# Transform configuration for logs
[server.telemetry.logs.transform]
# Field mappings (map log fields to OTLP semantic conventions)
[server.telemetry.logs.transform.field_map]
"msg" = "body"
"level" = "severity_text"
"target" = "instrumentation_scope.name"

# Severity mapping (map Rust log levels to OTLP severity)
[server.telemetry.logs.transform.severity_map]
"TRACE" = 1  # TRACE
"DEBUG" = 5  # DEBUG
"INFO" = 9   # INFO
"WARN" = 13  # WARN
"ERROR" = 17 # ERROR

# Logforth integration specific settings
[server.telemetry.logs.logforth]
# Enable OpenTelemetry appender in logforth
otlp_appender = true

# Keep existing appenders (stdout, etc.)
keep_existing_appenders = true

# FastTrace integration
[server.telemetry.logs.fastrace]
# Enable trace correlation
enable_trace_correlation = true

# Include FastTrace events as log attributes
include_events = true

# Performance settings
[server.telemetry.logs.performance]
# Use async export to avoid blocking
async_export = true

# Number of worker threads for log export
worker_threads = 1

# Circuit breaker configuration
[server.telemetry.logs.circuit_breaker]
enabled = true
# Number of consecutive failures before opening circuit
failure_threshold = 5
# Number of successes to close circuit
success_threshold = 2
# Time to wait in open state before trying again
timeout = "30s"

# Filtering configuration
[server.telemetry.logs.filter]
# Exclude logs from specific modules
exclude_modules = [
    "hyper::proto",
    "rustls::client",
    "h2::codec"
]

# Include only logs from specific modules (overrides exclude)
# include_modules = ["nexus", "mcp", "server"]

# Exclude logs with specific message patterns (regex)
exclude_patterns = [
    "^Health check",
    "^Metrics scraped"
]

# Sampling configuration for high-volume logs
[server.telemetry.logs.sampling]
# Enable sampling
enabled = false

# Sampling rate (0.0 to 1.0)
rate = 0.1

# Always sample errors regardless of rate
always_sample_errors = true
```

## Key Features

1. **Seamless Integration**: Works alongside existing stdout/json logging
2. **Trace Correlation**: Links logs with traces via FastTrace
3. **Batching**: Efficient export with configurable batch settings
4. **Filtering**: Control which logs get exported to reduce costs
5. **Security**: Full TLS/mTLS support for secure transport
6. **Reliability**: Circuit breaker prevents cascading failures

## Implementation Notes

- Integrates with existing logforth setup in `nexus/src/logger.rs`
- Uses fastrace-opentelemetry for trace correlation
- Maintains backward compatibility with current logging
- Environment variable substitution follows Nexus patterns
- OTLP appender would be conditionally added based on telemetry.enabled

## Libraries

- [fastrace-opentelemetry](https://docs.rs/fastrace-opentelemetry/latest/fastrace_opentelemetry/)
- [logforth](https://docs.rs/logforth/latest/logforth/)