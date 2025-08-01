# Audit Log Integration Options for Nexus

## Overview

This document outlines various integration options to make audit logging as seamless as possible for Nexus users, ranging from zero-config options to enterprise-grade solutions.

## 1. Zero-Configuration Options

### Local File with Rotation (Default)
```toml
# Default when no audit sink is configured
[server.audit]
enabled = true
# Automatically writes to /var/log/nexus/audit/
# with daily rotation and 90-day retention
```

Benefits:
- Works out of the box
- No external dependencies
- Can be collected by existing log agents (Filebeat, Fluent Bit, etc.)

### Stdout with Structured JSON (12-Factor App)
```toml
[server.audit]
enabled = true
sink = "stdout"
format = "json"
# Writes to stdout with special audit markers
# {"@audit":true,"event":"auth.success",...}
```

Benefits:
- Container-friendly
- Works with any log aggregator
- Easy to filter audit events by `@audit` field

## 2. Cloud Provider Native Integrations

### AWS Integration
```toml
[server.audit.aws]
enabled = true
# Auto-detects AWS environment
use_imds = true  # Instance metadata service

# Option A: CloudWatch Logs
cloudwatch.enabled = true
cloudwatch.log_group = "/aws/nexus/audit"
# Auto-creates log group if it doesn't exist

# Option B: Kinesis Firehose (for S3/Redshift)
firehose.enabled = true
firehose.stream_name = "nexus-audit-stream"

# Option C: EventBridge for real-time processing
eventbridge.enabled = true
eventbridge.event_bus = "nexus-audit-events"
```

### Google Cloud Integration
```toml
[server.audit.gcp]
enabled = true
# Auto-detects GCP environment

# Cloud Logging with automatic resource detection
logging.enabled = true
logging.log_name = "nexus-audit"

# Pub/Sub for streaming
pubsub.enabled = true
pubsub.topic = "nexus-audit-events"
```

### Azure Integration
```toml
[server.audit.azure]
enabled = true
# Auto-detects Azure environment

# Event Hubs
event_hubs.enabled = true
event_hubs.namespace = "{{ env.AZURE_EVENT_HUB_NAMESPACE }}"
event_hubs.hub_name = "audit-events"

# Application Insights
app_insights.enabled = true
app_insights.custom_events = true
```

## 3. Popular Webhook Integrations

### Generic Webhook (Most Flexible)
```toml
[server.audit.webhook]
enabled = true
url = "{{ env.AUDIT_WEBHOOK_URL }}"
headers = { "X-API-Key" = "{{ env.AUDIT_API_KEY }}" }

# Batching for efficiency
batch.enabled = true
batch.max_size = 100
batch.max_wait = "5s"

# Reliability
retry.enabled = true
retry.max_attempts = 3
retry.backoff = "exponential"
```

### Slack/Discord/Teams Alerts (High-Priority Events)
```toml
[server.audit.alerts]
enabled = true

# Send critical audit events to Slack
[[server.audit.alerts.destinations]]
type = "slack"
webhook_url = "{{ env.SLACK_SECURITY_WEBHOOK }}"
# Only send specific events
events = ["auth.failed", "authorization.denied", "config.changed"]
# Rate limiting to prevent spam
rate_limit = "10/minute"

[[server.audit.alerts.destinations]]
type = "pagerduty"
integration_key = "{{ env.PAGERDUTY_KEY }}"
events = ["auth.brute_force_detected"]
```

## 4. SIEM/Security Platform Integrations

### Splunk HTTP Event Collector (HEC)
```toml
[server.audit.splunk]
enabled = true
hec_url = "{{ env.SPLUNK_HEC_URL }}"
hec_token = "{{ env.SPLUNK_HEC_TOKEN }}"
# Automatic source type
source_type = "nexus:audit"
# Index routing
index = "security"

# SSL verification
tls.verify = true
tls.ca_cert = "/path/to/splunk-ca.pem"
```

### Elastic Security
```toml
[server.audit.elastic]
enabled = true
# Direct indexing to Elasticsearch
elasticsearch.urls = ["{{ env.ELASTIC_URL }}"]
elasticsearch.api_key = "{{ env.ELASTIC_API_KEY }}"
# Use data streams for automatic ILM
elasticsearch.data_stream = "logs-nexus.audit-default"

# Or use Logstash
logstash.enabled = true
logstash.url = "{{ env.LOGSTASH_URL }}"
logstash.tcp_port = 5514
```

### Datadog
```toml
[server.audit.datadog]
enabled = true
api_key = "{{ env.DD_API_KEY }}"
# Automatic site detection from DD_SITE env var
# Send as logs with special processing
logs.enabled = true
logs.service = "nexus-audit"
logs.tags = ["audit", "security", "compliance:soc2"]

# Also send as events for alerting
events.enabled = true
events.alert_type = "security"
```

## 5. Compliance-Focused Integrations

### Teleport Audit Streaming
```toml
[server.audit.teleport]
enabled = true
cluster = "{{ env.TELEPORT_CLUSTER }}"
# Uses Teleport's audit event format
forward_to_teleport = true
```

### Auth0 Log Streaming
```toml
[server.audit.auth0]
enabled = true
# Forward auth events to Auth0 for unified audit
domain = "{{ env.AUTH0_DOMAIN }}"
client_id = "{{ env.AUTH0_CLIENT_ID }}"
forward_auth_events = true
```

## 6. Database Integrations

### PostgreSQL with Automatic Schema
```toml
[server.audit.postgres]
enabled = true
url = "{{ env.AUDIT_DATABASE_URL }}"
# Auto-creates audit tables with proper schema
auto_migrate = true
# Partitioning for performance
partition_by = "month"
# Automatic old partition cleanup
retention_months = 84  # 7 years
```

### TimescaleDB for Time-Series
```toml
[server.audit.timescale]
enabled = true
url = "{{ env.TIMESCALE_URL }}"
# Automatic hypertable creation
hypertable.chunk_interval = "1 week"
# Compression after 1 month
compression.after = "1 month"
```

## 7. S3-Compatible Storage

### S3/MinIO/R2 Direct Write
```toml
[server.audit.s3]
enabled = true
# Works with AWS S3, MinIO, Cloudflare R2, etc.
endpoint = "{{ env.S3_ENDPOINT }}"  # Optional for AWS
bucket = "nexus-audit-logs"
region = "{{ env.AWS_REGION }}"

# Partitioning
prefix = "year={year}/month={month}/day={day}/hour={hour}/"

# Format options
format = "jsonl"  # or "parquet" for analytics
compression = "gzip"

# Lifecycle
lifecycle.enabled = true
lifecycle.transition_to_glacier_after_days = 90
```

## 8. Advanced Integrations

### Apache Kafka for Streaming
```toml
[server.audit.kafka]
enabled = true
brokers = ["kafka1:9092", "kafka2:9092"]
topic = "nexus-audit-events"
# Schema Registry support
schema_registry.url = "{{ env.SCHEMA_REGISTRY_URL }}"
schema_registry.auto_register = true

# Producer settings
producer.compression = "snappy"
producer.idempotence = true
```

### OpenTelemetry with Separate Pipeline
```toml
[server.audit.otlp]
enabled = true
# Different endpoint from operational logs
endpoint = "{{ env.AUDIT_OTLP_ENDPOINT }}"
# Special resource attributes
resource.attributes = {
    "audit.stream" = "true",
    "compliance.level" = "high"
}
# No sampling!
sampling.enabled = false
```

## 9. Easy Configuration Patterns

### Environment-Based Auto-Detection
```toml
[server.audit]
# Automatically detect and configure based on environment
auto_detect = true
# Falls back to local file if no cloud environment detected
```

The system would check:
1. AWS: Check for AWS_REGION, use CloudWatch
2. GCP: Check for GCP_PROJECT, use Cloud Logging
3. Azure: Check for AZURE_SUBSCRIPTION_ID, use Event Hubs
4. Kubernetes: Check for KUBERNETES_SERVICE_HOST, use stdout
5. Default: Local file with rotation

### Preset Compliance Profiles
```toml
[server.audit]
# Automatically configures audit logging for compliance
compliance_preset = "sox"  # or "hipaa", "pci", "gdpr"
```

This would automatically:
- Set appropriate retention periods
- Enable required event types
- Configure encryption
- Set up proper access controls

## 10. Client Libraries for Custom Integrations

### Audit Sink Plugin System
```rust
// Allow users to implement custom audit sinks
pub trait AuditSink: Send + Sync {
    async fn write(&self, events: Vec<AuditEvent>) -> Result<()>;
    async fn flush(&self) -> Result<()>;
}

// In config
[server.audit.custom]
enabled = true
plugin = "/path/to/custom_audit_sink.so"
config = { custom_field = "value" }
```

## Summary of Easiest Options

1. **For Developers**: Stdout JSON (works with any log collector)
2. **For Cloud Users**: Auto-detect cloud environment
3. **For Security Teams**: Direct Splunk/Elastic integration
4. **For Enterprises**: Webhook with existing SIEM
5. **For Compliance**: Preset profiles with automatic configuration

## Implementation Priority

1. **Phase 1**: Local file + Stdout JSON + Generic Webhook
2. **Phase 2**: AWS/GCP/Azure native integrations
3. **Phase 3**: Popular SIEMs (Splunk, Elastic, Datadog)
4. **Phase 4**: Specialized integrations (Kafka, custom plugins)

This approach ensures that every user, from individual developers to large enterprises, has an easy path to audit logging that fits their existing infrastructure.